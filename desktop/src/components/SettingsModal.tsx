import { useCallback, useEffect, useState, type ReactNode } from 'react';
import {
  X,
  FolderOpen,
  Bot,
  ShieldCheck,
  ShieldAlert,
  RefreshCw,
  FolderClock,
  Chrome,
  RotateCw,
  ExternalLink,
  Check,
  Trash2,
} from 'lucide-react';
import { MODELS, type ModelID } from '../api';
import type { HostSettings, HostSettingsPatch } from '../ipc';

interface SettingsModalProps {
  appVersion: string;
  projectDir: string | null;
  onClose: () => void;
  onPickProject: () => Promise<void>;
  onDefaultModel: (m: ModelID) => void;
  onApplyModelToCurrent?: (m: ModelID) => void;
  canApplyModelToCurrent?: boolean;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b border-lineSoft px-5 py-3.5 last:border-b-0">
      <h3 className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] text-dim">
        {title}
      </h3>
      {children}
    </section>
  );
}

function Row({ label, desc, control }: { label: string; desc?: string; control: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1.5">
      <div className="min-w-0">
        <p className="text-[12px] font-medium text-text">{label}</p>
        {desc && <p className="mt-0.5 text-[10px] leading-snug text-dim">{desc}</p>}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

function Toggle({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative h-5 w-9 rounded-full transition-colors ${
        on ? 'bg-emerald-400/80' : 'bg-panel3'
      } ${disabled ? 'opacity-50' : ''}`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${
          on ? 'left-[18px]' : 'left-0.5'
        }`}
      />
    </button>
  );
}

const actionBtn =
  'inline-flex items-center gap-1.5 rounded-lg border border-lineSoft bg-panel2 px-2.5 py-1.5 text-[11px] font-medium text-text transition-colors hover:border-line hover:bg-panel3';

export default function SettingsModal({
  appVersion,
  projectDir,
  onClose,
  onPickProject,
  onDefaultModel,
  onApplyModelToCurrent,
  canApplyModelToCurrent,
}: SettingsModalProps) {
  const [settings, setSettings] = useState<HostSettings | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<string | null>(null);
  const [probeNote, setProbeNote] = useState<string | null>(null);

  useEffect(() => {
    window.codebridge.getSettings().then(setSettings).catch(() => setNote('설정을 불러오지 못했습니다.'));
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const flash = useCallback((msg: string) => {
    setNote(msg);
    window.setTimeout(() => setNote(null), 3200);
  }, []);

  const patch = useCallback(
    async (p: HostSettingsPatch, msg?: string) => {
      setBusy('save');
      try {
        const res = await window.codebridge.setSettings(p);
        if (res.ok && res.settings) {
          setSettings(res.settings);
          if (msg) flash(msg);
        } else {
          flash(res.error ?? '저장에 실패했습니다.');
        }
      } catch {
        flash('저장에 실패했습니다.');
      } finally {
        setBusy(null);
      }
    },
    [flash],
  );

  const changeModel = (m: ModelID) => {
    onDefaultModel(m);
    patch({ model: m }, '기본 모델이 변경되었습니다. (새 대화부터 적용)');
  };

  const checkUpdates = async () => {
    setBusy('update');
    setUpdateInfo('확인 중…');
    try {
      const res = await window.codebridge.checkUpdates();
      if (!res.ok) {
        setUpdateInfo(res.error ?? '확인 실패');
      } else {
        // 결과는 getUpdateState로 잠시 후 반영된다
        const st = await window.codebridge.getUpdateState();
        setUpdateInfo(
          st.state === 'downloaded'
            ? `새 버전 v${st.version} 다운로드 완료 — 재시작하면 적용됩니다`
            : st.state === 'up-to-date'
              ? '현재 버전이 최신입니다'
              : st.state === 'available'
                ? `새 버전 v${st.version ?? '?'} 다운로드 중…`
                : `상태: ${st.state}`,
        );
      }
    } catch {
      setUpdateInfo('확인 실패');
    } finally {
      setBusy(null);
    }
  };

  const quitAndInstall = () => {
    window.codebridge.quitAndInstall();
  };

  const showChrome = async () => {
    setBusy('chrome');
    try {
      const res = await window.codebridge.showChrome();
      flash(res.ok ? '전용 Chrome을 화면에 표시했습니다.' : (res.error ?? 'Chrome을 열지 못했습니다.'));
    } finally {
      setBusy(null);
    }
  };

  const recheckLogin = async () => {
    setBusy('probe');
    setProbeNote('확인 중…');
    try {
      const p = await window.codebridge.probe();
      const cgpt = p.sites.find((s) => s.site === 'chatgpt');
      const gemini = p.sites.find((s) => s.site === 'gemini');
      const fmt = (s?: typeof cgpt) =>
        !s ? '상태 모름' : s.login_required ? '로그인 필요' : s.composer ? '로그인됨' : '탭 없음/응답 없음';
      setProbeNote(`ChatGPT: ${fmt(cgpt)} · Gemini: ${fmt(gemini)}`);
    } catch {
      setProbeNote('확인 실패 — 브릿지가 응답하지 않습니다');
    } finally {
      setBusy(null);
    }
  };

  const resetChromeProfile = async () => {
    if (!confirmingReset) {
      setConfirmingReset(true);
      return;
    }
    setBusy('reset');
    try {
      const res = await window.codebridge.resetChromeProfile();
      setConfirmingReset(false);
      flash(
        res.ok
          ? '전용 Chrome 프로필이 초기화되었습니다. 다음 요청부터 재로그인이 필요합니다.'
          : (res.error ?? '초기화 실패'),
      );
    } finally {
      setBusy(null);
    }
  };

  const restartBridge = async () => {
    setBusy('bridge');
    const res = await window.codebridge.restartBridge();
    flash(res.ok ? '브릿지를 재시작했습니다.' : (res.error ?? '재시작 실패'));
    setBusy(null);
  };

  const restartServices = async () => {
    setBusy('services');
    const res = await window.codebridge.restartServices();
    flash(res.ok ? '서비스를 전체 재시작했습니다.' : (res.error ?? '재시작 실패'));
    setBusy(null);
  };

  const openLogs = () => {
    window.codebridge.openLogsFolder();
  };

  const openReleases = () => {
    window.codebridge.openReleasesPage();
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/55 p-4 backdrop-blur-sm"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="nice-scroll flex max-h-[86vh] w-[520px] flex-col overflow-y-auto rounded-2xl border border-lineSoft bg-panel shadow-card fade-up"
        role="dialog"
        aria-modal="true"
        aria-label="설정"
      >
        {/* 헤더 */}
        <div className="sticky top-0 z-10 flex items-center justify-between border-b border-lineSoft bg-panel/95 px-5 py-3 backdrop-blur">
          <div className="flex items-center gap-2">
            <span className="grid h-6 w-6 place-items-center rounded-md border border-cyan-400/30 bg-cyan-400/10">
              <Bot size={13} className="text-cyan-300" strokeWidth={2} />
            </span>
            <h2 className="text-[13px] font-bold text-text">설정</h2>
          </div>
          <button
            onClick={onClose}
            aria-label="닫기"
            className="grid h-7 w-7 place-items-center rounded-md text-dim transition-colors hover:bg-panel2 hover:text-text"
          >
            <X size={15} />
          </button>
        </div>

        {!settings ? (
          <div className="px-5 py-10 text-center text-[12px] text-dim">불러오는 중…</div>
        ) : (
          <>
            {/* 프로젝트 */}
            <Section title="프로젝트">
              <Row
                label="작업 폴더"
                desc={projectDir ?? '선택되지 않음'}
                control={
                  <button onClick={onPickProject} className={actionBtn}>
                    <FolderOpen size={12} />
                    변경…
                  </button>
                }
              />
            </Section>

            {/* AI */}
            <Section title="AI">
              <Row
                label="기본 모델"
                desc="새 대화의 기본값입니다 (진행 중인 대화에는 영향 없음)"
                control={
                  <div className="flex gap-1 rounded-lg border border-lineSoft bg-panel2 p-1">
                    {MODELS.map((m) => {
                      const active = settings.model === m.id;
                      return (
                        <button
                          key={m.id}
                          onClick={() => changeModel(m.id)}
                          className={`rounded-md px-3 py-1.5 text-[11.5px] font-semibold transition-colors ${
                            active ? 'bg-accent-grad text-[#06202b]' : 'text-dim hover:text-text'
                          }`}
                        >
                          {m.name}
                        </button>
                      );
                    })}
                  </div>
                }
              />
              {canApplyModelToCurrent && (
                <button
                  onClick={() => {
                    onApplyModelToCurrent?.(settings.model);
                    flash('지금 대화에도 적용했습니다.');
                  }}
                  className="mt-1 text-[10.5px] font-medium text-cyan-300 transition-colors hover:text-cyan-200 hover:underline"
                >
                  이 모델을 지금 대화에도 적용 →
                </button>
              )}
            </Section>

            {/* 승인 */}
            <Section title="승인">
              <Row
                label="위험 명령 확인"
                desc={
                  settings.dangerConfirm
                    ? 'rm -rf, del /f 같은 파괴적 명령은 실행 전에 확인 카드를 띄웁니다 (권장)'
                    : '파괴적 명령도 확인 없이 실행됩니다. 신중하게 끄세요'
                }
                control={
                  <Toggle
                    on={settings.dangerConfirm}
                    onChange={(v) =>
                      patch({ dangerConfirm: v }, v ? '위험 명령 확인을 켰습니다.' : '위험 명령 확인을 껐습니다.')
                    }
                    disabled={busy === 'save'}
                  />
                }
              />
              {!settings.dangerConfirm && (
                <div className="mt-1.5 flex items-start gap-2 rounded-xl border border-amber-400/25 bg-amber-400/10 px-3 py-2">
                  <ShieldAlert size={14} className="mt-0.5 shrink-0 text-amber-300" strokeWidth={2.2} />
                  <p className="text-[10.5px] leading-snug text-amber-200">
                    확인 없이 파일 삭제 명령이 실행될 수 있습니다. AI가 삭제를 시도하면 결과를 되돌릴 수 없습니다.
                  </p>
                </div>
              )}
            </Section>

            {/* 인수인계 */}
            <Section title="인수인계">
              <Row
                label="스레드 한도"
                desc="이 메시지 수에 도달하면 새 웹 채팅으로 인수인계합니다"
                control={
                  <div className="flex gap-1 rounded-lg border border-lineSoft bg-panel2 p-1">
                    {[10, 25, 50].map((n) => {
                      const active = settings.handoverLimit === n;
                      return (
                        <button
                          key={n}
                          onClick={() => patch({ handoverLimit: n }, `스레드 한도가 ${n}개로 변경되었습니다.`)}
                          className={`rounded-md px-2.5 py-1.5 font-mono text-[11px] font-semibold transition-colors ${
                            active ? 'bg-accent-grad text-[#06202b]' : 'text-dim hover:text-text'
                          }`}
                        >
                          {n}
                        </button>
                      );
                    })}
                  </div>
                }
              />
              <Row
                label="긴 대화 안내 숨기기"
                desc="답변 끝의 'N개 쌓였습니다' 안내 문구를 표시하지 않습니다"
                control={
                  <Toggle
                    on={settings.hideContextNote}
                    onChange={(v) =>
                      patch(
                        { hideContextNote: v },
                        v ? '긴 대화 안내를 숨깁니다.' : '긴 대화 안내를 표시합니다.',
                      )
                    }
                    disabled={busy === 'save'}
                  />
                }
              />
            </Section>

            {/* 업데이트 */}
            <Section title="업데이트">
              <Row
                label="자동 확인"
                desc="앱 시작 시 새 버전을 자동으로 확인합니다"
                control={
                  <Toggle
                    on={settings.autoCheckUpdates}
                    onChange={(v) => patch({ autoCheckUpdates: v })}
                    disabled={busy === 'save'}
                  />
                }
              />
              <div className="mt-1.5 flex items-center gap-2">
                <button onClick={checkUpdates} className={actionBtn} disabled={busy === 'update'}>
                  <RefreshCw size={12} className={busy === 'update' ? 'animate-spin' : ''} />
                  지금 업데이트 확인
                </button>
                {updateInfo && <span className="text-[10.5px] text-dim">{updateInfo}</span>}
              </div>
            </Section>

            {/* 알림 */}
            <Section title="알림">
              <Row
                label="질문/완료 알림"
                desc="앱이 숨겨진 동안 답변 대기/작업 완료 시 Windows 토스트로 알립니다"
                control={
                  <Toggle
                    on={settings.notifyEnabled}
                    onChange={(v) =>
                      patch({ notifyEnabled: v }, v ? '알림을 켰습니다.' : '알림을 껐습니다.')
                    }
                    disabled={busy === 'save'}
                  />
                }
              />
            </Section>

            {/* 전용 Chrome */}
            <Section title="전용 Chrome (로그인)">
              <Row
                label="전용 Chrome 항상 보이기"
                desc="로그인/캡차가 잦으면 켜세요. 다음 Chrome 기동부터 적용됩니다."
                control={
                  <Toggle
                    on={settings.chromeAlwaysVisible}
                    onChange={(v) =>
                      patch(
                        { chromeAlwaysVisible: v },
                        v ? '전용 Chrome이 화면에 보입니다.' : '전용 Chrome이 숨겨집니다.',
                      )
                    }
                    disabled={busy === 'save'}
                  />
                }
              />
              <div className="mt-1.5 flex flex-wrap items-center gap-2">
                <button onClick={showChrome} className={actionBtn} disabled={busy === 'chrome'}>
                  <Chrome size={12} />
                  전용 Chrome 보이기
                </button>
                <button onClick={recheckLogin} className={actionBtn} disabled={busy === 'probe'}>
                  <ShieldCheck size={12} />
                  로그인 다시 확인
                </button>
                {!confirmingReset ? (
                  <button
                    onClick={resetChromeProfile}
                    className={`${actionBtn} border-red-400/30 text-red-300 hover:border-red-400/50`}
                    disabled={busy === 'reset'}
                  >
                    <Trash2 size={12} />
                    프로필 초기화
                  </button>
                ) : (
                  <span className="flex items-center gap-1.5">
                    <span className="text-[10.5px] text-red-300">로그인이 풀립니다. 정말 초기화?</span>
                    <button onClick={resetChromeProfile} className="rounded-md bg-red-500/80 px-2 py-1 text-[10.5px] font-bold text-white">
                      예
                    </button>
                    <button onClick={() => setConfirmingReset(false)} className="rounded-md border border-lineSoft bg-panel2 px-2 py-1 text-[10.5px] text-text">
                      아니오
                    </button>
                  </span>
                )}
              </div>
              {probeNote && <p className="mt-1.5 text-[10.5px] text-dim">{probeNote}</p>}
              <p className="mt-1 text-[9.5px] leading-snug text-dim/70">
                초기화하면 로그인 세션이 지워지고, 이전 프로필은 백업 폴더(chrome-profile.bak-*)로 이동합니다.
              </p>
            </Section>

            {/* 진단 */}
            <Section title="진단">
              <div className="flex flex-wrap items-center gap-2">
                <button onClick={openLogs} className={actionBtn}>
                  <FolderClock size={12} />
                  로그 폴더 열기
                </button>
                <button onClick={restartBridge} className={actionBtn} disabled={busy === 'bridge'}>
                  <RotateCw size={12} className={busy === 'bridge' ? 'animate-spin' : ''} />
                  브릿지 재시작
                </button>
                <button
                  onClick={restartServices}
                  className={`${actionBtn} border-amber-400/30 text-amber-300 hover:border-amber-400/50`}
                  disabled={busy === 'services'}
                >
                  <RotateCw size={12} className={busy === 'services' ? 'animate-spin' : ''} />
                  서비스 전체 재시작
                </button>
                <button onClick={openReleases} className={actionBtn}>
                  <ExternalLink size={12} />
                  릴리스 페이지
                </button>
              </div>
              <p className="mt-1 text-[9.5px] leading-snug text-dim/70">
                설정 변경(위험 명령 확인 / 스레드 한도)은 브릿지가 자동으로 재시작되어 바로 반영됩니다.
                "서비스 전체 재시작"은 진행 중인 요청을 중단합니다.
              </p>
            </Section>

            {/* 정보 */}
            <Section title="정보">
              <div className="flex items-center justify-between gap-2">
                <div>
                  <p className="text-[12px] font-semibold text-text">CodeBridge Desktop</p>
                  <p className="text-[10px] text-dim">v{appVersion} · 무료 배포 (코드 서명 없음)</p>
                </div>
                <Check size={14} className="text-emerald-400/70" />
              </div>
              <p className="mt-1.5 text-[9.5px] leading-snug text-dim/70">
                처음 실행 시 SmartScreen 경고가 뜰 수 있습니다 — "추가 정보 → 실행"을 선택하세요.
              </p>
            </Section>

            {note && (
              <div className="sticky bottom-0 border-t border-lineSoft bg-panel/95 px-5 py-2 text-center text-[11px] text-cyan-200 backdrop-blur">
                {note}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
