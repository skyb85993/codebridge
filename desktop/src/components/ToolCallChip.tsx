import {
  BookOpen,
  FilePenLine,
  FilePlus2,
  FolderSearch,
  SearchCode,
  TerminalSquare,
  Wrench,
  Check,
  Loader2,
  X,
} from 'lucide-react';
import type { AssistantPart } from '../api';

const ICONS: Record<string, typeof Wrench> = {
  read: BookOpen,
  glob: FolderSearch,
  grep: SearchCode,
  write: FilePlus2,
  edit: FilePenLine,
  bash: TerminalSquare,
  shell: TerminalSquare,
};

function pathOf(input?: Record<string, unknown>): string | null {
  if (!input) return null;
  const v = input.path ?? input.filePath ?? input.file ?? input.filepath;
  return typeof v === 'string' ? v : null;
}

export default function ToolCallChip({ part }: { part: Extract<AssistantPart, { type: 'tool' }> }) {
  const Icon = ICONS[part.name] ?? Wrench;
  const st = part.state?.status ?? (part.executed ? 'completed' : 'running');
  const cls =
    st === 'completed' ? 'done' : st === 'error' ? 'error' : 'running';
  const raw = pathOf(part.state?.input);
  const short = raw ? (raw.length > 52 ? '…' + raw.slice(-51) : raw) : null;

  return (
    <span className={`tool-chip ${cls}`} title={raw ?? part.name}>
      <Icon size={12} strokeWidth={2.2} />
      <span className="font-mono">{part.name}</span>
      {short && <span className="max-w-[18rem] truncate text-[10.5px] opacity-80">{short}</span>}
      {st === 'completed' && <Check size={11} strokeWidth={3} />}
      {(st === 'running' || st === 'streaming') && (
        <Loader2 size={11} className="animate-spin" strokeWidth={3} />
      )}
      {st === 'error' && <X size={11} strokeWidth={3} />}
    </span>
  );
}
