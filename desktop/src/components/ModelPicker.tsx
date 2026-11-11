import { Check, ChevronDown, Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { MODELS, ModelID, modelName } from '../api';

interface ModelPickerProps {
  value: ModelID;
  onChange: (id: ModelID) => void;
  disabled?: boolean;
}

export default function ModelPicker({ value, onChange, disabled }: ModelPickerProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, []);

  return (
    <div ref={ref} className="relative">
      <button
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-lg border border-lineSoft bg-panel2 px-2.5 py-1.5 text-[11.5px] text-text transition-colors hover:border-line disabled:opacity-50"
        title="모델 선택 (무료 웹 채팅)"
      >
        <span className="h-1.5 w-1.5 rounded-full bg-accent-grad" />
        {modelName(value)}
        <ChevronDown size={12} className="text-dim" />
      </button>

      {open && (
        <div className="glass-strong absolute bottom-11 left-0 z-30 w-64 overflow-hidden rounded-xl border border-line shadow-card">
          {MODELS.map((m) => (
            <button
              key={m.id}
              onClick={() => {
                onChange(m.id as ModelID);
                setOpen(false);
              }}
              className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left transition-colors hover:bg-panel2"
            >
              <span>
                <span className="block text-[12px] font-medium text-text">{m.name}</span>
                <span className="block text-[10px] text-dim">{m.tagline}</span>
              </span>
              {m.id === value && <Check size={14} className="text-mint" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function SendSpinner() {
  return (
    <Loader2 size={16} className="animate-spin text-[#06202b]" strokeWidth={2.5} />
  );
}
