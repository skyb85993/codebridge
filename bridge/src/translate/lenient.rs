//! 관대 파서 (Phase 0~1 핵심):
//! 웹 모델이 평소처럼 답변할 때의 마크다운 코드펜스를 찾아서,
//! 첫 줄 경로 주석을 인식 → write tool_calls로 승격한다.
//!
//! 지원 형태:
//!   - ``` / ~~~ 펜스 (길이 >= 3, 들여쓰기 허용, 3백틱 안 4백틱 중첩 케이스 처리)
//!   - 경로 주석 토큰: `path:` / `file:` / `filename:` / `filepath:` / `경로:` (=: 허용, 따옴표 허용)
//!   - 펜스 정보 문자열의 title="경로" 대체 인식
//!   - 절대경로/드라이브문자/`..` 거부 (워크스페이스 샌드박스 정책)

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// 펜스 정보의 언어 토큰 (예: "lua")
    pub lang: Option<String>,
    /// 정규화된 상대경로 (샌드박스 통과 시에만 Some)
    pub path: Option<String>,
    /// 변환 실패 사유: "no_path" | "unclosed" | ...
    pub issue: Option<String>,
    /// 파일 본문 (경로 주석 줄 제외)
    pub body: String,
}

pub fn parse(text: &str) -> Vec<ParsedFile> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let Some((fchar, flen, info)) = fence_open(lines[i]) else {
            i += 1;
            continue;
        };
        i += 1;

        let mut path: Option<String> = None;
        let mut issue: Option<String> = None;
        let mut started = false; // 본문이 시작됐는가(경로 줄 시도는 첫 비어있지 않은 줄 1회)
        let mut raw_body = String::new();
        let mut closed = false;

        while i < lines.len() {
            let l = lines[i];
            if fence_close(l, fchar, flen) {
                closed = true;
                i += 1;
                break;
            }
            if !started {
                if l.trim().is_empty() {
                    i += 1;
                    continue;
                }
                started = true;
                if let Some(p) = path_from_comment(l) {
                    path = Some(p);
                    i += 1;
                    continue;
                }
            }
            raw_body.push_str(l);
            raw_body.push('\n');
            i += 1;
        }

        if !closed {
            issue = Some("unclosed".to_string());
        }
        if path.is_none() {
            path = path_from_info(&info);
        }
        if path.is_none() && issue.is_none() {
            issue = Some("no_path".to_string());
        }
        let lang = info
            .split_whitespace()
            .next()
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty() && !s.contains('='));

        out.push(ParsedFile {
            lang,
            path,
            issue,
            body: raw_body.trim_end().to_string(),
        });
    }
    out
}

/// 펜스 열림 감지: 3+개의 연속 ` 또는 ~
fn fence_open(line: &str) -> Option<(char, usize, String)> {
    let t = line.trim_start();
    let first = t.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let n = t.chars().take_while(|&c| c == first).count();
    if n < 3 {
        return None;
    }
    // 틱 런은 ASCII이므로 n == 바이트 길이 (한글 info 안전)
    Some((first, n, t[n..].trim().to_string()))
}

/// 펜스 닫힘 감지: 열림과 같은 문자, 길이 >= 열림, 뒤에 추가 텍스트 없음
fn fence_close(line: &str, fchar: char, flen: usize) -> bool {
    let t = line.trim();
    if t.chars().next() != Some(fchar) {
        return false;
    }
    let m = t.chars().take_while(|&c| c == fchar).count();
    m >= flen && t[m..].trim().is_empty()
}

const PATH_TOKENS: [&str; 5] = ["filepath", "filename", "file", "path", "경로"];
/// 주석 접두어 — 이 접두어로 시작하는 라인에서만 경로 주석을 인식한다.
const COMMENT_PREFIXES: [&str; 6] = ["--", "//", "#", "/*", "<!--", ";"];

/// 코드펜스 첫 줄에서 경로 주석을 뽑는다.
/// 예: "-- path: A/B.lua", "// path: src/x.ts", "# 경로: a/b.py", "-- path=\"x.lua\"" 등
/// 주석 접두어로 시작하지 않는 코드 라인은 경로로 인식하지 않는다 (v0.5, CodeRabbit 지적).
pub fn path_from_comment(line: &str) -> Option<String> {
    let t = line.trim_start();
    let body = COMMENT_PREFIXES.iter().find_map(|p| t.strip_prefix(p))?;
    let body = body.trim_start();
    for tok in PATH_TOKENS {
        if let Some(p) = find_token_then_sep(body, tok) {
            // 토큰 바로 뒤 구분자까지 건너뛰고 값 추출
            let raw = &body[p + tok.len() + 1..];
            if let Some(v) = clean_value(raw) {
                if let Some(pth) = sanitize_path(&v) {
                    return Some(pth);
                }
            }
        }
    }
    None
}

/// `tok`가 등장하고 바로 다음 문자가 ':' 또는 '='인 위치를 찾는다.
fn find_token_then_sep(line: &str, tok: &str) -> Option<usize> {
    if let Some(p) = scan(line, tok) {
        return Some(p);
    }
    let lower = line.to_lowercase();
    // 소문자 변형 시 바이트 길이가 같아야 원문 오프셋 대응이 유효
    if lower.len() == line.len() {
        if let Some(p) = scan(&lower, tok) {
            return Some(p);
        }
    }
    None
}

fn scan(hay: &str, tok: &str) -> Option<usize> {
    let mut from = 0usize;
    loop {
        let rel = hay[from..].find(tok)?;
        let p = from + rel;
        // 단어 경계: 토큰 앞 문자가 식별자 문자(영숫자/_)이면 건너뛴다 (예: "mypath=" 등)
        let boundary_ok = p == 0
            || match hay[..p].chars().next_back() {
                Some(c) => !(c.is_alphanumeric() || c == '_'),
                None => true,
            };
        let rest = &hay[p + tok.len()..];
        match (boundary_ok, rest.chars().next()) {
            (true, Some(':')) | (true, Some('=')) => return Some(p),
            _ => {
                from = p + tok.len();
                if from >= hay.len() {
                    return None;
                }
            }
        }
    }
}

/// 값 정제: 따옴표 분기 / JSON 스타일 / 첫 토큰 추출
fn clean_value(raw: &str) -> Option<String> {
    let mut p = raw.trim().to_string();
    if p.is_empty() {
        return None;
    }
    if p.starts_with('"') {
        let inner = &p[1..];
        let end = inner.find('"').unwrap_or(inner.len());
        p = inner[..end].to_string();
    } else if p.starts_with('\'') {
        let inner = &p[1..];
        let end = inner.find('\'').unwrap_or(inner.len());
        p = inner[..end].to_string();
    } else {
        let end = p.find(char::is_whitespace).unwrap_or(p.len());
        p = p[..end].to_string();
        p = p.trim_end_matches(|c| c == ',' || c == ';' || c == '}').to_string();
    }
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

/// 워크스페이스 샌드박스 정책: 상대경로만 허용
pub fn sanitize_path(raw: &str) -> Option<String> {
    let mut p = raw.trim().to_string();
    p = p.trim_matches(|c| c == '"' || c == '\'').trim().to_string();
    p = p.trim_matches('`').trim().replace('\\', "/");
    if p.contains('\0') {
        return None;
    }
    p = p.trim_end_matches('/').to_string();
    if p.is_empty() || p.starts_with('/') {
        return None;
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        return None; // 드라이브 문자 (C:)
    }
    if p.split('/').any(|s| s == ".." || s == ".") {
        return None;
    }
    Some(p)
}

/// 펜스 정보 문자열의 title= / file= 로 경로 대체 인식
fn path_from_info(info: &str) -> Option<String> {
    if info.is_empty() {
        return None;
    }
    let lower = info.to_lowercase();
    if lower.len() != info.len() {
        return None;
    }
    for key in ["title", "file", "path"] {
        let needle = format!("{key}=");
        if let Some(p) = lower.find(&needle) {
            let raw = &info[p + needle.len()..];
            if let Some(v) = clean_value(raw) {
                if let Some(pth) = sanitize_path(&v) {
                    return Some(pth);
                }
            }
        }
    }
    None
}

/// 파싱 결과 → opencode write tool_calls. 경로 없는 블록은 이슈 목록으로 돌려준다.
pub fn to_tool_calls(files: &[ParsedFile]) -> (Vec<Value>, Vec<String>) {
    let mut calls = Vec::new();
    let mut issues = Vec::new();
    let mut written = 0usize;
    let mut skipped = 0usize;

    for f in files {
        match (&f.path, &f.issue) {
            (Some(p), None) => {
                written += 1;
                let args = json!({ "path": p, "content": f.body });
                calls.push(json!({
                    "id": format!("callw_{written:03}"),
                    "type": "function",
                    "function": {
                        "name": "write",
                        "arguments": args.to_string()
                    }
                }));
            }
            _ => {
                // 경로가 없거나, 펜스가 닫히지 않았거나(unclosed) 등 이슈가 있는 블록은 승격하지 않는다.
                skipped += 1;
                issues.push(format!(
                    "block#{} reason={:?} lang={:?}",
                    skipped,
                    f.issue,
                    f.lang
                ));
            }
        }
    }
    (calls, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_files_with_paths() {
        let text = "소개\n```lua\n-- path: ServerScriptService/GameManager.lua\nlocal a = 1\n```\n\n```lua\n-- path: StarterPlayer/StarterPlayerScripts/HUD.client.lua\nprint(1)\n```\n말미";
        let files = parse(text);
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0].path.as_deref(),
            Some("ServerScriptService/GameManager.lua")
        );
        assert!(files[0].body.contains("local a = 1"));
        assert_eq!(
            files[1].path.as_deref(),
            Some("StarterPlayer/StarterPlayerScripts/HUD.client.lua")
        );
        assert!(files[0].issue.is_none() && files[1].issue.is_none());
    }

    #[test]
    fn no_path_reported() {
        let text = "```text\n참고용 설명 블록\n```";
        let files = parse(text);
        assert_eq!(files.len(), 1);
        assert!(files[0].path.is_none());
        assert_eq!(files[0].issue.as_deref(), Some("no_path"));
    }

    #[test]
    fn inline_code_not_confused() {
        let text = "설명에 `-- path: X/oops.lua` 가 들어가도 무시됩니다";
        assert!(parse(text).is_empty());
    }

    #[test]
    fn slash_comment_path() {
        let text = "```ts\n// path: src/util.ts\nexport const a = 1;\n```";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), Some("src/util.ts"));
    }

    #[test]
    fn tilde_fence_works() {
        let text = "~~~lua\n-- path: a/b.lua\nprint(1)\n~~~";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), Some("a/b.lua"));
    }

    #[test]
    fn quoted_path_with_space() {
        let text = "```lua\n-- path: \"Maps/Round Main.lua\"\nprint(1)\n```";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), Some("Maps/Round Main.lua"));
    }

    #[test]
    fn title_fallback_from_info() {
        let text = "```lua title=\"Maps/Boss.lua\"\nprint('boss')\n```";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), Some("Maps/Boss.lua"));
        assert_eq!(files[0].lang.as_deref(), Some("lua"));
    }

    #[test]
    fn nested_backticks() {
        let text = "````md\n-- path: Docs/Name.md\n# 제목\n```lua\nprint('x')\n```\n끝\n````";
        let files = parse(text);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_deref(), Some("Docs/Name.md"));
        assert!(files[0].body.contains("```lua"));
    }

    #[test]
    fn drive_letter_rejected() {
        let text = "```lua\n-- path: C:/Temp/evil.lua\nx\n```";
        let files = parse(text);
        assert!(files[0].path.is_none());
        assert_eq!(files[0].issue.as_deref(), Some("no_path"));
    }

    #[test]
    fn parent_dir_rejected() {
        let text = "```lua\n-- path: ../evil.lua\nx\n```";
        let files = parse(text);
        assert!(files[0].path.is_none());
    }

    #[test]
    fn unclosed_fence_flagged_but_path_kept() {
        let text = "```lua\n-- path: a/b.lua\nprint(1)";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), Some("a/b.lua"));
        assert_eq!(files[0].issue.as_deref(), Some("unclosed"));
    }

    #[test]
    fn path_only_from_comment_prefixed_line() {
        let text = "```lua\nlocal path = \"Workspace/Part\"\nprint(1)\n```";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), None);
        assert_eq!(files[0].issue.as_deref(), Some("no_path"));
    }

    #[test]
    fn ident_boundary_rejected() {
        let text = "```lua\n-- mypath: Workspace/Part\nprint(1)\n```";
        let files = parse(text);
        assert_eq!(files[0].path.as_deref(), None);
    }

    #[test]
    fn unclosed_fence_not_promoted() {
        let text = "```lua\n-- path: a/b.lua\nprint(1)";
        let (calls, issues) = to_tool_calls(&parse(text));
        assert!(calls.is_empty());
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn tool_calls_shape() {
        let text = "```lua\n-- path: a/b.lua\nprint(1)\n```";
        let (calls, issues) = to_tool_calls(&parse(text));
        assert!(issues.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["type"], "function");
        assert_eq!(calls[0]["function"]["name"], "write");
        let args = calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["path"], "a/b.lua");
        assert_eq!(v["content"], "print(1)");
    }
}
