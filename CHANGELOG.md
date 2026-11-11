# 릴리즈 노트

## v0.1.0 (2026-11-10)

첫 공개 릴리즈.

- 전용 Chrome 자동 실행 (CDP 9333, 전용 프로필)
- 실 ChatGPT / Gemini 이중화, 모델 id `gemini` 포함 시 Gemini 자동 선택
- 인수인계 체인: `same_site_new_chat → gemini` 자동 로테이션
- `/v1/probe` — 두 사이트 로그인 상태 실시간 확인, 앱 트레이에 표시
- 인앱 자동 업데이트 (`electron-updater`, GitHub Releases 기준)
- 설정.json 폴더 선택 + 단일 인스턴스 잠금

## v0.2.1 (미릴리스)

- `webchat-chatgpt` (ChatGPT (web)) 모델 등록 — opencode.json 2곳 + `/v1/models`에 추가 (`gpt-` 금지 규칙 준수)
- iframe 임베드 실사용 E2E 검증 완료: 새 세션 → ChatGPT (web) → 로블록스 Lua 파일이 실제로 작성됨 (스크래치 폴더)
- 발견: webchat 모델이 "모델 관리"에서 기본 OFF → 첫 실행 시 표시되지 않음 (활성화 방법 문서화)

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

