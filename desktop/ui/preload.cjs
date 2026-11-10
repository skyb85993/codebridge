/**
 * CodeBridge Desktop — 렌더러에 노출하는 IPC 브릿지.
 * 렌더러는 sandbox 상태라 node 접근 불가. 필요한 동작만 여기서 노출한다.
 */
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('codebridge', {
  windowAction(action) {
    ipcRenderer.send('window:action', action);
  },
  async getStatus() {
    return ipcRenderer.invoke('app:getStatus');
  },
  async pickProject() {
    return ipcRenderer.invoke('project:pick');
  },
  async restartBridge() {
    return ipcRenderer.invoke('bridge:restart');
  },
  async reopenChrome() {
    return ipcRenderer.invoke('chrome:reopen');
  },
  async tailLogs() {
    return ipcRenderer.invoke('logs:tail');
  },
  async checkForUpdates() {
    return ipcRenderer.invoke('updater:check');
  },
});
