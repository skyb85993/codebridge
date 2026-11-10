use std::sync::{
    atomic::{AtomicU64, Ordering},
};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::archive;
use crate::automation;
use crate::fixture;
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
                "id": "webchat-mock",
                "object": "model",
                "created": 1700000001,
                "owned_by": "codebridge"
            },
            {
                "id": "webchat-gemini",
                "object": "model",
                "created": 1700000002,
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

/// OpenAI 호환 채팅 완성 엔드포인트.
/// Phase 0: mock 모드 — 고정 응답(마크다운 코드펜스)을 관대 파서로 번역해
/// write tool_calls로 승격까지 왕복 전체를 검증한다.
/// Phase 1+: browser 모드 — CDP로 로그인된 ChatGPT에 전송·응답 추출.
pub async fn chat_completions(
    State(app): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    if app.cfg.automation.mode == "browser" {
        return browser_completions(&app, &req).await;
    }
    if app.cfg.automation.mode != "mock" {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "usage: [automation] mode = \"mock\" | \"browser\"",
        )
            .into_response();
    }

    // 루프 가드(모의 모드 전용): 도구 실행 실패 → 동일 요청 재시도 반복을 즉시 끊는다.
    if req.messages.len() > 5 {
        let id = next_id();
        println!("[mock] loop-guard trip msgs={} id={id}", req.messages.len());
        let content = "(브릿지 루프 가드) 도구 실행 실패가 반복되어 여기서 중단합니다. 원인 수정 후 다시 실행해 주세요.";
        return if req.stream {
            sse::stream_response(
                id,
                req.model.clone(),
                content.to_string(),
                vec![],
                "stop".to_string(),
            )
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
        };
    }

    // [진단] opencode가 요청에 담아 보낸 도구 정의 이름을 덤프한다.
    if let Some(Value::Array(list)) = &req.tools {
        let names: Vec<&str> = list
            .iter()
            .map(|t| {
                let f = t.get("function").unwrap_or(t);
                f.get("name").and_then(|n| n.as_str()).unwrap_or("?")
            })
            .collect();
        println!("[mock] definitions({}) = {}", list.len(), names.join(","));
    } else {
        println!("[mock] definitions = <none sent>");
    }

    let task = user_text(&req);
    let preview: String = task.chars().take(120).collect();

    // opencode 내부 요청(대화 제목 생성 등)에는 도구 호출 없이 짧은 텍스트만 반환한다.
    let title_request = task.contains("Reply with only the title");

    // 진단 모드: [INTROSPECT] 요청에는 여러 도구를 섞어 호출해 가용성을 관찰한다.
    let (tool_calls, content, finish): (Vec<Value>, String, &'static str) =
        if req.messages.iter().any(|m| m.role == "tool") {
            // 도구 실행 결과를 받은 턴: 같은 호출을 반복하지 않고 텍스트로 마무리
            (vec![], fixture::ACK.to_string(), "stop")
        } else if title_request {
            (
                vec![],
                "CodeBridge 모의 세션".to_string(),
                "stop",
            )
        } else if task.contains("[INTROSPECT]") {
            (
                vec![
                    json!({
                        "id": "callprobe_read", "type": "function",
                        "function": { "name": "read", "arguments": "{\"path\":\"README.md\"}" }
                    }),
                    json!({
                        "id": "callprobe_glob", "type": "function",
                        "function": { "name": "glob", "arguments": "{\"pattern\":\"*.toml\"}" }
                    }),
                    json!({
                        "id": "callprobe_edit", "type": "function",
                        "function": {
                            "name": "edit",
                            "arguments": "{\"path\":\"README.md\",\"oldString\":\"# CodeBridge\",\"newString\":\"# CodeBridge (probe)\"}"
                        }
                    }),
                    json!({
                        "id": "callprobe_write", "type": "function",
                        "function": { "name": "write", "arguments": "{\"path\":\"Probe/probe.txt\",\"content\":\"probe\"}" }
                    }),
                ],
                "(introspect) 도구 가용성 점검: read / glob / edit / write 를 하나씩 호출합니다.".to_string(),
                "tool_calls",
            )
        } else {
            let reply = fixture::parts();
            let files = translate::lenient::parse(&reply.raw);
            let (calls, issues) = translate::lenient::to_tool_calls(&files);
            let mut text = format!("{}{}", reply.intro, reply.outro);
            if !issues.is_empty() {
                text.push_str(&format!(
                    "\n\n(안내: 경로 주석이 없거나 불완전(잘림 등)한 코드 블록 {}개는 저장 대상에서 제외했습니다.)",
                    issues.len()
                ));
            }
            let f = if calls.is_empty() { "stop" } else { "tool_calls" };
            (calls, text, f)
        };
    let id = next_id();

    println!(
        "[mock] model={} stream={} msgs={} calls={} id={id} task=\"{preview}\"",
        req.model,
        req.stream,
        req.messages.len(),
        tool_calls.len()
    );

    if req.stream {
        sse::stream_response(id, req.model.clone(), content, tool_calls, finish.to_string())
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
                    "content": content,
                    "tool_calls": tool_calls
                },
                "finish_reason": finish
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        }))
        .into_response()
    }
}

/// 전용 Chrome(9333)/ChatGPT(+Gemini) 상태 점검용 — 앱이 로그인 여부를 표시할 때 사용한다.
pub async fn probe_web(State(app): State<AppState>) -> Json<Value> {
    let engine = automation::engine::Engine::new(app.cfg.automation.cdp_port);
    let cgpt = engine.probe_site(automation::engine::Site::ChatGpt).await;
    let gem = engine.probe_site(automation::engine::Site::Gemini).await;
    let summarize = |name: &str, r: Result<(bool, String, String), automation::engine::EngineError>| {
        match r {
            Ok((composer, url, title)) => json!({
                "site": name,
                "ok": true,
                "composer": composer,
                "login_required": !composer,
                "url": url,
                "title": title,
            }),
            Err(e) => json!({
                "site": name,
                "ok": false,
                "login_required": true,
                "error": e.to_string(),
            }),
        }
    };
    let chatgpt = summarize("chatgpt", cgpt);
    let gemini = summarize("gemini", gem);
    let cg_ok = chatgpt["ok"].as_bool().unwrap_or(false) && !chatgpt["login_required"].as_bool().unwrap_or(true);
    let gm_ok = gemini["ok"].as_bool().unwrap_or(false) && !gemini["login_required"].as_bool().unwrap_or(true);
    Json(json!({
        "ok": cg_ok || gm_ok,
        "login_required": !(cg_ok || gm_ok),
        "sites": [chatgpt, gemini],
    }))
}

fn msg_tool_text(m: &ChatMessage) -> String {
    m.content
        .as_ref()
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
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

/// browser 모드: 실제 CDP 자동화 경로 (Phase 1~3).
async fn browser_completions(app: &AppState, req: &ChatRequest) -> Response {
    let id = next_id();
    let workspace =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let workspace_s = workspace.to_string_lossy().to_string();
    let arc = archive::Archive::new(&workspace_s);
    arc.record("USER", &user_text(req));
    let n = arc.bump_counter();

    // 도구 결과를 받은 턴: 웹사이트를 재호출하지 않고 마무리
    if req.messages.iter().any(|m| m.role == "tool") {
        let content = "파일 작업이 완료되었습니다. 추가로 수정하거나 이어서 작업할 내용이 있으면 말씀해 주세요.";
        arc.record("ASSISTANT", content);
        return ack_response(app, req, content);
    }

    let prompt = harness::flatten::build(req, &workspace, None);
    // DEBUG: 실제 전송되는 프롬프트를 파일로 덤프 (패브를 위한 임시 로그)
    let _ = std::fs::write(
        std::path::PathBuf::from(std::env::temp_dir()).join("codebridge_flatten_debug.txt"),
        &prompt,
    );
    println!(
        "[browser] cwd={} prompt_chars={} msg_count={}",
        workspace_s,
        prompt.chars().count(),
        n
    );

    let engine = automation::engine::Engine::new(app.cfg.automation.cdp_port);
    // 모델 id에 "gemini"가 포함되면 Gemini로 초기 사이트를 결정한다
    let site = automation::engine::Site::from_model(&req.model);
    let outcome = match engine.send_and_await_site(&prompt, site).await {
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
        },
    };
    let mut raw = outcome.text;
    let mut blocks = outcome.code_blocks;
    let mut active_site = site;

    // 응답이 잘렸으면(raw 기준) 한 번만 "이어서 계속"을 요청한다 (Phase 2, 쿼터 제한)
    if harness::flatten::has_unterminated_fence(&raw) {
        println!("[browser] unterminated fence detected; sending continuation");
        let cont_prompt = "이어서 계속 써 주세요. 파일을 나누어 출력해도 됩니다.";
        match engine.send_and_await_site(cont_prompt, active_site).await {
            Ok(o2) => {
                raw.push_str("\n\n(이어서 계속 받은 내용)\n\n");
                raw.push_str(&o2.text);
                blocks.extend(o2.code_blocks);
            }
            Err(e) => {
                raw.push_str(&format!("\n\n(이어서 계속 요청 실패: {e})"));
            }
        }
    }

    // 한도 배너 감지 또는 사용자 수동 트리거("지금 인수인계") → 인수인계 팩 → 체인 순서로 재전송
    let manual_handover = user_text(req).contains("지금 인수인계");
    if outcome.limit_hit || manual_handover {
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
        arc.record("HANDOVER", &pack);
        let fname = format!("handover-{id}.md");
        let saved = arc.write_artifact(&fname, &pack);
        println!("[browser] handover pack saved to {saved:?} (site={active_site:?})");

        if app.cfg.handover.mode == "auto" {
            let order = &app.cfg.handover.order;
            println!("[browser] running handover chain: {order:?}");
            let mut chain_done = false;
            let mut last_err = String::new();
            for step in order {
                match step.as_str() {
                    "same_site_new_chat" => {
                        match engine.rotate_new_chat_site(active_site).await {
                            Ok(_) => {
                                arc.reset_counter();
                                let retry =
                                    format!("{pack}\n\n위 요청을 이어서 진행해 주세요.");
                                match engine.send_and_await_site(&retry, active_site).await {
                                    Ok(o2) => {
                                        if o2.limit_hit {
                                            last_err = "같은 사이트 새 채팅에서도 한도 유지".into();
                                            continue; // 체인 다음 단계
                                        }
                                        raw = o2.text;
                                        blocks = o2.code_blocks;
                                        chain_done = true;
                                        break;
                                    }
                                    Err(e) => {
                                        last_err = format!("재전송 실패: {e}");
                                        continue;
                                    }
                                }
                            }
                            Err(e) => {
                                last_err = format!("새 채팅 교체 실패: {e}");
                                continue;
                            }
                        }
                    }
                    "gemini" => {
                        match engine.rotate_new_chat_site(automation::engine::Site::Gemini).await {
                            Ok(_) => {
                                active_site = automation::engine::Site::Gemini;
                                arc.reset_counter();
                                let retry =
                                    format!("{pack}\n\n위 요청을 이어서 진행해 주세요.");
                                match engine.send_and_await_site(&retry, active_site).await {
                                    Ok(o2) => {
                                        raw = o2.text;
                                        blocks = o2.code_blocks;
                                        chain_done = true;
                                        break;
                                    }
                                    Err(e) => {
                                        last_err = format!("Gemini 재전송 실패: {e}");
                                        continue;
                                    }
                                }
                            }
                            Err(e) => {
                                last_err = format!("Gemini 새 채팅 교체 실패: {e}");
                                continue;
                            }
                        }
                    }
                    other => {
                        last_err = format!("미지의 order 항목: {other}");
                    }
                }
            }
            if !chain_done {
                raw.push_str(&format!("\n\n(인수인계 체인 실패: {last_err})"));
            }
        }
    }

    if harness::flatten::has_unterminated_fence(&raw) {
        raw.push_str(
            "\n\n(참고: 응답이 잘렸을 수 있습니다 — 마지막 코드펜스가 닫히지 않았습니다. \"이어서 계속\"이라고 보내면 이어집니다.)",
        );
    }
    if n >= app.cfg.handover.thread_soft_limit {
        raw.push_str(&format!(
            "\n\n(참고: 이 스레드에 메시지가 {n}개 쌓였습니다. 한도 근접 시 자동 인수인계되며, 수동으로는 \"지금 인수인계\"라고 요청하세요.)"
        ));
    }

    arc.record("ASSISTANT", &raw);

    // 코드 블록이 DOM에서 추출되면 그것을 관대 파서에 넣는다 (raw는 렌더된 텍스트라 ```가 없음).
    // 추출 실패 시 raw 자체를 파싱하는 폴백을 유지한다.
    let parse_src = if blocks.is_empty() {
        raw.clone()
    } else {
        blocks
            .iter()
            .map(|c| format!("```lua\n{c}\n```"))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let files = translate::lenient::parse(&parse_src);
    let (calls, issues) = translate::lenient::to_tool_calls(&files);
    let mut text = raw.clone();
    if !issues.is_empty() {
        text.push_str(&format!(
            "\n\n(안내: 경로 주석이 없거나 불완전한 코드 블록 {}개는 저장 대상에서 제외했습니다.)",
            issues.len()
        ));
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
