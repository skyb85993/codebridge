#!/usr/bin/env node
/**
 * CodeBridge 렌더 스모크 체크.
 * 개발 인스턴스를 remote-debugging-port로 띄우고 CDP로 확인한다:
 *   1) 앱 셸이 렌더됐는가
 *   2) 에러 바운더리가 떴는가 ("문제가 발생했습니다")
 *   3) window.codebridge IPC가 응답하는가
 *
 * UI 크래시를 CI에서 걸러내기 위한 게이트 (v0.8.0 사태 방지).
 * usage: node scripts/smoke-check.mjs
 * exit 0 = 통과, exit 1 = 실패
 */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DESKTOP = path.join(ROOT, 'desktop');
const ELECTRON = path.join(DESKTOP, 'node_modules', 'electron', 'dist', 'electron.exe');
const PORT = 19351;
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
};

async function waitForCdp(timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/json/list`);
      const list = await r.json();
      const page = Array.isArray(list) && list.find((t) => (t.url || '').includes('ui-build/index.html'));
      if (page) return page;
    } catch {
      /* 아직 CDP 미기동 */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  return null;
}

if (!fs.existsSync(ELECTRON)) {
  console.error(`electron 바이너리 없음: ${ELECTRON}`);
  process.exit(1);
}

const isWin = process.platform === 'win32';
const killTree = (proc) => {
  if (!proc || proc.killed) return;
  if (isWin) {
    try {
      spawn('taskkill', ['/pid', String(proc.pid), '/t', '/f'], { windowsHide: true });
      return;
    } catch {
      /* 폴백 */
    }
  }
  try { proc.kill('SIGKILL'); } catch {}
};

const proc = spawn(ELECTRON, ['.', `--remote-debugging-port=${PORT}`], {
  cwd: DESKTOP,
  stdio: 'ignore',
  env: { ...process.env },
});

try {
  const page = await waitForCdp();
  check('electron CDP endpoint', !!page);
  if (page) {
    const ws = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((res, rej) => {
      ws.onopen = res;
      ws.onerror = rej;
      setTimeout(() => rej(new Error('ws open timeout')), 8000);
    });
    let id = 0;
    const pending = new Map();
    const send = (method, params = {}) =>
      new Promise((res, rej) => {
        id += 1;
        pending.set(id, { res, rej });
        ws.send(JSON.stringify({ id, method, params }));
      });
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id && pending.has(msg.id)) {
        const { res, rej } = pending.get(msg.id);
        pending.delete(msg.id);
        msg.error ? rej(new Error(JSON.stringify(msg.error))) : res(msg.result);
      }
    };
    const evaluate = async (expression) => {
      const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      return r && r.result ? r.result.value : undefined;
    };
    await new Promise((r) => setTimeout(r, 3000));
    const bodyText = await evaluate('(document.body && document.body.innerText) || ""');
    check('render completed', typeof bodyText === 'string' && bodyText.length > 10,
      JSON.stringify(String(bodyText).slice(0, 80)));
    check('no error boundary', !String(bodyText).includes('문제가 발생했습니다'));
    check(
      'app shell visible',
      /새 대화|무엇을 만들까요|프로젝트 폴더|폴더를 선택하세요|설정/.test(String(bodyText)),
      String(bodyText).slice(0, 60),
    );
    const boot = await evaluate(
      "window.codebridge ? window.codebridge.getBoot().then(b => JSON.stringify(b)) : 'no bridge'",
    );
    check(
      'window.codebridge IPC',
      typeof boot === 'string' && boot.startsWith('{'),
      String(boot).slice(0, 80),
    );
    ws.close();
  }
} catch (e) {
  console.error('ERROR', (e && e.message) || e);
  results.push({ name: 'runner', ok: false });
} finally {
  killTree(proc);
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
