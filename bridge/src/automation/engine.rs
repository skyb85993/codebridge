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
}

pub struct CompletedPage {
    pub browser: Browser,
    pub page: Page,
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

async fn eval_str(page: &Page, js: &str) -> Result<String, EngineError> {
    let v = page
        .evaluate(js)
        .await
        .map_err(|e| EngineError::Page(e.to_string()))?;
    Ok(match v.value() {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
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

    async fn find_tab(&self, browser: &Browser, site: Site) -> Result<Page, EngineError> {
        // handler가 대상 목록을 받을 때까지 재시도
        let mut pages = browser.pages().await.unwrap_or_default();
        for _ in 0..12 {
            if !pages.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            pages = browser.pages().await.unwrap_or_default();
        }
        let mut fallback = None;
        let want_host = site_selectors(site).host.to_lowercase();
        for p in pages {
            let u = p.url().await.ok().flatten().unwrap_or_default();
            // 정확한 origin(host) 검증: https + 정확히 일치하는 호스트만 신뢰 (example.com/chatgpt.com/c/x 같은 피싱 방지)
            let ok = u.starts_with("https://")
                && host_of(&u).map(|h| h == want_host).unwrap_or(false);
            if ok {
                // 대화/앱 URL 우선 (gemini.google.com/app/..., chatgpt.com/c/...)
                if site == Site::Gemini && u.contains("/app") {
                    return Ok(p);
                }
                if site == Site::ChatGpt && u.contains("/c/") {
                    return Ok(p);
                }
                fallback = Some(p);
            }
        }
        match fallback {
            Some(p) => Ok(p),
            None => browser
                .new_page(if site == Site::Gemini {
                    "https://gemini.google.com/app"
                } else {
                    "https://chatgpt.com/"
                })
                .await
                .map_err(|e| EngineError::Page(e.to_string())),
        }
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

    /// 컴포저 존재 여부/URL/타이틀을 읽어 올 수 있는지만 확인한다 (로그인 점검용)
    pub async fn probe_site(&self, site: Site) -> Result<(bool, String, String), EngineError> {
        // probe는 읽기 전용이므로 전송 작업 잠금을 잡지 않는다 (생성 중 UI 폴링 지연 방지).
        let sel = site_selectors(site).composer_sel;
        let browser = self.spawn_handler().await?;
        let page = self.find_tab(&browser, site).await?;
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
        let page = self.find_tab(&browser, site).await?;
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

    /// 프롬프트를 전송하고, 완료될 때까지 기다린 뒤 assistant 텍스트를 돌려준다.
    /// 프로세스 전역 잠금으로 같은 탭에 대한 동시 작업을 직렬화한다.
    pub async fn send_and_await_site(&self, prompt: &str, site: Site) -> Result<SendOutcome, EngineError> {
        let _op_guard = site_op_lock().await;
        let sel = site_selectors(site);
        let browser = self.spawn_handler().await?;
        let page = self.find_tab(&browser, site).await?;
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

        // 입력
        let insert = format!(
            r#"(() => {{
                const el = document.querySelector({sel});
                if (!el) return 'missing';
                el.focus();
                const ok = document.execCommand('insertText', false, {text});
                return ok ? 'ok' : 'failed';
            }})()"#,
            sel = js_string(sel.composer_sel),
            text = js_string(prompt),
        );
        let r = eval_str(&page, &insert).await?;
        if r != "ok" {
            return Err(EngineError::Page(format!("입력 실패: {r}")));
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

        let limit_hit = self.is_limit_banner(&page).await;
        drop(browser); // 브라우저는 유지 (로그인 세션)
        Ok(SendOutcome { text: last, limit_hit, code_blocks, truncated })
    }

    /// 이전 동작과의 호환: ChatGPT send
    pub async fn send_and_await(&self, prompt: &str) -> Result<SendOutcome, EngineError> {
        self.send_and_await_site(prompt, Site::ChatGpt).await
    }
}
