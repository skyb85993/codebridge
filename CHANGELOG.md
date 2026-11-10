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

## v0.2.0 (2026-11-11)

커스텀 UI 셸 릴리스.

- 자체 프레임리스 창 + 커스텀 타이틀바(최소화/최대화/닫기) + 사이드바(작업/로그/설정) + 상태바
- 채팅 본문은 opencode 웹 UI를 iframe으로 임베드 (브랜드 일체감)
- 상태바에서 브릿지/로그인/메시지 수/업데이트 상태 확인
- 업데이터: GitHub Releases 공개 저장소로 전환, 최신 릴리스 감지 검증 완료
- 설치 파일명을 latest.yml과 일치하도록 artifactName 고정

