# 릴리즈 노트

## v0.4.2 (2026-11-12)

중요 수정: CI에서 `opencode.exe`(210MB)가 gitignore로 제외되어 GitHub 릴리스 빌드에 opencode 서버가 누락되던 문제 — CI에서 npm으로 `@opencode/cli-windows-x64@2.0.31`을 받아 `resources/opencode/opencode.exe`로 복사하도록 수정. v0.4.0/v0.4.1 설치본의 창 미표시/서버 연결 실패 증상이 여기서 비롯됨.

## v0.4.1 (2026-11-11)

설치/업데이트 UI 브랜드 통일 릴리스.

- **NSIS 설치 마법사 커스텀 이미지**: `build/installer-header.bmp` (150×57), `build/installer-sidebar.bmp` (164×314), `installer.ico` / `uninstaller.ico` (48×48) — 앱 아이콘과 동일한 브랜드 글로우/그라디언트 스타일, `make_nsis_images.py`로 재생성 가능
- **인앱 업데이트 배너**: 네이티브 다이얼로그 제거 → 타이틀바 아래 글래스 배너 ("새 버전 준비됨" → 지금 재시작 / 나중에). 다운로드 중일 때는 진행 중 상태 표시
- **첫 실행 온보딩 재디자인**: 글로우 로고 + 그라디언트 워드마크 + 3단계 안내 카드(폴더 연결 → 자동 실행 → 바로 작성) + 그라디언트 CTA

## v0.4.0 (2026-11-11)

네이티브 React 프론트엔드로 전면 재작성한 릴리스.

- **프론트엔드 재구축**: opencode SPA iframe 제거 → 자체 React+Vite+Tailwind 프레임리스 셸 (그라디언트 타이틀바 / 세션 사이드바 / 채팅 / 모델 선택)
- **백엔드 정리**: mock harness(`fixture.rs`, mock 경로, `/v1/models`의 webchat-mock) 제거 → opencode browser-only
- **전용 Chrome 지연 실행**: 첫 요청 시 브릿지가 전용 Chrome을 실행(probe는 읽기 전용, 탭 스팸 방지)
- **레이아웃 버그 수정**: `#view-work` position:relative로 iframe 150px 붕괴하던 회귀
- **첫 실행 UX**: opencode.json 자동 시드(projectDir + webchat 모델 visibility)로 추가 단계 없이 동작
- **v0.3.0 수정분 포함**: CI/CD, 인수인계 이력, 로그인 배너, testpages 7/7 회귀
- 주의: 무서명 배포 (README "설치 시 SmartScreen 경고" 참조)

## v0.1.0 (2026-11-10)

첫 공개 릴리즈.

- 전용 Chrome 자동 실행 (CDP 9333, 전용 프로필)
- 실 ChatGPT / Gemini 이중화, 모델 id `gemini` 포함 시 Gemini 자동 선택
- 인수인계 체인: `same_site_new_chat → gemini` 자동 로테이션
- `/v1/probe` — 두 사이트 로그인 상태 실시간 확인, 앱 트레이에 표시
- 인앱 자동 업데이트 (`electron-updater`, GitHub Releases 기준)
- 설정.json 폴더 선택 + 단일 인스턴스 잠금

## v0.3.0 (2026-11-11)

CI/CD + 인수인계 UX + 회귀 자동화 릴리스.

- **CI/CD**: `master` push/PR → 빌드 체크(`ci.yml`: cargo test + 문법 검사 + testpages 계약 검사), 태그 `v*` push → `release.yml`이 자동 빌드→GitHub Release 생성 (릴리스 노트는 CHANGELOG에서 추출, `WIN_CSC_LINK` 시크릿이 있으면 자동 서명)
- **인수인계 이력 뷰** (사이드바 "인수인계"): 최근 인계 이벤트 JSONL 기록·표시 (대화 본문 아님), "지금 인수인계" 수동 트리거 버튼 (`POST /v1/handover/trigger` → 다음 browser-mode 요청이 인계 체인 실행)
- **로그인 필요 배너**: 상태바에 경고 + 원클릭 "전용 Chrome 열기" (kill 없이 재사용/실행)
- **testpages 회귀 자동화** (`scripts/testpages-check.mjs`): 엔진 선택자 계약을 반영한 가짜 ChatGPT DOM으로 HEADLESS Chrome + CDP 검증, **7/7 통과** (CI에도 포함)
- 아카이브 게이팅, handover 이벤트 로그 상한(64KB/128줄), README/릴리스 문서 갱신

## v0.2.1 (2026-11-11)

CodeRabbit 검사 수정 릴리스 (19건 전부 Major).

- webchat-chatgpt (ChatGPT (web)) 모델 추가 — browser 모드에서도 webchat-chatgpt 선택 가능
- **첫 실행 UX 수정**: opencode SPA 임베드 시 webchat 모델이 기본 숨김이던 것·프로젝트 미등록으로 "＋ 새 세션"이 안 되던 것 — 셸 프리로드가 SPA localStorage에 모델 표시 + 프로젝트 등록 + 홈 선택을 자동 시드 (opencode.global.dat:model / server / layout)
- **보안/안정성 수정**: 프롬프트 디버그 덤프 제거, 아카이브를 핸드오버 설정(archive, 기본 off)에 게이트, 탭 origin 검증(피싱 방지), 한도 배너 감지 범위를 배너성 요소로 제한, 사이트 작업 직렬화 잠금, 새 세대 응답 대기(stale 응답 승격 방지) + truncated 부분 응답은 파일 승격 중단, tool 턴은 마지막 메시지 기준으로만 판정(실패 시 실패 메시지), 사용자 요청은 절대 잘리지 않음(초과 시 명시적 거부), 수동 인수인계는 현재 요청만 감지, mock 모델은 browser 모드에서도 mock 경로로 라우팅, Gemini 재시도 한도 배너 체크, 새 채팅 대기 실패 시 에러, Chrome 재시작 시 종료 대기 후 CDP 확인, iframe은 opencode 서버가 200일 때만 로드(실패 시 오버레이 + 로그 링크)
- iframe 레이아웃 버그 수정(#view-work position:relative로 iframe 150px 붕괴)
- flatten content 배열 파싱, 스냅샷은 예산에 맞게 행 단위 절단

## v0.2.0 (2026-11-11)

커스텀 UI 셸 릴리스.

- 자체 프레임리스 창 + 커스텀 타이틀바(최소화/최대화/닫기) + 사이드바(작업/로그/설정) + 상태바
- 채팅 본문은 opencode 웹 UI를 iframe으로 임베드 (브랜드 일체감)
- 상태바에서 브릿지/로그인/메시지 수/업데이트 상태 확인
- 업데이터: GitHub Releases 공개 저장소로 전환, 최신 릴리스 감지 검증 완료
- 설치 파일명을 latest.yml과 일치하도록 artifactName 고정

