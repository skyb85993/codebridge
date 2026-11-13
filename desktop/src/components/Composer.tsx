import { FormEvent, KeyboardEvent, useEffect, useRef, useState } from 'react';
import { ArrowUp, Square, Download } from 'lucide-react';
import ModelPicker from './ModelPicker';
import { ModelID } from '../api';

interface ComposerProps {
  onSend: (text: string) => void;
  onInterrupt: () => void;
  busy: boolean;
  model: ModelID;
  onModelChange: (m: ModelID) => void;
  onExportSession?: () => void;
}

export default function Composer({ onSend, onInterrupt, busy, model, onModelChange, onExportSession }: ComposerProps) {
  const [text, setText] = useState('');
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const fill = (e: Event) => {
      const detail = (e as CustomEvent<string>).detail;
      if (detail) {
        setText(detail);
        requestAnimationFrame(() => ref.current?.focus());
      }
    };
    window.addEventListener('codebridge:fill-composer', fill);
    return () => window.removeEventListener('codebridge:fill-composer', fill);
  }, []);

  // 입력이 늘어날 때마다 높이 자동 조절 (최대 10줄)
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 240) + 'px';
  }, [text]);

  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    onSend(trimmed);
    setText('');
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="shrink-0 border-t border-lineSoft bg-panel/60 px-4 pb-4 pt-3 backdrop-blur-xl">
      <form
        className="mx-auto max-w-4xl"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="composer-input flex items-end gap-2 rounded-2xl p-2">
          <textarea
            ref={ref}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="메시지를 입력하세요…"
            rows={1}
            className="max-h-60 min-h-[44px] w-full resize-none bg-transparent px-2 py-2 text-[13.5px] leading-relaxed text-text placeholder-dim outline-none"
          />
          {onExportSession && (
            <button
              type="button"
              onClick={onExportSession}
              className="grid h-10 w-10 shrink-0 place-items-center rounded-xl text-dim transition-colors hover:bg-panel2 hover:text-text"
              title="대화 내보내기 (.md)"
              aria-label="대화 내보내기"
            >
              <Download size={15} strokeWidth={2} />
            </button>
          )}
          <ModelPicker value={model} onChange={onModelChange} disabled={busy} />
          {busy ? (
            <button
              type="button"
              onClick={onInterrupt}
              className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-danger/90 text-white transition-all hover:bg-danger"
              title="생성 중지"
            >
              <Square size={15} fill="currentColor" strokeWidth={0} />
            </button>
          ) : (
            <button
              type="submit"
              disabled={!text.trim()}
              className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-grad text-[#06202b] transition-all enabled:hover:shadow-glow disabled:opacity-30"
              title="전송 (Enter)"
            >
              <ArrowUp size={17} strokeWidth={2.6} />
            </button>
          )}
        </div>
        <p className="mt-1.5 pl-1 text-[10.5px] text-dim">
          Enter 전송 · Shift+Enter 줄바꿈 · 무료 웹 채팅 사용 · 파일은 프로젝트 폴더에 생성됩니다
        </p>
      </form>
    </div>
  );
}
