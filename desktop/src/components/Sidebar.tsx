import { useEffect, useMemo, useState } from 'react';
import { FolderOpen, Folder, MessageSquarePlus, Search, Circle, Trash2, Check, X, TextCursorInput } from 'lucide-react';
import { api, SessionInfo, formatRelTime } from '../api';
import type { HostProbe } from '../ipc';

interface SidebarProps {
  sessions: SessionInfo[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  projectDir: string | null;
  onPickProject: () => void;
  onRename: (id: string, title: string) => void;
  probe: HostProbe | null;
  onDelete: (id: string) => void | Promise<void>;
  onOpenProjectFolder: () => void;
  onShowChrome: () => Promise<{
    ok: boolean;
    action?: string;
    pid?: number;
    error?: string;
  }>;
}

export default function Sidebar({
  sessions,
  activeId,
  onSelect,
  onNew,
  projectDir,
  onPickProject,
  probe,
  onDelete,
  onRename,
  onOpenProjectFolder,
  onShowChrome,
}: SidebarProps) {
  const [query, setQuery] = useState('');
  const [opening, setOpening] = useState(false);
  const [showChromeError, setShowChromeError] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState('');
  const [contentMatchIds, setContentMatchIds] = useState<Set<string> | null>(null);

  // 제목에 없는 검색어는 세션 본문까지 검사 (최대 10개 세션만, 400ms 디바운스)
  useEffect(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      setContentMatchIds(null);
      return;
    }
    let cancelled = false;
    const t = setTimeout(() => {
      void (async () => {
        const set = new Set<string>();
        const candidates = sessions
          .filter((s) => !(s.title ?? '').toLowerCase().includes(q))
          .slice(0, 10);
        for (const s of candidates) {
          if (cancelled) return;
          try {
            const msgs = await api.getMessages(s.id);
            const hit = msgs.some((m) =>
              m.type === 'user'
                ? (m.text ?? '').toLowerCase().includes(q)
                : (m.content ?? []).some(
                    (p) => p.type === 'text' && (p.text ?? '').toLowerCase().includes(q),
                  ),
            );
            if (hit) set.add(s.id);
          } catch {
            /* 해당 세션은 건너뜀 */
          }
        }
        if (!cancelled) setContentMatchIds(set);
      })();
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query, sessions]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter(
      (s) =>
        (s.title ?? '새 대화').toLowerCase().includes(q) ||
        (contentMatchIds ? contentMatchIds.has(s.id) : false),
    );
  }, [sessions, query, contentMatchIds]);

  const commitRename = (id: string) => {
    const t = editTitle.trim();
    if (t) onRename(id, t);
    setEditingId(null);
  };

  const chatgpt = probe?.sites.find((s) => s.site === 'chatgpt');
  const gemini = probe?.sites.find((s) => s.site === 'gemini');
  const loginRequired =
    probe?.sites.some((s) => s.login_required) ?? (probe?.login_required ?? false);

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-lineSoft bg-panel/70 backdrop-blur-xl">
      {/* 프로젝트 */}
      <div className="px-4 pb-3 pt-4">
        <div className="mb-1.5 flex items-center justify-between">
          <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-dim">
            프로젝트
          </span>
          <button
            className="rounded-md p-1 text-dim transition-colors hover:bg-panel2 hover:text-text"
            title="프로젝트 폴더 열기"
            onClick={onOpenProjectFolder}
          >
            <Folder size={13} />
          </button>
          <button
            className="rounded-md p-1 text-dim transition-colors hover:bg-panel2 hover:text-text"
            title="프로젝트 폴더 변경…"
            onClick={onPickProject}
          >
            <FolderOpen size={13} />
          </button>
        </div>
        <div
          className="cursor-pointer truncate rounded-lg border border-lineSoft bg-panel2 px-2.5 py-1.5 font-mono text-[11px] text-text transition-colors hover:border-line"
          title={projectDir ?? '프로젝트 폴더 선택 필요'}
          onClick={onPickProject}
        >
          {projectDir ? projectDir.split('\\').pop() || projectDir : '폴더를 선택하세요'}
        </div>
      </div>

      {/* 새 대화 */}
      <div className="px-4">
        <button
          onClick={onNew}
          className="group flex w-full items-center justify-center gap-2 rounded-xl bg-accent-grad px-3 py-2 text-[12.5px] font-semibold text-[#06202b] transition-all hover:shadow-glow"
        >
          <MessageSquarePlus size={15} strokeWidth={2.2} />새 대화
        </button>
      </div>

      {/* 검색 */}
      <div className="px-4 pb-2 pt-3">
        <div className="flex items-center gap-2 rounded-lg border border-lineSoft bg-panel2 px-2.5 py-1.5 focus-within:border-accent/40">
          <Search size={12} className="shrink-0 text-dim" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="대화 검색…"
            className="w-full bg-transparent text-[12px] text-text placeholder-dim outline-none"
          />
        </div>
      </div>

      {/* 세션 목록 */}
      <nav className="nice-scroll min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 && (
          <div className="px-3 py-6 text-center text-[11.5px] text-dim">
            {query ? '검색 결과가 없습니다.' : '아직 대화가 없습니다.\n위의 "새 대화"를 눌러 시작하세요.'}
          </div>
        )}
        {filtered.map((s) => {
          const active = s.id === activeId;
          const confirming = confirmDeleteId === s.id;
          if (confirming) {
            return (
              <div key={s.id} className="mb-1 flex items-center gap-1.5 rounded-lg border border-red-400/25 bg-red-400/10 px-2 py-2">
                <Trash2 size={13} className="shrink-0 text-red-300" strokeWidth={2} />
                <span className="min-w-0 flex-1 truncate text-[11px] font-medium text-red-200">
                  삭제할까요?
                </span>
                <button
                  className="shrink-0 rounded-md bg-red-500/80 px-2 py-1 text-[10px] font-bold text-white transition-colors hover:bg-red-500"
                  onClick={() => {
                    setConfirmDeleteId(null);
                    void onDelete(s.id);
                  }}
                >
                  삭제
                </button>
                <button
                  className="shrink-0 rounded-md border border-lineSoft bg-panel2 px-2 py-1 text-[10px] text-dim transition-colors hover:text-text"
                  onClick={() => setConfirmDeleteId(null)}
                >
                  취소
                </button>
              </div>
            );
          }
          return (
            <div
              key={s.id}
              className={`group relative mb-1 flex w-full items-center overflow-hidden rounded-lg transition-colors ${
                active ? 'bg-panel3 text-text' : 'text-dim hover:bg-panel2 hover:text-text'
              }`}
            >
              {editingId === s.id ? (
                <div className="flex min-w-0 flex-1 items-center gap-1 px-2 py-1.5">
                  <input
                    autoFocus
                    value={editTitle}
                    onChange={(e) => setEditTitle(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') commitRename(s.id);
                      if (e.key === 'Escape') setEditingId(null);
                    }}
                    onBlur={() => commitRename(s.id)}
                    className="min-w-0 flex-1 rounded-md border border-accent/40 bg-panel2 px-2 py-1 text-[11.5px] text-text outline-none"
                    aria-label="대화 이름 변경"
                  />
                  <button
                    className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-emerald-400 hover:bg-panel3"
                    onClick={() => commitRename(s.id)}
                    aria-label="저장"
                  >
                    <Check size={13} />
                  </button>
                  <button
                    className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-dim hover:bg-panel3"
                    onClick={() => setEditingId(null)}
                    aria-label="취소"
                  >
                    <X size={13} />
                  </button>
                </div>
              ) : (
                <>
                  <button
                    onClick={() => onSelect(s.id)}
                    onDoubleClick={() => {
                      setEditingId(s.id);
                      setEditTitle(s.title?.trim() || '새 대화');
                    }}
                    className="min-w-0 flex-1 px-3 py-2 text-left"
                    title="더블 클릭하면 이름을 바꿀 수 있습니다"
                  >
                    {active && (
                      <span className="absolute left-0 top-2 h-[calc(100%-16px)] w-[3px] rounded-full bg-accent-grad" />
                    )}
                    <span className="block truncate text-[12px] leading-snug">
                      {s.title?.trim() || '새 대화'}
                      {query.trim() &&
                        !(s.title ?? '').toLowerCase().includes(query.trim().toLowerCase()) &&
                        contentMatchIds?.has(s.id) && (
                          <span className="ml-1.5 rounded-md bg-cyan-400/15 px-1 py-0.5 align-middle font-mono text-[9px] font-bold text-cyan-300">
                            본문
                          </span>
                        )}
                    </span>
                    <span className="mt-0.5 block text-[10px] text-dim">
                      {formatRelTime(s.time?.updated ?? s.time?.created)}
                    </span>
                  </button>
                  <button
                    className="mr-1 grid h-7 w-7 shrink-0 place-items-center rounded-md text-dim opacity-0 transition-all hover:bg-panel2 hover:text-text focus:opacity-100 focus:outline-none group-hover:opacity-100"
                    title="이름 변경"
                    aria-label="이름 변경"
                    onClick={() => {
                      setEditingId(s.id);
                      setEditTitle(s.title?.trim() || '새 대화');
                    }}
                  >
                    <TextCursorInput size={13} strokeWidth={2} />
                  </button>
                  <button
                    className="mr-1.5 grid h-7 w-7 shrink-0 place-items-center rounded-md text-dim opacity-0 transition-all hover:bg-red-400/15 hover:text-red-300 focus:opacity-100 focus:outline-none group-hover:opacity-100"
                    title="이 대화 삭제"
                    aria-label="이 대화 삭제"
                    onClick={() => setConfirmDeleteId(s.id)}
                  >
                    <Trash2 size={13} strokeWidth={2} />
                  </button>
                </>
              )}
            </div>
          );
        })}
      </nav>

      {/* 로그인 필요 배너 */}
      {loginRequired && (
        <div className="mx-2 mb-2 rounded-xl border border-amber-400/25 bg-amber-400/10 px-3 py-2.5">
          <p className="text-[11px] font-semibold leading-snug text-amber-300">
            웹 채팅 로그인이 필요해요
          </p>
          <p className="mt-0.5 text-[10px] leading-snug text-amber-200/70">
            버튼을 누르면 전용 Chrome이 화면에 표시됩니다. 로그인을 완료하면 창을 닫으세요.
          </p>
          <button
            onClick={() => {
              setOpening(true);
              setShowChromeError(null);
              onShowChrome()
                .then((r) => {
                  setOpening(false);
                  if (!r.ok) {
                    setShowChromeError(r.error ?? '전용 Chrome을 열지 못했습니다');
                  }
                })
                .catch((e: unknown) => {
                  setOpening(false);
                  setShowChromeError(e instanceof Error ? e.message : String(e));
                });
            }}
            className="mt-2 w-full rounded-lg bg-amber-400 px-2 py-1.5 text-[11px] font-bold text-[#332100] transition-all hover:bg-amber-300"
          >
            {opening ? '전용 Chrome 여는 중…' : '전용 Chrome 열기'}
          </button>
          {showChromeError && (
            <p className="mt-1.5 text-[10px] leading-snug text-red-300">{showChromeError}</p>
          )}
        </div>
      )}

      {/* 하단 상태 */}
      <div className="border-t border-lineSoft px-4 py-2.5">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-3 text-[10.5px] text-dim">
            <span className="flex items-center gap-1.5" title={chatgpt?.ok ? 'ChatGPT 연결됨' : 'ChatGPT 대기 중 (첫 요청 시 자동 실행)'}>
              <span className={`status-dot ${chatgpt?.ok ? 'ok' : 'warn'}`} />
              ChatGPT
            </span>
            <span className="flex items-center gap-1.5" title={gemini?.ok ? 'Gemini 연결됨' : 'Gemini 대기 중 (첫 요청 시 자동 실행)'}>
              <span className={`status-dot ${gemini?.ok ? 'ok' : 'warn'}`} />
              Gemini
            </span>
          </div>
          <span className="flex items-center gap-1 text-[9.5px] text-dim" title="첫 요청 전까지 Chrome은 실행되지 않습니다">
            <Circle size={7} className="fill-current text-mint/60" />
            Chrome 지연 실행
          </span>
        </div>
      </div>
    </aside>
  );
}
