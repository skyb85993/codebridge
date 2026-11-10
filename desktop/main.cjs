/**
 * CodeBridge Desktop — Electron main process.
 *
 * 역할:
 *   1) 사용자 프로젝트 폴더 선택 (최초 실행 시)
 *   2) 전용 Chrome(CDP 9333)을 자동 실행한다 — 사용자가 수동으로 크롬을 열 필요가 없다
 *   3) Rust 브릿지(codebridge-bridge.exe)를 자식 프로세스로 실행 (127.0.0.1:9224, browser 모드)
 *   4) opencode serve(v2 API+웹 UI)를 자식 프로세스로 실행 (127.0.0.1:7891)
 *   5) 세션 Basic 인증을 창의 모든 요청에 주입 후 opencode 웹 UI 로드
 *   6) 트레이 메뉴(창 열기/프로젝트 재선택/브릿지·Chrome 재시작/로그인 상태/종료) + 정리(자식 프로세스 종료)
 */
const {
  app,
  BrowserWindow,
  Tray,
  Menu,
  dialog,
  shell,
  session,
  nativeImage,
} = require('electron');
let autoUpdater = null;
try {
  ({ autoUpdater } = require('electron-updater'));
} catch (e) {
  console.error('[updater] require failed:', e);
}
const path = require('path');
const fs = require('fs');
const http = require('http');
const { spawn } = require('child_process');

// userData 경로를 dev/packaged 공통으로 고정한다 (%APPDATA%\CodeBridge)
app.setPath('userData', path.join(app.getPath('appData'), 'CodeBridge'));

const IS_DEV = !app.isPackaged;
const RES = IS_DEV ? path.join(__dirname, 'resources') : process.resourcesPath;
const BRIDGE_EXE = path.join(RES, 'bridge', 'codebridge-bridge.exe');
const BRIDGE_CONFIG = path.join(RES, 'bridge', 'config.toml');
const OPENCODE_EXE = path.join(RES, 'opencode', 'opencode.exe');
const OPENCODE_JSON_TEMPLATE = path.join(RES, 'opencode.json');
const DATA_DIR = app.getPath('userData');
const LOG_DIR = path.join(DATA_DIR, 'logs');
const SETTINGS_PATH = path.join(DATA_DIR, 'settings.json');

const SERVE_PORT = 7891;
const BRIDGE_PORT = 9224;
const CDP_PORT = 9333;

const CHROME_CANDIDATES = [
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
  path.join(process.env.LOCALAPPDATA || '', 'Google', 'Chrome', 'Application', 'chrome.exe'),
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  path.join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
];
const CHROME_PROFILE = path.join(DATA_DIR, 'chrome-profile');

let mainWindow = null;
let tray = null;
let bridgeProc = null;
let opencodeProc = null;
let chromeProc = null;
let servePassword = '';
let projectDir = '';
let appQuitting = false;
let probeTimer = null;
let loginRequired = false;
let updateState = 'idle'; // idle | checking | available | downloaded | up-to-date | error

function ensureDir(p) {
  fs.mkdirSync(p, { recursive: true });
}

function logStream(name) {
  ensureDir(LOG_DIR);
  return fs.createWriteStream(path.join(LOG_DIR, `${name}.log`), { flags: 'a' });
}

function readSettings() {
  try {
    return JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8'));
  } catch {
    return {};
  }
}

async function resolveProjectDir() {
  let dir = (readSettings().projectDir || '').trim();
  if (dir && fs.existsSync(dir)) return dir;
  // 경로가 유효하지 않으면 무효화하고 폴더를 다시 고르게 한다
  if (dir) {
    appendLog('app', `[app] stored projectDir invalid: ${dir}\n`);
    writeSettings({ ...readSettings(), projectDir: '' });
  }
  return null;
}

function writeSettings(s) {
  ensureDir(DATA_DIR);
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(s, null, 2));
}

/** 프로젝트 폴더에 opencode.json이 없으면 번들된 기본값을 복사한다 (있으면 건드리지 않음) */
function ensureOpencodeJson(dir) {
  const target = path.join(dir, 'opencode.json');
  if (fs.existsSync(target)) return false;
  try {
    fs.copyFileSync(OPENCODE_JSON_TEMPLATE, target);
    return true;
  } catch (e) {
    console.error('opencode.json template copy failed:', e);
    return false;
  }
}

async function pickProjectFolder() {
  const r = await dialog.showOpenDialog({
    title: 'CodeBridge — 프로젝트 폴더 선택',
    message: '로블록스 프로젝트의 루트 폴더를 선택하세요',
    properties: ['openDirectory', 'createDirectory'],
  });
  if (r.canceled || !r.filePaths.length) return null;
  return r.filePaths[0];
}

function spawnBridge(cwd) {
  if (bridgeProc) {
    try { bridgeProc.kill(); } catch {}
    bridgeProc = null;
  }
  const out = logStream('bridge');
  console.log('[bridge] spawn', BRIDGE_EXE, 'cwd=', cwd);
  bridgeProc = spawn(BRIDGE_EXE, [], {
    cwd,
    env: { ...process.env, CODEBRIDGE_CONFIG: BRIDGE_CONFIG },
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  bridgeProc.stdout.pipe(out);
  bridgeProc.stderr.pipe(out);
  bridgeProc.on('error', (e) => {
    console.error('[bridge] spawn error', e);
    appendLog('bridge', `[bridge] spawn error: ${e}\n`);
  });
  bridgeProc.on('exit', (code) => {
    console.log('[bridge] exit', code);
    appendLog('bridge', `[bridge] exit code=${code}\n`);
  });
}

function findBrowserExe() {
  return CHROME_CANDIDATES.find((p) => fs.existsSync(p)) || null;
}

function appendLog(name, text) {
  try {
    ensureDir(LOG_DIR);
    fs.appendFileSync(path.join(LOG_DIR, `${name}.log`), text);
  } catch {}
}

function spawnOpencode(cwd) {
  if (opencodeProc) {
    try { opencodeProc.kill(); } catch {}
    opencodeProc = null;
  }
  const out = logStream('opencode');
  console.log('[opencode] spawn serve port=' + SERVE_PORT, 'cwd=', cwd);
  opencodeProc = spawn(
    OPENCODE_EXE,
    ['serve', '--hostname', '127.0.0.1', '--port', String(SERVE_PORT)],
    { cwd, env: process.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true },
  );
  opencodeProc.stdout.pipe(out);
  opencodeProc.stderr.pipe(out);
  opencodeProc.on('error', (e) => {
    console.error('[opencode] spawn error', e);
    appendLog('opencode', `[opencode] spawn error: ${e}\n`);
  });
  opencodeProc.stdout.on('data', (chunk) => {
    const line = String(chunk);
    const m = line.match(/server password\s*(\S+)/);
    if (m) servePassword = m[1];
  });
  opencodeProc.on('exit', (code) => {
    console.log('[opencode] exit', code);
    appendLog('opencode', `[opencode] exit code=${code}\n`);
  });
}

function httpGet(pathname, password, timeoutMs = 3000) {
  return new Promise((resolve, reject) => {
    const auth = Buffer.from(`opencode:${password}`).toString('base64');
    const req = http.request(
      {
        host: '127.0.0.1',
        port: SERVE_PORT,
        path: pathname,
        method: 'GET',
        headers: { Authorization: `Basic ${auth}` },
        timeout: timeoutMs,
      },
      (res) => {
        let body = '';
        res.on('data', (d) => (body += d));
        res.on('end', () => resolve({ status: res.statusCode, body }));
      },
    );
    req.on('timeout', () => {
      req.destroy(new Error('timeout'));
    });
    req.on('error', reject);
    req.end();
  });
}

function httpGetRaw(port, pathname, timeoutMs = 2500) {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: '127.0.0.1', port, path: pathname, method: 'GET', timeout: timeoutMs },
      (res) => {
        let body = '';
        res.on('data', (d) => (body += d));
        res.on('end', () => resolve({ status: res.statusCode, body }));
      },
    );
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
    req.end();
  });
}

async function waitForChrome(timeoutMs = 25000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await httpGetRaw(CDP_PORT, '/json/version', 2000);
      if (r.status === 200) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  return false;
}

async function probeLoginOnce() {
  try {
    const r = await httpGetRaw(BRIDGE_PORT, '/v1/probe', 6000);
    if (r.status === 200) {
      try {
        const v = JSON.parse(r.body);
        loginRequired = !!v.login_required;
        if (tray) tray.setToolTip(loginRequired ? 'CodeBridge — ChatGPT 로그인 필요' : 'CodeBridge Desktop — 로그인됨');
        appendLog('probe', `[probe] login_required=${loginRequired} url=${v.url}\n`);
        return;
      } catch {}
    }
  } catch {}
  loginRequired = true;
  if (tray) tray.setToolTip('CodeBridge — Chrome/브릿지 연결 대기 중');
}

function startProbePolling() {
  if (probeTimer) clearInterval(probeTimer);
  probeTimer = setInterval(probeLoginOnce, 8000);
  // 즉시 1회
  probeLoginOnce();
}

async function waitForOpencode(timeoutMs = 90000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (servePassword) {
      try {
        const r = await httpGet('/api/session', servePassword, 2500);
        if (r.status === 200) return true;
      } catch {}
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  return false;
}

function splash(message, sub) {
  const html = `<!doctype html><html lang="ko"><head><meta charset="utf-8"><title>CodeBridge</title>
<style>body{margin:0;height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center;background:#0d1117;color:#c9d1d9;font-family:'Segoe UI',sans-serif}h1{font-size:20px;color:#58a6ff;margin:0 0 12px}p{color:#8b949e;font-size:13px;max-width:560px;text-align:center;line-height:1.7}code{background:#161b22;padding:2px 6px;border-radius:4px;color:#79c0ff}button{margin-top:18px;padding:8px 20px;background:#238636;border:0;border-radius:6px;color:#fff;font-size:13px;cursor:pointer}</style></head>
<body><h1>CodeBridge</h1><p>${message}</p>${sub ? `<p>${sub}</p>` : ''}<button onclick="location.href='codebridge://retry'">다시 시도</button></body></html>`;
  return 'data:text/html;charset=utf-8,' + encodeURIComponent(html);
}

function injectAuthHeader() {
  const auth = Buffer.from(`opencode:${servePassword}`).toString('base64');
  // 인증 헤더는 로컬 opencode 서버로 가는 요청에만 붙인다 (외부 호스트 유출 방지)
  const filter = { urls: [`http://127.0.0.1:${SERVE_PORT}/*`] };
  session.defaultSession.webRequest.onBeforeSendHeaders(filter, (details, callback) => {
    const { requestHeaders } = details;
    requestHeaders['Authorization'] = `Basic ${auth}`;
    callback({ requestHeaders });
  });
}

async function startServices() {
  if (!projectDir) {
    projectDir = (await resolveProjectDir()) || '';
  }
  if (!projectDir) {
    const picked = await pickProjectFolder();
    if (!picked) {
      app.quit();
      return false;
    }
    projectDir = picked;
    writeSettings({ ...readSettings(), projectDir });
  }
  ensureDir(projectDir);
  ensureOpencodeJson(projectDir);
  // 전용 Chrome을 먼저 띄운다 (가시 창 — 필요하면 사용자가 로그인/확인할 수 있게)
  spawnChrome();
  waitForChrome().then((ok) => appendLog('chrome', `[chrome] cdp_ready=${ok}\n`));
  spawnBridge(projectDir);
  spawnOpencode(projectDir);
  startProbePolling();
  return true;
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 920,
    minWidth: 1100,
    minHeight: 700,
    title: 'CodeBridge',
    backgroundColor: '#0d1117',
    autoHideMenuBar: true,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
    },
  });
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:/.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  // 실패 화면의 "다시 시도" 버튼 → codebridge://retry → 서비스 재시작 + UI 재부팅
  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (url === 'codebridge://retry') {
      event.preventDefault();
      startServices().then((started) => {
        if (started) bootUI();
      });
    }
  });
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
  mainWindow.loadURL(splash('CodeBridge를 준비하는 중…', '전용 Chrome(ChatGPT)과 브릿지, opencode 서버를 띄우고 있습니다. Chrome 창에서 ChatGPT에 한 번 로그인해 두면 이후에는 앱이 자동으로 띄웁니다.'));
}

/**
 * 전용 Chrome을 자동 실행한다 (가시 창, CDP 9333, 전용 프로필).
 * 이미 같은 프로필로 Chrome이 실행 중이면 새 창만 열리고 기존 인스턴스를 재사용한다.
 */
function spawnChrome() {
  const exe = findBrowserExe();
  if (!exe) {
    console.error('[chrome] no browser executable found in candidates');
    appendLog('chrome', '[chrome] not found\n');
    return null;
  }
  ensureDir(CHROME_PROFILE);
  const args = [
    `--remote-debugging-port=${CDP_PORT}`,
    `--user-data-dir=${CHROME_PROFILE}`,
    'https://chatgpt.com/',
  ];
  console.log('[chrome] spawn', exe);
  const proc = spawn(exe, args, {
    env: process.env,
    stdio: 'ignore',
    windowsHide: false,
    detached: true,
  });
  proc.on('error', (e) => {
    console.error('[chrome] spawn error', e);
    appendLog('chrome', `[chrome] spawn error: ${e}\n`);
  });
  chromeProc = proc;
  appendLog('chrome', `[chrome] spawn pid=${proc.pid} exe=${exe}\n`);
  return proc;
}

function killChild(proc) {
  if (!proc || proc.killed) return;
  try {
    spawn('taskkill', ['/pid', String(proc.pid), '/t', '/f'], { windowsHide: true });
  } catch {
    try { proc.kill(); } catch {}
  }
}

function setupAutoUpdate() {
  if (!autoUpdater) {
    appendLog('updater', '[updater] electron-updater unavailable; skipping\n');
    return;
  }
  try {
    autoUpdater.logger = console;
    autoUpdater.autoDownload = true;
    autoUpdater.autoInstallOnAppQuit = true;

    autoUpdater.on('update-available', (info) => {
      updateState = 'available';
      if (tray) tray.setToolTip(`CodeBridge — 업데이트 ${info.version} 다운로드 중`);
      appendLog('updater', `[updater] update-available v${info.version}\n`);
    });
    autoUpdater.on('update-not-available', (info) => {
      updateState = 'up-to-date';
      if (tray) tray.setToolTip('CodeBridge Desktop — 최신 버전');
      appendLog('updater', `[updater] up-to-date v${info.version}\n`);
    });
    autoUpdater.on('download-progress', (p) => {
      updateState = 'available';
      if (tray) tray.setToolTip(`CodeBridge — 업데이트 다운로드 중 ${Math.floor(p.percent || 0)}%`);
    });
    autoUpdater.on('update-downloaded', (info) => {
      updateState = 'downloaded';
      if (tray) tray.setToolTip('CodeBridge — 재시작하면 업데이트가 적용됩니다');
      appendLog('updater', `[updater] update-downloaded v${info.version}\n`);
      if (mainWindow) {
        const r = dialog.showMessageBoxSync(mainWindow, {
          type: 'info',
          buttons: ['지금 재시작', '나중에'],
          defaultId: 0,
          cancelId: 1,
          title: 'CodeBridge 업데이트',
          message: `새 버전(${info.version})이 준비되었습니다. 지금 재시작해서 적용할까요?`,
        });
        if (r === 0) {
          autoUpdater.quitAndInstall();
        }
      }
    });
    autoUpdater.on('error', (err) => {
      updateState = 'error';
      if (tray) tray.setToolTip('CodeBridge — 업데이트 확인 실패');
      appendLog('updater', `[updater] error: ${err && err.message ? err.message : err}\n`);
      console.error('[updater] error', err);
    });

    autoUpdater.checkForUpdatesAndNotify().catch((e) => {
      console.error('[updater] check failed', e);
      appendLog('updater', `[updater] check failed: ${e}\n`);
    });
  } catch (e) {
    console.error('[updater] setup failed', e);
    appendLog('updater', `[updater] setup failed: ${e}\n`);
  }
}

function buildTray() {
  const icon = nativeImage.createFromPath(path.join(RES, 'tray-icon.png'));
  tray = new Tray(icon.isEmpty() ? nativeImage.createEmpty() : icon);
  tray.setToolTip('CodeBridge Desktop');
  const menu = Menu.buildFromTemplate([
    {
      label: 'CodeBridge 열기',
      click: () => {
        if (mainWindow) {
          if (mainWindow.isMinimized()) mainWindow.restore();
          mainWindow.show();
          mainWindow.focus();
        } else {
          bootUI();
        }
      },
    },
    { type: 'separator' },
    {
      label: '프로젝트 폴더 다시 선택…',
      click: async () => {
        const picked = await pickProjectFolder();
        if (!picked) return;
        projectDir = picked;
        writeSettings({ ...readSettings(), projectDir });
        startServices();
        bootUI();
      },
    },
    {
      label: '브릿지 재시작',
      click: () => {
        if (!projectDir) return;
        spawnBridge(projectDir);
      },
    },
    {
      label: '전용 Chrome 재시작',
      click: () => {
        killChild(chromeProc);
        chromeProc = null;
        spawnChrome();
      },
    },
    {
      label: '업데이트 확인',
      click: async () => {
        if (!autoUpdater) {
          dialog.showMessageBoxSync({ type: 'warning', title: 'CodeBridge', message: '업데이터가 초기화되지 않았습니다.' });
          return;
        }
        try {
          const result = await autoUpdater.checkForUpdates();
          const latest = result && result.updateInfo ? result.updateInfo.version : '?';
          if (updateState === 'downloaded') {
            dialog.showMessageBoxSync({ type: 'info', title: 'CodeBridge', message: `새 버전(${latest})이 다운로드되어 있습니다. 종료/재시작 시 자동 설치됩니다.` });
          } else if (updateState === 'available') {
            dialog.showMessageBoxSync({ type: 'info', title: 'CodeBridge', message: `새 버전(${latest}) 다운로드 중입니다.` });
          } else if (updateState === 'up-to-date') {
            dialog.showMessageBoxSync({ type: 'info', title: 'CodeBridge', message: `현재 버전이 최신입니다 (v${app.getVersion()}).` });
          } else {
            dialog.showMessageBoxSync({ type: 'info', title: 'CodeBridge', message: `업데이트 확인 완료 (v${app.getVersion()}, 최신: ${latest}).` });
          }
        } catch (e) {
          dialog.showMessageBoxSync({ type: 'warning', title: 'CodeBridge', message: `업데이트 확인 실패: ${e && e.message ? e.message : e}\n(GitHub 릴리스가 공개된 후 동작합니다)` });
        }
      },
    },
    { type: 'separator' },
    {
      label: '로그 폴더 열기',
      click: () => shell.openPath(LOG_DIR),
    },
    { type: 'separator' },
    {
      label: '종료',
      click: () => {
        appQuitting = true;
        app.quit();
      },
    },
  ]);
  tray.setContextMenu(menu);
  tray.on('click', () => {
    if (mainWindow) {
      mainWindow.show();
      mainWindow.focus();
    } else {
      bootUI();
    }
  });
}

async function bootUI() {
  if (!mainWindow) createWindow();
  const ok = await waitForOpencode();
  if (!mainWindow) return;
  if (ok) {
    injectAuthHeader();
    mainWindow.loadURL(`http://127.0.0.1:${SERVE_PORT}/`);
    mainWindow.setTitle('CodeBridge');
  } else {
    mainWindow.loadURL(
      splash(
        'opencode 서버에 연결하지 못했습니다.',
        `브릿지/서버 로그를 확인하세요: <code>${LOG_DIR}</code><br>opencode.exe 경로: <code>${OPENCODE_EXE}</code>`,
      ),
    );
  }
}

process.on('uncaughtException', (e) => {
  try {
    ensureDir(LOG_DIR);
    fs.appendFileSync(path.join(LOG_DIR, 'boot.log'), `[boot] uncaughtException: ${e && (e.stack || e.message) || e}\n`);
  } catch {}
});
process.on('unhandledRejection', (e) => {
  try {
    ensureDir(LOG_DIR);
    fs.appendFileSync(path.join(LOG_DIR, 'boot.log'), `[boot] unhandledRejection: ${e && (e.stack || e.message) || e}\n`);
  } catch {}
});

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    } else {
      bootUI();
    }
  });

  app.whenReady().then(async () => {
    try {
      appendLog('boot', '[boot] whenReady: start\n');
      projectDir = '';
      ensureDir(DATA_DIR);
      ensureDir(LOG_DIR);
      appendLog('boot', '[boot] dirs ensured\n');
      createWindow();
      appendLog('boot', '[boot] window created\n');
      buildTray();
      appendLog('boot', '[boot] tray built\n');
      setupAutoUpdate();
      appendLog('boot', '[boot] auto-update setup done\n');
      const started = await startServices();
      appendLog('boot', `[boot] services started=${started}\n`);
      if (started) {
        bootUI();
      }
    } catch (e) {
      appendLog('boot', `[boot] whenReady error: ${e && (e.stack || e.message) || e}\n`);
    }
  });

  app.on('window-all-closed', () => {
    // 트레이 앱: 창이 닫혀도 백그라운드 유지 (Windows 관례는 다르므로, 창 닫힘 = 최소화 처리는 하지 않고 그대로 닫음)
    if (process.platform !== 'darwin') {
      // 사용자가 창을 닫아도 서비스는 유지한다. 완전 종료는 트레이의 "종료"로.
    }
  });

  app.on('before-quit', () => {
    appQuitting = true;
    if (probeTimer) clearInterval(probeTimer);
    killChild(bridgeProc);
    killChild(opencodeProc);
    killChild(chromeProc);
  });
}
