//! CDP 엔진 (Phase 1~3 핵심):
//! 전용 Chrome(CDP 포트)에 붙어 사이트를 찾아 입력→전송→완료 감지→응답 추출.
//! Site: ChatGpt | Gemini — 모델 id에 "gemini"가 포함되면 Gemini (provider/model로 호출부가 정함).
//! 선택자 근거: 2026-11-10 실측 (docs/spike-report.md, profiles.toml 참고).

use std::time::{Duration, Instant};

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures::StreamExt;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    ChatGpt,
    Gemini,
}

impl Site {
    pub fn from_model(model: &str) -> Site {
        if model.to_lowercase().contains("gemini") {
            Site::Gemini
        } else {
            Site::ChatGpt
        }
    }
}

struct SiteSelectors {
    host: &'static str,
    composer_sel: &'static str,
    assistant_sel: &'static str,
    code_root_sel: &'static str,
    stop_sel: &'static str,
    send_native: bool, // true면 CDP Element 네이티브 클릭 (Angular Material 계열)
    send_sel: &'static str,
    new_chat_sel: &'static str,
}

const CHATGPT_SEL: SiteSelectors = SiteSelectors {
    host: "chatgpt.com",
    composer_sel: "textarea#prompt-textarea, div#prompt-textarea, [contenteditable='true']",
    assistant_sel: "div[class*='DilResponseRoot'], [class*='entity-prose-inset']",
    code_root_sel: "[class*='DilResponseRoot'], [class*='entity-prose-inset']",
    stop_sel: "button[aria-label='중지'], button[data-testid='stop-button'], button[aria-label='Stop generating'], button[aria-label='Stop streaming']",
    send_native: false,
    send_sel: "button[data-testid='send-button'], button[aria-label='보내기'], button[aria-label='Send prompt'], button[aria-label='Send']",
    new_chat_sel: "button[aria-label='새 채팅'], button[aria-label='New chat'], button[data-testid='create-new-chat-button']",
};

const GEMINI_SEL: SiteSelectors = SiteSelectors {
    host: "gemini.google.com",
    composer_sel: "div.ql-editor, rich-textarea div[contenteditable='true'], [contenteditable='true']",
    assistant_sel: "message-content",
    code_root_sel: "message-content",
    stop_sel: "button[aria-label*='중지'], button[aria-label*='Stop'], button[aria-label*='stop']",
    send_native: true,
    send_sel: "button[aria-label*='보내']",
    new_chat_sel: "button[aria-label*='새 채팅'], button[aria-label='새 채팅']",
};

fn site_selectors(site: Site) -> &'static SiteSelectors {
    match site {
        Site::ChatGpt => &CHATGPT_SEL,
        Site::Gemini => &GEMINI_SEL,
    }
}

/// 한도 배너 텍스트 (두 사이트 공통 + 사이트별)
pub const LIMIT_TEXTS: &[&str] = &[
    "limit reached",
    "usage cap",
    "rate limit",
    "메시지 한도",
    "사용 한도",
    "사용량 한도",
    "일일 한도",
    "daily limit",
    "quota",
];

#[derive(Debug)]
pub enum EngineError {
    Connect(String),
    Page(String),
    Timeout,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Connect(e) => write!(f, "CDP 연결 실패({e})"),
            EngineError::Page(e) => write!(f, "페이지 오류({e})"),
            EngineError::Timeout => write!(f, "응답 대기 시간 초과"),
        }
    }
}

use std::sync::OnceLock;
use tokio::sync::{Mutex, MutexGuard};

/// 사이트 브라우저 작업 직렬화: 동시에 두 요청이 같은 탭의 컴포저를 건드리지 않도록
/// send/rotate/probe 전체에 거는 프로세스 전역 잠금.
static SITE_OP: OnceLock<Mutex<()>> = OnceLock::new();
async fn site_op_lock() -> MutexGuard<'static, ()> {
    SITE_OP.get_or_init(|| Mutex::new(())).lock().await
}

/// 전용 Chrome 실행 직렬화: 두 요청이 동시에 Chrome을 띄워
/// 디버그 포트(9333)를 두고 경합하는 것을 막는다.
static CHROME_SPAWN: OnceLock<Mutex<()>> = OnceLock::new();
async fn chrome_spawn_lock() -> MutexGuard<'static, ()> {
    CHROME_SPAWN.get_or_init(|| Mutex::new(())).lock().await
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://")?.1;
    let hostport = rest.split(['/', '?', '#']).next()?;
    let host = hostport.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

#[derive(Debug)]
pub struct SendOutcome {
    pub text: String,
    pub limit_hit: bool,
    /// 마지막 assistant 응답의 코드 블록들 (pre.innerText, DOM 순서)
    pub code_blocks: Vec<String>,
    /// 마감 시간까지 세대 완료를 확인하지 못하고 끝난 부분 응답이면 true
    pub truncated: bool,
    /// 마지막 assistant 요소 안의 이미지 URL들 (DOM 순서). CODEBRIDGE_IMAGE 마커와 매칭.
    pub images: Vec<String>,
}

pub struct CompletedPage {
    pub browser: Browser,
    pub page: Page,
}

#[derive(Clone, Copy)]
enum ImageOrDownload {
    Image,
    Download,
}

/// 표준 base64 디코더 (공백 허용, 패딩 필수). 실패 시 Err.
#[cfg(test)]
mod base64_tests {
    use super::*;

    #[test]
    fn base64_roundtrip_known() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello".to_vec());
        assert_eq!(base64_decode("AA==").unwrap(), vec![0u8]);
        assert_eq!(base64_decode("////").unwrap(), vec![255u8; 3]);
    }

    #[test]
    fn base64_rejects_garbage() {
        assert!(base64_decode("!!").is_err());
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &b in bytes {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let v = TABLE.iter().position(|&x| x == b).ok_or_else(|| format!("잘못된 문자: {}", b as char))? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

fn js_string(s: &str) -> String {
    // JSON 문자열은 U+2028/U+2029를 이스케이프하지 않는다.
    // 이 둘은 JS 파서에서 "줄 종결자"로 취급되어 문자열이 중간에서 끊기므로
    // (SyntaxError: Unexpected string) 반드시 \u2028/\u2029로 변환한다.
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

async fn eval_str(page: &Page, js: &str) -> Result<String, EngineError> {
    let v = page
        .evaluate(js)
        .await
        .map_err(|e| {
            // 실패한 스크립트 전문을 파일로 덤프한다 (디버깅의 핵심)
            let dir = std::env::temp_dir().join("opencode");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("eval_str_failed.js"), js);
            let snippet: String = js.chars().take(400).collect();
            println!("[browser] eval_str FAILED ({}chars): {}", js.chars().count(), snippet);
            EngineError::Page(e.to_string())
        })?;
    Ok(match v.value() {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
}

fn chrome_exe() -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        std::path::PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(std::path::PathBuf::from(local).join(r"Google\Chrome\Application\chrome.exe"));
    }
    candidates.into_iter().find(|p| p.exists())
}

fn chrome_profile_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("CODEBRIDGE_CHROME_PROFILE") {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return std::path::PathBuf::from(appdata).join("CodeBridge").join("chrome-profile");
    }
    std::path::PathBuf::from(".codebridge-chrome-profile")
}

/// 127.0.0.1:port/path 가 HTTP 200이면 true (CDP 생존 확인, 의존성 없이 raw TCP).
/// 연결/쓰기/읽기에 타임아웃을 걸고, 본문이 끝나기를 기다리지 않고 상태 라인이 오는지만 본다.
/// 주의: Chrome 157+ 의 devtools HTTP 서버는 HTTP/1.0 요청에는 응답하지 않는다 (HTTP/1.1 필수).
async fn http_ok(port: u16, path: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let task = async move {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).await.ok()?;
        let mut buf = [0u8; 512];
        let n = stream.read(&mut buf).await.ok()?;
        let head = String::from_utf8_lossy(&buf[..n]);
        Some(head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"))
    };
    match tokio::time::timeout(std::time::Duration::from_secs(4), task).await {
        Ok(Some(v)) => v,
        _ => false,
    }
}

pub struct Engine {
    pub cdp_port: u16,
}

impl Engine {
    pub fn new(cdp_port: u16) -> Self {
        Engine { cdp_port }
    }

    async fn spawn_handler(&self) -> Result<Browser, EngineError> {
        let endpoint = format!("http://127.0.0.1:{}", self.cdp_port);
        let (browser, mut handler) = Browser::connect(endpoint)
            .await
            .map_err(|e| EngineError::Connect(e.to_string()))?;
        tokio::spawn(async move {
            while handler.next().await.is_some() {}
        });
        Ok(browser)
    }

    /// 탭을 찾는다. allow_create=false이면 없을 때 None을 돌려준다 (probe처럼 읽기 전용인 경우).
    async fn find_tab(&self, browser: &Browser, site: Site, allow_create: bool) -> Result<Option<Page>, EngineError> {
        // 대상 탭이 아직 로딩 중(about:blank 등)일 수 있으므로, 매칭되는 탭이 생길 때까지
        // 재시도한 뒤에도 없을 때만 새 탭을 만든다 (중복 탭 방지).
        let deadline = Instant::now() + Duration::from_secs(if allow_create { 12 } else { 6 });
        let want_host = site_selectors(site).host.to_lowercase();
        loop {
            let pages = browser.pages().await.unwrap_or_default();
            let mut fallback = None;
            for p in pages {
                let u = p.url().await.ok().flatten().unwrap_or_default();
                // 정확한 origin(host) 검증: https + 정확히 일치하는 호스트만 신뢰 (example.com/chatgpt.com/c/x 같은 피싱 방지)
                let ok = u.starts_with("https://")
                    && host_of(&u).map(|h| h == want_host).unwrap_or(false);
                if ok {
                    // 대화/앱 URL 우선 (gemini.google.com/app/..., chatgpt.com/c/...)
                    if site == Site::Gemini && u.contains("/app") {
                        return Ok(Some(p));
                    }
                    if site == Site::ChatGpt && u.contains("/c/") {
                        return Ok(Some(p));
                    }
                    if fallback.is_none() {
                        fallback = Some(p);
                    }
                }
            }
            if let Some(f) = fallback {
                return Ok(Some(f));
            }
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if !allow_create {
            return Ok(None);
        }
        browser
            .new_page(if site == Site::Gemini {
                "https://gemini.google.com/app"
            } else {
                "https://chatgpt.com/"
            })
            .await
            .map(Some)
            .map_err(|e| EngineError::Page(e.to_string()))
    }

    async fn wait_for_composer(&self, page: &Page, site: Site, timeout_ms: u64) -> Result<bool, EngineError> {
        let sel = site_selectors(site).composer_sel;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if Instant::now() >= deadline {
                return Ok(false);
            }
            let r = eval_str(
                page,
                &format!(r#"String(!!document.querySelector({sel}))"#, sel = js_string(sel)),
            )
            .await?;
            if r == "true" {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn clear_composer(&self, page: &Page, site: Site) -> Result<(), EngineError> {
        let sel = site_selectors(site).composer_sel;
        let r = eval_str(
            page,
            &format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'missing';
                    el.focus();
                    document.execCommand('selectAll', false, null);
                    document.execCommand('delete', false, null);
                    return 'cleared';
                }})()"#,
                sel = js_string(sel)
            ),
        )
        .await?;
        if r != "cleared" {
            return Err(EngineError::Page(format!("composer clear failed: {r}")));
        }
        Ok(())
    }

    async fn click_send(&self, page: &Page, site: Site) -> Result<(), EngineError> {
        let sel = site_selectors(site);
        if sel.send_native {
            // 입력 직후 Angular가 버튼을 렌더링할 때까지 폴링 (최대 8초)
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                let found = eval_str(
                    page,
                    r#"String(!!document.querySelector("button[aria-label*='보내'], button[aria-label='Send message']"))"#,
                )
                .await?;
                if found == "true" {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(EngineError::Page(
                        "전송 버튼이 활성화되지 않았습니다 (텍스트 입력 미반영?)".into(),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        if sel.send_native {
            // Angular Material 버튼: CDP Element 핸들 클릭은 타임아웃됨 →
            // JS로 좌표를 읽어 CDP 마우스 이벤트로 클릭 (2026-11-10 실측으로 확인된 동작 경로)
            let rect_json = eval_str(
                page,
                r#"(() => {
                    const btn = document.querySelector("button[aria-label*='보내'], button[aria-label='Send message']");
                    if (!btn) return '{}';
                    const r = btn.getBoundingClientRect();
                    return JSON.stringify({ x: r.x, y: r.y, w: r.width, h: r.height });
                })()"#,
            )
            .await?;
            let v: Value = serde_json::from_str(&rect_json).unwrap_or(Value::Null);
            if v.get("x").is_none() {
                return Err(EngineError::Page("전송 버튼을 찾지 못했습니다".into()));
            }
            let cx = v["x"].as_f64().unwrap_or(0.0) + v["w"].as_f64().unwrap_or(0.0) / 2.0;
            let cy = v["y"].as_f64().unwrap_or(0.0) + v["h"].as_f64().unwrap_or(0.0) / 2.0;
            page.click(chromiumoxide::layout::Point::new(cx, cy))
                .await
                .map_err(|e| EngineError::Page(e.to_string()))?;
            return Ok(());
        }
        let r = eval_str(
            page,
            &format!(
                r#"(() => {{
                    const btn = document.querySelector({sel});
                    if (!btn) return 'missing';
                    if (btn.disabled) return 'disabled';
                    btn.click();
                    return 'clicked';
                }})()"#,
                sel = js_string(sel.send_sel)
            ),
        )
        .await?;
        if r != "clicked" {
            return Err(EngineError::Page(format!("전송 버튼 클릭 실패: {r}")));
        }
        Ok(())
    }

    /// 한도 배너 감지 (F2.16 확정 감지).
    /// 대화 본문 전체가 아니라 배너성 요소(role=alert/dialog, 클래스에 banner/toast/limit 등)만
    /// 검사한다 — "rate limit" 같은 단어가 대화에 등장해도 한도로 오판하지 않도록.
    pub async fn is_limit_banner(&self, page: &Page) -> bool {
        let js = r#"(() => {
            const sel = [
                '[role="alert"]', '[role="dialog"]', '[aria-modal="true"]',
                '[class*="banner" i]', '[class*="toast" i]', '[class*="notice" i]',
                '[class*="limit" i]', '[class*="modal" i]', '[class*="warning" i]',
                '[class*="overlay" i]'
            ].join(',');
            const els = [...document.querySelectorAll(sel)];
            if (els.length === 0) return '';
            return els.map(e => (e.innerText || '')).join('\n').slice(0, 4000);
        })()"#;
        let Ok(text) = eval_str(page, js).await else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let lower = text.to_lowercase();
        LIMIT_TEXTS.iter().any(|t| lower.contains(t))
    }

    /// 컴포저 존재 여부/URL/타이틀을 읽어 올 수 있는지만 확인한다 (로그인 점검용).
    /// Chrome이 안 떠 있으면 탭을 만들지 않고 "chrome-not-running"을 돌려준다 (읽기 전용).
    pub async fn probe_site(&self, site: Site) -> Result<(bool, String, String), EngineError> {
        // probe는 읽기 전용이므로 전송 작업 잠금을 잡지 않는다 (생성 중 UI 폴링 지연 방지).
        let sel = site_selectors(site).composer_sel;
        if !http_ok(self.cdp_port, "/json/version").await {
            return Ok((false, String::new(), "chrome-not-running".to_string()));
        }
        let browser = self.spawn_handler().await?;
        let page = match self.find_tab(&browser, site, false).await? {
            Some(p) => p,
            None => return Ok((false, String::new(), "chrome-not-running".to_string())),
        };
        let js = format!(
            r#"(() => JSON.stringify({{
                composer: !!document.querySelector({sel}),
                url: location.href,
                title: document.title
            }}))()"#,
            sel = js_string(sel)
        );
        let raw = eval_str(&page, &js).await?;
        drop(browser);
        let v: Value = serde_json::from_str(&raw).map_err(|e| EngineError::Page(e.to_string()))?;
        let composer = v.get("composer").and_then(|x| x.as_bool()).unwrap_or(false);
        let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
        Ok((composer, url, title))
    }

    /// 이전 동작과의 호환: ChatGPT probe
    pub async fn probe(&self) -> Result<(bool, String, String), EngineError> {
        self.probe_site(Site::ChatGpt).await
    }

    /// "새 채팅" 버튼을 클릭해 새 스레드를 만든다 (F2.18/19)
    pub async fn rotate_new_chat_site(&self, site: Site) -> Result<(), EngineError> {
        let _op_guard = site_op_lock().await;
        let sel = site_selectors(site);
        let browser = self.spawn_handler().await?;
        let page = self
            .find_tab(&browser, site, true)
            .await?
            .ok_or_else(|| EngineError::Page("새 채팅 탭을 찾거나 열지 못했습니다".into()))?;
        let r = eval_str(
            &page,
            &format!(
                r#"(() => {{
                    const btn = document.querySelector({sel});
                    if (!btn) return 'missing';
                    btn.click();
                    return 'clicked';
                }})()"#,
                sel = js_string(sel.new_chat_sel)
            ),
        )
        .await?;
        if r != "clicked" {
            return Err(EngineError::Page("새 채팅 버튼을 찾지 못했습니다".into()));
        }
        tokio::time::sleep(Duration::from_millis(1200)).await;
        if !self.wait_for_composer(&page, site, 20_000).await? {
            return Err(EngineError::Page(
                "새 채팅 후 입력창이 준비되지 않았습니다 (20초 초과)".into(),
            ));
        }
        drop(browser);
        Ok(())
    }

    /// 이전 동작과의 호환: ChatGPT rotate
    pub async fn rotate_new_chat(&self) -> Result<(), EngineError> {
        self.rotate_new_chat_site(Site::ChatGpt).await
    }

    fn dedicated_chrome_pid_path() -> std::path::PathBuf {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("CodeBridge");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("dedicated-chrome.pid");
        }
        std::path::PathBuf::from("dedicated-chrome.pid")
    }

    fn write_dedicated_chrome_pid(pid: u32) {
        let _ = std::fs::write(Self::dedicated_chrome_pid_path(), pid.to_string());
    }

    /// 전용 Chrome의 현재 pid를 돌려준다 (파일 존재 + 프로세스 살아있음 확인).
    /// 없거나 죽었으면 None.
    pub fn dedicated_chrome_pid() -> Option<u32> {
        std::fs::read_to_string(Self::dedicated_chrome_pid_path())
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|pid| super::winhide::process_alive(*pid))
    }

    /// 전용 Chrome이 CDP에 응답하지 않으면 그때 띄운다 (지연 실행).
    async fn spawn_chrome(&self, site: Site, hide: bool) -> Result<u32, EngineError> {
        let exe = chrome_exe().ok_or_else(|| {
            EngineError::Connect("Chrome 실행 파일을 찾지 못했습니다".into())
        })?;
        let profile = chrome_profile_dir();
        let _ = std::fs::create_dir_all(&profile);
        // 크래시 세션이 남아 있으면 옛 컴포저에 채워진 채 멈춰 있던 탭이 복원되어
        // 새 대화를 오염시킨다. 전용 프로필의 세션 스냅샷을 지우고 깨끗하게 시작한다.
        let default_dir = profile.join("Default");
        for rel in ["Current Session", "Current Tabs", "Last Session", "Last Tabs"] {
            let _ = std::fs::remove_file(default_dir.join(rel));
        }
        for dir in ["Sessions", "Sessions_Encrypted"] {
            if let Ok(entries) = std::fs::read_dir(default_dir.join(dir)) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let url = match site {
            Site::Gemini => "https://gemini.google.com/app",
            Site::ChatGpt => "https://chatgpt.com/",
        };
        let child = std::process::Command::new(&exe)
            .args([
                &format!("--remote-debugging-port={}", self.cdp_port),
                &format!("--user-data-dir={}", profile.display()),
                // 창을 화면 밖으로 — 어차피 SW_HIDE로 완전히 숨긴다
                "--window-position=-32000,-32000",
                "--window-size=1200x800",
                "--disable-session-crashed-bubble",
                "--no-first-run",
                url,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| EngineError::Connect(format!("전용 Chrome 실행 실패: {e}")))?;
        let chrome_pid = child.id();
        drop(child);
        // 콜드 스타트(크래시 세션 복구 포함)는 20초를 넘길 수 있다 — 최대 45초까지 기다린다.
        // 창이 생기는 시점이 제각각이므로 대기 루프 안에서 계속 숨김/표시를 시도한다.
        let mut done = false;
        for _ in 0..90 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if !done {
                if hide {
                    if super::winhide::hide_chrome_windows_for_pid(chrome_pid) > 0 {
                        done = true;
                        tracing::info!("전용 Chrome 창을 숨겼습니다 (pid={})", chrome_pid);
                    }
                } else if super::winhide::show_chrome_windows_for_pid(chrome_pid) > 0 {
                    done = true;
                    tracing::info!("전용 Chrome 창을 화면에 표시했습니다 (pid={})", chrome_pid);
                }
            }
            if http_ok(self.cdp_port, "/json/version").await {
                // 창이 늦게 생기는 경우를 위해 짧게 한 번 더 시도
                for _ in 0..20 {
                    if !done {
                        if hide {
                            if super::winhide::hide_chrome_windows_for_pid(chrome_pid) > 0 {
                                done = true;
                                tracing::info!("전용 Chrome 창을 숨겼습니다 (pid={})", chrome_pid);
                                break;
                            }
                        } else if super::winhide::show_chrome_windows_for_pid(chrome_pid) > 0 {
                            done = true;
                            tracing::info!("전용 Chrome 창을 화면에 표시했습니다 (pid={})", chrome_pid);
                            break;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Self::write_dedicated_chrome_pid(chrome_pid);
                return Ok(chrome_pid);
            }
        }
        Err(EngineError::Connect(
            "전용 Chrome이 뜨지 않았습니다 (45초 대기)".into(),
        ))
    }

    /// 전용 Chrome이 CDP에 응답하지 않으면 그때 띄운다 (지연 실행).
    async fn ensure_chrome(&self, site: Site) -> Result<(), EngineError> {
        if http_ok(self.cdp_port, "/json/version").await {
            return Ok(());
        }
        // 두 요청이 동시에 도착해도 Chrome은 하나만 띄운다:
        // 먼저 온 요청이 띄우는 동안 나머지는 기다렸다가 재사용한다.
        let _spawn_guard = chrome_spawn_lock().await;
        if http_ok(self.cdp_port, "/json/version").await {
            return Ok(());
        }
        // settings의 "전용 Chrome 항상 보이기"(CODEBRIDGE_CHROME_KEEP_VISIBLE=1)면 숨기지 않고 띄운다.
        // 이미 실행 중인 Chrome에는 소급 적용되지 않고, 다음 기동부터 반영된다.
        let keep_visible = std::env::var("CODEBRIDGE_CHROME_KEEP_VISIBLE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        self.spawn_chrome(site, !keep_visible).await.map(|_| ())
    }

    /// pid가 알려지지 않은(예: 숨겨진) Chrome을 보이게 새로 띄울 때 사용한다.
    /// 두 show 요청이 동시에 와도 하나의 Chrome만 띄운다 (hide와 같은 잠금 + 재확인).
    pub async fn show_chrome_visible(&self, site: Site) -> Result<u32, EngineError> {
        if http_ok(self.cdp_port, "/json/version").await {
            return Err(EngineError::Connect("이미 실행 중입니다".into()));
        }
        let _guard = chrome_spawn_lock().await;
        if http_ok(self.cdp_port, "/json/version").await {
            return Err(EngineError::Connect("이미 실행 중입니다".into()));
        }
        self.spawn_chrome(site, false).await
    }

    /// 프롬프트를 전송하고, 완료될 때까지 기다린 뒤 assistant 텍스트를 돌려준다.
    /// 프로세스 전역 잠금으로 같은 탭에 대한 동시 작업을 직렬화한다.
    pub async fn send_and_await_site(&self, prompt: &str, site: Site) -> Result<SendOutcome, EngineError> {
        self.ensure_chrome(site).await?;
        let _op_guard = site_op_lock().await;
        let sel = site_selectors(site);
        let browser = self.spawn_handler().await?;
        let page = self
            .find_tab(&browser, site, true)
            .await?
            .ok_or_else(|| EngineError::Page("탭을 찾거나 열지 못했습니다".into()))?;
        if !self.wait_for_composer(&page, site, 45_000).await? {
            return Err(EngineError::Page(
                "컴포저를 찾지 못했습니다 (로그인 상태를 확인하세요)".into(),
            ));
        }

        self.clear_composer(&page, site).await?;

        // 전송 전 기존 마지막 assistant 응답을 기억 (이전 턴 응답을 새 응답으로 오인하지 않기 위함)
        let pre_sent = eval_str(
            &page,
            &format!(
                r#"(() => {{
                    const a = document.querySelectorAll({sel});
                    const el = a[a.length - 1];
                    return el ? el.innerText : '';
                }})()"#,
                sel = js_string(sel.assistant_sel)
            ),
        )
        .await
        .unwrap_or_default();

        // 입력 — execCommand가 숨김 창 + 새 페이지에서 실패할 수 있어 3단계 폴백을 쓴다.
        // 삽입 후 컴포저 내용을 읽어 실제로 반영됐는지 확인하고, 안 됐으면 잠시 후 재시도한다.
        let mut inserted = false;
        for attempt in 1..=3 {
            let insert = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'missing';
                    el.focus();
                    // ① execCommand('insertText')
                    try {{
                        if (document.execCommand && document.execCommand('insertText', false, {text})) return 'execCommand';
                    }} catch (e) {{}}
                    // ② ClipboardEvent paste 시뮬레이션 (새 페이지/숨김 창에서 더 잘 동작)
                    try {{
                        const dt = new DataTransfer();
                        dt.setData('text/plain', {text});
                        el.dispatchEvent(new ClipboardEvent('paste', {{ clipboardData: dt, bubbles: true, cancelable: true }}));
                        return 'paste';
                    }} catch (e) {{}}
                    // ③ 직접 텍스트 세팅 + InputEvent (최후 수단)
                    try {{
                        if (el.isContentEditable) {{
                            el.innerText = {text};
                        }} else {{
                            const setter = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value').set;
                            setter.call(el, {text});
                        }}
                        el.dispatchEvent(new InputEvent('input', {{ bubbles: true, data: {text} }}));
                        return 'direct';
                    }} catch (e) {{
                        return 'failed';
                    }}
                }})()"#,
                sel = js_string(sel.composer_sel),
                text = js_string(prompt),
            );
            let r = eval_str(&page, &insert).await?;
            if r != "missing" && r != "failed" {
                // 삽입 결과 확인 (contenteditable/textarea 모두 커버)
                let check = eval_str(
                    &page,
                    &format!(
                        r#"(() => {{ const el = document.querySelector({sel}); if (!el) return ''; return el.innerText !== undefined ? el.innerText : (el.value ?? ''); }})()"#,
                        sel = js_string(sel.composer_sel)
                    ),
                )
                .await
                .unwrap_or_default();
                if !check.trim().is_empty() {
                    println!("[browser] 입력 삽입 성공 (attempt {attempt}, 방법={r})");
                    inserted = true;
                    break;
                }
                println!("[browser] 입력 삽입 후 확인 실패 (attempt {attempt}, 방법={r}): 컴포저가 비어 있음 — 재시도");
            } else {
                println!("[browser] 입력 삽입 실패 (attempt {attempt}): {r} — 재시도");
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        if !inserted {
            return Err(EngineError::Page(
                "입력 실패: 3회 재시도 후에도 컴포저에 텍스트가 반영되지 않았습니다".into(),
            ));
        }

        // Gemini: 입력 후 500ms 안정화 대기 + 'input' 이벤트 디스패치 (Angular 상태 동기화)
        if site == Site::Gemini {
            let _ = eval_str(
                &page,
                r#"(() => {
                    const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
                    if (!el) return 'missing';
                    el.dispatchEvent(new InputEvent('input', { bubbles: true, data: ' ' }));
                    el.dispatchEvent(new Event('change', { bubbles: true }));
                    return 'dispatched';
                })()"#,
            )
            .await;
            tokio::time::sleep(Duration::from_millis(400)).await;
        }

        // 전송
        self.click_send(&page, site).await?;

        // 완료 판정: 마지막 assistant 텍스트가 stable_needed회 연속 동일 & stop 버튼 없음
        // 단, 이번 전송에 의한 세대(텍스트 변화 또는 stop 버튼)를 최소 1회는 관찰해야 완료로 인정한다.
        // 그렇지 않으면 마감 시 truncated=true 로 부분 응답을 명시한다.
        let stable_needed = 4usize;
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut last = String::new();
        let mut stable = 0usize;
        let mut seen = false;
        let mut saw_generation = false;
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let cur = eval_str(
                &page,
                &format!(
                    r#"(() => {{
                        const a = document.querySelectorAll({sel});
                        const el = a[a.length - 1];
                        return el ? el.innerText : '';
                    }})()"#,
                    sel = js_string(sel.assistant_sel)
                ),
            )
            .await
            .unwrap_or_default();
            let stop = eval_str(
                &page,
                &format!(r#"String(!!document.querySelector({sel}))"#, sel = js_string(sel.stop_sel)),
            )
            .await
            .unwrap_or_else(|_| "false".into());

            if stop == "true" {
                saw_generation = true;
            }
            if !cur.is_empty() {
                seen = true;
            }
            if !cur.is_empty() && cur != pre_sent {
                saw_generation = true;
            }
            if cur == last && seen && stop == "false" && saw_generation {
                stable += 1;
            } else {
                stable = 0;
            }
            last = cur;
            if stable >= stable_needed {
                break;
            }
        }
        let truncated = stable < stable_needed;
        if last.is_empty() {
            return Err(EngineError::Timeout);
        }

        // 마지막 assistant 컨테이너의 코드 블록만 추출
        let blocks_json = eval_str(
            &page,
            &format!(
                r#"(() => {{
                    const roots = [...document.querySelectorAll({root})];
                    const root = roots[roots.length - 1];
                    if (!root) return '[]';
                    return JSON.stringify([...root.querySelectorAll('pre')].map(e => e.innerText));
                }})()"#,
                root = js_string(sel.code_root_sel)
            ),
        )
        .await
        .unwrap_or_else(|_| "[]".to_string());
        let code_blocks: Vec<String> =
            serde_json::from_str(&blocks_json).unwrap_or_default();

        // 마지막 assistant 요소 안의 이미지 URL들 (src/currentSrc 우선)
        let imgs_json = eval_str(
            &page,
            &format!(
                r#"(() => {{
                    const a = document.querySelectorAll({sel});
                    const el = a[a.length - 1];
                    if (!el) return '[]';
                    const imgs = [...el.querySelectorAll('img')].map(i => i.currentSrc || i.src || '').filter(s => /^https?:/.test(s));
                    return JSON.stringify(imgs);
                }})()"#,
                sel = js_string(sel.assistant_sel)
            ),
        )
        .await
        .unwrap_or_else(|_| "[]".to_string());
        let images: Vec<String> = serde_json::from_str(&imgs_json).unwrap_or_default();

        let limit_hit = self.is_limit_banner(&page).await;

        // CODEBRIDGE_IMAGE 마커가 있으면 각 마커 순서와 이미지 순서를 맞춰 파일로 저장한다.
        let last = self.persist_images(&page, &last, &images).await;

        drop(browser); // 브라우저는 유지 (로그인 세션)
        Ok(SendOutcome { text: last, limit_hit, code_blocks, truncated, images })
    }

    /// 이전 동작과의 호환: ChatGPT send
    pub async fn send_and_await(&self, prompt: &str) -> Result<SendOutcome, EngineError> {
        self.send_and_await_site(prompt, Site::ChatGpt).await
    }

    /// CODEBRIDGE_IMAGE/CODEBRIDGE_DOWNLOAD 마커가 있으면 파일을 다운로드하여
    /// 프로젝트 루트(현재 cwd) 기준 path에 저장하고,
    /// 마커를 "💾 path (N KB) 저장됨" 으로 교체한 텍스트를 돌려준다.
    ///
    /// 형식:
    ///   CODEBRIDGE_IMAGE {"path":"assets/logo.png"}                     — 이미지(DOM 순서 매칭)
    ///   CODEBRIDGE_DOWNLOAD {"path":"downloads/x.pdf", "url":"https://..."} — GPT가 URL을 아는 외부 파일
    async fn persist_images(&self, page: &Page, text: &str, images: &[String]) -> String {
        if !text.contains("CODEBRIDGE_") {
            return text.to_string();
        }
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut out: Vec<String> = Vec::with_capacity(text.lines().count() + 4);
        let mut img_idx = 0usize;
        for line in text.split('\n') {
            let trimmed = line.trim();
            // 공통 JSON 본문 추출
            let marker = if let Some(body) = trimmed.strip_prefix("CODEBRIDGE_IMAGE ") {
                Some((ImageOrDownload::Image, body))
            } else if let Some(body) = trimmed.strip_prefix("CODEBRIDGE_DOWNLOAD ") {
                Some((ImageOrDownload::Download, body))
            } else {
                None
            };
            let Some((kind, body)) = marker else {
                out.push(line.to_string());
                continue;
            };
            let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
            let rel = parsed.as_ref().and_then(|v| v.get("path").and_then(|p| p.as_str())).map(str::to_string);
            let Some(rel) = rel else {
                out.push(line.to_string());
                continue;
            };
            let Some(rel) = crate::translate::lenient::sanitize_path(&rel) else {
                out.push(format!("(파일 경로 거부: {})", rel));
                continue;
            };
            let target = root.join(&rel);
            if !target.starts_with(&root) {
                out.push(format!("(파일 경로 거부: {})", rel));
                continue;
            }
            match kind {
                ImageOrDownload::Image => {
                    let Some(url) = images.get(img_idx).cloned() else {
                        out.push(format!("(이미지 {}번째가 없습니다)", img_idx + 1));
                        continue;
                    };
                    img_idx += 1;
                    match self.download_via_cdp(page, &url, &target).await {
                        Ok(bytes) => out.push(format!("💾 {} ({} KB) 저장됨", rel, bytes / 1024)),
                        Err(e) => out.push(format!("(파일 저장 실패 — {}: {})", rel, e)),
                    }
                }
                ImageOrDownload::Download => {
                    let Some(url) = parsed
                        .as_ref()
                        .and_then(|v| v.get("url").and_then(|u| u.as_str()))
                        .map(str::to_string)
                    else {
                        out.push(format!("(다운로드 마커에 URL 없음: {})", rel));
                        continue;
                    };
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        out.push(format!("(다운로드 URL이 http(s)가 아님: {})", rel));
                        continue;
                    }
                    match self.download_via_cdp(page, &url, &target).await {
                        Ok(bytes) => out.push(format!("💾 {} ({} KB) 저장됨", rel, bytes / 1024)),
                        Err(e) => out.push(format!("(다운로드 실패 — {}: {})", rel, e)),
                    }
                }
            }
        }
        out.join("\n")
    }

    /// 파일 URL을 CDP fetch로 받아 base64로 디코드하여 파일에 쓴다.
    /// (Chrome의 챗GPT 세션 쿠키를 그대로 사용하므로 로그인이 필요한 이미지 URL도 받을 수 있다)
    async fn download_via_cdp(
        &self,
        page: &Page,
        url: &str,
        target: &std::path::Path,
    ) -> Result<usize, EngineError> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| EngineError::Page(format!("디렉터리 생성 실패: {e}")))?;
        }
        let js = format!(
            r#"(async () => {{
                try {{
                    const r = await fetch({url}, {{ credentials: 'include' }});
                    if (!r.ok) return JSON.stringify({{ ok: false, status: r.status }});
                    const b = await r.arrayBuffer();
                    const bytes = new Uint8Array(b);
                    let bin = '';
                    const CHUNK = 0x8000;
                    for (let i = 0; i < bytes.length; i += CHUNK) {{
                        bin += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
                    }}
                    return JSON.stringify({{ ok: true, data: btoa(bin) }});
                }} catch (e) {{
                    return JSON.stringify({{ ok: false, error: String(e) }});
                }}
            }})()"#,
            url = js_string(url)
        );
        let res = eval_str(page, &js).await?;
        let v: serde_json::Value = serde_json::from_str(&res)
            .map_err(|e| EngineError::Page(format!("다운로드 응답 파싱 실패: {e}")))?;
        if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
            let data = v
                .get("data")
                .and_then(|d| d.as_str())
                .ok_or_else(|| EngineError::Page("파일 데이터 없음".into()))?;
            let bytes = base64_decode(data)
                .map_err(|e| EngineError::Page(format!("파일 디코드 실패: {e}")))?;
            std::fs::write(target, &bytes)
                .map_err(|e| EngineError::Page(format!("파일 쓰기 실패: {e}")))?;
            Ok(bytes.len())
        } else {
            let why = v.get("status")
                .and_then(|s| s.as_i64())
                .map(|s| format!("HTTP {s}"))
                .or_else(|| v.get("error").and_then(|s| s.as_str()).map(str::to_string))
                .unwrap_or_else(|| "알 수 없는 오류".into());
            Err(EngineError::Page(format!("파일 다운로드 실패: {why}")))
        }
    }
}
