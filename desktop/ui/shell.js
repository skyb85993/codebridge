/* CodeBridge Desktop — 커스텀 셸 로직 */
'use strict';

const bridge = window.codebridge; // preload.cjs 가 노출하는 IPC 브릿지

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];

// ---------- 뷰 전환 ----------
$$('.nav-item').forEach((btn) => {
  btn.addEventListener('click', () => {
    $$('.nav-item').forEach((b) => b.classList.toggle('active', b === btn));
    const view = btn.dataset.view;
    $$('.view').forEach((v) => v.classList.toggle('active', v.id === `view-${view}`));
    if (view === 'logs') refreshLogs();
    if (view === 'settings') refreshSettings();
  });
});

// ---------- 윈도우 컨트롤 ----------
$('#win-min').addEventListener('click', () => bridge.windowAction('minimize'));
$('#win-max').addEventListener('click', () => bridge.windowAction('toggle-maximize'));
$('#win-close').addEventListener('click', () => bridge.windowAction('close'));

// ---------- 상태 폴링 ----------
const dot = () => $('#tb-status-dot');
const statusBridge = () => $('#sb-bridge');
const statusChatGPT = () => $('#sb-chatgpt');
const statusGemini = () => $('#sb-gemini');
const statusMessages = () => $('#sb-messages');
const statusUpdate = () => $('#sb-update');

async function pollStatus() {
  try {
    const s = await bridge.getStatus();
    dot().className = 'status-dot ' + (s.bridgeOk && (s.chatgptOk || s.geminiOk) ? 'ok' : s.bridgeOk ? 'warn' : 'err');
    statusBridge().textContent = s.bridgeOk ? '브릿지 ✓' : '브릿지 ✗';
    statusChatGPT().innerHTML = s.chatgptOk
      ? '<span style="color:#3fb950">ChatGPT ✓</span>'
      : '<span style="color:#f2cc60">ChatGPT 로그인 필요</span>';
    statusGemini().innerHTML = s.geminiOk
      ? '<span style="color:#3fb950">Gemini ✓</span>'
      : '<span style="color:#f2cc60">Gemini 로그인 필요</span>';
    statusMessages().textContent = `메시지 ${s.messageCount ?? '-'}`;
    statusUpdate().textContent = s.updateVersion ? `v${s.updateVersion}` : 'v?.?.?';
    $('#side-project-path').textContent = s.projectDir || '(미설정)';
    $('#set-bridge').textContent = s.bridgeOk ? 'OK (9224)' : '끊김';
    $('#set-chrome').textContent = s.chromeReady ? 'OK (9333)' : '실행 안 됨';
    $('#set-chatgpt').textContent = s.chatgptOk ? '로그인됨' : '로그인 필요';
    $('#set-gemini').textContent = s.geminiOk ? '로그인됨' : '로그인 필요';
    $('#set-version').textContent = `v${s.appVersion || '?'}`;
    $('#set-update').textContent = s.updateState === 'up-to-date' ? '최신 버전' : s.updateState || '대기';
    $('#side-update-state').textContent =
      s.updateState === 'available' ? '업데이트 다운로드 중…'
      : s.updateState === 'downloaded' ? '업데이트 준비됨 — 재시작 시 설치'
      : '자동 업데이트 ON';

    const frame = $('#webchat-frame');
    if (frame.getAttribute('src') !== s.webchatUrl) {
      frame.setAttribute('src', s.webchatUrl);
    }
  } catch (e) {
    dot().className = 'status-dot err';
    statusBridge().textContent = '브릿지 ✗';
  }
}

setInterval(pollStatus, 2000);

// ---------- 로그 ----------
async function refreshLogs() {
  const out = $('#logs-output');
  out.textContent = '불러오는 중…';
  try {
    const text = await bridge.tailLogs();
    out.textContent = text || '(로그가 비어 있습니다)';
    out.scrollTop = out.scrollHeight;
  } catch (e) {
    out.textContent = '로그를 불러오지 못했습니다: ' + e.message;
  }
}
$('#logs-refresh').addEventListener('click', refreshLogs);

// ---------- 설정 ----------
$('#restart-bridge').addEventListener('click', async () => {
  await bridge.restartBridge();
  setTimeout(pollStatus, 1200);
});
$('#reopen-chrome').addEventListener('click', async () => {
  await bridge.reopenChrome();
  setTimeout(pollStatus, 1500);
});
$('#check-update').addEventListener('click', async () => {
  const r = await bridge.checkForUpdates();
  $('#set-update').textContent = r.message;
  pollStatus();
});
$('#pick-project').addEventListener('click', async () => {
  const dir = await bridge.pickProject();
  if (dir) {
    pollStatus();
  }
});

// 시작 시 상태 1회
pollStatus();
