//! ChatGPT 컴포저 DOM 진단 — 새 채팅 페이지에서 execCommand('insertText')가
//! 왜 먹히지 않는지 확인하기 위한 읽기 전용 프로브.
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use std::time::Duration;

const COMPOSER_SEL: &str =
    "textarea#prompt-textarea, div#prompt-textarea, [contenteditable='true']";

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

async fn eval_str(page: &chromiumoxide::page::Page, js: &str) -> Result<String, Box<dyn std::error::Error>> {
    let v = page.evaluate(js).await?;
    Ok(match v.value() {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args().nth(1).unwrap_or_else(|| "9333".to_string());
    let endpoint = format!("http://127.0.0.1:{port}");
    println!("[composer_debug] connecting to {endpoint}");

    let (browser, mut handler) = Browser::connect(endpoint).await?;
    tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let mut pages = browser.pages().await.unwrap_or_default();
    for _ in 0..12 {
        if !pages.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        pages = browser.pages().await.unwrap_or_default();
    }
    for p in pages {
        let u = p.url().await.ok().flatten().unwrap_or_default();
        if !u.contains("chatgpt.com") {
            continue;
        }
        println!("[composer_debug] target page url = {u}");

        let report = eval_str(
            &p,
            &format!(
                r#"(() => {{
                    const sel = {sel};
                    const all = document.querySelectorAll(sel);
                    const info = [];
                    for (const el of all) {{
                        const rect = el.getBoundingClientRect();
                        info.push({{
                            tag: el.tagName,
                            id: el.id,
                            cls: String(el.className).slice(0, 80),
                            contentEditable: el.isContentEditable,
                            visible: rect.width > 0 && rect.height > 0,
                            rect: [rect.x, rect.y, rect.width, rect.height].map(v => Math.round(v)),
                            inDom: document.contains(el),
                            text: JSON.stringify((el.innerText ?? el.value ?? '').slice(0, 40)),
                            textLength: (el.innerText ?? el.value ?? '').length
                        }});
                    }}
                    const fullText = String(([...all].find(el => el.getBoundingClientRect().width > 0) ?? all[0]).innerText ?? '');
                    const head = fullText.slice(0, 300);
                    const ae = document.activeElement;
                    return JSON.stringify({{
                        matchCount: all.length,
                        matches: info,
                        activeTag: ae ? ae.tagName : null,
                        activeId: ae ? ae.id : null,
                        activeText: ae ? JSON.stringify((ae.innerText ?? ae.value ?? '').slice(0, 40)) : null,
                        head
                    }}, null, 1);
                }})()"#,
                sel = js_string(COMPOSER_SEL)
            ),
        )
        .await?;
        println!("[composer_debug] DOM report:\n{report}");

        // execCommand dry-run 진단 (텍스트 넣고 비우기 — 마커를 넣어 영향이 있는지 확인만 함)
        let test = eval_str(
            &p,
            &format!(
                r#"(() => {{
                    const sel = {sel};
                    const all = [...document.querySelectorAll(sel)];
                    const visible = all.filter(el => {{
                        const r = el.getBoundingClientRect();
                        const cs = getComputedStyle(el);
                        return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none';
                    }});
                    if (visible.length === 0) return 'no-visible-composer';
                    const el = visible[visible.length - 1];
                    const read = () => el.isContentEditable ? el.innerText : el.value;
                    const clearAll = () => {{
                        el.focus();
                        document.execCommand('selectAll', false, null);
                        document.execCommand('delete', false, null);
                    }};
                    const before = String(read() ?? '').length;
                    // 1) execCommand('insertText') on the VISIBLE node
                    el.focus();
                    let ins = 'throw';
                    try {{ ins = String(document.execCommand('insertText', false, 'CANARY_EXEC')); }} catch (e) {{ ins = 'err:' + e; }}
                    const afterExec = String(read() ?? '');
                    clearAll();
                    // 2) paste 시뮬레이션 on the VISIBLE node
                    el.focus();
                    let pasteRes = 'no-paste-api';
                    try {{
                        const dt = new DataTransfer();
                        dt.setData('text/plain', 'CANARY_PASTE');
                        const ok = el.dispatchEvent(new ClipboardEvent('paste', {{ clipboardData: dt, bubbles: true, cancelable: true }}));
                        pasteRes = 'dispatched:' + ok;
                    }} catch (e) {{ pasteRes = 'err:' + e; }}
                    const afterPaste = String(read() ?? '');
                    clearAll();
                    return JSON.stringify({{ before, visibleCount: visible.length, execCommand: ins, afterExec: afterExec.slice(0, 60), pasteRes, afterPaste: afterPaste.slice(0, 60), endLength: String(read() ?? '').length }});
                }})()"#,
                sel = js_string(COMPOSER_SEL)
            ),
        )
        .await?;
        println!("[composer_debug] insert mechanics probe:\n{test}");
        drop(browser);
        return Ok(());
    }
    println!("[composer_debug] no chatgpt.com page found");
    drop(browser);
    Ok(())
}
