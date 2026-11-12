import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import MessageList from './components/MessageList';
import Composer from './components/Composer';
import UpdateBanner from './components/UpdateBanner';
import { api, ChatMessage, ModelID, SessionInfo } from './api';
import type { HostProbe } from './ipc';
import brandIcon from '../build/icon.png';

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

  const showChrome = useCallback(
    () => window.codebridge.showChrome(),
    [],
  );

  const answerQuestion = useCallback(
    async (text: string) => {
      // 질문 카드 선택지는 사용자 메시지로 전송 (루프를 계속 돌리기 위해 busy를 올려둔다)
      await send(text);
    },
    [send],
  );

  const runApprovedShell = useCallback(async (command: string): Promise<string> => {
    const res = await window.codebridge.runApprovedShell(command);
    if (!res.ok) {
      return res.error ?? '실행 실패';
    }
    return res.output ?? '';
  }, []);

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
        <div className="pointer-events-none absolute -left-40 top-1/4 h-96 w-96 rounded-full bg-cyan-400/10 blur-3xl" />
        <div className="pointer-events-none absolute -right-40 bottom-1/4 h-96 w-96 rounded-full bg-emerald-400/10 blur-3xl" />
        <div className="relative flex flex-1 items-center justify-center">
          <div className="glass-strong flex w-[560px] flex-col items-center gap-6 rounded-3xl border border-cyan-400/15 p-10 text-center shadow-card fade-up">
            {/* 로고 글로우 */}
            <div className="relative">
              <div className="absolute inset-0 -m-6 rounded-full bg-gradient-to-br from-cyan-400/30 via-emerald-400/20 to-transparent blur-2xl" />
              <img src={brandIcon} alt="CodeBridge" className="relative h-24 w-24 rounded-2xl" />
            </div>

            <div className="flex flex-col items-center gap-1.5">
              <div className="text-grad text-4xl font-extrabold tracking-tight">CodeBridge</div>
              <p className="text-[11px] font-semibold uppercase tracking-[0.35em] text-dim">Free Web Chat → Code</p>
            </div>
            <p className="text-[13.5px] leading-relaxed text-dim">
              무료 웹 채팅(ChatGPT / Gemini)을 코딩 에이전트로 사용합니다.<br />
              코드는 이 폴더에 <span className="text-cyan-300">바로 작성</span>되고, 첫 실행도 추가 설정이 없습니다.
            </p>

            {/* 3단계 안내 */}
            <div className="grid w-full grid-cols-3 gap-3">
              {[
                { n: '1', t: '폴더 연결', d: '프로젝트 폴더를 한 번만 선택' },
                { n: '2', t: '자동 실행', d: '전용 Chrome은 첫 요청 때 실행' },
                { n: '3', t: '바로 작성', d: '생성된 파일은 폴더에 즉시 반영' },
              ].map((s) => (
                <div key={s.n} className="flex flex-col items-center gap-1.5 rounded-2xl border border-lineSoft bg-panel2/60 px-3 py-3.5">
                  <span className="grid h-6 w-6 place-items-center rounded-full border border-cyan-400/40 bg-cyan-400/10 font-mono text-[11px] font-bold text-cyan-300">
                    {s.n}
                  </span>
                  <span className="text-[12px] font-bold text-text">{s.t}</span>
                  <span className="text-[10px] leading-snug text-dim">{s.d}</span>
                </div>
              ))}
            </div>

            <button
              onClick={pickProject}
              className="group relative mt-1 flex items-center gap-2 rounded-2xl bg-accent-grad px-8 py-3 text-[14px] font-extrabold text-[#06202b] transition-all hover:shadow-[0_0_28px_rgba(88,214,255,0.45)]"
            >
              프로젝트 폴더 선택…
            </button>
            <p className="text-[10px] text-dim">
              처음 실행 시 SmartScreen 경고가 뜰 수 있습니다 (서명 없는 무료 배포) — "추가 정보 → 실행"
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="app-bg flex h-screen w-screen flex-col overflow-hidden">
      <TitleBar version={version} title={activeSession?.title ?? '새 대화'} />
      <UpdateBanner />
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
            onShowChrome={showChrome}
          />
        )}
        <main className="flex min-w-0 flex-1 flex-col">
          <MessageList
            messages={messages}
            busy={busy}
            onAnswer={answerQuestion}
            onRunApprovedShell={runApprovedShell}
          />
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
