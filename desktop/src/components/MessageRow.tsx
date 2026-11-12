import { AlertTriangle } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import type { ChatMessage } from '../api';
import ToolCallChip from './ToolCallChip';
import QuestionCard, { type QuestionMarker } from './QuestionCard';

const MARKER_PREFIX = 'CODEBRIDGE_QUESTION:';
const TOOL_FAILED_PREFIX = 'codebridge:tool-failed:';

function parseQuestionLine(line: string): QuestionMarker | null {
  const t = line.trim();
  if (!t.startsWith(MARKER_PREFIX)) return null;
  try {
    const v = JSON.parse(t.slice(MARKER_PREFIX.length).trim()) as QuestionMarker;
    if (v && typeof v.question === 'string' && Array.isArray(v.options)) return v;
  } catch {
    /* 깨진 마커는 무시 */
  }
  return null;
}

export default function MessageRow({
  message,
  onAnswer,
  onRunApprovedShell,
}: {
  message: ChatMessage;
  onAnswer?: (text: string) => void;
  onRunApprovedShell?: (command: string) => Promise<string>;
}) {
  if (message.type === 'user') {
    return (
      <div className="fade-up flex justify-end">
        <div className="bubble-user max-w-[78%] rounded-2xl rounded-br-md px-4 py-3 text-[13px] leading-relaxed text-text whitespace-pre-wrap">
          {(message as { text: string }).text}
        </div>
      </div>
    );
  }

  if (message.type === 'assistant') {
    const msg = message as Extract<ChatMessage, { type: 'assistant' }>;
    const hasParts = msg.content.length > 0;
    return (
      <div className="fade-up flex justify-start">
        <div className="bubble-assistant max-w-[88%] rounded-2xl rounded-bl-md px-4 py-3">
          {msg.content.map((part, i) => {
            if (part.type === 'text') {
              const lines = (part.text || '').split('\n');
              const segments: Array<
                | { type: 'text'; value?: string }
                | { type: 'card'; marker: QuestionMarker }
                | { type: 'tool-failed'; count: number }
              > = [];
              let buffer = '';
              for (const line of lines) {
                const trimmed = line.trim();
                // 도구 실패 배너 라인
                if (trimmed.startsWith(TOOL_FAILED_PREFIX)) {
                  if (buffer.trim()) segments.push({ type: 'text', value: buffer });
                  buffer = '';
                  const n = Number.parseInt(trimmed.slice(TOOL_FAILED_PREFIX.length), 10);
                  segments.push({ type: 'tool-failed', count: Number.isFinite(n) ? n : 0 });
                  continue;
                }
                const marker = parseQuestionLine(line);
                if (marker) {
                  if (buffer.trim()) segments.push({ type: 'text', value: buffer });
                  buffer = '';
                  segments.push({ type: 'card', marker });
                } else {
                  buffer += line + '\n';
                }
              }
              if (buffer.trim()) segments.push({ type: 'text', value: buffer });
              if (segments.length === 0 && part.text) {
                segments.push({ type: 'text', value: part.text });
              }
              return (
                <div key={i}>
                  {segments.map((seg, j) =>
                    seg.type === 'text' ? (
                      <div
                        key={j}
                        className="prose prose-sm prose-invert max-w-none text-[13px] leading-relaxed text-text
                          [&_pre]:my-2 [&_pre]:rounded-lg [&_pre]:border [&_pre]:border-lineSoft [&_pre]:bg-[#0c1017] [&_code]:rounded [&_code]:bg-[#0c1017]/60 [&_code]:px-1 [&_code]:py-0.5"
                      >
                        <ReactMarkdown>{seg.value ?? ''}</ReactMarkdown>
                      </div>
                    ) : seg.type === 'card' ? (
                      <QuestionCard
                        key={j}
                        marker={seg.marker}
                        onAnswer={(t) => onAnswer?.(t)}
                        onRunApprovedShell={onRunApprovedShell}
                      />
                    ) : (
                      <div
                        key={j}
                        className="my-2 flex items-start gap-2 rounded-xl border border-amber-400/25 bg-amber-400/10 px-3 py-2"
                        title="도구 호출이 불완전하여 일부가 실행되지 않았습니다"
                      >
                        <AlertTriangle size={14} className="mt-0.5 shrink-0 text-amber-300" strokeWidth={2.2} />
                        <span className="text-[11.5px] leading-snug text-amber-200">
                          도구 {seg.count ?? 1}건이 실행되지 않았습니다. "파일 전체를 다시 써줘"라고 요청해 보세요.
                        </span>
                      </div>
                    ),
                  )}
                </div>
              );
            }
            if (part.type === 'reasoning') {
              const text = part.reasoning ?? part.text ?? '';
              return (
                <div
                  key={i}
                  className="my-1.5 rounded-lg border border-lineSoft bg-panel2/60 px-3 py-2 text-[11.5px] italic text-dim"
                >
                  {text}
                </div>
              );
            }
            if (part.type === 'tool') {
              return (
                <div key={i} className="my-1.5">
                  <ToolCallChip part={part} />
                </div>
              );
            }
            return null;
          })}
          {!hasParts && (
            <div className="flex items-center gap-2 text-[12px] text-dim">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
              생각 중…
            </div>
          )}
          {msg.finish && msg.finish !== 'unknown' && msg.finish !== 'stop' && (
            <div className="mt-2 border-t border-lineSoft pt-1.5 font-mono text-[10px] text-dim">
              finish: {msg.finish}
            </div>
          )}
        </div>
      </div>
    );
  }

  return null;
}
