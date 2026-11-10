//! 컨텍스트 평탄화 (Phase 1 핵심 1):
//! opencode messages(system/user/assistant/tool) → 웹 채팅용 단일 프롬프트 1개.
//! 우선순위: 규약 > 최근 user > 도구 오류 > 도구 결과 > 스냅샷 — 예산 초과 시 뒤에서부터 자른다.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{ChatMessage, ChatRequest};

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

fn msg_text(m: &ChatMessage) -> String {
    m.content
        .as_ref()
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

/// messages → 단일 프롬프트. budget_chars 를 위해 가장 오래된 도구 결과부터 자른다.
pub fn build(req: &ChatRequest, workspace_root: &Path, budget_chars: Option<usize>) -> String {
    let budget = budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    let snapshot = snapshot(workspace_root);
    let latest_user = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(msg_text)
        .unwrap_or_default();
    let tool_results: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(msg_text)
        .collect();

    let mut sections: Vec<String> = Vec::new();
    sections.push("[파일 규약 헤더]\n".to_string() + FENCE_HEADER.trim());
    sections.push(format!("[이번 사용자 요청]\n{latest_user}"));

    // 도구 결과를 오래된 것부터 예산에 맞게 접기
    let mut kept: Vec<String> = Vec::new();
    let mut used = sections.iter().map(|s| s.len()).sum::<usize>() + snapshot.len();
    for (i, tr) in tool_results.iter().enumerate().rev() {
        let cost = tr.len() + 24;
        if used + cost > budget {
            kept.push(format!("(…오래된 도구 결과 {}건은 예산 초과로 생략)", i + 1));
            break;
        }
        used += cost;
        kept.push(format!("[도구 결과 #{}]\n{}", i + 1, tr));
    }
    kept.reverse();
    if !kept.is_empty() {
        sections.push(kept.join("\n\n"));
    }

    if !snapshot.trim().is_empty() {
        sections.push(format!("[현재 워크스페이스 파일]\n{snapshot}"));
    }
    sections.push("[답변 요청]\n위 규약에 따라 답변해 주세요.".to_string());

    let mut out = sections.join("\n\n");
    // 최종 안전 절단 (문자 경계)
    if out.chars().count() > budget {
        out = out.chars().take(budget).collect();
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
        let prompt = build(&req, &dir, Some(20_000));
        assert!(prompt.contains("중요한 작업"));
        assert!(prompt.contains("파일 출력 규약"));
        assert!(prompt.contains("main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn budget_respected() {
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
        let prompt = build(&req, std::path::Path::new("."), Some(200));
        assert!(prompt.chars().count() <= 200);
    }
}

