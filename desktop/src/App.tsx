import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import MessageList from './components/MessageList';
import Composer from './components/Composer';
import UpdateBanner from './components/UpdateBanner';
import SettingsModal from './components/SettingsModal';
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [exportNote, setExportNote] = useState<string | null>(null);
  const exportNoteTimer = useRef<number | undefined>(undefined);
  const [onboarded, setOnboarded] = useState<boolean | null>(null);
  const [recentProjects, setRecentProjects] = useState<string[]>([]);

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeId) ?? null,
    [sessions, activeId],
  );
  const busyRef = useRef(busy);
  busyRef.current = busy;
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  // busy 시작 시점의 마지막 assistant id — 이후 "다른" assistant가 terminal finish를 가질 때만 해제.
  // (카드로 답장을 보내도 이전 assistant의 stop이 목록에 남아 있어서, 그것만 보고 풀면
  //   사용자 메시지만 있는 상태에서 busy가 꺼져 폴링이 멈춘다)
  const busyAssistantRef = useRef<string | null>(null);

  // ---------- 부팅 ----------
  useEffect(() => {
    window.codebridge.getBoot().then((boot) => {
      setVersion(boot.appVersion);
      setProjectDir(boot.projectDir);
    });
    // 새 대화의 기본 모델을 settings에서 읽어 반영
    window.codebridge
      .getSettings()
      .then((s) => {
        setModel(s.model);
        setOnboarded(s.onboarded === true);
        setRecentProjects(s.recentProjects ?? []);
      })
      .catch(() => {
        setOnboarded(false);
      });
  }, []);

  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const applyDefaultModel = useCallback((m: ModelID) => setModel(m), []);

  // ---------- 프로젝트 없으면 전체 화면 폴더 선택 ----------
  const pickProject = useCallback(async () => {
    const dir = await window.codebridge.pickProject();
    if (dir) {
      setProjectDir(dir);
      setRecentProjects((prev) =>
        [dir, ...prev.filter((p) => p && p !== dir)].slice(0, 5),
      );
    }
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

  // busy 해제 판정: 마지막 assistant가 "종료성" finish를 가지면 끝난 것으로 간주.
  // 주의: "tool-calls"는 opencode 도구 루프의 중간 턴이다 — 여기서 busy를 풀면
  // 루프가 끝날 때까지의 후속 메시지(도구 결과/다음 답변)가 화면에 안 나타난다.
  useEffect(() => {
    if (!busy) return;
    const TERMINAL = new Set(['stop', 'length', 'content-filter', 'error']);
    const lastAssistant = [...messages].reverse().find((m) => m.type === 'assistant');
    const finish = (lastAssistant as { finish?: string } | undefined)?.finish;
    // busy 시작 시점의 마지막 assistant와 "다른" assistant가 terminal finish를 가질 때만 해제
    if (finish && TERMINAL.has(finish) && lastAssistant!.id !== busyAssistantRef.current) {
      setBusy(false);
      if (projectDir) refreshSessions(projectDir);
    }
  }, [messages, busy, projectDir, refreshSessions]);

  // ---------- 보이지 않는 동안 질문 카드 도착/작업 완료 알림 ----------
  const notifySeenRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (typeof document === 'undefined') return;
    if (document.visibilityState !== 'hidden') return;
    const lastAssistant = [...messages].reverse().find((m) => m.type === 'assistant');
    if (!lastAssistant) return;
    const a = lastAssistant as Extract<ChatMessage, { type: 'assistant' }>;
    const id = a.id ?? '';
    if (notifySeenRef.current.has(id)) return;
    const text = a.content
      .filter((p) => p.type === 'text')
      .map((p) => (p as { text?: string }).text ?? '')
      .join('\n');
    const hasQuestion = text.includes('CODEBRIDGE_QUESTION:');
    const TERMINAL = new Set(['stop', 'length', 'content-filter', 'error']);
    const done = !!a.finish && TERMINAL.has(a.finish);
    if (!hasQuestion && !done) return;
    notifySeenRef.current.add(id);
    // 설정에서 알림을 껐으면 발화하지 않음
    void (async () => {
      try {
        const s = await window.codebridge.getSettings();
        if (s && s.notifyEnabled === false) return;
      } catch {
        /* 게이트 실패 시 알림 그대로 */
      }
      window.codebridge
        .notify({
          title: 'CodeBridge',
          body: hasQuestion ? '질문이 도착했어요 — 답을 기다리고 있어요.' : '작업이 끝났어요.',
        })
        .catch(() => {
          /* 알림 미지원 환경 무시 */
        });
    })();
  }, [messages]);

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
      busyAssistantRef.current =
        [...messagesRef.current].reverse().find((m) => m.type === 'assistant')?.id ?? null;

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

  const flashNote = useCallback((msg: string) => {
    setExportNote(msg);
    if (exportNoteTimer.current) window.clearTimeout(exportNoteTimer.current);
    exportNoteTimer.current = window.setTimeout(() => setExportNote(null), 4000);
  }, []);

  // 대화 내보내기 — 현재 세션을 마크다운으로 저장
  const exportSession = useCallback(async () => {
    if (!activeId) return;
    try {
      const { messages: msgs, truncated } = await api.getRecentMessages(activeId);
      const title = activeSession?.title?.trim() || '대화';
      const lines: string[] = [
        `# ${title}`,
        '',
        `프로젝트: ${projectDir ?? ''}`,
        `내보낸 날짜: ${new Date().toLocaleString('ko-KR')}`,
        '',
      ];
      for (const m of msgs) {
        if (m.type === 'user') {
          lines.push('## 사용자', '', m.text ?? '', '');
        } else if (m.type === 'assistant') {
          lines.push('## 어시스턴트', '');
          for (const p of m.content) {
            if (p.type === 'text') lines.push(p.text ?? '', '');
            else if (p.type === 'tool') lines.push(`*(도구 호출: ${p.name})*`, '');
          }
          lines.push('');
        }
      }
      const safe =
        title
          .replace(/[\\/:*?"<>|]/g, '_')
          .replace(/\s+/g, ' ')
          .slice(0, 60)
          .trim() || '대화';
      const d = new Date();
      const pad = (n: number) => String(n).padStart(2, '0');
      const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}`;
      const res = await window.codebridge.exportSession({
        markdown: lines.join('\n'),
        filename: `${safe}-${stamp}.md`,
      });
      if (res.ok) {
        flashNote(truncated ? `저장했습니다 (긴 대화라 최근 200개만 포함) — ${res.path}` : `대화를 저장했습니다 — ${res.path}`);
      } else if (!res.cancelled) flashNote(res.error ?? '저장에 실패했습니다.');
    } catch {
      flashNote('내보내기를 완료하지 못했습니다.');
    }
  }, [activeId, activeSession, projectDir, flashNote]);

  // 최근 프로젝트 MRU 전환/제거
  const switchProject = useCallback(async (dir: string) => {
    const res = await window.codebridge.switchProject(dir);
    if (res.ok && res.projectDir) {
      setProjectDir(res.projectDir);
      // MRU를 최신화 (스위치 성공 경로에서도 setSettings를 통해 갱신)
      setRecentProjects((prev) => {
        const rest = prev.filter((p) => p && p !== res.projectDir);
        return [res.projectDir as string, ...rest].slice(0, 5);
      });
      flashNote(`프로젝트를 전환했습니다 — ${res.projectDir}`);
    } else {
      flashNote(res.error ?? '전환에 실패했습니다.');
    }
  }, [flashNote]);

  const removeRecentProject = useCallback(async (dir: string) => {
    await window.codebridge.removeRecentProject(dir);
    setRecentProjects((prev) => prev.filter((p) => p !== dir));
  }, []);

  // 전체 대화 일괄 삭제 (현재 프로젝트 기준)
  const deleteAllSessions = useCallback(async () => {
    if (!projectDir) return;
    try {
      const list = await api.listSessions(projectDir);
      const ids = list.map((s) => s.id);
      if (ids.length === 0) {
        flashNote('삭제할 대화가 없습니다.');
        return;
      }
      const results = await Promise.allSettled(ids.map((id) => api.deleteSession(id)));
      const okCount = results.filter((r) => r.status === 'fulfilled' && r.value).length;
      if (activeId && ids.includes(activeId)) {
        setActiveId(null);
        setMessages([]);
      }
      await refreshSessions(projectDir);
      flashNote(
        okCount === ids.length
          ? `대화 ${okCount}개를 삭제했습니다.`
          : `대화 ${okCount}/${ids.length}개를 삭제했습니다 (일부 실패).`,
      );
    } catch {
      flashNote('대화 삭제에 실패했습니다.');
    }
  }, [projectDir, activeId, refreshSessions, flashNote]);

  // 세션 이름 변경 (PATCH /api/session/{id})
  const renameSession = useCallback(
    async (id: string, title: string) => {
      const updated = await api.renameSession(id, title);
      if (updated && projectDir) refreshSessions(projectDir);
    },
    [projectDir, refreshSessions],
  );

  const deleteSession = useCallback(
    async (id: string) => {
      const ok = await api.deleteSession(id);
      if (!ok) {
        console.warn('[codebridge] 대화 삭제 실패:', id);
        return;
      }
      if (activeId === id) {
        setActiveId(null);
        setMessages([]);
      }
      if (projectDir) refreshSessions(projectDir);
    },
    [activeId, projectDir, refreshSessions],
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

  // 설정 화면의 "현재 대화에도 적용" — 진행 중인 세션의 모델도 바꾼다
  const applyModelToCurrent = useCallback(
    (m: ModelID) => {
      void changeModel(m);
    },
    [changeModel],
  );

  // ---------- 첫 실행: 폴더 선택 강제 ----------
  if (projectDir === undefined) {
    return <div className="app-bg h-screen w-screen bg-base" />;
  }

  if (!projectDir) {
    return (
      <div className="app-bg flex h-screen w-screen flex-col overflow-hidden">
        <TitleBar version={version} title="CodeBridge" onOpenSettings={openSettings} />
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
        {settingsOpen && (
          <SettingsModal
            appVersion={version}
            projectDir={projectDir}
            onClose={closeSettings}
            onPickProject={pickProject}
            onDefaultModel={applyDefaultModel}
            onApplyModelToCurrent={applyModelToCurrent}
            canApplyModelToCurrent={!!activeId}
            onSwitchProject={switchProject}
            onRemoveRecentProject={removeRecentProject}
            onExportSession={exportSession}
            canExportSession={!!activeId}
            onResetOnboarding={() => {
              window.codebridge.setSettings({ onboarded: false }).catch(() => {});
              setOnboarded(false);
              setSettingsOpen(false);
            }}
            onDeleteAllSessions={deleteAllSessions}
          />
        )}
      </div>
    );
  }

  return (
    <div className="app-bg flex h-screen w-screen flex-col overflow-hidden">
      <TitleBar version={version} title={activeSession?.title ?? '새 대화'} onOpenSettings={openSettings} />
      <UpdateBanner />
      {onboarded === false && (
        <div className="relative shrink-0 border-b border-lineSoft bg-panel/90 backdrop-blur-xl">
          <div className="mx-auto flex max-w-4xl items-center gap-4 px-4 py-1.5">
            <span className="hidden shrink-0 text-[10.5px] font-bold uppercase tracking-wider text-cyan-300 sm:inline">
              시작하기
            </span>
            <ol className="flex min-w-0 flex-1 items-center gap-3 text-[11px] text-dim">
              <li className="flex items-center gap-1.5">
                <span className="grid h-4 w-4 shrink-0 place-items-center rounded-full border border-cyan-400/40 bg-cyan-400/10 font-mono text-[9px] font-bold text-cyan-300">1</span>
                <span className="truncate">"TODO 하나 찾아서 고쳐줘" — 파일 수정을 맡겨보세요</span>
              </li>
              <span className="hidden text-dim/50 md:inline">→</span>
              <li className="hidden items-center gap-1.5 md:flex">
                <span className="grid h-4 w-4 shrink-0 place-items-center rounded-full border border-cyan-400/40 bg-cyan-400/10 font-mono text-[9px] font-bold text-cyan-300">2</span>
                <span className="truncate">답이 완료되면 폴더에 바로 반영됩니다</span>
              </li>
              <span className="hidden text-dim/50 lg:inline">→</span>
              <li className="hidden items-center gap-1.5 lg:flex">
                <span className="grid h-4 w-4 shrink-0 place-items-center rounded-full border border-cyan-400/40 bg-cyan-400/10 font-mono text-[9px] font-bold text-cyan-300">3</span>
                <span className="truncate">긴 대화는 자동으로 새 채팅으로 인수인계됩니다</span>
              </li>
            </ol>
            <button
              onClick={() => {
                setOnboarded(true);
                window.codebridge.setSettings({ onboarded: true }).catch(() => {});
              }}
              className="shrink-0 rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-2.5 py-1 text-[10.5px] font-semibold text-cyan-200 transition-colors hover:bg-cyan-400/20"
            >
              닫기
            </button>
          </div>
        </div>
      )}
      {exportNote && (
        <div className="pointer-events-none fixed bottom-16 left-1/2 z-40 max-w-[90%] -translate-x-1/2 truncate rounded-xl border border-cyan-400/30 bg-panel/95 px-4 py-2 text-[11px] font-medium text-cyan-100 shadow-card backdrop-blur">
          {exportNote}
        </div>
      )}
      {settingsOpen && (
        <SettingsModal
          appVersion={version}
          projectDir={projectDir}
          onClose={closeSettings}
          onPickProject={pickProject}
          onDefaultModel={applyDefaultModel}
          onApplyModelToCurrent={applyModelToCurrent}
          canApplyModelToCurrent={!!activeId}
          onSwitchProject={switchProject}
          onRemoveRecentProject={removeRecentProject}
          onExportSession={exportSession}
          canExportSession={!!activeId}
          onResetOnboarding={() => {
            window.codebridge.setSettings({ onboarded: false }).catch(() => {});
            setOnboarded(false);
            setSettingsOpen(false);
          }}
          onDeleteAllSessions={deleteAllSessions}
        />
      )}
      <div className="flex min-h-0 flex-1">
        {sidebarOpen && (
          <Sidebar
            sessions={sessions}
            activeId={activeId}
            onSelect={(id) => setActiveId(id)}
            onNew={newSession}
            projectDir={projectDir}
            onPickProject={pickProject}
            onRename={renameSession}
            probe={probe}
            onShowChrome={showChrome}
            onDelete={deleteSession}
            onSwitchProject={switchProject}
            onRemoveRecentProject={removeRecentProject}
            recentProjects={recentProjects}
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
