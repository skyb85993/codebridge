import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import MessageList from './components/MessageList';
import Composer from './components/Composer';
import { api, ChatMessage, ModelID, SessionInfo } from './api';
import type { HostProbe } from './ipc';

const DEFAULT_MODEL: ModelID = 'webchat-chatgpt';
const POLL_MS = 700;
const IDLE_PROBE_MS = 5000;
const IDLE_SESSIONS_MS = 10000;
const BUSY_PROBE_MS = 30000;

export default function App() {
  const [projectDir, setProjectDir] = useState<string | null | undefined>(undefined);
  const [version, setVersion] = useState('0.0.0');
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [model, setModel] = useState<ModelID>(DEFAULT_MODEL);
  const [busy, setBusy] = useState(false);
  const [probe, setProbe] = useState<HostProbe | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeId) ?? null,
    [sessions, activeId],
  );
  const busyRef = useRef(busy);
  busyRef.current = busy;

  // ---------- 부팅 ----------
  useEffect(() => {
    window.codebridge.getBoot().then((boot) => {
      setVersion(boot.appVersion);
      setProjectDir(boot.projectDir);
    });
  }, []);

  // ---------- 프로젝트 없으면 전체 화면 폴더 선택 ----------
  const pickProject = useCallback(async () => {
    const dir = await window.codebridge.pickProject();
    if (dir) setProjectDir(dir);
  }, []);

  // ---------- 프로젝트별 세션 로드 ----------
  const refreshSessions = useCallback(
    async (dir: string) => {
      try {
        const list = await api.listSessions(dir);
        setSessions(list);
        return list;
      } catch {
        setSessions([]);
        return [];
      }
    },
    [],
  );

  useEffect(() => {
    if (!projectDir) return;
    refreshSessions(projectDir).then((list) => {
      if (list.length > 0) setActiveId((cur) => cur ?? list[0].id);
    });
  }, [projectDir, refreshSessions]);

  // ---------- 메시지 로드 ----------
  const loadMessages = useCallback(async (id: string) => {
    try {
      const msgs = await api.getMessages(id);
      setMessages(msgs);
    } catch {
      setMessages([]);
    }
  }, []);

  useEffect(() => {
    if (!activeId) return;
    loadMessages(activeId);
  }, [activeId, loadMessages]);

  // ---------- 활성 세션의 모델 동기화 ----------
  useEffect(() => {
    const m = activeSession?.model?.id;
    if (m && (m === 'webchat-chatgpt' || m === 'webchat-gemini')) {
      setModel(m);
    }
  }, [activeSession?.model?.id]);

  // ---------- busy 중 메시지 폴링 ----------
  useEffect(() => {
    if (!activeId || !busy) return;
    const t = setInterval(() => loadMessages(activeId), POLL_MS);
    return () => clearInterval(t);
  }, [activeId, busy, loadMessages]);

  // busy 해제 판정: 마지막 assistant가 finish를 가지면 끝난 것으로 간주
  useEffect(() => {
    if (!busy) return;
    const lastAssistant = [...messages].reverse().find((m) => m.type === 'assistant');
    const finish = (lastAssistant as { finish?: string } | undefined)?.finish;
    if (finish && finish !== 'unknown') {
      setBusy(false);
      if (projectDir) refreshSessions(projectDir);
    }
  }, [messages, busy, projectDir, refreshSessions]);

  // ---------- 폴링 (probe) ----------
  useEffect(() => {
    const tick = async () => {
      try {
        const p = await api.probe();
        setProbe(p);
      } catch {
        setProbe(null);
      }
    };
    tick();
    const t = setInterval(tick, busyRef.current ? BUSY_PROBE_MS : IDLE_PROBE_MS);
    return () => clearInterval(t);
  }, [busy]);

  // ---------- 세션 목록 주기 갱신 (idle) ----------
  useEffect(() => {
    if (!projectDir || busy) return;
    const t = setInterval(() => refreshSessions(projectDir), IDLE_SESSIONS_MS);
    return () => clearInterval(t);
  }, [projectDir, busy, refreshSessions]);

  // ---------- 액션 ----------
  const newSession = useCallback(() => {
    // 빈 채팅 상태로 전환 (세션은 첫 메시지에서 생성)
    setActiveId(null);
    setMessages([]);
  }, []);

  const ensureSession = useCallback(async (): Promise<string | null> => {
    if (activeId) return activeId;
    if (!projectDir) return null;
    const created = await api.createSession(projectDir, { id: model, providerID: 'webchat' });
    if (!created) return null;
    setSessions((cur) => [created, ...cur]);
    setActiveId(created.id);
    return created.id;
  }, [activeId, projectDir, model]);

  const send = useCallback(
    async (text: string) => {
      if (busy) return;
      if (!projectDir) {
        await pickProject();
        return;
      }
      setBusy(true);

      try {
        const sid = await ensureSession();
        if (!sid) {
          setBusy(false);
          return;
        }
        // 사용자의 메시지는 opencode가 즉시 서버에 저장하므로, 첫 폴링부터 보인다.
        await api.sendPrompt(sid, text);
        setTimeout(() => loadMessages(sid), 400);
      } catch {
        setBusy(false);
      }
    },
    [busy, projectDir, pickProject, ensureSession, loadMessages],
  );

  const interrupt = useCallback(async () => {
    if (!activeId) return;
    try {
      await api.interrupt(activeId);
    } finally {
      setBusy(false);
      setTimeout(() => loadMessages(activeId), 500);
    }
  }, [activeId, loadMessages]);

  const changeModel = useCallback(
    async (m: ModelID) => {
      setModel(m);
      if (activeId) {
        try {
          await api.switchModel(activeId, { id: m, providerID: 'webchat' });
        } catch {
          /* 무시 — 다음 세션에도 반영됨 */
        }
      }
    },
    [activeId],
  );

  // ---------- 첫 실행: 폴더 선택 강제 ----------
  if (projectDir === undefined) {
    return <div className="app-bg h-screen w-screen bg-base" />;
  }

  if (!projectDir) {
    return (
      <div className="app-bg flex h-screen w-screen flex-col overflow-hidden">
        <TitleBar version={version} title="CodeBridge" />
        <div className="flex flex-1 items-center justify-center">
          <div className="glass-strong glow-border flex w-[480px] flex-col items-center gap-5 rounded-2xl p-8 text-center fade-up">
            <div className="text-grad text-3xl font-extrabold tracking-tight">CodeBridge</div>
            <p className="text-[13px] leading-relaxed text-dim">
              작업할 프로젝트 폴더를 선택하세요.<br />
              생성되는 코드는 이 폴더에 직접 작성됩니다.
            </p>
            <button
              onClick={pickProject}
              className="rounded-xl bg-accent-grad px-6 py-2.5 text-[13px] font-bold text-[#06202b] transition-shadow hover:shadow-glow"
            >
              프로젝트 폴더 선택…
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="app-bg flex h-screen w-screen flex-col overflow-hidden">
      <TitleBar version={version} title={activeSession?.title ?? '새 대화'} />
      <div className="flex min-h-0 flex-1">
        {sidebarOpen && (
          <Sidebar
            sessions={sessions}
            activeId={activeId}
            onSelect={(id) => setActiveId(id)}
            onNew={newSession}
            projectDir={projectDir}
            onPickProject={pickProject}
            probe={probe}
          />
        )}
        <main className="flex min-w-0 flex-1 flex-col">
          <MessageList messages={messages} busy={busy} />
          <Composer
            onSend={send}
            onInterrupt={interrupt}
            busy={busy}
            model={model}
            onModelChange={changeModel}
          />
        </main>
      </div>
    </div>
  );
}
