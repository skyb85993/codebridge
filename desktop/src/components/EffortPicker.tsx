import { Check, ChevronDown, Zap, Brain, Sparkles } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

export type Effort = 'low' | 'medium' | 'high';

const OPTIONS: Array<{ id: Effort; label: string; desc: string; Icon: typeof Zap }> = [
  { id: 'low', label: '낮음', desc: '빠르고 가볍게', Icon: Zap },
  { id: 'medium', label: '중간', desc: '균형 잡기', Icon: Brain },
  { id: 'high', label: '높음', desc: '깊게 생각하고 다중 검증', Icon: Sparkles },
];

interface EffortPickerProps {
  value: Effort;
  onChange: (id: Effort) => void;
  disabled?: boolean;
}

export default function EffortPicker({ value, onChange, disabled }: EffortPickerProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, []);

  const current = OPTIONS.find((o) => o.id === value) ?? OPTIONS[1];
  const Icon = current.Icon;

  return (
    <div ref={ref} className="relative">
      <button
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-lg border border-lineSoft bg-panel2 px-2.5 py-1.5 text-[11.5px] text-text transition-colors hover:border-line disabled:opacity-50"
        title="추론 강도 — GPT에게 얼마나 깊게 생각하게 할지"
      >
        <Icon size={12} className="text-dim" />
        {current.label}
        <ChevronDown size={12} className="text-dim" />
      </button>

      {open && (
        <div className="glass-strong absolute bottom-11 right-0 z-30 w-60 overflow-hidden rounded-xl border border-line shadow-card">
          {OPTIONS.map((o) => {
            const I = o.Icon;
            return (
              <button
                key={o.id}
                onClick={() => {
                  onChange(o.id);
                  setOpen(false);
                }}
                className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left transition-colors hover:bg-panel2"
              >
                <span className="flex items-start gap-2">
                  <I size={13} className="mt-0.5 shrink-0 text-dim" />
                  <span>
                    <span className="block text-[12px] font-medium text-text">{o.label}</span>
                    <span className="block text-[10px] text-dim">{o.desc}</span>
                  </span>
                </span>
                {o.id === value && <Check size={14} className="text-mint" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
