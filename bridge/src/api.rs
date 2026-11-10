use std::sync::{
    atomic::{AtomicU64, Ordering},
};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fixture;
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

/// OpenAI 호환 모델 목록. (Phase 0: mock만, Phase 1+: 실제 자동화 모델 추가)
pub async fn list_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": "webchat-mock",
                "object": "model",
                "created": 1700000000,
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
pub async fn chat_completions(
    State(app): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    if app.cfg.automation.mode != "mock" {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "automation engine is Phase 1+; set [automation] mode=\"mock\"",
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
