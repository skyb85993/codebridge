# CodeBridge

무료 웹 채팅(ChatGPT 등)을 "모델"로 사용하는 코딩 에이전트 시스템.
하네스는 오픈코드를 재사용하고, 이 저장소에는 **웹 채팅 브릿지(Rust)**와 설정 산출물이 들어간다.

> 상세 계획 문서는 워크스페이스 밖(`.opencode/plan`)에 있다. 저장소에는 코드만 포함한다.

## 현재 상태 (Phase 0 스파이크)

- [x] bridge/mock: 로컬 OpenAI 호환 서버 (`/v1/models`, `/v1/chat/completions` SSE)
- [x] lenient 파서: 코드펜스+경로 주석 → write tool_calls 승격 (유닛 테스트 포함)
- [x] opencode.json: `webchat/webchat-mock` 프로바이더 등록
- [x] 도구 가용성 조사: v2 patch 플러그인이 모델 id에 "gpt-" 포함 시 edit/write를 정의에서 삭제 → 모델 id는 "gpt-"를 피해야 함
- [x] **E2E 완성 (실 ChatGPT)**: 자연어 요청 → 실제 응답 → bridge가 DOM 코드 블록 추출 → 관대 파서 → opencode write 실행 → 실제 파일 생성 (`GameManager.lua`, `HUD.client.lua`)
- [x] **인수인계 체인 실증**: "지금 인수인계" 트리거 → 팩 생성 → 새 채팅 교체 → 팩 첫 메시지 재전송 → 코드 블록 추출 → write 승격 성공 (2026-11-10)
- [ ] (남음) Gemini 이중화 (Phase 3) / 잘림 시 자동 재전송 폴리시 / Studio 실행 확인
- [x] CDP 자동화 스파이크: 전용 Chrome + CDP로 ChatGPT에 PING/PING2 전송 → 응답 추출 성공 (2026-11-10)
- [x] **브라우저 엔진(실연결) 완료**: flatten + CDP 전송/감지/추출 + 아카이브 + 한도 감지 + 인수인계 팩/교체/재전송 (실 ChatGPT 검증)
- [x] ~~Ollama 연동 확인~~ → **v0.5 배제** (2026-11-10 사용자 결정: 로컬 AI 전면 배제)

## 빠른 시작

```powershell
# 1) 브릿지 실행 (mock 모드, 포트 9224)
cd codebridge\bridge
cargo run

# 2) 다른 터미널에서 (codebridge 폴더에서)
opencode run -m webchat/webchat-mock "미니 게임 파일 만들어줘"

# 3) 확인: ServerScriptService/GameManager.lua 등 파일이 생성되어야 함
```

## CodeBridge Desktop (v0.1 — 독립 앱 포장, 개발 중)

`desktop/`는 **Electron**으로 만든 독립 설치파일 앱입니다. 앱이 전용 Chrome(CDP 9333, 로그인된 전용 프로필)을 자동 실행하고, opencode serve(공식 웹 UI+v2 API)와 브릿지를 자식 프로세스로 띄웁니다. 기본 모드는 **browser(실 ChatGPT 자동화)**, 번들 설정은 `desktop/resources/bridge/config.toml`에서 변경 가능합니다. 별도 React UI는 Phase 4(커스텀 UI)에서 추가 예정.

```powershell
cd codebridge\desktop
npm install
npm run dist     # → dist\CodeBridge Setup <버전>.exe (NSIS)
npm start        # 개발 모드 (electron)
```

최초 실행 시 프로젝트 폴더를 선택하면, 해당 폴더에 `opencode.json`(webchat→브릿지 9224)이 없을 때만 복사됩니다.
앱 창을 닫아도 트레이의 "CodeBridge"가 백그라운드에서 유지되며, 트레이 메뉴에서 프로젝트 재선택/브릿지 재시작/로그 폴더/종료가 가능합니다.

## curl로 헬스체크

```powershell
curl.exe http://127.0.0.1:9224/health
curl.exe http://127.0.0.1:9224/v1/models
```

## 터미널 인코딩 관련

PowerShell에서 한글 로그가 깨지면 `chcp 65001` 또는 `[Console]::OutputEncoding = [Text.Encoding]::UTF8` 사용.
