/**
 * CodeBridge Desktop — Electron main process.
 *
 * 역할:
 *   1) 사용자 프로젝트 폴더 선택 (최초 실행 시)
 *   2) Rust 브릿지(codebridge-bridge.exe)를 자식 프로세스로 실행 (127.0.0.1:9224)
 *   3) opencode serve(v2 API+웹 UI)를 자식 프로세스로 실행 (127.0.0.1:7891)
 *   4) 세션 Basic 인증을 창의 모든 요청에 주입 후 opencode 웹 UI 로드
 *   5) 트레이 메뉴(프로젝트 재선택/브릿지 재시작/로그/종료) + 정리(자식 프로세스 종료)
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

let mainWindow = null;
let tray = null;
let bridgeProc = null;
let opencodeProc = null;
let servePassword = '';
let projectDir = '';
let appQuitting = false;

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
  bridgeProc.on('exit', (code) => {
    console.log('[bridge] exit', code);
    appendLog('bridge', `[bridge] exit code=${code}\n`);
  });
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
  spawnBridge(projectDir);
  spawnOpencode(projectDir);
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
  mainWindow.loadURL(splash('CodeBridge를 준비하는 중…', '브릿지와 opencode 서버를 띄우고 있습니다.'));
}

function killChild(proc) {
  if (!proc || proc.killed) return;
  try {
    spawn('taskkill', ['/pid', String(proc.pid), '/t', '/f'], { windowsHide: true });
  } catch {
    try { proc.kill(); } catch {}
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
    projectDir = readSettings().projectDir || '';
    ensureDir(DATA_DIR);
    ensureDir(LOG_DIR);
    createWindow();
    buildTray();
    const started = await startServices();
    if (started) {
      bootUI();
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
    killChild(bridgeProc);
    killChild(opencodeProc);
  });
}
