export {};

export type HostSite = 'chatgpt' | 'gemini';
export type HostSiteStatus = {
  site: HostSite;
  ok: boolean;
  composer: boolean;
  login_required: boolean;
  chrome?: boolean;
  url?: string;
  title?: string;
  error?: string;
};
export type HostProbe = {
  ok: boolean;
  chrome_running: boolean;
  chrome_not_running: boolean;
  login_required: boolean;
  sites: HostSiteStatus[];
};
export type HostBoot = { projectDir: string | null; appVersion: string };
export type HostSettings = {
  model: 'webchat-chatgpt' | 'webchat-gemini';
  autoCheckUpdates: boolean;
  dangerConfirm: boolean;
  handoverLimit: number;
  chromeAlwaysVisible: boolean;
  projectDir: string | null;
  logDir: string;
};
export type HostMode = 'app:ready';
export type UpdateState =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloaded'
  | 'up-to-date'
  | 'error';
export type HostUpdateState = { state: UpdateState; version: string | null; appVersion: string };

export type HostSettingsPatch = Partial<
  Pick<HostSettings, 'model' | 'autoCheckUpdates' | 'dangerConfirm' | 'handoverLimit' | 'chromeAlwaysVisible'>
>;

declare global {
  interface Window {
    codebridge: {
      getBoot(): Promise<HostBoot>;
      probe(): Promise<HostProbe>;
      pickProject(): Promise<string | null>;
      ocFetch(
        path: string,
        init?: { method?: string; body?: unknown },
      ): Promise<{ status: number; json: unknown }>;
      windowAction(action: 'minimize' | 'toggle-maximize' | 'close'): void;
      showChrome(): Promise<{ ok: boolean; action?: string; pid?: number; error?: string }>;
      runApprovedShell(command: string): Promise<{ ok: boolean; output?: string; error?: string }>;
      getUpdateState(): Promise<HostUpdateState>;
      quitAndInstall(): Promise<{ ok: boolean; message?: string }>;
      getSettings(): Promise<HostSettings>;
      setSettings(patch: HostSettingsPatch): Promise<{ ok: boolean; settings?: HostSettings; error?: string }>;
      openLogsFolder(): Promise<{ ok: boolean; error?: string }>;
      openReleasesPage(): Promise<{ ok: boolean; error?: string }>;
      checkUpdates(): Promise<{ ok: boolean; state?: UpdateState; version?: string | null; error?: string }>;
      resetChromeProfile(): Promise<{ ok: boolean; backup?: string | null; error?: string }>;
      restartBridge(): Promise<{ ok: boolean; error?: string }>;
      restartServices(): Promise<{ ok: boolean; error?: string }>;
      notify(payload: { title?: string; body?: string }): Promise<{ ok: boolean; error?: string }>;
    };
  }
}
