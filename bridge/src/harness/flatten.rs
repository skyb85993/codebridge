//! 컨텍스트 평탄화 (Phase 1 핵심 1):
//! opencode messages(system/user/assistant/tool) → 웹 채팅용 단일 프롬프트 1개.
//! 우선순위: 규약 > 최근 user > 도구 오류 > 도구 결과 > 스냅샷 — 예산 초과 시 뒤에서부터 자른다.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{ChatMessage, ChatRequest};
use serde_json::Value;

const FENCE_HEADER: &str = include_str!("../../../prompts/fence_convention.md");
const DEFAULT_BUDGET_CHARS: usize = 12_000;
const MAX_SNAPSHOT_FILES: usize = 80;
const MAX_FILE_DEPTH: usize = 3;

/// 워크스페이스 파일 스냅샷 (경로+크기). 바이너리/빌드 산출물 제외.
pub fn snapshot(root: &Path) -> String {
    let mut out = String::new();
    let mut entries: Vec<(PathBuf, u64)> = Vec::new();
    collect(root, root, 0, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, size) in entries.iter().take(MAX_SNAPSHOT_FILES) {
        let _ = writeln!(out, "- {} ({})", rel.display(), human_size(*size));
    }
    out
}

fn collect(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    if out.len() >= MAX_SNAPSHOT_FILES || depth > MAX_FILE_DEPTH {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for ent in read.flatten() {
        if out.len() >= MAX_SNAPSHOT_FILES {
            return;
        }
        let name = ent.file_name();
        let name = name.to_string_lossy();
        // 제외: dotfiles, 빌드 산출물, 의존성
        if name.starts_with('.') {
            continue;
        }
        const SKIP: [&str; 8] =
            ["target", "node_modules", "dist", "build", "archives", ".chrome-profile", ".git", "out"];
        if SKIP.iter().any(|s| name == *s) {
            continue;
        }
        let path = ent.path();
        if ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect(root, &path, depth + 1, out);
        } else if let Ok(meta) = ent.metadata() {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push((rel, meta.len()));
        }
    }
}

fn human_size(n: u64) -> String {
    if n >= 1_048_576 {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// OpenAI 호환 content를 문자열로 정규화한다.
/// content는 문자열 또는 content parts 배열({"type":"text","text":"..."} 등)일 수 있다.
fn msg_text(m: &ChatMessage) -> String {
    match m.content.as_ref() {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                // {"type":"text","text":"..."} 우선, 문자열 part 자체도 허용
                p.get("text")
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .or_else(|| p.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// messages → 단일 프롬프트.
/// 우선순위: 규약 > 최근 user > 도구 결과 > 스냅샷 — 현재 요청은 절대 자르지 않는다.
/// 규약+현재 요청 자체가 예산을 넘으면 Err로 명시적으로 거부한다.
pub fn build(req: &ChatRequest, workspace_root: &Path, budget_chars: Option<usize>) -> Result<String, String> {
    let budget = budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    let header = format!("[파일 규약 헤더]\n{}", FENCE_HEADER.trim());
    let answer = "[답변 요청]\n위 규약에 따라 답변해 주세요.";
    let latest_user = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(msg_text)
        .unwrap_or_default();
    let user_sec = format!("[이번 사용자 요청]\n{latest_user}");

    let fixed_len = header.chars().count() + user_sec.chars().count() + answer.chars().count() + 8;
    if fixed_len > budget {
        return Err(format!(
            "사용자 요청이 너무 깁니다 ({fixed_len}자 > 예산 {budget}자). 요청을 나누어 보내 주세요."
        ));
    }

    let remaining = budget - fixed_len;

    // 도구 결과를 오래된 것부터 남은 예산에 맞게 접기
    let tool_results: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(msg_text)
        .collect();
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    for (i, tr) in tool_results.iter().enumerate().rev() {
        let cost = tr.chars().count() + 24;
        if used + cost > remaining {
            kept.push(format!("(…오래된 도구 결과 {}건은 예산 초과로 생략)", i + 1));
            break;
        }
        used += cost;
        kept.push(format!("[도구 결과 #{}]\n{}", i + 1, tr));
    }
    kept.reverse();

    // 스냅샷: 남은 공간에 맞게 행 단위로 자른다 (잘랐으면 표시)
    let snap_full = snapshot(workspace_root);
    let snap_room = remaining.saturating_sub(used);
    let snapshot = truncate_snapshot(&snap_full, snap_room);

    let mut sections: Vec<String> = Vec::new();
    sections.push(header);
    sections.push(user_sec);
    if !kept.is_empty() {
        sections.push(kept.join("\n\n"));
    }
    if !snapshot.trim().is_empty() {
        sections.push(format!("[현재 워크스페이스 파일]\n{snapshot}"));
    }
    sections.push(answer.to_string());

    Ok(sections.join("\n\n"))
}

fn truncate_snapshot(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for line in s.lines() {
        let cost = line.chars().count() + 1;
        if used + cost > max_chars {
            break;
        }
        out.push_str(line);
        out.push('\n');
        used += cost;
    }
    if out.is_empty() {
        out.push_str("(…워크스페이스 스냅샷은 예산 초과로 생략)\n");
    } else {
        out.push_str("(…스냅샷이 예산에 맞게 일부만 포함됨)\n");
    }
    out
}

/// 관대 파서에 넘기기 전, 웹 답변에서 코드펜스 잘림 여부를 미리 감지한다 (Phase 2).
pub fn has_unterminated_fence(text: &str) -> bool {
    let mut open = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            open = !open;
        }
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterminated_fence_detected() {
        assert!(has_unterminated_fence("```lua\nprint(1)"));
        assert!(!has_unterminated_fence("```lua\nprint(1)\n```"));
        assert!(!has_unterminated_fence("설명만 있습니다"));
    }

    #[test]
    fn build_contains_header_and_user_request() {
        let msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!("첫 번째 요청")),
            },
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!("중요한 작업: 게임 만들어줘")),
            },
        ];
        let req = ChatRequest {
            model: "mock".into(),
            messages: msgs,
            stream: false,
            tools: None,
        };
        let dir = std::env::temp_dir().join(format!("cb_flat_{}", std::process::id()));
        let out_dir = dir.join("src");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("main.rs"), b"fn main() {}").unwrap();
        let prompt = build(&req, &dir, Some(20_000)).unwrap();
        assert!(prompt.contains("중요한 작업"));
        assert!(prompt.contains("파일 출력 규약"));
        assert!(prompt.contains("main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_request_rejected_explicitly() {
        let big = "가".repeat(5000);
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: Some(serde_json::json!(big)),
        }];
        let req = ChatRequest {
            model: "mock".into(),
            messages: msgs,
            stream: false,
            tools: None,
        };
        let result = build(&req, std::path::Path::new("."), Some(200));
        assert!(result.is_err(), "oversized request must be rejected explicitly");
        let err = result.unwrap_err();
        assert!(err.contains("요청이 너무 깁니다"));
    }

    #[test]
    fn content_array_text_parts_extracted() {
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: Some(serde_json::json!([
                { "type": "text", "text": "첫 줄" },
                { "type": "image_url", "image_url": { "url": "https://example.invalid/x.png" } },
                { "type": "text", "text": "둘째 줄" }
            ])),
        }];
        let req = ChatRequest {
            model: "mock".into(),
            messages: msgs,
            stream: false,
            tools: None,
        };
        let prompt = build(&req, std::path::Path::new("."), Some(20_000)).unwrap();
        assert!(prompt.contains("첫 줄"));
        assert!(prompt.contains("둘째 줄"));
    }
}

