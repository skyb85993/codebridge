import { useEffect, useMemo, useRef } from 'react';
import type { ChatMessage } from '../api';
import MessageRow from './MessageRow';

interface MessageListProps {
  messages: ChatMessage[];
  busy: boolean;
}

export default function MessageList({ messages, busy }: MessageListProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const stickRef = useRef(true);

  const visible = useMemo(
    () => messages.filter((m) => m.type === 'user' || m.type === 'assistant'),
    [messages],
  );

  // 새 메시지가 오면 맨 아래로 (사용자가 위로 스크롤한 경우엔 유지)
  useEffect(() => {
    if (stickRef.current) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
    }
  }, [messages, busy]);

  const onScroll = () => {
    const el = containerRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    stickRef.current = distance < 120;
  };

  return (
    <div ref={containerRef} onScroll={onScroll} className="nice-scroll min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-4xl flex-col gap-4 px-6 py-6">
        {visible.length === 0 && (
          <div className="mt-16 flex flex-col items-center gap-4 text-center fade-up">
            <div className="text-grad text-4xl font-extrabold tracking-tight">무엇을 만들까요?</div>
            <p className="max-w-md text-[13px] leading-relaxed text-dim">
              로블록스 게임 코드를 자유 형식으로 요청하세요.<br />
              웹 채팅(무료)을 모델로 사용하며, 파일은 이 프로젝트 폴더에 바로 작성됩니다.
            </p>
            <div className="flex flex-wrap justify-center gap-2">
              {[
                '로블록스 점프 게임 스켈레톤 만들어줘',
                '이 프로젝트 구조 설명해줘',
                '지금까지 만든 코드에서 TODO 하나 골라 고쳐줘',
              ].map((q) => (
                <PromptChip key={q} text={q} />
              ))}
            </div>
          </div>
        )}
        {visible.map((m) => (
          <MessageRow key={m.id} message={m} />
        ))}
        {busy && visible.length > 0 && !isLastAssistantDone(visible) && (
          <div className="flex items-center gap-2 pl-1 text-[11.5px] text-dim">
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
            답변을 받아오는 중…
          </div>
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function PromptChip({ text }: { text: string }) {
  return (
    <button
      className="rounded-full border border-lineSoft bg-panel2 px-3.5 py-1.5 text-[12px] text-dim transition-all hover:border-accent/40 hover:text-text"
      onClick={() => window.dispatchEvent(new CustomEvent('codebridge:fill-composer', { detail: text }))}
    >
      {text}
    </button>
  );
}

function isLastAssistantDone(messages: ChatMessage[]): boolean {
  const last = [...messages].reverse().find((m) => m.type === 'assistant');
  if (!last) return false;
  const msg = last as Extract<ChatMessage, { type: 'assistant' }>;
  return msg.finish === 'stop' || msg.finish === 'tool-calls' || msg.finish === 'length';
}
