/**
 * CodeBridge Desktop — opencode SPA(iframe, http://127.0.0.1:7891) 첫 실행 시드.
 * 세션의 모든 webContents에 주입되지만 origin이 정확히 SPA 서버일 때만 동작한다.
 * opencode 데스크톱 preload가 해주던 것을 대신한다:
 *   1) webchat-* 모델을 모델 피커에 보이게 (기본값 OFF 문제)
 *   2) 앱에서 선택한 프로젝트 폴더를 SPA의 프로젝트 목록에 등록 (＋ 새 세션이 동작하게)
 * 어떤 경우에도 SPA를 죽이지 않도록 전부 방어적으로 처리한다.
 */
'use strict';

try {
  if (location.origin !== 'http://127.0.0.1:7891') return;
  if (typeof localStorage === 'undefined') return;

  const { ipcRenderer } = require('electron');
  const boot = ipcRenderer.sendSync('spa:bootstrapSync') || {};
  const projectDir = typeof boot.projectDir === 'string' ? boot.projectDir.trim() : '';

  function readJson(key) {
    try {
      const raw = localStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    } catch {
      return null;
    }
  }
  function writeJson(key, value) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* ignore */
    }
  }
  const norm = (p) => String(p).replace(/\//g, '\\').replace(/\\+$/g, '').toLowerCase();

  // ---------- 1) 모델 표시 시드 ----------
  function seedModelVisibility() {
    const KEY = 'opencode.global.dat:model';
    const cur = readJson(KEY) || { user: [], recent: [], variant: {} };
    if (!Array.isArray(cur.user)) cur.user = [];
    let changed = false;
    for (const id of ['webchat-chatgpt', 'webchat-gemini', 'webchat-mock']) {
      if (!cur.user.some((u) => u && u.providerID === 'webchat' && u.modelID === id)) {
        cur.user.push({ providerID: 'webchat', modelID: id, visibility: 'show' });
        changed = true;
      }
    }
    // 최근 사용 모델이 비어 있으면 ChatGPT를 기본으로
    if (!Array.isArray(cur.recent) || cur.recent.length === 0) {
      cur.recent = [{ providerID: 'webchat', modelID: 'webchat-chatgpt' }];
      changed = true;
    }
    if (changed) writeJson(KEY, cur);
  }

  // ---------- 2) 프로젝트 등록 시드 ----------
  function seedServerStore(projectDir) {
    const KEY = 'opencode.global.dat:server';
    const cur = readJson(KEY) || {
      list: [],
      hidden: {},
      projects: {},
      lastProject: {},
      recentlyClosed: {},
    };
    cur.projects = cur.projects && typeof cur.projects === 'object' ? cur.projects : {};
    cur.lastProject = cur.lastProject && typeof cur.lastProject === 'object' ? cur.lastProject : {};
    const local = Array.isArray(cur.projects.local) ? cur.projects.local : [];
    const want = norm(projectDir);
    const exists = local.some((e) => e && norm(e.worktree || '') === want);
    cur.projects.local = exists
      ? local
      : [{ worktree: projectDir, expanded: true }, ...local];
    cur.lastProject.local = projectDir;
    writeJson(KEY, cur);
  }

  function seedLayoutSelection(projectDir) {
    const SERVER = 'http://127.0.0.1:7891';
    const KEY = 'opencode.global.dat:layout';
    const cur = readJson(KEY) || {};
    const home = cur.home && typeof cur.home === 'object' ? cur.home : {};
    const sel = home.selection;
    const currentServer = sel && typeof sel === 'object' ? String(sel.server || '') : '';
    // 사용자가 이미 이 서버를 선택했다면 폴더만 앱 설정으로 맞춘다 (다른 서버면 건드리지 않음)
    if (!currentServer || currentServer === SERVER) {
      home.selection = { server: SERVER, directory: projectDir };
      cur.home = home;
      writeJson(KEY, cur);
    }
  }

  try {
    seedModelVisibility();
  } catch {}
  if (projectDir) {
    try {
      seedServerStore(projectDir);
    } catch {}
    try {
      seedLayoutSelection(projectDir);
    } catch {}
  }
} catch {
  /* 시드는 어디까지나 보조 — SPA 동작을 깨면 안 됨 */
}
