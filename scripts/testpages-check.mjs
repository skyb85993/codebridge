#!/usr/bin/env node
/**
 * CodeBridge testpages regression check.
 * Verifies the fake chat page mirrors the exact DOM contract the Rust engine expects
 * (selectors from bridge/src/automation/engine.rs CHATGPT_SEL, banner detection from
 * is_limit_banner), so selector/DOM edits break loudly instead of silently.
 *
 * usage: node scripts/testpages-check.mjs
 * exit 0 = all checks passed, exit 1 = failure
 */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TESTPAGE = path.join(ROOT, 'testpages', 'chatgpt.html');

const CHROME_CANDIDATES = [
  process.env.CHROME_PATH,
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
].filter(Boolean);

function findChrome() {
  for (const c of CHROME_CANDIDATES) {
    if (fs.existsSync(c)) return c;
  }
  throw new Error('Chrome executable not found (set CHROME_PATH)');
}

function serve() {
  const server = http.createServer((req, res) => {
    const p = path.join(ROOT, 'testpages', path.basename(req.url.split('?')[0]) || 'chatgpt.html');
    fs.readFile(p, (err, data) => {
      if (err) { res.writeHead(404); res.end('not found'); return; }
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(data);
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port }));
  });
}

function waitFor(ws, method, params, match, timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    let id = 0;
    const timer = setTimeout(() => reject(new Error(`timeout waiting for ${match}`)), timeoutMs);
    const onMsg = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id === 1 && msg.error) { clearTimeout(timer); reject(new Error(JSON.stringify(msg.error))); return; }
      if (msg.method === match || (match === 'loadEventFired' && msg.method === 'Page.loadEventFired')) {
        clearTimeout(timer); ws.removeEventListener('message', onMsg); resolve(msg);
      }
    };
    ws.addEventListener('message', onMsg);
    ws.send(JSON.stringify({ id: 1, method, params }));
  });
}

async function cdpConnect(url) {
  const list = await (await fetch(url)).json();
  const target = list.find((t) => t.type === 'page');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  return ws;
}

async function evalJson(ws, expression) {
  const id = Math.floor(Math.random() * 1e9) + 2;
  const result = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('eval timeout')), 10000);
    const onMsg = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id === id) {
        clearTimeout(timer); ws.removeEventListener('message', onMsg);
        msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
      }
    };
    ws.addEventListener('message', onMsg);
    ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }));
  });
  return result.result.value;
}

const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
};

const { server, port } = await serve();
const chrome = findChrome();
console.log(`chrome: ${chrome}`);
const debugPort = 19337;
const chromeArgs = () => [
  '--headless=new',
  `--remote-debugging-port=${debugPort}`,
  '--no-first-run',
  '--disable-gpu',
  '--disable-features=Translate',
  '--no-sandbox',
  `--user-data-dir=${path.join(ROOT, '.testpages-profile')}`,
  'about:blank',
];
const launchChrome = () => spawn(chrome, chromeArgs(), { stdio: 'ignore' });

const spawnChromeOnce = async () => {
  let proc = launchChrome();
  // Chrome 기동 대기 (헤드리스도 환경에 따라 15초 넘길 수 있음)
  for (let i = 0; i < 75 && !list; i++) {
    try {
      list = await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json();
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  if (list) return proc;
  // 실패 시 한 번만 죽이고 재시도 (러너의 Chrome flake 완화)
  try { proc.kill(); } catch {}
  await new Promise((r) => setTimeout(r, 1500));
  proc = launchChrome();
  for (let i = 0; i < 75 && !list; i++) {
    try {
      list = await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json();
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  return proc;
};

let list = null;
const proc = await spawnChromeOnce();

try {
  // spawnChromeOnce가 두 Chrome 모두 확인했으므로, 실패 시에만 여기까지 옴
  if (!list) throw new Error('Chrome CDP did not come up');

  const ws = await cdpConnect(`http://127.0.0.1:${debugPort}/json`);

  const navigate = async (u) => {
    await new Promise((resolve, reject) => {
      const id = 901;
      const timer = setTimeout(() => reject(new Error('nav timeout')), 15000);
      const onMsg = (ev) => {
        const msg = JSON.parse(ev.data);
        if (msg.id === id) { clearTimeout(timer); ws.removeEventListener('message', onMsg); msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(); }
      };
      ws.addEventListener('message', onMsg);
      ws.send(JSON.stringify({ id, method: 'Page.navigate', params: { url: u } }));
    });
    await new Promise((r) => setTimeout(r, 800));
  };

  await navigate(`http://127.0.0.1:${port}/chatgpt.html`);

  // 1) DOM 계약 (엔진 CHATGPT_SEL 과 동일 선택자)
  const composerSel = "textarea#prompt-textarea, div#prompt-textarea, [contenteditable='true']";
  const sendSel = "button[data-testid='send-button'], button[aria-label='보내기'], button[aria-label='Send prompt'], button[aria-label='Send']";
  const newChatSel = "button[aria-label='새 채팅'], button[aria-label='New chat'], button[data-testid='create-new-chat-button']";
  const assistantSel = "div[class*='DilResponseRoot'], [class*='entity-prose-inset']";
  check('composer selector (engine)', (await evalJson(ws, `String(!!document.querySelector("${composerSel}"))`)) === 'true');
  check('send button selector (engine)', (await evalJson(ws, `String(!!document.querySelector("${sendSel}"))`)) === 'true');
  check('new chat selector (engine)', (await evalJson(ws, `String(!!document.querySelector("${newChatSel}"))`)) === 'true');

  // 2) 전송 → 응답 스트리밍 → 완료까지 (엔진의 완료 판정 방식과 동일하게 stop 버튼 소멸 대기)
  await evalJson(ws, `(() => { const t = document.querySelector("${composerSel}"); t.value = 'Roblox 게임 만들어줘'; return 'ok'; })()`);
  await evalJson(ws, `document.querySelector("${sendSel}").click()`);
  let sawStop = false;
  for (let i = 0; i < 200; i++) {
    const stopVisible = (await evalJson(ws, `(() => { const b = document.querySelector("button[data-testid='stop-button'], button[aria-label='Stop generating']"); return String(!!b && b.style.display !== 'none'); })()`)) === 'true';
    const assistantCount = (await evalJson(ws, `document.querySelectorAll("${assistantSel}").length`));
    if (stopVisible) sawStop = true;
    if (sawStop && !stopVisible && assistantCount >= 1) break;
    await new Promise((r) => setTimeout(r, 100));
  }
  check('response streamed with stop button lifecycle', sawStop);
  const answer = await evalJson(ws, `(() => { const a = document.querySelectorAll("${assistantSel}"); const el = a[a.length - 1]; const pre = el && el.querySelector('pre'); return pre ? pre.innerText : ''; })()`);
  check('fixture file content includes path comment', String(answer).includes('path: ServerScriptService/GameManager.lua'));

  // 3) 한도 모드: role=alert 배너 + 제한 텍스트
  await navigate(`http://127.0.0.1:${port}/chatgpt.html?mode=limit`);
  const limitTxt = await evalJson(ws, `(() => { const el = document.querySelector('[role="alert"]'); return el ? el.innerText : ''; })()`);
  check('limit banner visible with LIMIT_TEXTS-like text', String(limitTxt).toLowerCase().includes('rate limit'), String(limitTxt).slice(0, 60));
  const composerDisabled = await evalJson(ws, `String(document.querySelector("${composerSel}").disabled)`);
  check('composer disabled under limit', composerDisabled === 'true');

  ws.close();
} catch (e) {
  console.error('ERROR', e.message || e);
  results.push({ name: 'runner', ok: false });
} finally {
  try { proc.kill(); } catch {}
  server.close();
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
