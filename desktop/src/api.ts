export type ModelID = 'webchat-chatgpt' | 'webchat-gemini';

export const MODELS: Array<{ id: ModelID; providerID: string; name: string; tagline: string }> = [
  { id: 'webchat-chatgpt', providerID: 'webchat', name: 'ChatGPT', tagline: '무료 · 웹 자동화' },
  { id: 'webchat-gemini', providerID: 'webchat', name: 'Gemini', tagline: '무료 · 웹 자동화' },
];
export const modelName = (id: string | undefined) => MODELS.find((m) => m.id === id)?.name ?? id ?? '?';

export type SessionInfo = {
  id: string;
  title?: string;
  time: { created: number; updated: number; idle?: number };
  model?: { id: string; providerID: string };
  cost?: number;
  tokens?: { input?: number; output?: number };
};

export type ToolState =
  | { status: 'running'; input?: Record<string, unknown> }
  | { status: 'completed'; input?: Record<string, unknown>; content?: unknown }
  | { status: 'error'; input?: Record<string, unknown>; error?: unknown }
  | { status: 'streaming'; input?: Record<string, unknown> };

export type AssistantPart =
  | { type: 'text'; text: string }
  | { type: 'reasoning'; reasoning?: string; text?: string }
  | { type: 'tool'; id?: string; name: string; executed?: boolean; state: ToolState };

export type ChatMessage =
  | { type: 'user'; id: string; text: string; time: { created: number } }
  | {
      type: 'assistant';
      id: string;
      time: { created: number; completed?: number };
      content: AssistantPart[];
      finish?: 'stop' | 'length' | 'tool-calls' | 'content-filter' | 'error' | 'unknown';
      tokens?: { input?: number; output?: number };
    }
  | {
      type: 'model-selected' | 'agent-selected' | 'synthetic' | 'system' | 'skill' | 'shell';
      id: string;
      time?: { created: number };
      text?: string;
    }
  | { type: string; id: string; [key: string]: unknown };

const oc = (path: string, init?: { method?: string; body?: unknown }) =>
  window.codebridge.ocFetch(path, init);

export const api = {
  listSessions: async (directory: string): Promise<SessionInfo[]> => {
    const r = await oc(
      `/api/session?directory=${encodeURIComponent(directory)}&order=desc&limit=60`,
    );
    return (r.json?.data as SessionInfo[]) ?? [];
  },
  createSession: async (directory: string, model: { id: string; providerID: string }) => {
    const r = await oc('/api/session', {
      method: 'POST',
      body: { location: { directory }, model },
    });
    return r.json?.data as SessionInfo | undefined;
  },
  getMessages: async (sessionId: string): Promise<ChatMessage[]> => {
    const r = await oc(`/api/session/${sessionId}/message?order=asc&limit=300`);
    return (r.json?.data as ChatMessage[]) ?? [];
  },
  sendPrompt: (sessionId: string, text: string) =>
    oc(`/api/session/${sessionId}/prompt`, { method: 'POST', body: { text } }),
  switchModel: (sessionId: string, model: { id: string; providerID: string }) =>
    oc(`/api/session/${sessionId}/model`, { method: 'POST', body: { model } }),
  interrupt: (sessionId: string) =>
    oc(`/api/session/${sessionId}/interrupt`, { method: 'POST', body: {} }),
  probe: () => window.codebridge.probe(),
};

export const formatCost = (cost?: number) => (typeof cost === 'number' ? cost.toFixed(4) : null);

export function formatRelTime(ts?: number): string {
  if (!ts) return '';
  const diff = Date.now() - ts;
  const m = Math.floor(diff / 60000);
  if (m < 1) return '방금';
  if (m < 60) return `${m}분 전`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}시간 전`;
  const d = Math.floor(h / 24);
  if (d < 7) return `${d}일 전`;
  return new Date(ts).toLocaleDateString();
}
