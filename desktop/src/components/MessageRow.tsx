import ReactMarkdown from 'react-markdown';
import type { ChatMessage } from '../api';
import ToolCallChip from './ToolCallChip';

export default function MessageRow({ message }: { message: ChatMessage }) {
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
              return (
                <div
                  key={i}
                  className="prose prose-sm prose-invert max-w-none text-[13px] leading-relaxed text-text
                    [&_pre]:my-2 [&_pre]:rounded-lg [&_pre]:border [&_pre]:border-lineSoft [&_pre]:bg-[#0c1017] [&_code]:rounded [&_code]:bg-[#0c1017]/60 [&_code]:px-1 [&_code]:py-0.5"
                >
                  <ReactMarkdown>{part.text}</ReactMarkdown>
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
              답변 생성 중…
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
