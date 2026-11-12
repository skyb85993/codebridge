/**
 * CodeBridge Desktop — Electron main process (v0.4.x)
 *
 * 역할:
 *   1) 창(프레임리스) + 트레이 최소 구성 (프로젝트 폴더 변경 / 지금 인수인계 / 업데이트 확인 / 종료)
 *   2) Rust 브릿지(codebridge-bridge.exe, 127.0.0.1:9224) + opencode serve(127.0.0.1:7891)를 자식 프로세스로 실행
 *   3) 렌더러가 필요로 하는 IPC만 노출 (Basic 인증은 main에서만)
 *   4) 전용 Chrome은 "명령이 들어올 때" 브릿지가 띄운다 (지연 실행, main은 관리하지 않음)
 *   5) 프로젝트 폴더가 없으면 렌더러가 폴더 선택 화면을 띄운다 (네이티브 대화상자 최소화)
 */
const {
  app,
  BrowserWindow,
  Tray,
  Menu,
  dialog,
  nativeImage,
  ipcMain,
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
const { spawn, spawnSync } = require('child_process');

app.setPath('userData', path.join(app.getPath('appData'), 'CodeBridge'));

const IS_DEV = !app.isPackaged;
const RES = IS_DEV ? path.join(__dirname, 'resources') : process.resourcesPath;
const BRIDGE_EXE = path.join(RES, 'bridge', 'codebridge-bridge.exe');
const BRIDGE_CONFIG = path.join(RES, 'bridge', 'config.toml');
const OPENCODE_EXE = path.join(RES, 'opencode', 'opencode.exe');
const OPENCODE_JSON_TEMPLATE = path.join(RES, 'opencode.json');
const UI_DIR = path.join(__dirname, 'ui-build');
const DATA_DIR = app.getPath('userData');
const LOG_DIR = path.join(DATA_DIR, 'logs');
const SETTINGS_PATH = path.join(DATA_DIR, 'settings.json');
const CHROME_PROFILE = path.join(DATA_DIR, 'chrome-profile');

const SERVE_PORT = 7891;
const BRIDGE_PORT = 9224;

let mainWindow = null;
let tray = null;
let bridgeProc = null;
let opencodeProc = null;
let servePassword = '';
let projectDir = '';
let appQuitting = false;
let updateState = 'idle'; // idle | checking | available | downloaded | up-to-date | error
let updateVersion = null; // 사용 가능한 최신 버전 (예: "0.4.1")

function ensureDir(p) {
  fs.mkdirSync(p, { recursive: true });
}

function appendLog(name, line) {
  try {
    ensureDir(LOG_DIR);
    fs.appendFileSync(path.join(LOG_DIR, `${name}.log`), line);
  } catch {}
}

function logStream(name) {
  const logPath = path.join(LOG_DIR, `${name}.log`);
  ensureDir(LOG_DIR);
  const stream = fs.createWriteStream(logPath, { flags: 'a' });
  return { stream, logPath };
}

// ---------- 설정 ----------
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
async function resolveProjectDir() {
  const saved = readSettings().projectDir;
  if (saved && fs.existsSync(saved)) return saved;
  if (saved) appendLog('boot', `[boot] saved projectDir missing: ${saved}\n`);
  return '';
}

function killChild(proc) {
  if (!proc || proc.killed) return;
  try {
    spawn('taskkill', ['/pid', String(proc.pid), '/t', '/f'], { windowsHide: true });
  } catch {
    try { proc.kill(); } catch {}
  }
}

// ---------- opencode.json (전역 템플릿 → 프로젝트 폴더 동기화) ----------
function ensureOpencodeJson(dir) {
  const target = path.join(dir, 'opencode.json');
  try {
    if (fs.existsSync(target)) {
      // 개발 잔재(mock 모델)가 남아 있으면 템플릿으로 교체
      const cur = fs.readFileSync(target, 'utf8');
      if (cur.includes('webchat-mock')) {
        fs.copyFileSync(OPENCODE_JSON_TEMPLATE, target);
        appendLog('boot', '[boot] opencode.json migrated (mock removed)\n');
      }
      return false;
    }
    fs.copyFileSync(OPENCODE_JSON_TEMPLATE, target);
    return true;
  } catch (e) {
    appendLog('boot', `[boot] opencode.json ensure failed: ${e}\n`);
    return false;
  }
}

function pickProjectFolder() {
  return dialog
    .showOpenDialog(mainWindow, {
      title: 'CodeBridge — 프로젝트 폴더 선택',
      message: '로블록스 프로젝트의 루트 폴더를 선택하세요',
      properties: ['openDirectory', 'createDirectory'],
    })
    .then((r) => (r.canceled || !r.filePaths.length ? null : r.filePaths[0]));
}

// ---------- 서비스 spawn ----------
function rotateLargeLog(name, capBytes = 5 * 1024 * 1024) {
  try {
    const p = path.join(LOG_DIR, name);
    const st = fs.statSync(p);
    if (st.size > capBytes) fs.renameSync(p, `${p}.1`);
  } catch {
    /* 로그 파일이 아직 없으면 무시 */
  }
}

function spawnBridge(cwd) {
  if (bridgeProc) killChild(bridgeProc);
  rotateLargeLog('bridge.log');
  const { stream } = logStream('bridge');
  const env = {
    ...process.env,
    CODEBRIDGE_CONFIG: BRIDGE_CONFIG,
    CODEBRIDGE_CHROME_PROFILE: CHROME_PROFILE,
  };
  bridgeProc = spawn(BRIDGE_EXE, [], { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  bridgeProc.stdout.pipe(stream);
  bridgeProc.stderr.pipe(stream);
  bridgeProc.on('error', (e) => {
    appendLog('bridge', `[bridge] spawn error: ${e}\n`);
    console.error('[bridge] spawn error', e);
  });
  bridgeProc.on('exit', (code) => {
    appendLog('bridge', `[bridge] exited code=${code}\n`);
  });
  appendLog('bridge', `[bridge] spawn pid=${bridgeProc.pid} cwd=${cwd}\n`);
  return bridgeProc;
}

function spawnOpencode(cwd) {
  if (opencodeProc) killChild(opencodeProc);
  const { stream } = logStream('opencode');
  servePassword = require('crypto').randomBytes(24).toString('base64url');
  const args = ['serve', '--hostname', '127.0.0.1', '--port', String(SERVE_PORT)];
  const env = {
    ...process.env,
    OPENCODE_SERVER_PASSWORD: servePassword,
  };
  opencodeProc = spawn(OPENCODE_EXE, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  opencodeProc.stdout.pipe(stream);
  opencodeProc.stderr.pipe(stream);
  opencodeProc.on('error', (e) => {
    appendLog('opencode', `[opencode] spawn error: ${e}\n`);
    console.error('[opencode] spawn error', e);
  });
  opencodeProc.on('exit', (code) => {
    appendLog('opencode', `[opencode] exited code=${code}\n`);
  });
  appendLog('opencode', `[opencode] spawn serve port=${SERVE_PORT} cwd=${cwd}\n`);
  return opencodeProc;
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

function httpGetJson(port, pathname, timeoutMs = 3000) {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: '127.0.0.1', port, path: pathname, method: 'GET', timeout: timeoutMs },
      (res) => {
        let raw = '';
        res.on('data', (d) => (raw += d));
        res.on('end', () => {
          try { resolve({ status: res.statusCode, json: JSON.parse(raw) }); }
          catch { resolve({ status: res.statusCode, json: null, body: raw }); }
        });
      },
    );
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
    req.end();
  });
}

function httpPostJson(port, pathname, body, headers, timeoutMs = 8000) {
  return new Promise((resolve, reject) => {
    const data = body ? JSON.stringify(body) : '';
    const req = http.request(
      {
        host: '127.0.0.1',
        port,
        path: pathname,
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(data),
          ...(headers || {}),
        },
        timeout: timeoutMs,
      },
      (res) => {
        let raw = '';
        res.on('data', (d) => (raw += d));
        res.on('end', () => {
          try { resolve({ status: res.statusCode, json: JSON.parse(raw) }); }
          catch { resolve({ status: res.statusCode, json: null, body: raw }); }
        });
      },
    );
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
    req.end(data);
  });
}

async function waitForBridge(timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await httpGetRaw(BRIDGE_PORT, '/health', 1500);
      if (r.status === 200) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 300));
  }
  return false;
}

async function waitForOpencode(timeoutMs = 90000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await httpGetRaw(SERVE_PORT, '/global/health', 2000);
      if (r.status === 200) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  return false;
}

async function startServices(dir) {
  if (!dir) return false;
  ensureDir(DATA_DIR);
  ensureDir(LOG_DIR);
  ensureOpencodeJson(dir);
  spawnBridge(dir);
  spawnOpencode(dir);
  const [bridgeOk, opencodeOk] = await Promise.all([waitForBridge(), waitForOpencode()]);
  appendLog('boot', `[boot] services bridgeOk=${bridgeOk} opencodeOk=${opencodeOk}\n`);
  return bridgeOk && opencodeOk;
}

// ---------- 자동 업데이트 ----------
function setupAutoUpdate() {
  if (!autoUpdater) {
    appendLog('updater', '[updater] electron-updater unavailable\n');
    return;
  }
  try {
    autoUpdater.logger = console;
    autoUpdater.autoDownload = true;
    autoUpdater.autoInstallOnAppQuit = true;

    autoUpdater.on('update-available', (info) => {
      updateState = 'available';
      updateVersion = info && info.version ? String(info.version) : null;
      if (tray) tray.setToolTip(`CodeBridge — 업데이트 ${info.version} 다운로드 중`);
      appendLog('updater', `[updater] update-available v${info.version}\n`);
    });
    autoUpdater.on('update-not-available', () => {
      updateState = 'up-to-date';
      updateVersion = null;
      if (tray) tray.setToolTip('CodeBridge Desktop');
    });
    autoUpdater.on('download-progress', () => {
      updateState = 'available';
    });
    autoUpdater.on('update-downloaded', (info) => {
      updateState = 'downloaded';
      updateVersion = info && info.version ? String(info.version) : updateVersion;
      if (tray) tray.setToolTip('CodeBridge — 재시작하면 업데이트가 적용됩니다');
      appendLog('updater', `[updater] update-downloaded v${info.version}\n`);
    });
    autoUpdater.on('error', (err) => {
      updateState = 'error';
      appendLog('updater', `[updater] error: ${err && err.message ? err.message : err}\n`);
    });

    autoUpdater.checkForUpdatesAndNotify().catch((e) => {
      appendLog('updater', `[updater] check failed: ${e}\n`);
    });
  } catch (e) {
    appendLog('updater', `[updater] setup failed: ${e}\n`);
  }
}

// ---------- 트레이 ----------
function buildTray() {
  const icon = nativeImage.createFromPath(path.join(RES, 'tray-icon.png'));
  tray = new Tray(icon.isEmpty() ? nativeImage.createEmpty() : icon);
  tray.setToolTip('CodeBridge Desktop');
  tray.setContextMenu(
    Menu.buildFromTemplate([
      {
        label: 'CodeBridge 열기',
        click: () => {
          if (mainWindow) {
            if (mainWindow.isMinimized()) mainWindow.restore();
            mainWindow.show();
            mainWindow.focus();
          }
        },
      },
      { type: 'separator' },
      {
        label: '프로젝트 폴더 변경…',
        click: async () => {
          const picked = await pickProjectFolder();
          if (!picked) return;
          projectDir = picked;
          writeSettings({ ...readSettings(), projectDir });
          appendLog('boot', `[boot] projectDir changed: ${picked}\n`);
          // 새 프로젝트로 서비스 재시작
          await startServices(projectDir);
        },
      },
      {
        label: '지금 인수인계',
        click: () => {
          httpPostJson(BRIDGE_PORT, '/v1/handover/trigger', {}, {}, 5000)
            .then((r) => appendLog('boot', `[boot] handover trigger -> ${r.status} ${r.body ?? ''}\n`))
            .catch((e) => appendLog('boot', `[boot] handover trigger error: ${e}\n`));
        },
      },
      { type: 'separator' },
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
            dialog.showMessageBoxSync({
              type: 'info',
              title: 'CodeBridge 업데이트',
              message: updateState === 'downloaded'
                ? `새 버전(${latest})이 다운로드되어 있습니다. 종료 시 설치됩니다.`
                : updateState === 'up-to-date'
                  ? `현재 버전(${app.getVersion()})이 최신입니다.`
                  : `최신 버전: ${latest} (현재 ${app.getVersion()})`,
            });
          } catch (e) {
            dialog.showMessageBoxSync({
              type: 'warning',
              title: 'CodeBridge',
              message: `업데이트 확인 실패: ${e && e.message ? e.message : e}`,
            });
          }
        },
      },
      {
        label: '종료',
        click: () => {
          app.quit();
        },
      },
    ]),
  );
}

// ---------- 창 ----------
function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 840,
    minWidth: 1000,
    minHeight: 640,
    title: 'CodeBridge',
    backgroundColor: '#0a0e14',
    frame: false,
    autoHideMenuBar: true,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      preload: path.join(__dirname, 'preload.cjs'),
    },
  });
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
  mainWindow.loadFile(path.join(UI_DIR, 'index.html'));
}

// 전용 Chrome의 프로세스 id를 기억해 둔다 (%APPDATA%CodeBridge\dedicated-chrome.pid).
// 앱 종료 시 이 pid로 전용 Chrome을 함께 종료하고, 로그인이 필요할 때 다시 띄우는 데도 쓴다.
const CHROME_PID_PATH = path.join(DATA_DIR, 'dedicated-chrome.pid');

function readChromePid() {
  try {
    const n = Number(fs.readFileSync(CHROME_PID_PATH, 'utf8').trim());
    return Number.isInteger(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

function findDedicatedChromePidByWmi() {
  try {
    const q = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'chrome.exe' -and $_.CommandLine -match 'chrome-profile' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1 -ExpandProperty ProcessId",
      ],
      { windowsHide: true, encoding: 'utf8', timeout: 8000 },
    );
    const n = Number((q.stdout || '').trim());
    return Number.isInteger(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

// 전용 Chrome 트리 전체를 종료한다. pid 파일이 우선, 없거나 죽었으면 WMI로 탐색한다.
// (Chrome은 앱을 꺼도 계속 떠 있으면 안 된다 — before-quit에서 호출)
function findDedicatedChromePid() {
  const fromFile = readChromePid();
  if (fromFile && pidAlive(fromFile)) return fromFile;
  return findDedicatedChromePidByWmi();
}

function killDedicatedChrome() {
  const pid = findDedicatedChromePid();
  if (pid) {
    try {
      spawnSync('taskkill', ['/pid', String(pid), '/t', '/f'], { windowsHide: true });
    } catch {
      /* 이미 종료된 경우 무시 */
    }
  }
  try {
    fs.unlinkSync(CHROME_PID_PATH);
  } catch {
    /* 파일이 없으면 무시 */
  }
}

function pidAlive(pid) {
  try {
    const r = spawnSync('tasklist', ['/FI', `PID eq ${pid}`, '/NH'], {
      windowsHide: true,
      encoding: 'utf8',
    });
    return !!r.stdout && r.stdout.includes(String(pid));
  } catch {
    return false;
  }
}

// ---------- IPC ----------
// 전용 Chrome을 화면에 꺼낸다 (로그인/캡차 해결용). 숨겨진 인스턴스가 있으면 그 창을 앞으로,
// 없으면 WMI로 전용 프로필을 쓰는 브라우저 프로세스를 찾아 꺼내고, 그것도 없으면 새로 띄운다.
ipcMain.handle('app:showChrome', async () => {
  try {
    let pid = readChromePid();
    if (!pid || !pidAlive(pid)) {
      pid = findDedicatedChromePidByWmi();
    }
    const path = pid ? `/v1/chrome/show?pid=${pid}` : '/v1/chrome/show';
    const r = await httpGetJson(BRIDGE_PORT, path, 20000);
    appendLog('boot', `[boot] showChrome -> ${JSON.stringify(r.json || {})}\n`);
    return r.json || { ok: false, error: 'bridge returned no body' };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

ipcMain.on('window:action', (_e, action) => {
  if (!mainWindow) return;
  switch (action) {
    case 'minimize': mainWindow.minimize(); break;
    case 'toggle-maximize': mainWindow.isMaximized() ? mainWindow.unmaximize() : mainWindow.maximize(); break;
    case 'close': mainWindow.close(); break;
  }
});

ipcMain.handle('app:getBoot', () => ({
  projectDir: projectDir || null,
  appVersion: app.getVersion(),
}));

ipcMain.handle('app:getUpdateState', () => ({
  state: updateState,
  version: updateVersion,
  appVersion: app.getVersion(),
}));

ipcMain.handle('updater:quitAndInstall', () => {
  if (updateState !== 'downloaded') {
    return { ok: false, message: '다운로드가 아직 완료되지 않았습니다.' };
  }
  appendLog('updater', '[updater] quitAndInstall (from banner)\n');
  autoUpdater.quitAndInstall();
  return { ok: true };
});

ipcMain.handle('app:pickProject', async () => {
  const picked = await pickProjectFolder();
  if (!picked) return null;
  projectDir = picked;
  writeSettings({ ...readSettings(), projectDir });
  appendLog('boot', `[boot] projectDir selected: ${picked}\n`);
  await startServices(projectDir);
  return picked;
});

function ocFetchJson(pathname, init) {
  return new Promise((resolve) => {
    const method = (init && init.method) || 'GET';
    const data = init && init.body != null ? JSON.stringify(init.body) : undefined;
    const req = http.request(
      {
        host: '127.0.0.1',
        port: SERVE_PORT,
        path: pathname,
        method,
        timeout: 30000,
        headers: {
          Authorization:
            'Basic ' + Buffer.from(`opencode:${servePassword}`).toString('base64'),
          ...(data
            ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(data) }
            : {}),
        },
      },
      (res) => {
        let raw = '';
        res.on('data', (d) => (raw += d));
        res.on('end', () => {
          try { resolve({ status: res.statusCode, json: JSON.parse(raw) }); }
          catch { resolve({ status: res.statusCode, json: null, body: raw.slice(0, 4096) }); }
        });
      },
    );
    req.on('timeout', () => { req.destroy(); resolve({ status: 0, json: null, error: 'timeout' }); });
    req.on('error', (e) => resolve({ status: 0, json: null, error: String(e) }));
    if (data) req.end(data); else req.end();
  });
}

ipcMain.handle('oc:fetch', (_e, pathname, init) => ocFetchJson(pathname, init));

ipcMain.handle('bridge:fetch', (_e, pathname, init) => httpGetJson(BRIDGE_PORT, pathname));

function probeBridge() {
  return httpGetJson(BRIDGE_PORT, '/v1/probe', 4000).then((r) => r.json);
}
ipcMain.handle('app:probe', async () => {
  const r = await httpGetJson(BRIDGE_PORT, '/v1/probe', 4000).catch(() => null);
  return r && r.json
    ? r.json
    : {
        ok: false,
        chrome_running: false,
        chrome_not_running: true,
        login_required: false,
        sites: [],
      };
});

// ---------- 종료 처리 ----------
process.on('uncaughtException', (e) => {
  appendLog('boot', `[boot] uncaughtException: ${e && (e.stack || e.message) || e}\n`);
});
process.on('unhandledRejection', (e) => {
  appendLog('boot', `[boot] unhandledRejection: ${e && (e.stack || e.message) || e}\n`);
});

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.show();
      mainWindow.focus();
    }
  });

  app.whenReady().then(async () => {
    try {
      appendLog('boot', '[boot] whenReady: start\n');
      ensureDir(DATA_DIR);
      ensureDir(LOG_DIR);
      projectDir = await resolveProjectDir();
      createWindow();
      buildTray();
      setupAutoUpdate();
      appendLog('boot', `[boot] window created; projectDir=${projectDir || '(unset)'}\n`);
      if (projectDir) {
        startServices(projectDir);
      }
      // 프로젝트 폴더가 없으면 렌더러가 화면에서 선택한다
    } catch (e) {
      appendLog('boot', `[boot] whenReady error: ${e && (e.stack || e.message) || e}\n`);
    }
  });

  app.on('window-all-closed', () => {
    // 창을 닫으면 백그라운드에 남지 않고 완전히 종료한다
    app.quit();
  });

  app.on('before-quit', () => {
    appQuitting = true;
    killChild(bridgeProc);
    killChild(opencodeProc);
    killDedicatedChrome();
  });
}
