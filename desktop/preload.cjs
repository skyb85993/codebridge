/**
 * CodeBridge Desktop — 렌더러에 노출하는 IPC 브릿지.
 * 렌더러는 sandbox 상태. 필요한 동작만 여기서 노출한다.
 */
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('codebridge', {
  getBoot() {
    return ipcRenderer.invoke('app:getBoot');
  },
  probe() {
    return ipcRenderer.invoke('app:probe');
  },
  pickProject() {
    return ipcRenderer.invoke('app:pickProject');
  },
  ocFetch(path, init) {
    return ipcRenderer.invoke('oc:fetch', path, init);
  },
  showChrome() {
    return ipcRenderer.invoke('app:showChrome');
  },
  runApprovedShell(command) {
    return ipcRenderer.invoke('app:runApprovedShell', command);
  },
  windowAction(action) {
    ipcRenderer.send('window:action', action);
  },
  getUpdateState() {
    return ipcRenderer.invoke('app:getUpdateState');
  },
  quitAndInstall() {
    return ipcRenderer.invoke('updater:quitAndInstall');
  },
  getSettings() {
    return ipcRenderer.invoke('app:getSettings');
  },
  setSettings(patch) {
    return ipcRenderer.invoke('app:setSettings', patch);
  },
  openLogsFolder() {
    return ipcRenderer.invoke('app:openLogsFolder');
  },
  openReleasesPage() {
    return ipcRenderer.invoke('app:openReleasesPage');
  },
  checkUpdates() {
    return ipcRenderer.invoke('app:checkUpdates');
  },
  resetChromeProfile() {
    return ipcRenderer.invoke('app:resetChromeProfile');
  },
  restartBridge() {
    return ipcRenderer.invoke('app:restartBridge');
  },
  restartServices() {
    return ipcRenderer.invoke('app:restartServices');
  },
  notify(payload) {
    return ipcRenderer.invoke('app:notify', payload);
  },
  openProjectFolder() {
    return ipcRenderer.invoke('app:openProjectFolder');
  },
});
