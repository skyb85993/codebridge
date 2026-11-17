#!/usr/bin/env node
/**
 * CodeBridge 스모크 체크 (2단계).
 *   Phase A — 빈 프로필: 앱이 렌더링되고 window.codebridge IPC가 응답하며
 *            폴더 선택 온보딩 화면(안전)으로 뜬다.
 *   Phase B — projectDir=ROOT 시드 프로필: 서비스가 기동해창이
 *            opencode Web UI(http://127.0.0.1:7891/)로 전환되고 title=OpenCode가 된다.
 *
 * CI 게이트 — Electron CDP 디버깅 포트를 열고 원격으로 검사한다.
 * usage: node smoke-check.mjs
 * exit 0 = 성공, exit 1 = 실패
 */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DESKTOP = path.join(ROOT, 'desktop');
const ELECTRON = path.join(DESKTOP, 'node_modules', 'electron', 'dist', 'electron.exe');
const PORT_A = 19351;
const PORT_B = 19352;
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
};

const isWin = process.platform === 'win32';
const killTree = (proc) =>
  new Promise((resolve) => {
    if (!proc || proc.killed || !proc.pid) return resolve();
    if (isWin) {
      try {
        const t = spawn('taskkill', ['/pid', String(proc.pid), '/t', '/f'], {
          windowsHide: true,
          stdio: 'ignore',
        });
        t.on('exit', () => resolve());
        t.on('error', () => resolve());
        setTimeout(() => { try { t.kill(); } catch {} resolve(); }, 5000);
        return;
      } catch { /* 폴백 */ }
    }
    try { proc.kill('SIGKILL'); } catch {}
    resolve();
  });

const rmProfileWithRetry = async (dir, attempts = 5, delayMs = 400) => {
  for (let i = 0; i < attempts; i++) {
    try { fs.rmSync(dir, { recursive: true, force: true }); return true; } catch { await new Promise((r) => setTimeout(r, delayMs)); }
  }
  return false;
};

const httpGet = (port, pathname, timeoutMs = 2000) => new Promise((resolve) => {
  const req = http.request({ host: '127.0.0.1', port, path: pathname, timeout: timeoutMs }, (res) => {
    let b = ''; res.on('data', (d) => (b += d)); res.on('end', () => resolve({ status: res.statusCode, body: b }));
  });
  req.on('error', () => resolve({ status: 0 }));
  req.on('timeout', () => { try { req.destroy(); } catch {} ; resolve({ status: 0 }); });
  req.end();
});

const waitForPage = async (port, urlInclude, timeoutMs) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json/list`);
      if (r.ok) {
        const list = await r.json();
        const page = Array.isArray(list) && list.find((t) => (t.url || '').includes(urlInclude));
        if (page) return page;
      }
    } catch { /* 아직 CDP 미기동 */ }
    await new Promise((r) => setTimeout(r, 250));
  }
  return null;
};

const connectWs = async (page, ms = 8000) => {
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; setTimeout(() => rej(new Error('ws open timeout')), ms); });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? rej(new Error(JSON.stringify(msg.error))) : res(msg.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((res, rej) => { id += 1; pending.set(id, { res, rej }); ws.send(JSON.stringify({ id, method, params })); });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    return r && r.result ? r.result.value : undefined;
  };
  return { evaluate, close: () => ws.close() };
};

const launchElectron = (profileDir, port) => {
  try { fs.mkdirSync(profileDir, { recursive: true }); } catch {}
  const proc = spawn(ELECTRON, ['.', `--remote-debugging-port=${port}`], {
    cwd: DESKTOP,
    stdio: 'ignore',
    env: { ...process.env, CODEBRIDGE_USER_DATA_DIR: profileDir },
  });
  return proc;
};

const test = async () => {
  if (!fs.existsSync(ELECTRON)) {
    console.error(`electron 바이너리 없음: ${ELECTRON}`);
    process.exit(1);
  }
  const smokeProfile = path.join(ROOT, '.smoke-profile');
  const webuiProfile = path.join(ROOT, '.smoke-profile-webui');
  fs.rmSync(smokeProfile, { recursive: true, force: true });
  fs.rmSync(webuiProfile, { recursive: true, force: true });

  // Phase A — 빈 프로필: 폴더 선택 온보딩 화면
  console.log('\n=== Phase A: blank profile (folder picker) ===');
  {
    const proc = launchElectron(smokeProfile, PORT_A);
    try {
      const page = await waitForPage(PORT_A, 'ui-build/index.html', 30_000);
      check('A: electron CDP endpoint', !!page);
      if (page) {
        const cdp = await connectWs(page, 8000);
        await new Promise((r) => setTimeout(r, 3000));
        const bodyText = await cdp.evaluate('(document.body && document.body.innerText) || ""');
        check('A: render completed', typeof bodyText === 'string' && bodyText.length > 10,
          JSON.stringify(String(bodyText).slice(0, 80)));
        check('A: no error boundary', !String(bodyText).includes('문제가 발생했습니다'));
        check('A: shell visible',
          /새 대화|무엇을 만들까요|프로젝트 폴더|폴더를 선택하세요|설정/.test(String(bodyText)),
          String(bodyText).slice(0, 60));
        const boot = await cdp.evaluate(
          "window.codebridge ? window.codebridge.getBoot().then(b => JSON.stringify(b)) : 'no bridge'");
        check('A: window.codebridge IPC', typeof boot === 'string' && boot.startsWith('{'),
          String(boot).slice(0, 80));
        cdp.close();
      }
    } catch (e) {
      console.error('ERROR', (e && e.message) || e);
      results.push({ name: 'A: runner', ok: false });
    } finally {
      await killTree(proc);
      await rmProfileWithRetry(smokeProfile, 5, 400);
    }
  }

  // Phase B — projectDir seeded: 서비스 기동 후 opencode Web UI로 전환
  console.log('\n=== Phase B: projectDir seeded (opencode Web UI) ===');
  {
    fs.mkdirSync(webuiProfile, { recursive: true });
    fs.writeFileSync(path.join(webuiProfile, 'settings.json'),
      JSON.stringify({ projectDir: ROOT }), 'utf8');
    const proc = launchElectron(webuiProfile, PORT_B);
    try {
      // 서비스 헬스 (최대 90초)
      const t0 = Date.now();
      let healthy = null;
      while (Date.now() - t0 < 90_000) {
        const b = await httpGet(9224, '/health', 1500);
        const o = await httpGet(7891, '/global/health', 1500);
        if (b.status === 200 && o.status === 200) { healthy = { bridge: b.status, opencode: o.status }; break; }
        await new Promise((r) => setTimeout(r, 500));
      }
      check('B: bridge+opencode healthy', !!healthy, JSON.stringify(healthy || {}));
      // Web UI 페이지가 뜨는지 (eligible redirect — loadOpencodeUi()가 전환)
      const page = await waitForPage(PORT_B, '127.0.0.1:7891/', 30_000);
      check('B: window navigated to opencode Web UI', !!page,
        page ? (page.url + ' / title=' + page.title) : 'no page found');
      if (page) {
        const cdp = await connectWs(page, 8000);
        // 페이지가 로드되어 OpenCode title로 세팅될 때까지 폴링
        const t0w = Date.now();
        let title = undefined;
        while (Date.now() - t0w < 20_000) {
          const v = await cdp.evaluate('document.title');
          if (v === 'OpenCode') { title = v; break; }
          await new Promise((r) => setTimeout(r, 250));
        }
        check('B: window title=OpenCode', title === 'OpenCode', 'title=' + String(title));
        const info = await cdp.evaluate(
          "fetch('/global/health').then(r => r.status).catch(() => 0)");
        check('B: /global/health fetch', info === 200, 'status=' + String(info));
        cdp.close();
      }
    } catch (e) {
      console.error('ERROR', (e && e.message) || e);
      results.push({ name: 'B: runner', ok: false });
    } finally {
      await killTree(proc);
      await rmProfileWithRetry(webuiProfile, 5, 400);
    }
  }

  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
  process.exit(failed.length ? 1 : 0);
};

test().catch((e) => { console.error(e); process.exit(1); });
