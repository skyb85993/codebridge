//! Phase 0 스파이크 A — CDP(chromiumoxide)로 전용 Chrome을 연결해
//! chatgpt.com 자동화의 성패를 검증한다.
//!
//! 사용법:
//!   cargo run --example cdp_spike -- <port> probe          # 현재 상태 조회 (읽기 전용)
//!   cargo run --example cdp_spike -- <port> send "메시지"   # 실제 전송 + 응답 추출
//!
//! send 모드는: 컴포저 확인 → insertText로 입력 → 전송 버튼 클릭 →
//! 마지막 assistant 텍스트가 N회 연속 동일해질 때까지 폴링 → 추출.
use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::time::{Duration, Instant};

const COMPOSER_SEL: &str =
    "textarea#prompt-textarea, div#prompt-textarea, [contenteditable='true']";
const ASSISTANT_SEL: &str =
    "div[class*='DilResponseRoot'], [class*='entity-prose-inset']";
const SEND_SEL: &str =
    "button[data-testid='send-button'], button[aria-label='보내기'], button[aria-label='Send prompt'], button[aria-label='Send']";
const STOP_SEL: &str =
    "button[data-testid='stop-button'], button[aria-label='중지'], button[aria-label='Stop generating'], button[aria-label='Stop streaming']";

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

async fn eval_str(page: &Page, js: &str) -> Result<String, Box<dyn std::error::Error>> {
    let v = page.evaluate(js).await?;
    Ok(match v.value() {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
}

async fn wait_for_composer(page: &Page, timeout_ms: u64) -> Result<bool, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if Instant::now() >= deadline {
            return Ok(false);
        }
        let r = eval_str(
            page,
            &format!(
                r#"String(!!document.querySelector({sel}))"#,
                sel = js_string(COMPOSER_SEL),
            ),
        )
        .await
        .unwrap_or_else(|_| "false".into());
        if r == "true" {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn probe(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = format!(
        r#"(() => {{
            const composer = document.querySelector({sel_c});
            const loginBtn = [...document.querySelectorAll('button, a')]
                .find(e => /(log in|sign in|로그인)/i.test((e.textContent || '').trim()));
            return JSON.stringify({{
                title: document.title,
                url: location.href,
                composer: !!composer,
                loginBtn: !!loginBtn,
                stopBtn: !!document.querySelector({sel_stop}),
                sendBtn: !!document.querySelector({sel_send}),
                bodyText: (document.body ? document.body.innerText : '').slice(0, 400)
            }}, null, 2);
        }})()"#,
        sel_c = js_string(COMPOSER_SEL),
        sel_stop = js_string(STOP_SEL),
        sel_send = js_string(SEND_SEL),
    );
    println!("[probe]\n{}", eval_str(page, &js).await?);
    Ok(())
}

async fn tabs(browser: &chromiumoxide::browser::Browser) -> Result<(), Box<dyn std::error::Error>> {
    let pages = browser.pages().await.unwrap_or_default();
    for (i, p) in pages.into_iter().enumerate() {
        let u = p.url().await.ok().flatten().unwrap_or_default();
        let info = eval_str(
            &p,
            r#"(() => JSON.stringify({ asst: document.querySelectorAll('[data-message-author-role="assistant"]').length, tail: document.body ? document.body.innerText.slice(-160) : '' }))()"#,
        )
        .await
        .unwrap_or_else(|_| "?".into());
        println!("[tabs] #{i} {u}\n        {info}");
    }
    Ok(())
}

async fn find(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const out = [];
        const els = document.querySelectorAll('*');
        for (const el of els) {
            if (el.children.length > 0) continue; // 리프 노드만
            const t = el.innerText || '';
            if (!t.includes('PING')) continue;
            const chain = [];
            let n = el;
            for (let k = 0; k < 6 && n; k++) {
                chain.push({ tag: n.tagName, cls: (n.className || '').toString().slice(0, 80).replace(/\s+/g, ' '), testid: n.getAttribute('data-testid'), role: n.getAttribute('role'), turn: n.getAttribute('data-turn'), author: n.getAttribute('data-message-author-role') });
                n = n.parentElement;
            }
            out.push({ leaf: { tag: el.tagName, cls: (el.className || '').toString().slice(0, 60) }, chain });
        }
        return JSON.stringify(out, null, 1).slice(0, 6000);
    })()"#;
    println!("[find]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn dom(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const c = (s) => document.querySelectorAll(s).length;
        const previews = (s) => [...document.querySelectorAll(s)].map(e => (e.innerText || '').slice(0, 80));
        const roleBoth = c('[data-message-author-role]');
        const asst = c('[data-message-author-role="assistant"]');
        const user = c('[data-message-author-role="user"]');
        return JSON.stringify({
            roleBoth, asst, user,
            DilResponseRoot: c('[class*="DilResponseRoot"]'),
            DilResponseRootText: previews('[class*="DilResponseRoot"]'),
            entityInset: c('[class*="entity-prose-inset"]'),
            entityInsetText: previews('[class*="entity-prose-inset"]'),
            TextBase: c('[class*="TextBase"]'),
            bgUser: c('[class*="bg-user-message"]'),
            prewrap: c('.whitespace-pre-wrap'),
            my4: c('.my-4')
        }, null, 1);
    })()"#;
    println!("[dom]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn buttons(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const els = [...document.querySelectorAll('button, div[role="button"], a[role="button"]')];
        return JSON.stringify(els.map(e => ({
            tag: e.tagName,
            testid: e.getAttribute('data-testid'),
            aria: e.getAttribute('aria-label'),
            text: (e.innerText || '').trim().slice(0, 80),
            disabled: !!e.disabled,
            vis: !!(e.offsetWidth || e.offsetHeight)
        })), null, 1);
    })()"#;
    println!("[buttons]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn send(page: &Page, msg: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 1) 컴포저 비우기 (이전 잔여 텍스트 제거)
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
            sel = js_string(COMPOSER_SEL),
        ),
    )
    .await?;
    println!("[send] clear = {r}");

    // 2) insertText 입력
    let insert = format!(
        r#"(() => {{
            const el = document.querySelector({sel});
            if (!el) return 'missing';
            el.focus();
            const ok = document.execCommand('insertText', false, {text});
            return ok ? 'ok' : 'failed';
        }})()"#,
        sel = js_string(COMPOSER_SEL),
        text = js_string(msg),
    );
    let r = eval_str(page, &insert).await?;
    println!("[send] insert = {r}");
    if r != "ok" {
        return Ok(());
    }

    // 2) 전송 버튼 클릭
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
    let r = eval_str(page, &click).await?;
    println!("[send] click = {r}");
    if r != "clicked" {
        return Ok(());
    }

    // 3) 스트리밍 종료 대기: 마지막 assistant 텍스트가 stable_needed회 연속 동일 & stop 버튼 없음
    let stable_needed = 4;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last = String::new();
    let mut stable = 0usize;
    let mut seen_nonempty = false;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let cur = eval_str(
            page,
            &format!(
                r#"(() => {{ const a = document.querySelectorAll({sel}); const el = a[a.length - 1]; return el ? el.innerText : ''; }})()"#,
                sel = js_string(ASSISTANT_SEL),
            ),
        )
        .await
        .unwrap_or_default();
        let stop = eval_str(
            page,
            &format!(r#"String(!!document.querySelector({sel}))"#, sel = js_string(STOP_SEL)),
        )
        .await
        .unwrap_or_else(|_| "false".into());

        if !cur.is_empty() {
            seen_nonempty = true;
        }
        if cur == last && seen_nonempty && stop == "false" {
            stable += 1;
        } else {
            stable = 0;
        }
        last = cur.clone();
        if stable >= stable_needed {
            break;
        }
    }

    println!("[send] === last assistant message ({} chars) ===\n{last}", last.chars().count());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args().nth(1).unwrap_or_else(|| "9333".to_string());
    let mode = std::env::args().nth(2).unwrap_or_else(|| "probe".to_string());
    let endpoint = format!("http://127.0.0.1:{port}");
    println!("[cdp_spike] connecting to {endpoint} (mode={mode})");

    let (browser, mut handler) = Browser::connect(endpoint).await?;
    tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    if mode == "tabs" {
        tokio::time::sleep(Duration::from_millis(800)).await;
        tabs(&browser).await?;
        drop(browser);
        return Ok(());
    }

    // 기존 chatgpt 탭 재사용 → 없으면 새로 연다 (핸들러가 대상 목록을 수신할 때까지 짧게 재시도)
    let mut pages = browser.pages().await.unwrap_or_default();
    for _ in 0..12 {
        if !pages.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        pages = browser.pages().await.unwrap_or_default();
    }
    println!("[cdp_spike] open pages after retry: {}", pages.len());
    let mut target_page = None;
    let mut fallback = None;
    for p in pages {
        let u = p.url().await.ok().flatten().unwrap_or_default();
        if u.contains("chatgpt.com/c/") {
            // 대화 탭 우선 (자동화 대상)
            target_page = Some(p);
            break;
        }
        if u.contains("chatgpt") || u.contains("auth") {
            fallback = Some(p);
        }
    }
    let target_page = target_page.or(fallback);
    let page = match target_page {
        Some(p) => p,
        None => {
            println!("[cdp_spike] no chatgpt tab; opening one");
            browser.new_page("https://chatgpt.com/").await?
        }
    };

    match mode.as_str() {
        "probe" => probe(&page).await?,
        "buttons" => buttons(&page).await?,
        "dom" => dom(&page).await?,
        "find" => find(&page).await?,
        "send" => {
            if !wait_for_composer(&page, 30_000).await? {
                eprintln!("[send] composer not found in 30s (로그인 필요 또는 페이지 로드 실패)");
                return Ok(());
            }
            let msg = std::env::args()
                .nth(3)
                .unwrap_or_else(|| "PING이라고만 답해 주세요.".to_string());
            send(&page, &msg).await?
        }
        other => eprintln!("unknown mode: {other} (probe|send)"),
    }

    drop(browser); // 브라우저는 유지, CDP 연결만 종료
    Ok(())
}
