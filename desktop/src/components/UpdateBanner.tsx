import { useEffect, useState } from 'react';
import { Download, RotateCw, Sparkles, X } from 'lucide-react';
import type { HostUpdateState } from '../ipc';

const POLL_MS = 4000;

export default function UpdateBanner() {
  const [info, setInfo] = useState<HostUpdateState | null>(null);
  const [dismissed, setDismissed] = useState<string | null>(null);

  useEffect(() => {
    const tick = async () => {
      try {
        setInfo(await window.codebridge.getUpdateState());
      } catch {
        /* 무시 */
      }
    };
    tick();
    const t = setInterval(tick, POLL_MS);
    return () => clearInterval(t);
  }, []);

  const state = info?.state ?? 'idle';
  const version = info?.version ?? null;
  const appVersion = info?.appVersion ?? '';
  const visible = (state === 'downloaded' || state === 'available') && dismissed !== state;

  if (!visible) return null;

  const isDownloaded = state === 'downloaded';

  return (
    <div className="relative shrink-0 border-b border-lineSoft bg-panel/85 backdrop-blur-xl">
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-cyan-400/70 to-transparent" />
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-gradient-to-r from-transparent via-emerald-400/70 to-transparent" />

      <div className="flex items-center gap-3 px-4 py-1.5">
        <span className="relative grid h-7 w-7 place-items-center rounded-lg border border-cyan-400/30 bg-cyan-400/10">
          {isDownloaded ? (
            <Sparkles size={15} className="text-cyan-300" strokeWidth={2} />
          ) : (
            <Download size={15} className="animate-bounce text-cyan-300" strokeWidth={2} />
          )}
        </span>

        <div className="min-w-0 flex-1 leading-tight">
          <p className="text-[12px] font-semibold text-text">
            {isDownloaded ? '업데이트 준비 완료' : '업데이트 다운로드 중'}
            {version ? <span className="ml-1.5 font-mono text-[10.5px] text-dim">v{appVersion} → v{version}</span> : null}
          </p>
          <p className="text-[11px] text-dim">
            {isDownloaded
              ? '지금 재시작하면 새 버전이 적용됩니다.'
              : '앱을 계속 사용해도 됩니다. 백그라운드에서 받아옵니다.'}
          </p>
        </div>

        {isDownloaded && (
          <button
            onClick={() => window.codebridge.quitAndInstall()}
            className="group flex shrink-0 items-center gap-1.5 rounded-lg bg-gradient-to-r from-cyan-400 to-emerald-400 px-3.5 py-1.5 text-[12px] font-bold text-[#06202b] transition-all hover:shadow-[0_0_16px_rgba(88,214,255,0.4)]"
          >
            <RotateCw size={13} strokeWidth={2.4} className="transition-transform group-hover:rotate-45" />
            지금 재시작
          </button>
        )}
        <button
          onClick={() => setDismissed(state)}
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-dim transition-colors hover:bg-panel2 hover:text-text"
          title="나중에"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
