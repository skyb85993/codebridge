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

async fn code_probe(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const dump = (sel) => [...document.querySelectorAll(sel)].map(e => ({
            cls: (e.className || '').toString().slice(0, 80),
            tag: e.tagName,
            testid: e.getAttribute('data-testid'),
            lang: e.getAttribute('data-language'),
            textHead: (e.innerText || '').slice(0, 120)
        }));
        return JSON.stringify({
            pre: dump('pre'),
            code: dump('code'),
            codeblock: dump('[class*="code-block"], [class*="CodeBlock"], [data-testid*="code"]'),
            my4code: dump('.my-4 code'),
            firstPreInner: (() => { const p = document.querySelector('pre'); return p ? p.innerText.slice(0, 200) : null })()
        }, null, 1);
    })()"#;
    println!("[code]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_hints(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const all = [...document.querySelectorAll('button, div[role="button"]')];
        const hit = all.filter(e => {
            const s = ((e.getAttribute('aria-label')||'') + ' ' + (e.innerText||'')).toLowerCase();
            return /전송|보내|send|마이크|음성|mic|voice|중지|stop|첨부|attach|plus|선호/.test(s);
        });
        return JSON.stringify(hit.map(e => ({
            tag: e.tagName,
            aria: e.getAttribute('aria-label'),
            testid: e.getAttribute('data-testid'),
            text: (e.innerText || '').slice(0, 60),
            cls: (e.className || '').toString().slice(0, 60)
        })), null, 1);
    })()"#;
    println!("[gemini_hints]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_insert(page: &Page, msg: &str) -> Result<(), Box<dyn std::error::Error>> {
    let clear = r#"(() => {
        const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"], div[contenteditable="true"]');
        if (!el) return 'missing';
        el.focus();
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        const ok = document.execCommand('insertText', false, 'PING이라고만 답해 주세요.');
        return ok ? 'inserted' : 'failed';
    })()"#;
    let r = eval_str(page, clear).await?;
    println!("[gemini_insert] insert = {r}");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let js = r#"(() => {
        const all = [...document.querySelectorAll('button, div[role="button"]')];
        return JSON.stringify(all.filter(e => {
            const s = ((e.getAttribute('aria-label')||'') + ' ' + (e.innerText||'')).toLowerCase();
            return /전송|send|발송|arrow|음성|mic|stop|중지/.test(s);
        }).map(e => ({
            tag: e.tagName,
            aria: e.getAttribute('aria-label'),
            testid: e.getAttribute('data-testid'),
            text: (e.innerText || '').slice(0, 40),
            cls: (e.className || '').toString().slice(0, 60)
        })), null, 1);
    })()"#;
    println!("[gemini_insert]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_big_insert(page: &Page, prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 먼저 컴포저를 비운다
    let clear = format!(
        r#"(() => {{
            const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
            if (!el) return 'missing';
            el.focus();
            document.execCommand('selectAll', false, null);
            document.execCommand('delete', false, null);
            return 'cleared';
        }})()"#,
    );
    println!("[gemini_big_insert] clear = {}", eval_str(page, &clear).await?);
    let insert = format!(
        r#"(() => {{
            const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
            if (!el) return 'missing';
            el.focus();
            const ok = document.execCommand('insertText', false, {});
            return 'insert-' + ok;
        }})()"#,
        js_string(prompt),
    );
    println!("[gemini_big_insert] insert = {}", eval_str(page, &insert).await?);
    // 엔진이 하는 것과 동일하게 InputEvent 디스패치 후 상태를 본다
    let _ = eval_str(
        page,
        r#"(() => {
            const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
            if (!el) return 'missing';
            el.dispatchEvent(new InputEvent('input', { bubbles: true, data: ' ' }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
            return 'dispatched';
        })()"#,
    )
    .await?;
    for i in 0..4 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let st = eval_str(
            page,
            r#"(() => JSON.stringify({
                composerLen: (document.querySelector('div.ql-editor') || {}).innerText?.length || 0,
                hasSendBtn: !!document.querySelector("button[aria-label*='보내']"),
                sendLike: [...document.querySelectorAll('button')].map(b => b.getAttribute('aria-label')).filter(l => l && /보내|Send|send|전송/.test(l))
            }))()"#,
        )
        .await?;
        println!("[gemini_big_insert] t{i} {st}");
    }
    Ok(())
}

async fn gemini_send(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let rect_json = eval_str(
        page,
        r#"(() => {
            const btn = document.querySelector("button[aria-label='메시지 보내기'], button[aria-label='Send message'], button[aria-label*='보내기']");
            if (!btn) return '{}';
            const r = btn.getBoundingClientRect();
            return JSON.stringify({ x: r.x, y: r.y, w: r.width, h: r.height, disabled: btn.disabled });
        })()"#,
    )
    .await?;
    println!("[gemini_send] rect = {rect_json}");
    let v: serde_json::Value = serde_json::from_str(&rect_json).unwrap_or(serde_json::json!({}));
    if v.get("x").is_none() {
        println!("[gemini_send] send button not found");
        return Ok(());
    }
    let cx = v["x"].as_f64().unwrap_or(0.0) + v["w"].as_f64().unwrap_or(0.0) / 2.0;
    let cy = v["y"].as_f64().unwrap_or(0.0) + v["h"].as_f64().unwrap_or(0.0) / 2.0;
    println!("[gemini_send] clicking at ({cx:.0}, {cy:.0})");
    page.click(chromiumoxide::layout::Point::new(cx, cy)).await?;
    // 응답 완료까지 대기: model 응답 텍스트 안정 감시 (DilResponseRoot 대신 Gemini 컨테이너)
    let stable_needed = 4;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut last = String::new();
    let mut stable = 0usize;
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let cur = eval_str(
            page,
            r#"(() => {
                const els = document.querySelectorAll('[data-message-author-role="model"], .model-response-text, message-content, [class*="response-container"]');
                const el = els[els.length - 1];
                return el ? el.innerText : '';
            })()"#,
        ).await.unwrap_or_default();
        let stop = eval_str(
            page,
            r#"String(!!document.querySelector("button[aria-label*='중지'], button[aria-label*='Stop'], button[aria-label*='stop']"))"#,
        ).await.unwrap_or_else(|_| "false".into());
        if cur == last && !cur.is_empty() && stop == "false" {
            stable += 1;
        } else if cur != last {
            stable = 0;
        }
        last = cur;
        if stable >= stable_needed { break; }
    }
    println!("[gemini_send] reply (first 300):\n{}", &last[..last.len().min(300)]);
    Ok(())
}

async fn gemini_reply_probe(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const out = [];
        const els = document.querySelectorAll('*');
        for (const el of els) {
            if (el.children.length > 2) continue;
            const t = (el.innerText || '').trim();
            if (t === 'PING' && el.offsetWidth > 0) {
                const chain = [];
                let n = el;
                for (let k = 0; k < 5 && n; k++) {
                    chain.push({ tag: n.tagName, cls: (n.className || '').toString().slice(0, 60), testid: n.getAttribute('data-testid'), role: n.getAttribute('role'), aria: n.getAttribute('aria-label') });
                    n = n.parentElement;
                }
                out.push(chain);
            }
        }
        return JSON.stringify(out, null, 1).slice(0, 5000);
    })()"#;
    println!("[gemini_reply]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_enter(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let before = eval_str(
        page,
        r#"(document.querySelector('div.ql-editor') ? document.querySelector('div.ql-editor').innerText : 'no-el')"#,
    ).await?;
    println!("[gemini_enter] composer before = {before}");
    let r = eval_str(
        page,
        r#"(() => {
            const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
            if (!el) return 'missing';
            el.focus();
            for (const type of ['keydown', 'keypress', 'keyup']) {
                el.dispatchEvent(new KeyboardEvent(type, {key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true}));
            }
            return 'enter-dispatched';
        })()"#,
    ).await?;
    println!("[gemini_enter] {r}");
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    let tail = eval_str(
        page,
        r#"(document.body ? document.body.innerText.slice(-400) : '')"#,
    ).await?;
    println!("[gemini_enter] bodyTail =\n{tail}");
    Ok(())
}

async fn gemini_native_click(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let el = page.find_element("button[aria-label*='보내']").await?;
    el.click().await?;
    println!("[gemini_native_click] clicked via Element handle");
    for _ in 0..15 {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    }
    let tail = eval_str(
        page,
        r#"(document.body ? document.body.innerText.slice(-400) : '')"#,
    ).await?;
    println!("[gemini_native_click] bodyTail =\n{tail}");
    Ok(())
}

async fn gemini_input_events(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let r = eval_str(
        page,
        r#"(() => {
            const el = document.querySelector('div.ql-editor, rich-textarea div[contenteditable="true"]');
            if (!el) return 'missing';
            el.focus();
            document.execCommand('selectAll', false, null);
            document.execCommand('delete', false, null);
            const ok = document.execCommand('insertText', false, 'PING이라고만 답해 주세요.');
            el.dispatchEvent(new InputEvent('input', { bubbles: true, data: ' ' }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
            el.dispatchEvent(new KeyboardEvent('keyup', { key: 'a', bubbles: true }));
            return 'dispatched insert=' + ok;
        })()"#,
    )
    .await?;
    println!("[gemini_input_events] {r}");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let r2 = eval_str(
        page,
        r#"(() => {
            const btn = document.querySelector("button[aria-label='메시지 보내기'], button[aria-label='Send message'], button[aria-label*='보내기']");
            return JSON.stringify({ disabled: btn.disabled, ariaDisabled: btn.getAttribute('aria-disabled'), cls: btn.className.slice(0,120) });
        })()"#,
    )
    .await?;
    println!("[gemini_input_events] send button state = {r2}");
    Ok(())
}

async fn gemini_selector_test(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const a = document.querySelector("button[aria-label*='보내']");
        const b = document.querySelector("button[aria-label^='보내']");
        const c = document.querySelector("button[aria-label='메시지 보내기']");
        const d = document.querySelectorAll("button[aria-label]");
        const labs = [...d].map(x => x.getAttribute('aria-label'));
        return JSON.stringify({ a: !!a, b: !!b, c: !!c, sendLike: labs.filter(l => l && l.includes('보내')) }, null, 1);
    })()"#;
    println!("[gemini_selector_test]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_inspect(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const btn = document.querySelector("button[aria-label='메시지 보내기'], button[aria-label='Send message'], button[aria-label*='보내기']");
        if (!btn) return 'send button not found';
        const r = btn.getBoundingClientRect();
        const cx = Math.round(r.x + r.width / 2), cy = Math.round(r.y + r.height / 2);
        const at = document.elementFromPoint(cx, cy);
        return JSON.stringify({
            outer: btn.outerHTML.slice(0, 600),
            rect: { x: r.x, y: r.y, w: r.width, h: r.height },
            center: { cx, cy },
            atTag: at ? at.tagName : null,
            atCls: at ? (at.className || '').toString().slice(0, 80) : null,
            atOuter: at ? at.outerHTML.slice(0, 400) : null,
            composerText: (document.querySelector('div.ql-editor') ? document.querySelector('div.ql-editor').innerText : null)
        }, null, 1);
    })()"#;
    println!("[gemini_inspect]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_dump(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const all = [...document.querySelectorAll('button, div[role="button"]')];
        return JSON.stringify(all.map(e => ({
            tag: e.tagName,
            aria: e.getAttribute('aria-label'),
            testid: e.getAttribute('data-testid'),
            text: (e.innerText || '').slice(0, 40),
            cls: (e.className || '').toString().slice(0, 50)
        })), null, 1);
    })()"#;
    println!("[gemini_dump]\n{}", eval_str(page, js).await?);
    Ok(())
}

async fn gemini_probe(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    let js = r#"(() => {
        const q = (s) => document.querySelectorAll(s).length;
        const txt = (e) => (e ? e.innerText : '').trim().slice(0, 200);
        const composer = document.querySelector('div.ql-editor, div[contenteditable="true"], textarea, rich-textarea div[contenteditable="true"]');
        return JSON.stringify({
            title: document.title,
            url: location.href,
            composer: !!composer,
            composerCls: composer ? (composer.className || '').slice(0, 80) : null,
            buttons: [...document.querySelectorAll('button')].slice(0, 20).map(b => ({
                aria: b.getAttribute('aria-label'),
                testid: b.getAttribute('data-testid'),
                text: (b.innerText || '').slice(0, 30)
            })),
            bodyTail: (document.body ? document.body.innerText : '').slice(-400)
        }, null, 1);
    })()"#;
    println!("[gemini]\n{}", eval_str(page, js).await?);
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

    // 기존 chatgpt/gemini 탭 재사용 → 없으면 새로 연다 (핸들러가 대상 목록을 수신할 때까지 짧게 재시도)
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
        if mode.starts_with("gemini") {
            if u.contains("gemini.google.com") {
                target_page = Some(p);
                break;
            }
        } else {
            if u.contains("chatgpt.com/c/") {
                target_page = Some(p);
                break;
            }
            if u.contains("chatgpt") || u.contains("auth") {
                fallback = Some(p);
            }
        }
    }
    let target_page = target_page.or(fallback);
    let page = match target_page {
        Some(p) => p,
        None => {
            let url = if mode.starts_with("gemini") { "https://gemini.google.com/" } else { "https://chatgpt.com/" };
            println!("[cdp_spike] no tab found; opening {url}");
            browser.new_page(url).await?
        }
    };

    match mode.as_str() {
        "probe" => probe(&page).await?,
        "buttons" => buttons(&page).await?,
        "dom" => dom(&page).await?,
        "gemini" => gemini_probe(&page).await?,
        "gemini2" => gemini_hints(&page).await?,
        "gemini3" => gemini_insert(&page, "PING이라고만 답해 주세요.").await?,
        "gemini4" => gemini_dump(&page).await?,
        "gemini5" => gemini_send(&page).await?,
        "gemini6" => gemini_reply_probe(&page).await?,
        "gemini7" => gemini_enter(&page).await?,
        "gemini8" => gemini_inspect(&page).await?,
        "gemini12" => {
            let prompt = std::fs::read_to_string(std::env::temp_dir().join("codebridge_flatten_debug.txt"))
                .unwrap_or_else(|_| "file not found".to_string());
            gemini_big_insert(&page, &prompt).await?
        }
        "gemini10" => gemini_native_click(&page).await?,
        "gemini11" => gemini_selector_test(&page).await?,
        "code" => code_probe(&page).await?,
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
