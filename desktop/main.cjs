/**
 * CodeBridge Desktop — Electron main process (v0.6.x)
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
  shell,
  Notification,
  screen,
} = require('electron');
let autoUpdater = null;
try {
  ({ autoUpdater } = require('electron-updater'));
} catch (e) {
  console.error('[updater] require failed:', e);
}
// 개발/검증 환경에서 별도 userData를 쓴다 (릴리스 스모크 게이트가 설치본과 충돌하지 않도록).
// CODEBRIDGE_USER_DATA_DIR를 주면 그 경로로, 아니면 %APPDATA%\CodeBridge.
const ISOLATED_USER_DATA = process.env.CODEBRIDGE_USER_DATA_DIR;

const path = require('path');
const fs = require('fs');
const http = require('http');
const { spawn, spawnSync } = require('child_process');

app.setPath('userData', ISOLATED_USER_DATA || path.join(app.getPath('appData'), 'CodeBridge'));

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

// ---------- 최근 프로젝트 (MRU, 최대 5개) ----------
function addRecentProject(dir) {
  try {
    const cur = readSettings();
    const list = Array.isArray(cur.recentProjects) ? cur.recentProjects.filter((p) => p && p !== dir && fs.existsSync(p)) : [];
    list.unshift(dir);
    writeSettings({ ...cur, recentProjects: list.slice(0, 5) });
  } catch {
    /* MRU 갱신 실패는 무시 */
  }
}

// ---------- 창 크기/위치 기억 ----------
let lastLayout = null;
let boundsSaveTimer = null;
function readWindowBounds() {
  try {
    const b = readSettings().windowBounds;
    if (b && Number.isFinite(b.width) && Number.isFinite(b.height) && b.width >= 640 && b.height >= 480) {
      return b;
    }
  } catch {
    /* 무시 */
  }
  return null;
}

// 저장된 창 배치를 디스플레이 workArea 안쪽으로 클램프 (모니터 해제/해상도 변경 대비)
function clampWindowBounds(saved, area) {
  const clamp = (v, min, max) => Math.min(Math.max(v, min), Math.max(min, max));
  const width = clamp(saved.width, 1000, area.width);
  const height = clamp(saved.height, 640, area.height);
  return {
    width,
    height,
    x: clamp(saved.x, area.x, area.x + area.width - width),
    y: clamp(saved.y, area.y, area.y + area.height - height),
  };
}
function saveWindowBoundsSoon() {
  clearTimeout(boundsSaveTimer);
  boundsSaveTimer = setTimeout(() => {
    try {
      if (lastLayout) writeSettings({ ...readSettings(), windowBounds: lastLayout });
    } catch {
      /* 무시 */
    }
  }, 400);
}
function trackWindowBounds(win) {
  const capture = () => {
    if (!mainWindow) return;
    const b = mainWindow.getNormalBounds();
    lastLayout = { x: b.x, y: b.y, width: b.width, height: b.height, isMaximized: mainWindow.isMaximized() };
    saveWindowBoundsSoon();
  };
  win.on('move', capture);
  win.on('resize', capture);
  win.on('close', () => {
    clearTimeout(boundsSaveTimer);
    try {
      if (lastLayout) writeSettings({ ...readSettings(), windowBounds: lastLayout });
    } catch {
      /* 무시 */
    }
  });
}

// 설정 기본값 — 키가 없으면 기본값으로 병합한다.
const SETTINGS_DEFAULTS = {
  model: 'webchat-chatgpt',      // 새 대화의 기본 모델
  autoCheckUpdates: true,        // 실행 시 자동 업데이트 확인
  dangerConfirm: true,           // 위험 명령 확인 카드 (브릿지에 CODEBRIDGE_DANGER_CONFIRM로 전달)
  handoverLimit: 25,             // 인수인계 소프트 리밋 (브릿지에 CODEBRIDGE_HANDOVER_LIMIT로 전달)
  chromeAlwaysVisible: false,    // 전용 Chrome을 항상 화면에 표시 (CODEBRIDGE_CHROME_KEEP_VISIBLE로 전달)
  notifyEnabled: true,           // 질문 카드/완료 Windows 알림
  hideContextNote: false,        // "메시지 N개 쌓였습니다" 안내 숨김 (CODEBRIDGE_NO_CONTEXT_NOTE로 전달)
  startAtBoot: false,            // Windows 시작 시 함께 실행
  recentProjects: [],            // 최근 프로젝트 폴더 목록 (최신순, 최대 5개)
  onboarded: false,              // 첫 실행/온보딩 안내 표시 여부 (사용자가 닫으면 true)
  exportDir: '',                 // 대화 내보내기 마지막 저장 폴더
};
function getSettings() {
  return { ...SETTINGS_DEFAULTS, ...readSettings() };
}
async function resolveProjectDir() {
  const saved = readSettings().projectDir;
  if (saved && fs.existsSync(saved)) {
    addRecentProject(saved);
    return saved;
  }
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

// 자식/손자 프로세스 트리가 종료될 때까지 기다린다 (taskkill 발사와 무관하게 exit 대기).
// 포트 재사용 바인딩 실패를 방지하기 위해 교체 스폰 전에 호출한다.
function waitChildExit(proc, timeoutMs = 8000) {
  return new Promise((resolve) => {
    if (!proc || proc.killed || !proc.pid) {
      resolve(true);
      return;
    }
    if (proc.exitCode !== null || proc.signalCode) {
      resolve(true);
      return;
    }
    let done = false;
    const finish = (ok) => {
      if (done) return;
      done = true;
      clearTimeout(t);
      resolve(ok);
    };
    const t = setTimeout(() => finish(false), timeoutMs);
    proc.once('exit', () => finish(true));
  });
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
      message: '작업할 프로젝트의 루트 폴더를 선택하세요',
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

async function spawnBridge(cwd) {
  if (bridgeProc) {
    killChild(bridgeProc);
    await waitChildExit(bridgeProc, 8000);
  }
  rotateLargeLog('bridge.log');
  const { stream } = logStream('bridge');
  const settings = getSettings();
  const env = {
    ...process.env,
    CODEBRIDGE_CONFIG: BRIDGE_CONFIG,
    CODEBRIDGE_CHROME_PROFILE: CHROME_PROFILE,
    CODEBRIDGE_DANGER_CONFIRM: settings.dangerConfirm ? '1' : '0',
    CODEBRIDGE_HANDOVER_LIMIT: String(settings.handoverLimit),
    CODEBRIDGE_CHROME_KEEP_VISIBLE: settings.chromeAlwaysVisible ? '1' : '0',
    CODEBRIDGE_NO_CONTEXT_NOTE: settings.hideContextNote ? '1' : '0',
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

async function spawnOpencode(cwd) {
  if (opencodeProc) {
    killChild(opencodeProc);
    await waitChildExit(opencodeProc, 8000);
  }
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
  await spawnBridge(dir);
  // opencode는 부팅 직후 webchat 프로바이더(브릿지 /v1/models)를 조회한다.
  // 브릿지가 9224에 바인딩되기 전에 opencode가 조회하면 opencode는 모델을
  // "unavailable"로 표시하고 세션이 실패한다 (실사용 사례: 3분간 '처리 중' 멈춤,
  //  opencode.log "SessionRunnerModel.ModelUnavailableError").
  // 반드시 브릿지 health를 기다린 뒤에 opencode를 띄운다.
  const t0 = Date.now();
  const bridgeOk = await waitForBridge(30000);
  appendLog('boot', `[boot] bridge healthy=${bridgeOk} after ${Date.now() - t0}ms\n`);
  await spawnOpencode(dir);
  const opencodeOk = await waitForOpencode(90000);
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

    if (getSettings().autoCheckUpdates) {
      autoUpdater.checkForUpdatesAndNotify().catch((e) => {
        appendLog('updater', `[updater] check failed: ${e}\n`);
      });
    } else {
      appendLog('updater', '[updater] auto-check disabled in settings (manual check still works)\n');
    }
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
  const savedBounds = readWindowBounds();
  const area = screen.getPrimaryDisplay().workArea; // {x, y, width, height}
  const base = savedBounds ?? { x: undefined, y: undefined, width: 1280, height: 840 };
  const clamped = clampWindowBounds(
    {
      width: base.width,
      height: base.height,
      x: Number.isFinite(base.x) ? base.x : area.x + 40,
      y: Number.isFinite(base.y) ? base.y : area.y + 40,
    },
    area,
  );
  const windowOpts = {
    width: clamped.width,
    height: clamped.height,
    minWidth: 1000,
    minHeight: 640,
    title: 'CodeBridge',
    x: clamped.x,
    y: clamped.y,
  };
  mainWindow = new BrowserWindow({
    ...windowOpts,
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

  // 창 크기/위치 기억
  trackWindowBounds(mainWindow);
  if (savedBounds?.isMaximized) mainWindow.maximize();

  // 렌더러 console 메시지를 파일로 남긴다 (크래시 진단용)
  mainWindow.webContents.on('console-message', (_event, level, message, line, sourceId) => {
    try {
      rotateLargeLog('renderer.log');
      appendLog('renderer', `[renderer] L${level} ${String(message).slice(0, 1000)} (${path.basename(String(sourceId || ''))}:${line})\n`);
    } catch {
      /* 로깅 실패는 무시 */
    }
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
  mainWindow.loadFile(path.join(UI_DIR, 'index.html'));
}

// 전용 Chrome의 프로세스 id를 기억해 둔다 (%APPDATA%CodeBridge\dedicated-chrome.pid).
// 앱 종료 시 이 pid로 전용 Chrome을 함께 종료하고, 로그인이 필요할 때 다시 띄우는 데도 쓴다.
const CHROME_PID_PATH = path.join(DATA_DIR, 'dedicated-chrome.pid');

// 전용 Chrome의 정확한 user-data-dir (프로필 경로). find/verify 모두 이 경로로 일치시킨다.
function profileNeedle() {
  const needle = path.join(process.env.APPDATA || '', 'CodeBridge', 'chrome-profile');
  // PS 단일 따옴표 이스케이프 (경로에 따옴표가 있으면 두 번 반복)
  return needle.replace(/'/g, "''");
}

// 전용 프로필을 쓰는 브라우저(최상위) Chrome 프로세스의 pid를 WMI로 찾는다.
function findDedicatedChromePidByWmi() {
  try {
    const needle = profileNeedle();
    const r = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'chrome.exe' -and $_.CommandLine -like '*--user-data-dir=${needle}*' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1 -ExpandProperty ProcessId`,
      ],
      { windowsHide: true, encoding: 'utf8', timeout: 8000 },
    );
    const n = Number((r.stdout || '').trim());
    return Number.isInteger(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

// pid가 진짜 전용 Chrome 브라우저 프로세스인지 검증한다 (pid 재활용 방지).
// 이름이 chrome.exe이고 정확히 같은 --user-data-dir을 가져야 신뢰한다.
function isDedicatedChrome(pid) {
  try {
    const needle = profileNeedle();
    const r = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -eq ${pid} -and $_.Name -eq 'chrome.exe' -and $_.CommandLine -like '*--user-data-dir=${needle}*' } | Measure-Object | Select-Object -ExpandProperty Count`,
      ],
      { windowsHide: true, encoding: 'utf8', timeout: 8000 },
    );
    return Number((r.stdout || '').trim()) > 0;
  } catch {
    return false;
  }
}

// pid 파일을 읽되 반드시 전용 Chrome 프로세스인지 검증한 뒤에만 돌려준다.
function readChromePid() {
  try {
    const n = Number(fs.readFileSync(CHROME_PID_PATH, 'utf8').trim());
    if (!Number.isInteger(n) || n <= 0) return null;
    return isDedicatedChrome(n) ? n : null;
  } catch {
    return null;
  }
}

// 전용 Chrome 트리 전체를 종료한다. pid 파일이 우선, 없거나 가짜면 WMI로 탐색한다.
// (Chrome은 앱을 꺼도 계속 떠 있으면 안 된다 — before-quit에서 호출)
function findDedicatedChromePid() {
  return readChromePid() || findDedicatedChromePidByWmi();
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

// ---------- IPC ----------
// 전용 Chrome을 화면에 꺼낸다 (로그인/캡차 해결용). 숨겨진 인스턴스가 있으면 그 창을 앞으로,
// 없으면 WMI로 전용 프로필을 쓰는 브라우저 프로세스를 찾아 꺼내고, 그것도 없으면 새로 띄운다.
ipcMain.handle('app:showChrome', async () => {
  try {
    let pid = readChromePid();
    if (!pid) {
      pid = findDedicatedChromePidByWmi();
    }
    const path = pid ? `/v1/chrome/show?pid=${pid}` : '/v1/chrome/show';
    const r = await httpGetJson(BRIDGE_PORT, path, 60000);
    appendLog('boot', `[boot] showChrome -> ${JSON.stringify(r.json || {})}\n`);
    return r.json || { ok: false, error: 'bridge returned no body' };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// 전용 Chrome을 다시 숨긴다 (보이기로 꺼낸 창을 화면에서 치움)
ipcMain.handle('app:hideChrome', async () => {
  try {
    let pid = readChromePid();
    if (!pid) {
      pid = findDedicatedChromePidByWmi();
    }
    const path = pid ? `/v1/chrome/hide?pid=${pid}` : '/v1/chrome/hide';
    const r = await httpGetJson(BRIDGE_PORT, path, 10000);
    appendLog('boot', `[boot] hideChrome -> ${JSON.stringify(r.json || {})}\n`);
    return r.json || { ok: false, error: 'bridge returned no body' };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// 사용자가 "위험한 명령 확인 카드"에서 승인한 셸 명령을 앱이 직접 실행한다.
// 브릿지/웹챗을 다시 부르지 않고 PowerShell에서 실행하고 결과를 돌려준다.
ipcMain.handle('app:runApprovedShell', async (_e, command) => {
  if (typeof command !== 'string' || !command.trim()) {
    return { ok: false, error: 'command가 비어 있습니다' };
  }
  try {
    const cwd = (projectDir && fs.existsSync(projectDir)) ? projectDir : process.cwd();
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], {
      cwd,
      encoding: 'utf8',
      timeout: 120000,
      maxBuffer: 1 * 1024 * 1024,
      windowsHide: true,
    });
    const out = [result.stdout, result.stderr].filter(Boolean).join('\n').trim();
    appendLog('shell', `[shell] approved cmd=${command.slice(0,120)}\n${out.slice(0,2000)}\n`);
    if (result.error) {
      return { ok: false, error: String(result.error) };
    }
    return { ok: true, output: out.slice(0, 4000) || '(출력이 없습니다)' };
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
  addRecentProject(picked);
  appendLog('boot', `[boot] projectDir selected: ${picked}\n`);
  await startServices(projectDir);
  return picked;
});

// ---------- 설정(IPC) ----------
ipcMain.handle('app:getSettings', () => ({ ...getSettings(), logDir: LOG_DIR }));

ipcMain.handle('app:setSettings', async (_e, patch) => {
  if (!patch || typeof patch !== 'object') return { ok: false, error: '잘못된 요청입니다.' };
  const cur = getSettings();
  const next = { ...cur };
  if (patch.model === 'webchat-chatgpt' || patch.model === 'webchat-gemini') next.model = patch.model;
  if (typeof patch.autoCheckUpdates === 'boolean') next.autoCheckUpdates = patch.autoCheckUpdates;
  if (typeof patch.dangerConfirm === 'boolean') next.dangerConfirm = patch.dangerConfirm;
  if (patch.handoverLimit != null) {
    const n = Number(patch.handoverLimit);
    if (Number.isInteger(n) && [10, 25, 50].includes(n)) next.handoverLimit = n;
  }
  if (typeof patch.chromeAlwaysVisible === 'boolean') next.chromeAlwaysVisible = patch.chromeAlwaysVisible;
  if (typeof patch.notifyEnabled === 'boolean') next.notifyEnabled = patch.notifyEnabled;
  if (typeof patch.hideContextNote === 'boolean') next.hideContextNote = patch.hideContextNote;
  if (typeof patch.onboarded === 'boolean') next.onboarded = patch.onboarded;
  if (typeof patch.startAtBoot === 'boolean') {
    next.startAtBoot = patch.startAtBoot;
    try {
      app.setLoginItemSettings({ openAtLogin: patch.startAtBoot });
      appendLog('settings', `[settings] startAtBoot -> ${patch.startAtBoot}\n`);
    } catch (e) {
      appendLog('settings', `[settings] setLoginItemSettings failed: ${e}\n`);
    }
  }
  if (patch.projectDir != null) {
    return { ok: false, error: 'projectDir은 폴더 선택으로만 변경할 수 있습니다.' };
  }
  writeSettings(next);
  // 브릿지 env가 필요한 값은 브릿지만 즉시 재시작 (opencode/세션은 유지)
  const bridgeAffected =
    cur.dangerConfirm !== next.dangerConfirm ||
    cur.handoverLimit !== next.handoverLimit ||
    cur.chromeAlwaysVisible !== next.chromeAlwaysVisible ||
    cur.hideContextNote !== next.hideContextNote;
  if (bridgeAffected && projectDir) {
    appendLog('settings', `[settings] bridge restart (dangerConfirm=${next.dangerConfirm} handoverLimit=${next.handoverLimit})\n`);
    await spawnBridge(projectDir);
  }
  return { ok: true, settings: next };
});

ipcMain.handle('app:openLogsFolder', async () => {
  try {
    ensureDir(LOG_DIR);
    const err = await shell.openPath(LOG_DIR);
    return err ? { ok: false, error: err } : { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

ipcMain.handle('app:openReleasesPage', async () => {
  try {
    await shell.openExternal('https://github.com/skyb85993/codebridge/releases');
    return { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// 프로젝트 폴더를 탐색기로 연다 (에이전트가 막 수정한 파일을 바로 확인할 때)
ipcMain.handle('app:openProjectFolder', async () => {
  if (!projectDir || !fs.existsSync(projectDir)) {
    return { ok: false, error: '프로젝트 폴더가 선택되지 않았거나 존재하지 않습니다.' };
  }
  try {
    const err = await shell.openPath(projectDir);
    return err ? { ok: false, error: err } : { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// 수동 업데이트 확인 — 완료 여부는 app:getUpdateState를 폴링해 알린다.
ipcMain.handle('app:checkUpdates', async () => {
  if (!autoUpdater) return { ok: false, error: '업데이터가 이 빌드에서 비활성화되어 있습니다.' };
  try {
    updateState = 'checking';
    await autoUpdater.checkForUpdates();
    return { ok: true, state: updateState, version: updateVersion };
  } catch (e) {
    updateState = 'error';
    appendLog('updater', `[updater] manual check failed: ${e && e.message ? e.message : e}\n`);
    return { ok: false, error: String(e && e.message ? e.message : e) };
  }
});

// 전용 Chrome 종료 + 프로필 폴더를 .bak으로 옮겨 초기화 (다음 요청부터 재로그인 필요).
ipcMain.handle('app:resetChromeProfile', async () => {
  try {
    killDedicatedChrome();
    const bak = `${CHROME_PROFILE}.bak-${Date.now()}`;
    if (fs.existsSync(CHROME_PROFILE)) {
      fs.renameSync(CHROME_PROFILE, bak);
      appendLog('settings', `[settings] chrome-profile reset -> moved to ${path.basename(bak)}\n`);
    } else {
      appendLog('settings', '[settings] chrome-profile reset (profile folder did not exist)\n');
    }
    if (projectDir) await spawnBridge(projectDir);
    return { ok: true, backup: fs.existsSync(bak) ? bak : null };
  } catch (e) {
    appendLog('settings', `[settings] chrome-profile reset failed: ${e && e.message ? e.message : e}\n`);
    return { ok: false, error: String(e && e.message ? e.message : e) };
  }
});

// 브릿지만 재시작 (설정 env 반영, opencode 유지)
ipcMain.handle('app:restartBridge', async () => {
  if (!projectDir) return { ok: false, error: '프로젝트 폴더가 선택되지 않았습니다.' };
  await spawnBridge(projectDir);
  const ok = await waitForBridge(30000);
  appendLog('boot', `[boot] restartBridge bridge healthy=${ok}\n`);
  return ok ? { ok: true } : { ok: false, error: '브릿지가 30초 내에 준비되지 않았습니다. 잠시 후 다시 시도해 주세요.' };
});

// 렌더러 알림 — 질문 카드 도착/작업 완료 시 토스트. 클릭하면 창을 띄운다.
ipcMain.handle('app:notify', (_e, payload) => {
  try {
    if (!Notification.isSupported()) return { ok: false, error: '알림 미지원' };
    const title = (payload && typeof payload.title === 'string' && payload.title) || 'CodeBridge';
    const body = (payload && typeof payload.body === 'string' && payload.body) || '';
    const n = new Notification({ title, body, silent: false });
    n.on('click', () => {
      if (mainWindow) {
        if (mainWindow.isMinimized()) mainWindow.restore();
        mainWindow.show();
        mainWindow.focus();
      }
    });
    n.show();
    return { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// 브릿지+opencode 서비스 전체 재시작 (진행 중인 요청은 중단됨 — UI에서 경고)
ipcMain.handle('app:restartServices', async () => {
  if (!projectDir) return { ok: false, error: '프로젝트 폴더가 선택되지 않았습니다.' };
  try {
    const ok = await startServices(projectDir);
    appendLog('settings', `[settings] services restarted (ok=${ok})\n`);
    return { ok };
  } catch (e) {
    appendLog('settings', `[settings] services restart failed: ${e}\n`);
    return { ok: false, error: String(e) };
  }
});

// 최근 프로젝트 MRU에서 항목 제거
ipcMain.handle('app:removeRecentProject', async (_e, dir) => {
  const cur = Array.isArray(getSettings().recentProjects) ? getSettings().recentProjects : [];
  writeSettings({ ...readSettings(), recentProjects: cur.filter((p) => p !== dir) });
  return { ok: true };
});

// MRU에서 폴더를 즉시 전환 (서비스 재시작 포함)
ipcMain.handle('app:switchProject', async (_e, dir) => {
  if (typeof dir !== 'string' || !dir.trim()) {
    return { ok: false, error: '잘못된 경로입니다.' };
  }
  let stat;
  try {
    stat = fs.statSync(dir);
  } catch {
    return { ok: false, error: '폴더를 찾을 수 없습니다.' };
  }
  if (!stat.isDirectory()) {
    return { ok: false, error: '디렉토리가 아닙니다.' };
  }
  projectDir = dir;
  writeSettings({ ...readSettings(), projectDir });
  addRecentProject(dir);
  appendLog('boot', `[boot] project switched to ${dir}\n`);
  try {
    const ok = await startServices(dir);
    if (!ok) return { ok: false, error: '서비스를 재시작하지 못했습니다.' };
  } catch (e) {
    appendLog('boot', `[boot] restart after switch failed: ${e}\n`);
    return { ok: false, error: String(e) };
  }
  return { ok: true, projectDir: dir };
});

// 대화 내보내기 — 저장 대화상자를 띄우고 마크다운으로 저장
ipcMain.handle('app:exportSession', async (_e, payload) => {
  try {
    const markdown = payload && typeof payload.markdown === 'string' ? payload.markdown : '';
    if (!markdown) return { ok: false, error: '내보낼 내용이 없습니다.' };
    const filename =
      payload && typeof payload.filename === 'string' && payload.filename.trim()
        ? payload.filename.trim()
        : 'conversation.md';
    const result = await dialog.showSaveDialog(mainWindow, {
      title: '대화 내보내기',
      defaultPath:
        (getSettings().exportDir ? path.join(getSettings().exportDir, filename) : filename) ||
        'conversation.md',
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    });
    if (result.canceled || !result.filePath) return { ok: false, cancelled: true };
    fs.writeFileSync(result.filePath, markdown, 'utf8');
    writeSettings({ ...readSettings(), exportDir: path.dirname(result.filePath) });
    appendLog('settings', `[export] wrote ${result.filePath} (${markdown.length} chars)\n`);
    return { ok: true, path: result.filePath };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
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
        startServices(projectDir).catch((e) =>
          appendLog('boot', `[boot] startServices failed: ${e}\n`),
        );
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
