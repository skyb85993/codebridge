import React from 'react';
import { AlertTriangle, RefreshCw, FolderOpen } from 'lucide-react';

interface State {
  error: Error | null;
}

/**
 * 렌더러 전체를 감싸는 에러 바운더리.
 * 크래시 시 흰 화면 대신 복구 안내 화면을 띄운다.
 */
export default class ErrorBoundary extends React.Component<{ children: React.ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('[codebridge] renderer crash:', error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      const msg = this.state.error.message || String(this.state.error);
      return (
        <div className="app-bg flex h-screen w-screen flex-col items-center justify-center gap-4 bg-base px-6 text-center">
          <span className="grid h-14 w-14 place-items-center rounded-2xl border border-amber-400/30 bg-amber-400/10">
            <AlertTriangle size={26} className="text-amber-300" strokeWidth={2} />
          </span>
          <div className="text-grad text-xl font-extrabold">문제가 발생했습니다</div>
          <p className="max-w-md text-[12px] leading-relaxed text-dim">
            일시적인 오류가 발생했습니다. 다시 불러오면 대부분 해결됩니다.
          </p>
          <pre className="max-w-lg truncate rounded-lg border border-lineSoft bg-panel2 px-3 py-1.5 font-mono text-[10px] text-dim">
            {msg}
          </pre>
          <div className="flex items-center gap-2">
            <button
              onClick={() => window.location.reload()}
              className="flex items-center gap-1.5 rounded-xl bg-accent-grad px-4 py-2 text-[12px] font-bold text-[#06202b] transition-all hover:shadow-glow"
            >
              <RefreshCw size={13} strokeWidth={2.4} />
              다시 불러오기
            </button>
            <button
              onClick={() => window.codebridge?.openLogsFolder?.()}
              className="flex items-center gap-1.5 rounded-xl border border-lineSoft bg-panel2 px-4 py-2 text-[12px] font-semibold text-text transition-colors hover:border-line"
            >
              <FolderOpen size={13} strokeWidth={2.2} />
              로그 폴더 열기
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
