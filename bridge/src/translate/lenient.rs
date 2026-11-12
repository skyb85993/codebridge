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

// ---------------------------------------------------------------------------
// TOOL 라인 파싱 (Phase 2~3): raw innerText에는 펜스 마커가 사라지므로
// "TOOL 이름 {JSON}" 한 줄(또는 TOOL 블록)을 인식해 opencode의 tool_calls로 승격한다.
//
// 형식:
//   TOOL read {"path": "src/GameManager.lua"}
//   또는 TOOL read
//   {"path": "src/GameManager.lua"}
//   END_TOOL
//
// 안전:
//   - 경로 계열인자는 모두 sanitize_path 샌드박스를 거친다
//   - 파괴적 bash 명령은 자동 실행 대신 CODEBRIDGE_QUESTION 마커로 변환 (사용자 확인)
// ---------------------------------------------------------------------------

const TOOL_NAME: &[&str] = &["read", "glob", "grep", "websearch", "edit", "shell", "question"];
const TOOL_END_LINE: &str = "END_TOOL";

/// 파싱 결과: opencode로 보낼 tool_calls + 사용자에게 보여줄 마커 라인들.
#[derive(Debug, Default)]
pub struct ToolPromotion {
    /// opencode tool_calls (write promotion은 여기 포함되지 않음 — 별도 처리).
    pub calls: Vec<Value>,
    /// `question` 블록 또는 위험한 `shell`에서 자동 승격된 confirm/question 마커.
    pub question_lines: Vec<String>,
    /// 파이프라인 진단용 이슈들 (사용자 노출 아님).
    pub issues: Vec<String>,
}

enum ToolBlock {
    Call(Value),
    QuestionMarker(String),
    Skip(String),
}

/// raw innerText에서 TOOL 라인을 찾아 tool_calls로 승격한다.
pub fn extract_tool_calls(raw: &str) -> ToolPromotion {
    let mut out = ToolPromotion::default();
    let mut call_idx = 1usize;
    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0usize;

    while i < lines.len() {
        let t = lines[i].trim_start();
        if !t.starts_with("TOOL ") {
            i += 1;
            continue;
        }
        let rest = &t["TOOL ".len()..];
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").trim().to_string();
        let inline = parts.next().map(str::trim).unwrap_or("");

        // 본문: 같은 줄에 있으면 inline, 아니면 END_TOOL 까지 모은다.
        let body = if !inline.is_empty() {
            inline.to_string()
        } else {
            if !TOOL_NAME.contains(&name.as_str()) {
                i += 1;
                continue;
            }
            let mut body = String::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim() == TOOL_END_LINE {
                    i += 1;
                    break;
                }
                body.push_str(l);
                body.push('\n');
                i += 1;
            }
            body.trim().to_string()
        };

        match classify_tool_call(&name, &body, &mut call_idx) {
            ToolBlock::Call(v) => out.calls.push(v),
            ToolBlock::QuestionMarker(line) => out.question_lines.push(line),
            ToolBlock::Skip(reason) => out.issues.push(format!("{name}: {reason}")),
        }
        i += 1;
    }
    out
}

/// TOOL 블록 본문(JSON)을 파싱하고, 이름과 맞춰 opencode call을 만든다.
fn classify_tool_call(name: &str, body: &str, call_idx: &mut usize) -> ToolBlock {
    let v: Value = match parse_tool_body(body) {
        Ok(v) => v,
        Err(e) => return ToolBlock::Skip(format!("본문 파싱 실패: {e}")),
    };
    if !v.is_object() {
        return ToolBlock::Skip("JSON object 필요".into());
    }

    match name {
        "read" => {
            let p = arg_str_any(&v, &["path", "file", "filepath", "filename", "src", "directory"]).as_deref().map(sanitize_path).flatten();
            let Some(path) = p else {
                return ToolBlock::Skip("path 없음 또는 거부".into());
            };
            let mut args = serde_json::Map::new();
            args.insert("path".into(), Value::String(path));
            if let Some(off) = arg_num(&v, "offset") {
                args.insert("offset".into(), Value::from(off));
            }
            if let Some(lim) = arg_num(&v, "limit") {
                args.insert("limit".into(), Value::from(lim));
            }
            ToolBlock::Call(make_call(call_idx, "read", Value::Object(args)))
        }
        "glob" => {
            let Some(pattern) = arg_str_any(&v, &["pattern", "glob", "match", "name"]) else {
                return ToolBlock::Skip("pattern 없음".into());
            };
            let mut args = serde_json::Map::new();
            args.insert("pattern".into(), Value::String(pattern));
            if let Some(p) = arg_str_any(&v, &["path", "dir", "directory"]) {
                if let Some(p) = sanitize_path(&p) {
                    args.insert("path".into(), Value::String(p));
                }
            }
            if let Some(lim) = arg_num(&v, "limit") {
                args.insert("limit".into(), Value::from(lim));
            }
            if let Some(h) = arg_bool(&v, "hidden") {
                args.insert("hidden".into(), Value::Bool(h));
            }
            ToolBlock::Call(make_call(call_idx, "glob", Value::Object(args)))
        }
        "grep" => {
            let Some(pattern) = arg_str_any(&v, &["pattern", "regex", "query", "needle"]) else {
                return ToolBlock::Skip("pattern 없음".into());
            };
            let mut args = serde_json::Map::new();
            args.insert("pattern".into(), Value::String(pattern));
            if let Some(p) = arg_str_any(&v, &["path", "dir", "directory"]) {
                if let Some(p) = sanitize_path(&p) {
                    args.insert("path".into(), Value::String(p));
                }
            }
            if let Some(inc) = arg_str(&v, "include") {
                args.insert("include".into(), Value::String(inc));
            }
            if let Some(l) = arg_num(&v, "limit") {
                args.insert("limit".into(), Value::from(l));
            }
            if let Some(lit) = arg_bool(&v, "literal") {
                args.insert("literal".into(), Value::Bool(lit));
            }
            if let Some(cs) = arg_bool(&v, "caseSensitive") {
                args.insert("caseSensitive".into(), Value::Bool(cs));
            }
            ToolBlock::Call(make_call(call_idx, "grep", Value::Object(args)))
        }
        "edit" => {
            let p = arg_str_any(&v, &["path", "file", "filepath", "filename", "src"]).as_deref().map(sanitize_path).flatten();
            let (Some(path), Some(old), Some(new)) = (
                p,
                arg_str_any(&v, &["oldString", "old", "find", "before", "source"]),
                arg_str_any(&v, &["newString", "new", "after", "replace", "target"]),
            ) else {
                return ToolBlock::Skip("path/oldString/newString 중 하나 없음 또는 path 거부".into());
            };
            let mut args = serde_json::Map::new();
            args.insert("path".into(), Value::String(path));
            args.insert("oldString".into(), Value::String(old));
            args.insert("newString".into(), Value::String(new));
            if arg_bool(&v, "replaceAll").unwrap_or(false) {
                args.insert("replaceAll".into(), Value::Bool(true));
            }
            ToolBlock::Call(make_call(call_idx, "edit", Value::Object(args)))
        }
        "websearch" => {
            let Some(query) = arg_str_any(&v, &["query", "q", "search", "term", "needle"]) else {
                return ToolBlock::Skip("query 없음".into());
            };
            let mut args = serde_json::Map::new();
            args.insert("query".into(), Value::String(query));
            if let Some(n) = arg_num(&v, "limit") {
                args.insert("limit".into(), Value::from(n));
            }
            ToolBlock::Call(make_call(call_idx, "websearch", Value::Object(args)))
        }
        "shell" => {
            let Some(command) = arg_str_any(&v, &["command", "cmd", "run", "script", "exe"]) else {
                return ToolBlock::Skip("command 없음".into());
            };
            if is_destructive_command(&command) {
                // 위험한 명령은 자동 실행 대신 사용자 확인 카드로
                return ToolBlock::QuestionMarker(danger_shell_line(&command));
            }
            let mut args = serde_json::Map::new();
            args.insert("command".into(), Value::String(command));
            if let Some(w) = arg_str(&v, "workdir") {
                if let Some(w) = sanitize_path(&w) {
                    args.insert("workdir".into(), Value::String(w));
                }
            }
            if let Some(t) = arg_num(&v, "timeout") {
                args.insert("timeout".into(), Value::from(t));
            }
            if let Some(b) = arg_bool(&v, "background") {
                args.insert("background".into(), Value::Bool(b));
            }
            ToolBlock::Call(make_call(call_idx, "shell", Value::Object(args)))
        }
        "question" => match serde_json::to_string(&v) {
            Ok(_) => ToolBlock::QuestionMarker(question_marker(&v)),
            Err(_) => ToolBlock::Skip("질문 JSON 직렬화 실패".into()),
        },
        _ => ToolBlock::Skip(format!("알 수 없는 도구: {name}")),
    }
}

fn make_call(idx: &mut usize, name: &str, args: Value) -> Value {
    let id = format!("callt_{idx:03}");
    *idx += 1;
    json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": args.to_string() }
    })
}

fn arg_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

/// 여러 키 중 하나라도 등장하는 값 (오타/약칭 허용).
fn arg_str_any(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| arg_str(v, k))
}

/// TOOL 본문 파싱: JSON 객체(`{...}`)이거나 `key=value key2="값" ...` 형식이다.
/// KV 모드는 ChatGPT 같은 웹 모델이 쉽게 출력하는 형태다 (따옴표 없이도 동작).
fn parse_tool_body(body: &str) -> Result<Value, String> {
    let body = body.trim();
    if body.starts_with('{') {
        return serde_json::from_str(body).map_err(|e| format!("JSON 파싱 실패: {e}"));
    }
    let mut obj = serde_json::Map::new();
    for part in split_kv(body) {
        let (k, v) = split_kv_item(&part).ok_or_else(|| format!("key=value 파싱 실패: {part}"))?;
        obj.insert(k, Value::String(v));
    }
    Ok(Value::Object(obj))
}

/// 공백으로 구분된 key=value 나열 (따옴표 안 공백은 토큰으로 취급).
fn split_kv(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            ' ' | '\t' if !in_quotes => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

fn split_kv_item(part: &str) -> Option<(String, String)> {
    let p = part.trim();
    let eq = p.find('=')?;
    let key = p[..eq].trim();
    if key.is_empty() {
        return None;
    }
    let mut val = p[eq + 1..].trim().to_string();
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        val = val[1..val.len() - 1].to_string();
    }
    // 모델이 `\n`을 문자 그대로 쓰는 경우를 실제 개행으로 변환 (KV 한 줄 안에서 줄바꿈을 쓰는 관례).
    let val = val.replace("\\n", "\n");
    Some((key.to_string(), val))
}

fn arg_num(v: &Value, key: &str) -> Option<u64> {
    v.get(key)?;
    v.get(key)?
        .as_u64()
        .or_else(|| v.get(key)?.as_str()?.parse::<u64>().ok())
}
fn arg_bool(v: &Value, key: &str) -> Option<bool> {
    v.get(key)?;
    v.get(key)?
        .as_bool()
        .or_else(|| match v.get(key)?.as_str()? {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        })
}

/// 마커 라인 타입: 위험한 명령 확인용.
const MARKER_DANGER_SHELL: &str = "danger_shell";

fn danger_shell_line(command: &str) -> String {
    let json = json!({
        "header": "위험한 명령",
        "question": format!("다음 명령은 되돌릴 수 없습니다. 실행할까요?\n\n{}", command),
        "options": [
            { "label": "실행", "description": "위험! 되돌릴 수 없음" },
            { "label": "취소", "description": "실행하지 않음" }
        ],
        "multiple": false,
        "command": command,
        "marker": MARKER_DANGER_SHELL,
    });
    format!("CODEBRIDGE_QUESTION: {json}")
}

/// 모델이 준 opencode question 스키마 그대로 마커에 실어 UI가 그대로 렌더하도록 한다.
fn question_marker(v: &Value) -> String {
    let q0 = v
        .get("questions")
        .and_then(|q| q.get(0))
        .cloned()
        .unwrap_or(Value::Null);
    let json = json!({
        "header": q0.get("header").and_then(|h| h.as_str()).unwrap_or("질문"),
        "question": q0.get("question").and_then(|q| q.as_str()).unwrap_or("어떻게 할까요?"),
        "options": q0.get("options").cloned().unwrap_or(Value::Array(vec![])),
        "multiple": q0.get("multiple").and_then(|m| m.as_bool()).unwrap_or(false),
        "marker": "question",
    });
    format!("CODEBRIDGE_QUESTION: {json}")
}

/// 파괴적 명령 패턴 (대소문자 무시). 여기에 걸리면 자동 실행하지 않고 사용자 확인 카드로 간다.
/// 숫자 방지: 빈 명령/파라미터만으로는 걸리지 않는다.
pub fn is_destructive_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    const PATTERNS: &[&str] = &[
        "rm -rf", "rm -fr", "rm -r ", "rd /s", "rmdir /s", "del /f", "del /q", "del /s",
        "format ", "format:", "format c", "diskpart", "reg delete", "reg add", "bcdedit",
        "bootrec", "bcboot", "schtasks /create", "schtasks /delete", "icacls /deny",
        "takeown", "cacls", "net user", "net stop ", "sc stop", "sc delete", "sc config",
        "dism /", "attrib -r", "attrib -s", "cipher /d", "quickformat", "mkfs", "fdisk",
        "partprobe", "dd if=", "shutdown", "reboot", "poweroff", "init 0", "rd /q",
        "move /y", "deltree",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_read_block_promotes() {
        let text = "확인해보겠습니다\nTOOL read {\"path\": \"src/engine.ts\"}\n네";
        let out = extract_tool_calls(text);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0]["function"]["name"], "read");
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["path"], "src/engine.ts");
    }

    #[test]
    fn tool_multiline_block_promotes() {
        let text = "TOOL shell\n{\"command\": \"ls\", \"workdir\": \"docs\"}\nEND_TOOL";
        let out = extract_tool_calls(text);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0]["function"]["name"], "shell");
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["command"], "ls");
    }

    #[test]
    fn tool_path_sandbox_rejects_parent() {
        let out = extract_tool_calls("TOOL read {\"path\": \"../secret.txt\"}");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.issues.len(), 1);
    }

    #[test]
    fn tool_path_sandbox_rejects_drive_absolute() {
        let out = extract_tool_calls("TOOL read {\"path\": \"C:/secret.txt\"}");
        assert_eq!(out.calls.len(), 0);
    }

    #[test]
    fn websearch_query_promotes() {
        let out = extract_tool_calls("TOOL websearch query=\"코드브릿지\"");
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0]["function"]["name"], "websearch");
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["query"], "코드브릿지");
    }

    #[test]
    fn glob_pattern_promotes_kv() {
        let out = extract_tool_calls("TOOL glob pattern=src/**/*.ts");
        assert_eq!(out.calls.len(), 1);
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["pattern"], "src/**/*.ts");
    }

    #[test]
    fn grep_pattern_promotes_kv() {
        let out = extract_tool_calls("TOOL grep pattern=TODO path=src");
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0]["function"]["name"], "grep");
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["pattern"], "TODO");
        assert_eq!(v["path"], "src");
    }

    #[test]
    fn edit_alias_keys_promote_with_newlines() {
        let out = extract_tool_calls("TOOL edit path=src/x.ts oldString=\"a\" newString=\"b\"");
        assert_eq!(out.calls.len(), 1);
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["oldString"], "a");
        assert_eq!(v["newString"], "b");
    }

    #[test]
    fn edit_old_new_aliases_promote() {
        let out = extract_tool_calls("TOOL edit path=src/x.ts old=\"hello\" new=\"world\"");
        assert_eq!(out.calls.len(), 1);
        let args = out.calls[0]["function"]["arguments"].as_str().unwrap();
        let v: Value = serde_json::from_str(args).unwrap();
        assert_eq!(v["oldString"], "hello");
        assert_eq!(v["newString"], "world");
    }

    #[test]
    fn question_single_line_promotes_marker() {
        let out = extract_tool_calls("TOOL question header=방향 question=어떻게? options=A,B");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.question_lines.len(), 1);
    }

    #[test]
    fn shell_preview_unsafe_alias_promotes() {
        let out = extract_tool_calls("TOOL shell cmd=\"ls\"");
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0]["function"]["name"], "shell");
    }

    #[test]
    fn shell_destructive_converts_to_question() {
        let out = extract_tool_calls("TOOL shell {\"command\": \"rm -rf /\"}");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.question_lines.len(), 1);
        assert!(out.question_lines[0].starts_with("CODEBRIDGE_QUESTION: "));
        assert!(out.question_lines[0].contains("danger_shell"));
    }

    #[test]
    fn shell_destructive_detection_matrix() {
        assert!(is_destructive_command("rm -rf /"));
        assert!(is_destructive_command("del /f x.txt"));
        assert!(is_destructive_command("rd /s /q old\")"));
        assert!(is_destructive_command("format c:"));
        assert!(is_destructive_command("reg delete HKLM\\Software\\x /f"));
        assert!(is_destructive_command("bcdedit /deletevalue foo"));
        assert!(is_destructive_command("Shutdown /s /t 0"));
        assert!(!is_destructive_command("git status"));
        assert!(!is_destructive_command("npm test"));
        assert!(!is_destructive_command("cargo test --release"));
    }

    #[test]
    fn tool_question_block_becomes_marker() {
        let out = extract_tool_calls("TOOL question {\"questions\":[{\"header\":\"방향\",\"question\":\"어떻게 할까요?\",\"options\":[{\"label\":\"A\",\"description\":\"x\"}],\"multiple\":false}]}");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.question_lines.len(), 1);
        let line = &out.question_lines[0];
        assert!(line.starts_with("CODEBRIDGE_QUESTION: "));
        let json = line.trim_start_matches("CODEBRIDGE_QUESTION: ");
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["header"], "방향");
        assert_eq!(v["question"], "어떻게 할까요?");
        assert_eq!(v["options"][0]["label"], "A");
    }

    #[test]
    fn tool_unknown_tool_ignored_with_issue() {
        let out = extract_tool_calls("TOOL teleport {\"x\": 1}");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.issues.len(), 1);
    }

    #[test]
    fn tool_edit_needs_all_fields() {
        let out = extract_tool_calls("TOOL edit {\"path\": \"src/a.ts\", \"oldString\": \"a\"}");
        assert_eq!(out.calls.len(), 0);
        assert_eq!(out.issues.len(), 1);
    }

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
