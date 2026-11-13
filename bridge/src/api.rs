use std::sync::{
    atomic::{AtomicU64, Ordering},
};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::archive;
use crate::automation;
use crate::harness;
use crate::sse;
use crate::translate;
use crate::AppState;

static REQ_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    let n = REQ_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("chatcmpl-codebridge-{n:04}")
}

pub async fn health() -> &'static str {
    "ok: CodeBridge bridge"
}

/// "지금 인수인계" 수동 트리거 — 다음 browser-mode 요청에서 인수인계 체인을 강제한다.
pub async fn trigger_handover(State(app): State<AppState>) -> Json<Value> {
    if app.cfg.automation.mode != "browser" {
        return Json(json!({
            "ok": false,
            "message": "mock 모드에서는 인수인계를 쓸 수 없습니다 (config.toml의 [automation] mode를 browser로)",
        }));
    }
    app.manual_handover.store(true, Ordering::SeqCst);
    Json(json!({
        "ok": true,
        "message": "다음 요청에서 자동으로 인수인계가 실행됩니다",
    }))
}

/// 최근 인수인계 이벤트 목록.
pub async fn handover_events(State(_app): State<AppState>) -> Json<Value> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let arc = archive::Archive::new(&cwd.to_string_lossy());
    Json(json!({
        "events": arc.handover_events(30),
    }))
}

/// OpenAI 호환 모델 목록.
pub async fn list_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": "webchat-chatgpt",
                "object": "model",
                "created": 1700000000,
                "owned_by": "codebridge"
            },
            {
                "id": "webchat-gemini",
                "object": "model",
                "created": 1700000002,
                "owned_by": "codebridge"
            },
            {
                "id": "webchat-title",
                "object": "model",
                "created": 1700000004,
                "owned_by": "codebridge"
            }
        ]
    }))
}

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub tool_calls: Option<Value>,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Option<Value>,
}

fn user_text(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .filter(|m| m.role == "user")
        .filter_map(|m| m.content.as_ref().and_then(|c| c.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_req(model: &str, content: &str) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(Value::String(content.to_string())),
                tool_calls: None,
            }],
            stream: false,
            tools: None,
        }
    }

    #[test]
    fn title_extracts_request_line() {
        let r = mk_req(
            "webchat-title",
            "헤더\n\n[이번 사용자 요청]\n로블록스 게임 스켈레톤 만들어줘\n\n[답변 요청]\n지시",
        );
        assert_eq!(title_from(&r), "로블록스 게임 스켈레톤 만들어줘");
    }

    #[test]
    fn title_uses_first_nonempty_line() {
        let r = mk_req("webchat-title", "\n\n  첫 줄 제목입니다  \n두 번째 줄");
        assert_eq!(title_from(&r), "첫 줄 제목입니다");
    }

    #[test]
    fn title_truncates_to_34_chars() {
        let r = mk_req("webchat-title", "가".repeat(80).as_str());
        assert_eq!(title_from(&r).chars().count(), 34);
    }

    #[test]
    fn title_empty_falls_back_to_default() {
        let r = mk_req("webchat-title", "[답변 요청]\n[답변 요청]");
        assert_eq!(title_from(&r), "새 대화");
    }
}

/// OpenAI 호환 채팅 완성 엔드포인트.
/// browser 모드 전용 — CDP로 로그인된 웹 채팅에 전송·응답 추출. (mock 경로는 제거됨)
pub async fn chat_completions(
    State(app): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    if app.cfg.automation.mode != "browser" {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "automation mode must be \"browser\" (the mock harness was removed)",
        )
            .into_response();
    }

    browser_completions(&app, &req).await
}

/// 전용 Chrome(9333)/ChatGPT(+Gemini) 상태 점검용 — 앱이 상태를 표시할 때 사용한다.
/// Chrome이 안 떠 있으면 chrome:false로 보고한다 (읽기 전용, 탭을 만들지 않음).
pub async fn probe_web(State(app): State<AppState>) -> Json<Value> {
    let engine = automation::engine::Engine::new(app.cfg.automation.cdp_port);
    let cgpt = engine.probe_site(automation::engine::Site::ChatGpt).await;
    let gem = engine.probe_site(automation::engine::Site::Gemini).await;
    let summarize = |name: &str, r: Result<(bool, String, String), automation::engine::EngineError>| {
        match r {
            Ok((composer, url, title)) => {
                let chrome_running = title != "chrome-not-running";
                json!({
                    "site": name,
                    "ok": composer,
                    "chrome": chrome_running,
                    "composer": composer,
                    "login_required": chrome_running && !composer,
                    "url": url,
                    "title": title,
                })
            }
            Err(e) => json!({
                "site": name,
                "ok": false,
                "chrome": true,
                "login_required": true,
                "error": e.to_string(),
            }),
        }
    };
    let chatgpt = summarize("chatgpt", cgpt);
    let gemini = summarize("gemini", gem);
    let chrome_up = chatgpt["chrome"].as_bool().unwrap_or(false) || gemini["chrome"].as_bool().unwrap_or(false);
    let cg_ok = chatgpt["ok"].as_bool().unwrap_or(false);
    let gm_ok = gemini["ok"].as_bool().unwrap_or(false);
    Json(json!({
        "ok": cg_ok || gm_ok,
        "chrome_running": chrome_up,
        "login_required": chrome_up && !(cg_ok || gm_ok),
        "chrome_not_running": !chrome_up,
        "sites": [chatgpt, gemini],
    }))
}

/// 전용 Chrome을 팝업합니다 (로그인/캡차 해결용).
/// pid 쿼리를 주면 그 프로세스의 창을 화면 앞으로 꺼내고, 없으면 보이는 창으로 새로 띄운다.
pub async fn show_chrome(
    State(app): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    if let Some(pid) = q.get("pid").and_then(|s| s.parse::<u32>().ok()) {
        if super::automation::winhide::process_alive(pid) {
            let n = super::automation::winhide::show_chrome_windows_for_pid(pid);
            return Json(json!({
                "ok": n > 0,
                "action": if n > 0 { "shown" } else { "no-window" },
                "pid": pid,
            }));
        }
        return Json(json!({ "ok": false, "action": "not-running", "pid": pid }));
    }
    let engine = automation::engine::Engine::new(app.cfg.automation.cdp_port);
    match engine
        .show_chrome_visible(automation::engine::Site::ChatGpt)
        .await
    {
        Ok(pid) => Json(json!({ "ok": true, "action": "spawned", "pid": pid })),
        Err(e) if e.to_string().contains("이미 실행 중") => {
            Json(json!({ "ok": true, "action": "already-running" }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 현재 tool 턴의 결과에서 실패 징후를 찾는다. opencode tool 결과는 JSON이거나 일반 텍스트.
fn tool_text_looks_failed(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "\"success\": false",
        "\"ok\": false",
        "error",
        "failed",
        "failure",
        "exception",
        "permission denied",
        "denied",
        "not allowed",
        "거부",
        "실패",
        "오류",
    ]
    .iter()
    .any(|t| lower.contains(t))
}

fn tool_turn_summary(tool_txt: &str) -> (String, bool) {
    if tool_txt.trim().is_empty() {
        return (
            "도구가 실행되었습니다. 이어서 진행할 내용이 있으면 말씀해 주세요.".to_string(),
            true,
        );
    }
    if tool_text_looks_failed(tool_txt) {
        return (String::new(), false);
    }
    (
        "파일 작업이 완료되었습니다. 추가로 수정하거나 이어서 작업할 내용이 있으면 말씀해 주세요."
            .to_string(),
        true,
    )
}

/// 제목 생성 에이전트용 가짜 모델: opencode의 small_model("webchat/webchat-title")을
/// 여기서 즉시 축약 제목으로 응답한다. ChatGPT/Gemini를 호출하지 않아 쿼터를 쓰지 않는다.
fn title_from(req: &ChatRequest) -> String {
    let joined = req
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| msg_tool_text(m))
        .collect::<Vec<_>>()
        .join("\n");
    let mut t = joined;
    if let Some(i) = t.find("[이번 사용자 요청]") {
        t = t[i + "[이번 사용자 요청]".len()..].to_string();
    }
    if let Some(i) = t.find("[답변") {
        t = t[..i].to_string();
    }
    let line = t
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let clean: String = line.chars().take(34).collect();
    if clean.is_empty() {
        "새 대화".to_string()
    } else {
        clean
    }
}

fn msg_tool_text(m: &ChatMessage) -> String {
    match m.content.as_ref() {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .or_else(|| p.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// 도구 실행 결과를 받은 턴: 같은 내용을 또 생성하지 않도록 웹사이트를 재호출하지 않고
/// 마무리 텍스트로 짧게 응답한다 (토큰/쿼터 절약, Phase 2 간소화).
/// flatten이 현재 단일 턴 재전송 구조라 (assistant 히스토리 포함 X) 동일 답변이 반복되는 것을 방지.
fn ack_response(app: &AppState, req: &ChatRequest, content: &str) -> Response {
    let id = next_id();
    if req.stream {
        sse::stream_response(id, req.model.clone(), content.to_string(), vec![], "stop".to_string())
    } else {
        Json(json!({
            "id": id,
            "object": "chat.completion",
            "created": sse::now_secs(),
            "model": req.model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        }))
        .into_response()
    }
}

/// 이전 tool 결과 직후(마지막 메시지가 role=tool): 마무리 조건이 되면 ACK,
/// 아니면 None을 반환해서 ChatGPT에게 이어서 보내게 한다 (read/grep/question 루프).
/// write/edit/shell만 있었고 결과에 실패 징후가 없으면 엔드턴 ACK.
/// 실패 징후가 있으면 None을 반환해서 모델이 다시 고치도록 한다 (MAX_TOOL_TURNS가 무한 루프를 막는다).
fn ack_response_for_tools(app: &AppState, req: &ChatRequest) -> Option<Response> {
    let tool_count = tool_turn_count(req);
    if tool_count == 0 {
        return None;
    }
    if tool_count >= MAX_TOOL_TURNS {
        return Some(ack_response(
            app,
            req,
            "도구 호출 횟수가 많아서 여기까지 실행했습니다. 필요한 부분을 확인하고 알려주세요.",
        ));
    }
    let names = last_tool_names(req);
    let all_terminal =
        !names.is_empty() && names.iter().all(|n| TERMINAL_TOOLS.contains(&n.as_str()));
    if !all_terminal {
        return None;
    }
    // 실패 징후가 있으면 모델에게 재시도 기회를 준다 (무한 루프는 MAX_TOOL_TURNS로 제한).
    if let Some(last) = req.messages.last() {
        let txt = msg_tool_text(last);
        if tool_text_looks_failed(&txt) {
            return None;
        }
    }
    Some(ack_response(app, req, "도구 실행이 완료되었습니다."))
}

/// 마지막 assistant 메시지의 tool_calls에서 도구 이름 목록을 뽑는다.
fn last_tool_names(req: &ChatRequest) -> Vec<String> {
    req.messages
        .iter()
        .rev()
        .find_map(|m| {
            if m.role == "assistant" {
                m.tool_calls.as_ref().map(|tc| {
                    tc.as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| {
                                    v.get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|n| n.as_str())
                                        .map(str::to_string)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// 이 요청에서 도구 루프 턴 수 (role=tool 메시지 개수).
fn tool_turn_count(req: &ChatRequest) -> usize {
    req.messages.iter().filter(|m| m.role == "tool").count()
}

/// 마지막 도구 실행을 끝낸 것으로 간주할 도구들:
/// 파일 쓰기/수정처럼 "결과를 다시 모델에 보여줄 필요가 없는" 것들.
const TERMINAL_TOOLS: &[&str] = &["write", "edit", "shell"];

/// 도구 루프 안전 한도: 이 횟수를 넘기면 모델을 더 부르지 않고 마무리한다.
const MAX_TOOL_TURNS: usize = 10;

/// browser 모드: 실제 CDP 자동화 경로 (Phase 1~3).
async fn browser_completions(app: &AppState, req: &ChatRequest) -> Response {
    // 제목 생성 에이전트 요청은 ChatGPT를 호출하지 않고 즉시 축약 제목으로 응답한다
    // (opencode의 small_model = "webchat/webchat-title").
    if req.model == "webchat-title" {
        let title = title_from(req);
        tracing::info!("[title] 제목 요청 인터셉트 — \"{}\"", title);
        return ack_response(app, req, &title);
    }
    let id = next_id();
    let workspace =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let workspace_s = workspace.to_string_lossy().to_string();
    let archive_enabled = app.cfg.handover.archive;
    let arc = archive::Archive::new(&workspace_s);
    if archive_enabled {
        arc.record("USER", &user_text(req));
    }
    let n = arc.bump_counter();

    // 도구 실행 결과를 막 받은 턴(마지막 메시지가 tool):
    //   write/edit/shell 이면 엔드턴으로 ACK(웹챗 재호출 없음).
    //   read/grep/glob/question 이면 결과를 프롬프트에 넣고 모델을 계속 불러 작업을 이어간다.
    if let Some(ack) = ack_response_for_tools(app, req) {
        if archive_enabled {
            if let Some(last) = req.messages.last() {
                let tool_txt = msg_tool_text(last);
                arc.record("ASSISTANT", &tool_turn_summary(&tool_txt).0);
            }
        }
        return ack;
    }

    let prompt = match harness::flatten::build(req, &workspace, None) {
        Ok(p) => p,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, e).into_response();
        }
    };
    println!(
        "[browser] cwd={} prompt_chars={} msg_count={}",
        workspace_s,
        prompt.chars().count(),
        n
    );

    let engine = automation::engine::Engine::new(app.cfg.automation.cdp_port);
    // 모델 id에 "gemini"가 포함되면 Gemini로 초기 사이트를 결정한다
    let site = automation::engine::Site::from_model(&req.model);

    // 최신 user 메시지로 수동 인수인계 트리거 여부를 먼저 판정한다.
    // (기존: 원본 요청을 먼저 보낸 뒤 체인을 또 보내 이중 소비되던 문제)
    let latest_user_hoist = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(msg_tool_text)
        .unwrap_or_default();
    let manual_flag = app.manual_handover.swap(false, Ordering::SeqCst);
    let forced_handover = manual_flag || latest_user_hoist.contains("지금 인수인계");

    let (mut outcome, mut active_site) = if forced_handover && app.cfg.handover.mode == "auto" {
        let snap = harness::flatten::snapshot(&workspace);
        let tool_summary: String = req
            .messages
            .iter()
            .filter(|m| m.role == "tool")
            .map(msg_tool_text)
            .collect::<Vec<_>>()
            .join("\n");
        let pack = automation::handover::build_pack(
            &workspace_s,
            &user_text(req),
            &tool_summary,
            &snap,
        );
        if archive_enabled {
            arc.record("HANDOVER", &pack);
        }
        let fname = format!("handover-{id}.md");
        if archive_enabled {
            let saved = arc.write_artifact(&fname, &pack);
            println!("[browser] handover pack saved to {saved:?} (site={site:?})");
        } else {
            println!("[browser] handover pack built in memory (archive off) (site={site:?})");
        }
        arc.log_handover_event(
            "handover_started",
            &format!("site={site:?} limit_hit=false manual=true msgs={}", req.messages.len()),
        );
        match run_handover_chain(&engine, app, &arc, site, &pack).await {
            Ok(pair) => {
                arc.log_handover_event("chain_ok", &format!("site={:?}", pair.1));
                pair
            }
            Err(e) => {
                arc.log_handover_event("chain_failed", &format!("{e} — 원본 요청으로 폴백"));
                (send_normal(&engine, &prompt, site, app).await, site)
            }
        }
    } else {
        (send_normal(&engine, &prompt, site, app).await, site)
    };

    // 테스트 훅: CODEBRIDGE_FORCE_LIMIT_ONCE=1로 시작하면 한도 배너를 1회 강제로 흉내낸다
    // (수동 인수인계와 같은 one-shot 패턴 — 자동 체인이 실제로 도는지 검증용).
    if app.force_limit_hit.swap(false, Ordering::SeqCst) {
        outcome.limit_hit = true;
        println!("[browser] CODEBRIDGE_FORCE_LIMIT_ONCE — 한도 배너를 강제로 흉내냅니다");
    }

    let mut raw = outcome.text;
    let mut blocks = outcome.code_blocks;
    let mut truncated = outcome.truncated;

    // 잘린 응답이면 "이어서 계속"을 1회 요청 (Phase 2, 쿼터 제한).
    // DOM 블록이든 raw든 truncated면 파일 승격은 하지 않는다.
    if truncated || harness::flatten::has_unterminated_fence(&raw) {
        println!("[browser] truncated/unterminated response; sending continuation (truncated={truncated})");
        let cont_prompt = "이어서 계속 써 주세요. 파일을 나누어 출력해도 됩니다.";
        match engine.send_and_await_site(cont_prompt, active_site).await {
            Ok(o2) => {
                raw.push_str("\n\n(이어서 계속 받은 내용)\n\n");
                raw.push_str(&o2.text);
                blocks.extend(o2.code_blocks);
                truncated = o2.truncated;
            }
            Err(e) => {
                raw.push_str(&format!("\n\n(이어서 계속 요청 실패: {e})"));
            }
        }
    }

    // 한도 배너 감지 시에만 자동 인수인계 (수동 트리거는 전송 전에 이미 처리됨)
    if outcome.limit_hit {
        arc.log_handover_event(
            "handover_started",
            &format!("site={active_site:?} limit_hit={} msgs={}", outcome.limit_hit, req.messages.len()),
        );
        let snap = harness::flatten::snapshot(&workspace);
        let tool_summary: String = req
            .messages
            .iter()
            .filter(|m| m.role == "tool")
            .map(msg_tool_text)
            .collect::<Vec<_>>()
            .join("\n");
        let pack = automation::handover::build_pack(
            &workspace_s,
            &user_text(req),
            &tool_summary,
            &snap,
        );
        if archive_enabled {
            arc.record("HANDOVER", &pack);
        }
        let fname = format!("handover-{id}.md");
        if archive_enabled {
            let saved = arc.write_artifact(&fname, &pack);
            println!("[browser] handover pack saved to {saved:?} (site={active_site:?})");
        } else {
            println!("[browser] handover pack built in memory (archive off) (site={active_site:?})");
        }

        if app.cfg.handover.mode == "auto" {
            match run_handover_chain(&engine, app, &arc, active_site, &pack).await {
                Ok((o2, site2)) => {
                    raw = o2.text;
                    blocks = o2.code_blocks;
                    truncated = o2.truncated;
                    active_site = site2;
                    arc.log_handover_event("chain_ok", &format!("site={site2:?}"));
                }
                Err(e) => {
                    raw.push_str(&format!("\n\n(인수인계 체인 실패: {e})"));
                }
            }
        }
    }

    if truncated {
        // 부분 응답: 불완전한 코드가 파일로 승격되지 않도록 명시 (사용자에게 이어가라고 안내)
        raw.push_str(
            "\n\n(참고: 응답이 도중에 끊겼습니다. 파일 쓰기는 건너뛰었습니다 — \"이어서 계속\"이라고 보내면 이어집니다.)",
        );
    }
    // 설정 "긴 대화 안내 숨기기"(CODEBRIDGE_NO_CONTEXT_NOTE=1)면 N개 쌓였습니다 표기 생략
    let hide_context_note = std::env::var("CODEBRIDGE_NO_CONTEXT_NOTE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !hide_context_note && n >= app.cfg.handover.thread_soft_limit {
        raw.push_str(&format!(
            "\n\n(참고: 이 스레드에 메시지가 {n}개 쌓였습니다. 한도 근접 시 자동 인수인계되며, 수동으로는 \"지금 인수인계\"라고 요청하세요.)"
        ));
    }

    if archive_enabled {
        arc.record("ASSISTANT", &raw);
    }

    // 코드 블록이 DOM에서 추출되면 그것을 관대 파서에 넣는다 (raw는 렌더된 텍스트라 ```가 없음).
    // 추출 실패 시 raw 자체를 파싱하는 폴백을 유지한다.
    // 잘린 응답은 불완전한 코드일 수 있으므로 파일 승격을 하지 않는다.
    let parse_src = if blocks.is_empty() {
        raw.clone()
    } else {
        blocks
            .iter()
            .map(|c| format!("```lua\n{c}\n```"))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let files = if truncated {
        Vec::new()
    } else {
        translate::lenient::parse(&parse_src)
    };
    let (write_calls, issues) = translate::lenient::to_tool_calls(&files);
    let tool_promo = if truncated {
        translate::lenient::ToolPromotion::default()
    } else {
        translate::lenient::extract_tool_calls_in(&raw, Some(workspace.as_path()))
    };
    let mut calls = write_calls;
    calls.extend(tool_promo.calls);
    let mut issues = issues;
    issues.extend(tool_promo.issues);
    let question_lines = tool_promo.question_lines;
    let mut text = raw.clone();
    if !issues.is_empty() {
        // UI가 파싱할 수 있는 마커. MessageRow가 `codebridge:tool-failed:N` 라인을
        // 빨간색 안내 배지로 렌더한다.
        text.push_str(&format!("\n\ncodebridge:tool-failed:{}\n", issues.len()));
    }
    if !question_lines.is_empty() {
        // CODEBRIDGE_QUESTION: {...} 마커를 텍스트에 그대로 싣는다 — UI가 파싱해 카드로 렌더한다.
        text.push_str("\n\n");
        for q in &question_lines {
            text.push_str(q);
            text.push('\n');
        }
    }
    if text.trim().is_empty() {
        text = "응답이 비어 있습니다 — 모델이 출력을 생성하지 못했습니다. 다시 시도해 주세요.".to_string();
    }
    let finish = if calls.is_empty() { "stop" } else { "tool_calls" }.to_string();

    println!(
        "[browser] model={} stream={} msgs={} calls={} id={id}",
        req.model,
        req.stream,
        req.messages.len(),
        calls.len()
    );

    if req.stream {
        sse::stream_response(id, req.model.clone(), text, calls, finish)
    } else {
        Json(json!({
            "id": id,
            "object": "chat.completion",
            "created": sse::now_secs(),
            "model": req.model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": text,
                    "tool_calls": calls
                },
                "finish_reason": finish
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        }))
        .into_response()
    }
}

/// 일반 전송 한 번을 정규화된 SendOutcome으로 돌려준다.
/// 실패(Chrome 없음 등)도 문자열 응답으로 변환되므로 Err 없이 반환된다.
async fn send_normal(
    engine: &automation::engine::Engine,
    prompt: &str,
    site: automation::engine::Site,
    app: &AppState,
) -> automation::engine::SendOutcome {
    match engine.send_and_await_site(prompt, site).await {
        Ok(o) => o,
        Err(e) => automation::engine::SendOutcome {
            text: format!(
                "(브릿지 오류) {}에 보내지 못했습니다: {e}. Chrome(전용 프로필, CDP {})과 로그인 상태를 확인해 주세요.",
                match site {
                    automation::engine::Site::ChatGpt => "ChatGPT",
                    automation::engine::Site::Gemini => "Gemini",
                },
                app.cfg.automation.cdp_port
            ),
            limit_hit: false,
            code_blocks: Vec::new(),
            truncated: false,
        },
    }
}

/// 인수인계 체인 (same_site_new_chat → gemini). 성공 시 (응답, 해당 사이트).
/// 전부 실패하면 Err(마지막 오류)를 반환하고 호출자가 폴백을 결정한다.
async fn run_handover_chain(
    engine: &automation::engine::Engine,
    app: &AppState,
    arc: &archive::Archive,
    site: automation::engine::Site,
    pack: &str,
) -> Result<(automation::engine::SendOutcome, automation::engine::Site), String> {
    let order = &app.cfg.handover.order;
    println!("[browser] running handover chain: {order:?}");
    let mut last_err = String::new();
    for step in order {
        match step.as_str() {
            "same_site_new_chat" => {
                match engine.rotate_new_chat_site(site).await {
                    Ok(_) => {
                        arc.reset_counter();
                        let retry = format!("{pack}\n\n위 요청을 이어서 진행해 주세요.");
                        match engine.send_and_await_site(&retry, site).await {
                            Ok(o2) => {
                                if o2.limit_hit {
                                    arc.log_handover_event("same_site_limit", "새 채팅에서도 한도 유지");
                                    last_err = "같은 사이트 새 채팅에서도 한도 유지".into();
                                    continue;
                                }
                                arc.log_handover_event("same_site_ok", "같은 사이트 새 채팅에서 이어감");
                                return Ok((o2, site));
                            }
                            Err(e) => {
                                arc.log_handover_event("same_site_error", &e.to_string());
                                last_err = format!("재전송 실패: {e}");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        arc.log_handover_event("rotate_error", &e.to_string());
                        last_err = format!("새 채팅 교체 실패: {e}");
                        continue;
                    }
                }
            }
            "gemini" => {
                let gemini_site = automation::engine::Site::Gemini;
                match engine.rotate_new_chat_site(gemini_site).await {
                    Ok(_) => {
                        arc.reset_counter();
                        let retry = format!("{pack}\n\n위 요청을 이어서 진행해 주세요.");
                        match engine.send_and_await_site(&retry, gemini_site).await {
                            Ok(o2) => {
                                if o2.limit_hit {
                                    arc.log_handover_event("gemini_limit", "Gemini 한도 배너 — 체인 중단");
                                    last_err = "Gemini 재전송에서도 한도 배너 — 체인 중단".into();
                                    break; // Gemini가 마지막 단계 — 폴백 없음
                                }
                                arc.log_handover_event("gemini_ok", "Gemini로 이어감");
                                return Ok((o2, gemini_site));
                            }
                            Err(e) => {
                                last_err = format!("Gemini 재전송 실패: {e}");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        last_err = format!("Gemini 새 채팅 교체 실패: {e}");
                        arc.log_handover_event("gemini_rotate_error", &e.to_string());
                        continue;
                    }
                }
            }
            other => {
                last_err = format!("미지의 order 항목: {other}");
            }
        }
    }
    arc.log_handover_event("chain_failed", &last_err);
    Err(last_err)
}
