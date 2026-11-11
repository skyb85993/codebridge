import { Minus, Square, X } from 'lucide-react';
import icon from '../../build/icon.png';

interface TitleBarProps {
  version: string;
  title: string;
}

export default function TitleBar({ version, title }: TitleBarProps) {
  return (
    <header className="drag-region flex h-11 shrink-0 items-center justify-between border-b border-lineSoft bg-panel/80 pl-3 pr-1 backdrop-blur-xl">
      <div className="flex min-w-0 items-center gap-2.5">
        <img src={icon} alt="" className="h-5 w-5 rounded-md" />
        <span className="text-grad text-[13px] font-bold tracking-wide">CodeBridge</span>
        <span className="rounded-md border border-lineSoft bg-panel2 px-1.5 py-0.5 font-mono text-[10px] text-dim">
          v{version}
        </span>
      </div>

      <div className="no-drag flex h-full min-w-0 flex-1 items-center justify-center px-4">
        <span className="truncate text-[12px] text-dim">{title}</span>
      </div>

      <div className="no-drag flex h-full items-stretch">
        <button
          className="grid h-full w-11 place-items-center text-dim transition-colors hover:bg-panel2 hover:text-text"
          aria-label="최소화"
          onClick={() => window.codebridge.windowAction('minimize')}
        >
          <Minus size={14} strokeWidth={2} />
        </button>
        <button
          className="grid h-full w-11 place-items-center text-dim transition-colors hover:bg-panel2 hover:text-text"
          aria-label="최대화"
          onClick={() => window.codebridge.windowAction('toggle-maximize')}
        >
          <Square size={12} strokeWidth={2} />
        </button>
        <button
          className="grid h-full w-11 place-items-center text-dim transition-colors hover:bg-[#e81123] hover:text-white"
          aria-label="닫기"
          onClick={() => window.codebridge.windowAction('close')}
        >
          <X size={14} strokeWidth={2} />
        </button>
      </div>
    </header>
  );
}
