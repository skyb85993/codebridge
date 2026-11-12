import { useEffect, useState } from 'react';
import { AlertTriangle, Check, ChevronRight, CornerDownLeft, ShieldQuestion } from 'lucide-react';

export interface QuestionMarker {
  header: string;
  question: string;
  options: Array<{ label: string; description?: string }>;
  multiple: boolean;
  command?: string;
  marker: 'question' | 'danger_shell';
}

export default function QuestionCard({
  marker,
  onAnswer,
  onRunApprovedShell,
}: {
  marker: QuestionMarker;
  onAnswer: (text: string) => void;
  onRunApprovedShell?: (command: string) => Promise<string>;
}) {
  const [selected, setSelected] = useState<string[]>([]);
  const [typed, setTyped] = useState('');
  const [sent, setSent] = useState(false);

  const isDangerShell = marker.marker === 'danger_shell';

  useEffect(() => {
    setSelected([]);
    setTyped('');
    setSent(false);
  }, [marker.question, marker.command]);

  const toggle = (label: string) => {
    if (sent) return;
    if (marker.multiple) {
      setSelected((cur) => (cur.includes(label) ? cur.filter((l) => l !== label) : [...cur, label]));
    } else {
      setSelected([label]);
    }
  };

  const submitAnswer = (label: string) => {
    if (sent) return;
    setSent(true);
    const isApprove =
      isDangerShell && /^(실행|run|approve|execute)$/i.test(label.trim());
    if (isApprove && marker.command && onRunApprovedShell) {
      setSent(true);
      onRunApprovedShell(marker.command).then((out) => {
        onAnswer(`[승인됨] 위험한 명령 실행 결과:\n\n${out}`);
      });
      return;
    }
    onAnswer(`선택: ${label}`);
  };

  const submitMulti = () => {
    if (sent || selected.length === 0) return;
    setSent(true);
    onAnswer(`선택: ${selected.join(', ')}`);
  };

  const submitTyped = () => {
    if (sent) return;
    const t = typed.trim();
    if (!t) return;
    setSent(true);
    onAnswer(t);
  };

  // 위험한 셸 명령 확인 카드: 실행/취소
  if (isDangerShell) {
    const [execLabel = '실행', cancelLabel = '취소'] = marker.options.map((o) => o.label);
    return (
      <div className="my-2 overflow-hidden rounded-2xl border border-red-400/40 bg-red-500/10">
        <div className="flex items-center gap-2 border-b border-red-400/20 bg-red-500/15 px-4 py-2.5">
          <AlertTriangle size={15} className="shrink-0 text-red-400" strokeWidth={2.2} />
          <span className="text-[12px] font-bold text-red-300">{marker.header}</span>
        </div>
        <div className="px-4 py-3">
          <pre className="overflow-x-auto whitespace-pre-wrap rounded-lg border border-red-400/30 bg-[#180b0b] px-3 py-2 font-mono text-[11.5px] leading-relaxed text-red-200/90">
            {marker.command}
          </pre>
          <p className="mt-2 text-[11.5px] leading-snug text-red-200/70">{marker.question?.split('\n')[0]}</p>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => submitAnswer(execLabel)}
              className="flex-1 rounded-lg border border-red-400/40 bg-red-500/20 px-3 py-1.5 text-[12px] font-bold text-red-200 transition-colors hover:bg-red-500/30"
            >
              {execLabel}
            </button>
            <button
              onClick={() => submitAnswer(cancelLabel)}
              className="flex-1 rounded-lg border border-lineSoft bg-panel2 px-3 py-1.5 text-[12px] font-semibold text-dim transition-colors hover:text-text"
            >
              {cancelLabel}
            </button>
          </div>
        </div>
      </div>
    );
  }

  const optCount = marker.options.length;

  return (
    <div className="my-2 overflow-hidden rounded-2xl border border-cyan-400/25 bg-cyan-400/5">
      <div className="flex items-center gap-2 border-b border-cyan-400/15 bg-cyan-400/10 px-4 py-2.5">
        <ShieldQuestion size={15} className="shrink-0 text-cyan-300" strokeWidth={2.2} />
        <span className="text-[12px] font-bold text-cyan-200">{marker.header}</span>
        {sent && (
          <span className="ml-auto flex items-center gap-1 text-[10px] text-dim">
            <Check size={12} strokeWidth={3} /> 보냈어요
          </span>
        )}
      </div>

      <div className="px-4 py-3">
        <p className="text-[13px] leading-snug text-text">{marker.question}</p>

        {optCount > 0 && (
          <div className="mt-3 flex flex-col gap-1.5">
            {marker.options.map((opt) => {
              const active = selected.includes(opt.label);
              return (
                <button
                  key={opt.label}
                  onClick={() => {
                    if (marker.multiple) {
                      toggle(opt.label);
                    } else {
                      submitAnswer(opt.label);
                    }
                  }}
                  disabled={sent}
                  className={`group flex items-start gap-2 rounded-xl border px-3 py-2 text-left transition-all ${
                    active
                      ? 'border-cyan-400/50 bg-cyan-400/10'
                      : 'border-lineSoft bg-panel2/40 hover:border-cyan-400/30 hover:bg-panel2'
                  } ${sent ? 'opacity-60' : ''}`}
                >
                  <span
                    className={`mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-[4px] border ${
                      active ? 'border-cyan-400 bg-cyan-400 text-[#06202b]' : 'border-line'
                    }`}
                  >
                    {active && <Check size={11} strokeWidth={3.5} />}
                  </span>
                  <span className="min-w-0">
                    <span className="block text-[12px] font-semibold text-text">{opt.label}</span>
                    {opt.description && (
                      <span className="block text-[11px] leading-snug text-dim">{opt.description}</span>
                    )}
                  </span>
                  {!marker.multiple && (
                    <ChevronRight size={13} className="ml-auto mt-1 shrink-0 text-dim transition-transform group-hover:translate-x-0.5" />
                  )}
                </button>
              );
            })}
          </div>
        )}

        {marker.multiple && optCount > 0 && (
          <button
            onClick={submitMulti}
            disabled={sent || selected.length === 0}
            className="mt-2.5 w-full rounded-lg bg-accent-grad px-3 py-1.5 text-[11px] font-bold text-[#06202b] transition-all disabled:opacity-40"
          >
            {selected.length === 0 ? '선택하세요' : `${selected.length}개 선택 · 확인`}
          </button>
        )}

        <div className="mt-2 flex items-center gap-1.5">
          <input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.nativeEvent.isComposing) submitTyped();
            }}
            disabled={sent}
            placeholder="직접 입력…"
            className="h-8 min-w-0 flex-1 rounded-lg border border-lineSoft bg-panel2 px-2.5 text-[12px] text-text outline-none transition-colors placeholder:text-dim focus:border-cyan-400/40"
          />
          <button
            onClick={submitTyped}
            disabled={sent || !typed.trim()}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-lineSoft bg-panel2 text-dim transition-colors hover:text-text disabled:opacity-40"
            title="전송"
          >
            <CornerDownLeft size={13} strokeWidth={2.2} />
          </button>
        </div>
      </div>
    </div>
  );
}
