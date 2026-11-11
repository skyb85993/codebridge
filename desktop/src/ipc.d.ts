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
export type HostMode = 'app:ready';

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
    };
  }
}
