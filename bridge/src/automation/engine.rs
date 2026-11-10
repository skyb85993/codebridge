//! CDP 엔진 (Phase 1~2 핵심):
//! 전용 Chrome(CDP 포트)에 붙어 ChatGPT 탭을 찾아 입력→전송→완료 감지→응답 추출.
//! 선택자 근거: 2026-11-10 실측 (docs/spike-report.md, profiles.toml 참고).

use std::time::{Duration, Instant};

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures::StreamExt;
use serde_json::Value;

pub const COMPOSER_SEL: &str =
    "textarea#prompt-textarea, div#prompt-textarea, [contenteditable='true']";
const ASSISTANT_SEL: &str =
    "div[class*='DilResponseRoot'], [class*='entity-prose-inset']";
const SEND_SEL: &str =
    "button[data-testid='send-button'], button[aria-label='보내기'], button[aria-label='Send prompt'], button[aria-label='Send']";
const STOP_SEL: &str =
    "button[aria-label='중지'], button[data-testid='stop-button'], button[aria-label='Stop generating'], button[aria-label='Stop streaming']";
const NEW_CHAT_SEL: &str =
    "button[aria-label='새 채팅'], button[aria-label='New chat'], button[data-testid='create-new-chat-button']";

/// profiles.toml [profiles.chatgpt].limit_texts와 동기화 (한/영)
pub const LIMIT_TEXTS: &[&str] = &["limit reached", "usage cap", "메시지 한도", "사용 한도"];

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

#[derive(Debug)]
pub struct SendOutcome {
    pub text: String,
    pub limit_hit: bool,
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

    async fn find_chatgpt_tab(&self, browser: &Browser) -> Result<Page, EngineError> {
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
        for p in pages {
            let u = p.url().await.ok().flatten().unwrap_or_default();
            if u.contains("chatgpt.com/c/") {
                return Ok(p); // 대화 탭 우선
            }
            if u.contains("chatgpt") || u.contains("auth") {
                fallback = Some(p);
            }
        }
        match fallback {
            Some(p) => Ok(p),
            None => browser
                .new_page("https://chatgpt.com/")
                .await
                .map_err(|e| EngineError::Page(e.to_string())),
        }
    }

    async fn wait_for_composer(&self, page: &Page, timeout_ms: u64) -> Result<bool, EngineError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if Instant::now() >= deadline {
                return Ok(false);
            }
            let r = eval_str(
                page,
                &format!(
                    r#"String(!!document.querySelector({sel}))"#,
                    sel = js_string(COMPOSER_SEL)
                ),
            )
            .await?;
            if r == "true" {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// 컴포저를 비운다.
    async fn clear_composer(&self, page: &Page) -> Result<(), EngineError> {
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
                sel = js_string(COMPOSER_SEL)
            ),
        )
        .await?;
        if r != "cleared" {
            return Err(EngineError::Page(format!("composer clear failed: {r}")));
        }
        Ok(())
    }

    /// 페이지 텍스트에 한도 배너가 보이면 true (F2.16 확정 감지).
    pub async fn is_limit_banner(&self, page: &Page) -> bool {
        let js = r#"(document.body ? document.body.innerText : '')"#.to_string();
        let Ok(text) = eval_str(page, &js).await else {
            return false;
        };
        let lower = text.to_lowercase();
        LIMIT_TEXTS.iter().any(|t| lower.contains(t))
    }

    /// "새 채팅" 버튼을 클릭해 새 스레드를 만든다 (F2.18/19).
    pub async fn rotate_new_chat(&self) -> Result<(), EngineError> {
        let browser = self.spawn_handler().await?;
        let page = self.find_chatgpt_tab(&browser).await?;
        let r = eval_str(
            &page,
            &format!(
                r#"(() => {{
                    const btn = document.querySelector({sel});
                    if (!btn) return 'missing';
                    btn.click();
                    return 'clicked';
                }})()"#,
                sel = js_string(NEW_CHAT_SEL)
            ),
        )
        .await?;
        if r != "clicked" {
            return Err(EngineError::Page("새 채팅 버튼을 찾지 못했습니다".into()));
        }
        // 새 대화 화면이 될 때까지 컴포저 대기
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let _ = self.wait_for_composer(&page, 20_000).await?;
        drop(browser);
        Ok(())
    }

    /// 컴포저 존재 여부/URL/타이틀을 읽어 올 수 있는지만 확인한다 (로그인 점검용).
    pub async fn probe(&self) -> Result<(bool, String, String), EngineError> {
        let browser = self.spawn_handler().await?;
        let page = self.find_chatgpt_tab(&browser).await?;
        let js = format!(
            r#"(() => JSON.stringify({{
                composer: !!document.querySelector({sel}),
                url: location.href,
                title: document.title
            }}))()"#,
            sel = js_string(COMPOSER_SEL)
        );
        let raw = eval_str(&page, &js).await?;
        drop(browser);
        let v: Value = serde_json::from_str(&raw).map_err(|e| EngineError::Page(e.to_string()))?;
        let composer = v.get("composer").and_then(|x| x.as_bool()).unwrap_or(false);
        let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
        Ok((composer, url, title))
    }

    pub async fn send_and_await(&self, prompt: &str) -> Result<SendOutcome, EngineError> {
        let browser = self.spawn_handler().await?;
        let page = self.find_chatgpt_tab(&browser).await?;
        if !self.wait_for_composer(&page, 45_000).await? {
            return Err(EngineError::Page(
                "컴포저를 찾지 못했습니다 (로그인 상태를 확인하세요)".into(),
            ));
        }

        self.clear_composer(&page).await?;

        // 입력
        let insert = format!(
            r#"(() => {{
                const el = document.querySelector({sel});
                if (!el) return 'missing';
                el.focus();
                const ok = document.execCommand('insertText', false, {text});
                return ok ? 'ok' : 'failed';
            }})()"#,
            sel = js_string(COMPOSER_SEL),
            text = js_string(prompt),
        );
        let r = eval_str(&page, &insert).await?;
        if r != "ok" {
            return Err(EngineError::Page(format!("입력 실패: {r}")));
        }

        // 전송
        let click = format!(
            r#"(() => {{
                const btn = document.querySelector({sel});
                if (!btn) return 'missing';
                if (btn.disabled) return 'disabled';
                btn.click();
                return 'clicked';
            }})()"#,
            sel = js_string(SEND_SEL),
        );
        let r = eval_str(&page, &click).await?;
        if r != "clicked" {
            return Err(EngineError::Page(format!("전송 버튼 클릭 실패: {r}")));
        }

        // 완료 판정: 마지막 assistant 텍스트가 stable_needed회 연속 동일 & stop 버튼 없음
        let stable_needed = 4usize;
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut last = String::new();
        let mut stable = 0usize;
        let mut seen = false;
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
                    sel = js_string(ASSISTANT_SEL)
                ),
            )
            .await
            .unwrap_or_default();
            let stop = eval_str(
                &page,
                &format!(r#"String(!!document.querySelector({sel}))"#, sel = js_string(STOP_SEL)),
            )
            .await
            .unwrap_or_else(|_| "false".into());

            if !cur.is_empty() {
                seen = true;
            }
            if cur == last && seen && stop == "false" {
                stable += 1;
            } else {
                stable = 0;
            }
            last = cur;
            if stable >= stable_needed {
                break;
            }
        }
        if last.is_empty() {
            return Err(EngineError::Timeout);
        }

        let limit_hit = self.is_limit_banner(&page).await;
        drop(browser); // 브라우저는 유지 (로그인 세션)
        Ok(SendOutcome { text: last, limit_hit })
    }
}
