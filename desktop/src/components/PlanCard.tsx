import { ListChecks } from 'lucide-react';

/**
 * GPT가 CODEBRIDGE_PLAN 마커로 내보낸 작업 계획을 체크리스트 카드로 렌더링한다.
 * 마커는 MessageRow에서 숨기고 여기서만 보인다.
 */
export default function PlanCard({ steps }: { steps: string[] }) {
  return (
    <div className="my-2 overflow-hidden rounded-2xl border border-accent/30 bg-accent/5">
      <div className="flex items-center gap-2 border-b border-accent/20 bg-accent/10 px-4 py-2.5">
        <ListChecks size={15} className="shrink-0 text-accent" strokeWidth={2.2} />
        <span className="text-[12px] font-bold text-text">작업 계획</span>
        <span className="ml-auto text-[10px] text-dim">{steps.length}단계</span>
      </div>
      <ol className="px-4 py-3">
        {steps.map((s, i) => (
          <li key={i} className="flex items-start gap-2.5 py-1.5 text-[12.5px] leading-snug text-text">
            <span className="mt-0.5 grid h-[18px] w-[18px] shrink-0 place-items-center rounded-md border border-accent/40 bg-accent/10 font-mono text-[10px] font-bold text-accent">
              {i + 1}
            </span>
            <span className="min-w-0">{s}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}
