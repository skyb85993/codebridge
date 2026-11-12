//! 컨텍스트 평탄화 (Phase 1 핵심 1):
//! opencode messages(system/user/assistant/tool) → 웹 채팅용 단일 프롬프트 1개.
//! 우선순위: 규약 > 최근 user > 도구 오류 > 도구 결과 > 스냅샷 — 예산 초과 시 뒤에서부터 자른다.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{ChatMessage, ChatRequest};
use serde_json::Value;

const FENCE_HEADER: &str = include_str!("../../../prompts/fence_convention.md");
const DEFAULT_BUDGET_CHARS: usize = 32_000;
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
    let header = format!("{}\n", FENCE_HEADER.trim());

    // 새 구조: 헤더(규약) → 대화 기록(user/assistant/tool) → 워크스페이스 → 마지막 지시
    // 모델이 직전에 어떤 도구를 호출했는지, 결과가 뭐였는지 인지한 채로 다음 행동을 고르게 한다.
    let mut convo = String::new();
    for m in &req.messages {
        match m.role.as_str() {
            "user" => {
                let txt = msg_text(m);
                convo.push_str("사용자: ");
                convo.push_str(txt.trim());
                convo.push('\n');
            }
            "assistant" => {
                // 이 assistant 턴의 텍스트
                let txt = msg_text(m);
                let t = txt.trim();
                if !t.is_empty() {
                    convo.push_str("당신: ");
                    convo.push_str(t);
                    convo.push('\n');
                }
                // 이 assistant 턴의 도구 호출을 KV 형식으로 다시 쓰기
                if let Some(tcs) = &m.tool_calls {
                    if let Some(a) = tcs.as_array() {
                        for tc in a {
                            if let Some(call_line) = tool_call_line(tc) {
                                convo.push_str(&call_line);
                                convo.push('\n');
                            }
                        }
                    }
                }
            }
            "tool" => {
                let txt = msg_text(m);
                let t = txt.trim();
                convo.push_str("도구:\n");
                if t.chars().count() > 6_000 {
                    let mut s: String = t.chars().take(6_000).collect();
                    s.push_str("\n…(생략)\n");
                    convo.push_str(&s);
                } else {
                    convo.push_str(t);
                }
                convo.push('\n');
            }
            _ => {}
        }
        if convo.chars().count() > budget {
            break;
        }
    }

    // 워크스페이스 파일 목록은 작게 유지 — 가장 중요한 건 대화 기록
    let snapshot = snapshot(workspace_root);
    let mut prompt = String::new();
    prompt.push_str(&header);
    prompt.push_str("\n## 대화 기록\n");
    prompt.push_str(convo.trim_end());
    prompt.push_str("\n\n[현재 워크스페이스 파일]\n");
    prompt.push_str(snapshot.trim_end());
    prompt.push_str("\n\n## 당신의 차례입니다\n위 대화를 이어받아 TOOL 한 줄만 출력하세요. 설명 없이, 인사 없이.\n");

    if let Ok(out) = std::env::var("BRIDGE_DEBUG_PROMPT") {
        let _ = std::fs::write(&out, &prompt);
    }
    Ok(prompt)
}

/// 도구 호출 JSON을 `TOOL name key=value ...` 한 줄로 다시 쓰는 헬퍼.
/// opencode가 보낸 arguments는 JSON 문자열이라 파싱해서 KV 형식으로 변환해 준다.
fn tool_call_line(tc: &Value) -> Option<String> {
    let f = tc.get("function")?;
    let name = f.get("name")?.as_str()?;
    let args_str = f.get("arguments")?.as_str()?;
    let args: Value = serde_json::from_str(args_str).ok()?;
    let obj = args.as_object()?;
    let mut parts = vec![format!("TOOL {name}")];
    for (k, v) in obj {
        // 값에 공백이 있으면 항상 따옴표로 감싸 준다
        let val = match v {
            Value::String(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                if escaped.contains(char::is_whitespace) {
                    format!("\"{escaped}\"")
                } else {
                    escaped
                }
            }
            other => other.to_string(),
        };
        parts.push(format!("{k}={val}"));
    }
    Some(parts.join(" "))
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
                tool_calls: None,
            },
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!("중요한 작업: 게임 만들어줘")),
                tool_calls: None,
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
        assert!(prompt.contains("에이전트"));
        assert!(prompt.contains("main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_produces_header_record_and_tail() {
        let msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!("중요한 작업: 만들기")),
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::json!("TOOL read path=src/main.rs")),
                tool_calls: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: Some(serde_json::json!("fn main() {}")),
                tool_calls: None,
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
        assert!(prompt.contains("에이전트"));
        assert!(prompt.contains("## 대화 기록"));
        assert!(prompt.contains("사용자:"));
        assert!(prompt.contains("TOOL read path=src/main.rs"));
        assert!(prompt.contains("도구:"));
        assert!(prompt.contains("당신의 차례입니다"));
        let _ = std::fs::remove_dir_all(&dir);
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
            tool_calls: None,
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

