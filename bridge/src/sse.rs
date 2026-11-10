use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio_stream::wrappers::ReceiverStream;

/// 채팅 완성 응답을 OpenAI 호환 SSE 스트리밍으로 흉내 낸다.
/// 웹 채팅은 사실상 일괄 수신이므로, 완성 텍스트를 작은 덩어리로 쪼개 전송한다.
pub fn stream_response(
    id: String,
    model: String,
    content: String,
    tool_calls: Vec<Value>,
    finish: String,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(128);

    tokio::spawn(async move {
        let created = now_secs();

        // 1) role 델타
        let role = json!({"role": "assistant", "content": ""});
        let _ = tx.send(Ok(delta_chunk(&id, &model, created, role, None))).await;

        // 2) content 델타 (90자 단위, 문자 경계 안전)
        for part in split_chars(&content, 90) {
            let d = json!({ "content": part });
            let _ = tx
                .send(Ok(delta_chunk(&id, &model, created, d, None)))
                .await;
        }

        // 3) tool_calls 델타 (OpenAI 도구 호출 스트리밍 형식)
        for (i, tc) in tool_calls.iter().enumerate() {
            let name = tc["function"]["name"].as_str().unwrap_or("");
            let args = tc["function"]["arguments"].as_str().unwrap_or("");
            let head = json!({
                "tool_calls": [{
                    "index": i,
                    "id": tc["id"].clone(),
                    "type": "function",
                    "function": { "name": name, "arguments": "" }
                }]
            });
            let _ = tx.send(Ok(delta_chunk(&id, &model, created, head, None))).await;
            for part in split_chars(args, 180) {
                let d = json!({"tool_calls":[{"index": i, "function": {"arguments": part}}]});
                let _ = tx
                    .send(Ok(delta_chunk(&id, &model, created, d, None)))
                    .await;
            }
        }

        // 4) 종료 델타 + [DONE]
        let _ = tx
            .send(Ok(delta_chunk(&id, &model, created, json!({}), Some(finish.as_str()))))
            .await;
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
        // tx가 drop되며 스트림 자연 종료
    });

    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
        .into_response()
}

use std::convert::Infallible;

fn delta_chunk(id: &str, model: &str, created: i64, delta: Value, finish: Option<&str>) -> Event {
    let v = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish
        }]
    });
    Event::default().data(v.to_string())
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 문자 경계를 깨지 않고 n자 단위로 쪼갠다 (한글 안전).
pub fn split_chars(text: &str, n: usize) -> Vec<String> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(n)
        .map(|c| c.iter().collect::<String>())
        .collect()
}
