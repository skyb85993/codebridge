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
/// 직접 업로드할 파일 크기 상한 (ChatGPT 무료 첨부 제한을 고려)
pub const MAX_ATTACH_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// 한 번에 첨부할 최대 파일 수
pub const MAX_ATTACH_FILES: usize = 10;

/// flatten 결과 — 프롬프트 본문 + 직접 업로드할 파일(절대 경로).
/// read 도구로 읽은 파일이 업로드 대상이면 그 내용은 본문에서 빠지고
/// `(파일 첨부됨: <경로>)`로 대체된다 (flatten의 inline=false 경로).
pub struct FlattenResult {
    pub prompt: String,
    pub attach: Vec<PathBuf>,
}

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

/// opencode read 도구 호출의 path 인자를 못 읽으면 None.
/// opencode read 스키마는 {"path": "..."} (예전 버전은 filePath/file 도 사용).
fn read_tool_path_arg(tc: &Value) -> Option<String> {
    let obj = tc.get("function")?.get("arguments")?.as_str()?;
    let args: Value = serde_json::from_str(obj).ok()?;
    let o = args.as_object()?;
    o.get("path")
        .or_else(|| o.get("filePath"))
        .or_else(|| o.get("file"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 절대 경로로 만들고 업로드 가능한지(파일 존재·파일·크기 상한) 검사.
/// 불가하면 None — 그 경우 호출자는 인라인 폴백을 쓴다.
fn attachable_path(raw: &str, workspace_root: &Path) -> Option<PathBuf> {
    let p = Path::new(raw);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        workspace_root.join(p)
    };
    // canonicalize로 .. traversal 방지
    let canon = std::fs::canonicalize(&abs).ok()?;
    let meta = std::fs::metadata(&canon).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_ATTACH_FILE_BYTES {
        return None;
    }
    Some(canon)
}

fn flatten_for_tool(body: &str) -> String {
    let t = body.trim();
    if t.chars().count() > 6_000 {
        let mut s: String = t.chars().take(6_000).collect();
        s.push_str("\n…(생략)\n");
        s
    } else {
        t.to_string()
    }
}

/// messages → 단일 프롬프트 + 인라인 업로드 대상.
/// 우선순위: 규약 > 최근 user > 도구 결과 > 스냅샷 — 현재 요청은 절대 자르지 않는다.
/// 규약+현재 요청 자체가 예산을 넘으면 Err로 명시적으로 거부한다.
/// attach_reads = true면 read 도구로 읽은 파일을 직접 업로드하도록 대상만 모으고,
/// 그 자리에는 `(파일 첨부됨: <경로>)`로 대체한다. false면 인라인(기존 동작).
pub fn build(req: &ChatRequest, workspace_root: &Path, budget_chars: Option<usize>) -> Result<FlattenResult, String> {
    build_with(req, workspace_root, budget_chars, true)
}

/// attach_reads=false 폴백 — 업로드가 실패했을 때 내용을 인라인으로 넘긴다.
pub fn build_inline(
    req: &ChatRequest,
    workspace_root: &Path,
    budget_chars: Option<usize>,
) -> Result<FlattenResult, String> {
    build_with(req, workspace_root, budget_chars, false)
}

fn build_with(
    req: &ChatRequest,
    workspace_root: &Path,
    budget_chars: Option<usize>,
    attach_reads: bool,
) -> Result<FlattenResult, String> {
    let budget = budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    let header = format!("{}\n", FENCE_HEADER.trim());

    // 새 구조: 헤더(규약) → 대화 기록(user/assistant/tool) → 워크스페이스 → 마지막 지시
    // 모델이 직전에 어떤 도구를 호출했는지, 결과가 뭐였는지 인지한 채로 다음 행동을 고르게 한다.
    let mut convo = String::new();
    // assistant의 read 호출 순서에 맞춰, tool 메시지를 그 순서대로 대응시킨다
    let mut pending_tools: std::collections::VecDeque<Option<PathBuf>> = std::collections::VecDeque::new();
    let mut attach: Vec<PathBuf> = Vec::new();
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
                            if attach_reads {
                                pending_tools.push_back(read_tool_path_arg(tc).and_then(|raw| {
                                    (attach.len() + pending_tools.len() < MAX_ATTACH_FILES)
                                        .then(|| attachable_path(&raw, workspace_root))
                                        .flatten()
                                }));
                            } else {
                                pending_tools.push_back(None);
                            }
                        }
                    }
                }
            }
            "tool" => {
                let txt = msg_text(m);
                let t = txt.trim();
                let read_path = pending_tools.pop_front().flatten();
                if read_path.is_some() && attach_reads {
                    // 직접 업로드 — 본문에서 내용을 빼고 첨부로 넘긴다
                    let p = read_path.unwrap_or_default();
                    if attach.len() < MAX_ATTACH_FILES {
                        attach.push(p.clone());
                        convo.push_str("도구: (아래 파일로 첨부됨)\n");
                        convo.push_str(&format!("- {}\n", p.display()));
                    } else {
                        convo.push_str("도구:\n");
                        convo.push_str(&flatten_for_tool(t));
                        convo.push('\n');
                    }
                } else {
                    convo.push_str("도구:\n");
                    convo.push_str(&flatten_for_tool(t));
                    convo.push('\n');
                }
            }
            _ => {}
        }
        if convo.chars().count() > budget {
            break;
        }
    }

    let effort = std::env::var("CODEBRIDGE_REASONING_EFFORT")
        .unwrap_or_else(|_| "medium".to_string())
        .to_lowercase();
    let effort_line = match effort.as_str() {
        "low" => "- 가능한 한 짧고 가볍게 답하세요. 깊게 생각하지 마세요.\n",
        "high" => "- 깊게 생각하고, 직접 검증한 뒤에 답하세요. 추정과 실제를 구분하세요.\n",
        _ => "",
    };
    // 워크스페이스 파일 목록은 작게 유지 — 가장 중요한 건 대화 기록
    let snapshot = snapshot(workspace_root);
    let mut prompt = String::new();
    prompt.push_str(&header);
    if !effort_line.is_empty() {
        prompt.push_str(effort_line);
    }
    prompt.push_str("\n## 대화 기록\n");
    prompt.push_str(convo.trim_end());
    prompt.push_str("\n\n[현재 워크스페이스 파일]\n");
    prompt.push_str(snapshot.trim_end());
    prompt.push_str(
        "\n\n## 당신의 차례입니다\n\
위 대화의 **가장 마지막 사용자 요청**을 처리하세요.\n\
- 아직 실행하지 않았다면 TOOL 한 줄을 출력하세요 (설명 없이).\n\
- 이미 실행하고 결과를 확인했다면, 그 결과로 짧게 끝맺으면 됩니다.\n\
- 이미 완료된 작업을 다시 하지는 마세요.\n\
- 새 요청이 들어왔는데 '완료'라고만 답하지 마세요.\n\
- 새 요청이라면 `CODEBRIDGE_PLAN {\"steps\":[...]}` 한 줄을 먼저 출력하고, 바로 이어서 첫 단계의 TOOL을 출력하세요. 이미 계획을 출력했다면 건너뛰세요.\n",
    );

    if let Ok(out) = std::env::var("BRIDGE_DEBUG_PROMPT") {
        let _ = std::fs::write(&out, &prompt);
    }
    Ok(FlattenResult { prompt, attach })
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

// 테스트 모듈 스코프 공유 — 두 테스트가 같은 프로세스 ID를 쓰므로 반드시 하나의 카운터를 쓴다.
#[cfg(test)]
static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cb_flat_{}_{}", std::process::id(), n));
        let out_dir = dir.join("src");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("main.rs"), b"fn main() {}").unwrap();
        let flat = build(&req, &dir, Some(20_000)).unwrap();
        let prompt = flat.prompt;
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
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cb_flat_{}_{}", std::process::id(), n));
        let out_dir = dir.join("src");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("main.rs"), b"fn main() {}").unwrap();
        let flat = build(&req, &dir, Some(20_000)).unwrap();
        let prompt = flat.prompt;
        assert!(prompt.contains("에이전트"));
        assert!(prompt.contains("## 대화 기록"));
        assert!(prompt.contains("사용자:"));
        assert!(prompt.contains("TOOL read path=src/main.rs"));
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
        let flat = build(&req, std::path::Path::new("."), Some(20_000)).unwrap();
        let prompt = flat.prompt;
        assert!(prompt.contains("첫 줄"));
        assert!(prompt.contains("둘째 줄"));
    }
}

