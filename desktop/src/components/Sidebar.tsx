import { useMemo, useState } from 'react';
import { FolderOpen, MessageSquarePlus, Search, Circle } from 'lucide-react';
import { SessionInfo, formatRelTime } from '../api';
import type { HostProbe } from '../ipc';

interface SidebarProps {
  sessions: SessionInfo[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  projectDir: string | null;
  onPickProject: () => void;
  probe: HostProbe | null;
}

export default function Sidebar({
  sessions,
  activeId,
  onSelect,
  onNew,
  projectDir,
  onPickProject,
  probe,
}: SidebarProps) {
  const [query, setQuery] = useState('');

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => (s.title ?? '새 대화').toLowerCase().includes(q));
  }, [sessions, query]);

  const chatgpt = probe?.sites.find((s) => s.site === 'chatgpt');
  const gemini = probe?.sites.find((s) => s.site === 'gemini');

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
          return (
            <button
              key={s.id}
              onClick={() => onSelect(s.id)}
              className={`group relative mb-1 w-full rounded-lg px-3 py-2 text-left transition-colors ${
                active ? 'bg-panel3 text-text' : 'text-dim hover:bg-panel2 hover:text-text'
              }`}
            >
              {active && (
                <span className="absolute left-0 top-2 h-[calc(100%-16px)] w-[3px] rounded-full bg-accent-grad" />
              )}
              <span className="block truncate text-[12px] leading-snug">
                {s.title?.trim() || '새 대화'}
              </span>
              <span className="mt-0.5 block text-[10px] text-dim">
                {formatRelTime(s.time?.updated ?? s.time?.created)}
              </span>
            </button>
          );
        })}
      </nav>

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
