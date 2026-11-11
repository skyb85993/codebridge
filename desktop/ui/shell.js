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
    if (view === 'handover') refreshHandover();
    if (view === 'settings') refreshSettings();
  });
});

// ---------- 윈도우 컨트롤 ----------
$('#win-min').addEventListener('click', () => bridge.windowAction('minimize'));
$('#win-max').addEventListener('click', () => bridge.windowAction('toggle-maximize'));
$('#win-close').addEventListener('click', () => bridge.windowAction('close'));

// ---------- 작업 뷰 iframe 게이팅 ----------
// opencode 서버(7891)가 실제로 응답할 때만 iframe에 src를 넣는다.
// startupFailed면 오버레이를 보여주고 새로고침 경로를 제공한다.
let iframeLoaded = false;
const overlay = () => $('#work-overlay');
const showOverlay = () => overlay().classList.remove('hidden');
const hideOverlay = () => overlay().classList.add('hidden');
function loadChatFrame(url) {
  const frame = $('#webchat-frame');
  if (frame.getAttribute('src') !== url) {
    frame.setAttribute('src', url);
    iframeLoaded = true;
  }
  hideOverlay();
}

$('#work-retry').addEventListener('click', () => {
  window.location.reload(); // 셸 전체 재로딩 → pollStatus가 다시 상태를 판정
});
$('#work-logs').addEventListener('click', () => {
  const logsNav = document.querySelector('.nav-item[data-view="logs"]');
  if (logsNav) logsNav.click();
});

const $loginBanner = () => $('#login-banner');
const $loginText = () => $('#login-banner-text');
const $loginAction = () => $('#login-banner-action');

function updateLoginBanner(s) {
  const banner = $loginBanner();
  if (!banner) return;
  const chatgptNeed = !s.chatgptOk;
  const geminiNeed = !s.geminiOk;
  if (s.bridgeOk && (chatgptNeed || geminiNeed)) {
    const who = chatgptNeed && geminiNeed ? 'ChatGPT·Gemini' : chatgptNeed ? 'ChatGPT' : 'Gemini';
    $loginText().textContent = `${who} 로그인이 필요합니다 — 전용 Chrome 창에서 로그인해 주세요.`;
    banner.classList.remove('hidden');
  } else {
    banner.classList.add('hidden');
  }
}

// ---------- 인수인계 ----------
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

async function refreshHandover() {
  const status = $('#handover-status');
  const list = $('#handover-list');
  try {
    const r = await bridge.getHandoverEvents();
    const events = (r && Array.isArray(r.events)) ? r.events : [];
    if (!events.length) {
      list.textContent = '최근 인계 기록이 없습니다.';
      status.textContent = '현재 스레드에 인계 기록이 없습니다.';
      status.className = 'handover-status muted';
      return;
    }
    status.textContent = `최근 인계 ${events.length}건`;
    status.className = 'handover-status ok-text';
    list.innerHTML = events
      .slice()
      .reverse()
      .map((e) => {
        const kind = String(e.kind || '');
        const cls = kind.endsWith('_ok') ? 'ok' : kind.includes('limit') ? 'warn' : (kind.includes('error') || kind.includes('failed')) ? 'err' : '';
        return `<div class="handover-item ${cls}"><span class="handover-at">${esc(e.at)}</span><span class="handover-kind">${esc(kind)}</span><span class="handover-detail">${esc(e.detail)}</span></div>`;
      })
      .join('');
  } catch (e) {
    status.textContent = '이력을 불러오지 못했습니다.';
    status.className = 'handover-status err-text';
  }
}

function wireHandover() {
  const btn = $('#handover-now');
  const status = $('#handover-status');
  if (!btn) return;
  btn.addEventListener('click', async () => {
    btn.disabled = true;
    status.textContent = '인수인계를 요청하는 중…';
    status.className = 'handover-status muted';
    try {
      const r = await bridge.triggerHandover();
      status.textContent = r.message || (r.ok ? '요청됨' : '요청 거부됨');
      status.className = 'handover-status ' + (r.ok ? 'ok-text' : 'err-text');
    } catch (e) {
      status.textContent = '인수인계를 요청하지 못했습니다.';
      status.className = 'handover-status err-text';
    } finally {
      btn.disabled = false;
    }
    setTimeout(refreshHandover, 800);
  });
}

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

    updateLoginBanner(s);

    // iframe은 opencode 서버가 실제로 200을 줄 때까지 로드하지 않는다
    if (s.opencodeOk && s.webchatUrl) {
      loadChatFrame(s.webchatUrl);
    } else if (s.startupFailed && !iframeLoaded) {
      showOverlay();
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

// ---------- 로그인 배너 ----------
$loginAction().addEventListener('click', async () => {
  try {
    const r = await bridge.launchDedicatedChrome();
    if (r && r.alreadyRunning) {
      $loginText().textContent = '전용 Chrome이 이미 실행 중입니다 — 창을 앞으로 가져와서 로그인하세요.';
    }
  } catch {}
});

// ---------- 인수인계 ----------
wireHandover();

// 시작 시 상태 1회
pollStatus();
