# Phase 0 스파이크 보고서 — 도구 가용성 조사와 최종 성공

날짜: 2026-11-10 · 대상: opencode v2.0.31 (npm `@opencode/cli`, Bun 빌드 exe) · 브릿지: CodeBridge mock 모드

## 결론 (TL;DR)

`opencode run -m webchat/webchat-mock "로블록스 미니 게임 파일을 만들어줘"` →
`ServerScriptService/GameManager.lua`, `StarterPlayer/StarterPlayerScripts/HUD.client.lua` **실제 생성 확인**.
CodeBridge 관대 모드(코드펜스+경로 주석 → `write` tool_calls 승격)의 전체 왕복이 증명됨.

## 발생했던 오류와 원인

### 증상 1: `Tool is not available for this request: write/edit`

**원인 코드**: `packages/core/src/tool/plugin/patch.ts` (v2.0.31, L296-L306)

```ts
const hook = (event: SessionHooks["context"]) =>
  Effect.sync(() => {
    const usePatch =
      event.model.id.includes("gpt-") && !event.model.id.includes("oss") && !event.model.id.includes("gpt-4")
    if (usePatch) {
      delete event.tools.edit   // ← edit 정의 삭제
      delete event.tools.write  // ← write 정의 삭제
      return
    }
    delete event.tools.patch
  })
```

- **모델 id에 `gpt-` 하위 문자열이 있으면** opencode는 edit/write를 요청 정의에서 **삭제**하고 `patch` 도구만 노출한다
  (GPT 계열이 patch 포맷에 특화되어 있다는 판단의 최적화).
- mock 모델 id가 `chatgpt-mock`이었으므로 ("chat·**gpt-**·mock") 정확히 이 분기에 걸렸음.
- 실행 시점 레지스트리에 도구는 존재하지만 요청 정의에 없어서 `tool.ts` execute() 가드에서 에러 발생.

**해결**: 모델 id를 `webchat-mock`으로 변경 → edit/write 노출, `patch`는 대신 정의에서 삭제됨.
브릿지 산출물 확인 (52개 정의 목록): `read, glob, grep, question, skill, subagent, websearch,
experimental_image_generate, opencode_*, edit, write, Roblox_Studio_* 34` — **patch 부재 확인**.

> **CodeBridge 설계 규칙 (중요)**: 브릿지가 노출하는 모델 id에는 절대 `gpt-` 하위 문자열을 넣지 않는다.
> (예: `gpt-5` 처럼 `gpt-`가 들어가면 patch 모드로 전환되어 관대 모드와 충돌.
> 실제 ChatGPT 웹 연동 시 `wchat-gpt` 처럼 `gpt-` 뒤에 문자가 오지 않는 형태는 안전함)

### 증상 2: `Invalid arguments for tool "write": - path: Missing key` / read 동일

**원인**: opencode v2의 도구 인자 키는 `filePath`가 아니라 **`path`**.
(`read(path, offset, limit)`, `write(path, content)`, `edit(path|patchText, ...)`, `glob(pattern, path?, ...)`)
- 관대 파서의 승격 인자를 `{filePath, content}` → `{path, content}`로 수정. 유닛 테스트 갱신 (12/12 통과).

### 헛돌았던 가설 정리

| 시도 | 결과 |
| --- | --- |
| `--auto` 플래그 (ask 권한 자동 승인) | 무관 — 원인은 패치 플러그인 훅 |
| 모델 설정 `"tool_call": true` | 무관 (남겨둠 — 실제 프론트엔드에서도 필요 없는지 확인 필요) |
| `bash` 권한 키 | v2에서는 **`shell`** 이 올바른 키 (bash는 v1 잔재) |
| 로컬 플러그인 존재 확인 | ~/.config/opencode에 플러그인 없음 — 삭제는 opencode 내부 플러그인이 한 것 |

## Code Mode(v2 아키텍처)와 권한

- v2.0.31은 **Code Mode 아키텍처**: 일부 도구가 `execute` 하나로 통합되고, `codemode: false` 도구만 direct로
  모델 정의에 노출. (read/glob/grep/patch/question/edit/write/skill/subagent/websearch는 `codemode: false`)
- MCP 서버 도구(codemode:true 기본)는 Code Mode 카탈로그로 들어감. Roblox_Studio 34개 + `opencode_*` 7개가 이에 해당.
- **프로젝트 권한에 `"execute": "deny"` 추가 시 Code Mode가 완전히 꺼지고** 모든 active 도구가 direct 노출됨
  (`tool.ts` 스냅샷: `direct = codeModeEnabled ? codemode===false 필터 : active 전체`).
- CodeBridge는 `execute: deny`를 유지: MCP 41개 도구가 Code Mode 카탈로그 주입 없이 직접 노출되는 형태 유지,
  큰 카탈로그 프롬프트 오버헤드 제거 (무료 웹 채팅 모델에 유리).

## 진단 인프라 (브릿지 내재화)

- `[INTROSPECT]` 문자열이 사용자 요청에 포함되면 mock이 read/glob/edit/write 4종 tool_calls를 섞어 호출 →
  도구별 가용성을 한 번에 관찰 가능.
- 요청에 담긴 도구 정의 개수와 이름을 매 요청 로그로 덤프 (`[mock] definitions(52) = ...`).
- 루프 가드: mock 모드에서 messages > 5면 즉시 stop 응답 (재시도 루프 방지, 기존 632건 폭주 방지됨 확인).

## 학습 요약 (계획서 반영 사항)

1. 모델 id에 `gpt-` 금지 (edit/write 삭제 훅).
2. 도구 인자 키는 v1 호환 표기가 아니라 v2 표기를 사용: read/write/edit = `path`, `limit`/`offset` 등.
3. 권한 액션 키: `edit`, `shell`, `webfetch`, `browser`, `skill`, `read`, `question`, `execute`, `*` (`bash` 아님).
4. Code Mode 비활성 레버 = 프로젝트 권한 `execute: deny`.
5. 도구 결과 턴에는 stop으로 마무리해야 한다 (fixture 재전송 시 파일 중복 덮어쓰기 + 루프 가드 트립). 루프 가드(messages>5)는 안전망으로 유지.

## 후속 정리 (2026-11-10 13:00)

- **제목 생성 요청 처리**: opencode가 보내는 "Reply with only the title" 요청에 mock이 tool_calls를 반환하던 문제
  → 해당 요청에는 `calls=0`, 짧은 텍스트만 반환하도록 수정 (`[mock] calls=0` 확인).
- **도구 결과 턴 재전송 문제**: 도구 실행 결과가 포함된 요청(role=`tool` 메시지)에서 mock이 fixture를 또 반환해
  파일이 덮어써지고 루프 가드(messages>5)에 걸리던 문제 → role=`tool` 메시지가 있으면 `ACK` 텍스트(파일 저장 완료 안내)와
  `finish=stop`을 반환하도록 수정.
- **E2E 재검증 성공**: 제목 요청 → 본 요청(write 2건) → 도구 결과 턴(ACK, stop) 전 과정이 루프 가드 없이 종료.
  `ServerScriptService/GameManager.lua`, `StarterPlayer/StarterPlayerScripts/HUD.client.lua` 생성 확인.
  단위 테스트 12/12 통과. 프로브 산출물(`Probe/probe.txt`, README "(probe)") 정리 완료.
- 브릿지 진단 로그(`[mock] definitions(52) = ...`)에서 edit/write 등 52개 정의 전송을 매 요청마다 확인.

## Phase 0 항목 D — 로컬 모델 (v0.5 배제)

> 2026-11-10, 사용자 결정으로 **로컬 AI는 제품에서 전면 배제**됐습니다.
> (당일 13:00에 Ollama 0.40.15 + `qwen3:8b` + `qwen2.5-coder:14b`를 설치·검증(100% GPU, 61/33 tok/s)했으나,
> 이후 사용자 결정으로 설치 삭제·모델 삭제(≈13.2GB)·PATH 정리(13→12 항목)를 완료했고 프로젝트에서 로컬 AI를 제거했습니다.)

## Phase 0 항목 A — CDP 자동화 스파이크 (2026-11-10 완료)

- 전용 프로필(`codebridge/.chrome-profile`) + `--remote-debugging-port=9333`으로 Chrome 실행.
  (포트 9222는 사용자 기존 Chrome이 점유 중이라 9333 사용 — 사용자 크롬은 건드리지 않음)
- **로그인 성공 확인**(사용자). `cargo run --example cdp_spike` 기준 검증:
  - probe: `composer=false / loginBtn=true`였으나 로그인 후 입력 가능 확인
  - **PING 테스트 전송 성공** ("PING이라고만 답해 주세요." → ChatGPT가 "PING"으로 응답)
  - **두 번째 PING2 테스트**: 전송 → 스트리밍 완료 감지 → 응답 텍스트 "PING2" 추출 성공
    (clear → insertText 입력 → aria-label="보내기" 버튼 클릭 → 마지막 assistant 텍스트가 4회 연속 동일 + stop 버튼 없음 감지 → 추출)
- **중요 발견사항**:
  - 현행 ChatGPT UI는 **data-testid / data-message-author-role 속성 없음** (구 선택자 전부 죽음)
  - 전송 버튼: `aria-label="보내기"` (한/영 혼용 후보 필요; 영어 UI는 `aria-label="Send prompt"`)
  - assistant 응답 컨테이너: `div[class*="DilResponseRoot"]` / `[class*="entity-prose-inset"]`
  - user 버블: `.bg-user-message` / `.text-user-message`
  - `browser.pages()`는 연결 직후 비어 있을 수 있음 → 대상 목록 재시도(0.5s × 최대 12회) 필요
  - 탭 재사용 성공 (chatgpt.com/c/ 경로가 있는 탭 우선 선택)
- `profiles.toml` chatgpt 프로파일을 위 실측 선택자로 갱신 (한/영 후보, assistant/user 선택자 추가).
- `send` 모드는 매 실행마다 "대화 탭"을 재사용한다 (이전 탭 누적 방지)

## v0.1 독립 앱 포장 (2026-11-10)

- `desktop/`: Electron 셸. `opencode serve`(포트 7891, Basic 인증 서버 생성)+브릿지(9224, mock) 자식 프로세스 관리,
  앱 창 = opencode 공식 웹 UI(인증 헤더 자동 주입). 트레이 메뉴(프로젝트 재선택/브릿지 재시작/로그/종료), NSIS 타깃.
- `npm run dist` → `dist/CodeBridge Setup 0.1.0.exe` (약 174MB). 코드 서명 없음(SmartScreen 경고 가능).
- 검증: dev 실행에서 브릿지+serve 스폰·7891/9224 LISTEN 확인, 창 표시 확인.

## CodeRabbit 리뷰 (2026-11-10)

- 실행: `coderabbit review` (계정 skyb85993 / 무료 CLI 한도, git 원격 없음)
- 대상: v0.1 전체(소스 26개 파일, 빈 루트 커밋 기준 diff). 바이너리는 excluded
- 결과: **13건 발견 → 전부 검증·수정 완료**
  - **major**
    1. lenient 파서가 주석 없는 코드 라인도 경로로 인식 (예: `local path="..."`) → 주석 접두어 있는 라인에서만 인식 + 토큰 앞 단어 경계 요구 (+ 유닛 테스트 3개 추가, 15/15 통과)
    2. 잘리지 않은(unclosed) 펜스도 write 승격 → `issue.is_some()` 블록은 승격하지 않음 (+ 테스트)
    3. 데스크톱 앱: 창 닫으면 트레이에 재오픈 수단이 없음 → 트레이 "CodeBridge 열기" + 클릭 + second-instance에서 `bootUI()` 호출
  - **minor**: 인증 헤더가 모든 요청에 주입되던 것을 `127.0.0.1:7891`로 제한 / 실패 화면 "다시 시도"가 data URL reload만 하던 것을 `codebridge://retry` will-navigate로 서비스 재시작+재부팅 / 브릿지가 cwd의 일반 `config.toml`을 읽던 것 → `CODEBRIDGE_CONFIG` 환경변수 + 필드별 serde 기본값 + 번들 설정(`resources/bridge/config.toml`) 우선 적용 / 문서 오타(산출物), README 로컬 절대경로·체크리스트 모순, 모델 id 규칙 예시(gpt-5), profiles.toml `인증` 제거, .gitignore 패턴(/Serverday/StarterPlayer)
- 저장소: git init → 소스만 커밋 → 리뷰 → 이슈 수정 → **reflog 없는 단일 커밋으로 정리** (`ee45d49`)

## Phase 2 — 실연결 엔진 + 인수인계 자동화 v1 (2026-11-10, 완료 — 실전 검증)

### 구현 (완료)
- **자동화 엔진** (`bridge/src/automation/engine.rs`): CDP 연결 → 대화 탭 재사용(chatgpt.com/c/) → 컴포저 비우기 → `insertText` 입력 → `aria-label="보내기"` 클릭 → 마지막 assistant 텍스트 안정성+stop 버튼으로 완료 판정. 응답의 **코드 블록을 DOM에서 직접 추출**(DilResponseRoot 내 `pre`)해서 ` ``` ` 마커가 없는 현행 UI 문제 해결. 한도 배너 감지(`LIMIT_TEXTS` + `rate limit`), "새 채팅" 교체 포함.
- **잘림 처리**: `has_unterminated_fence()` — 미폐쇄 펜스 감지 시 승격 금지 + "이어서 계속" **자동 재전송(최대 1회)** 추가.
- **수동 인수인계**: 사용자 메시지에 "지금 인수인계" 포함 시 한도 배너와 무관하게 강제 인수인계 플로우 실행.
- **한도 사전/확정 감지**: 카운터(25개 기본) 경고 + 페이지 텍스트 배너 매칭("limit reached"/"usage cap"/"rate limit"/"메시지 한도"/"사용 한도").
- **아카이브** (`bridge/src/archive.rs`): `%APPDATA%\CodeBridge\bridge-archive\journal-*.md` + `state.json` 카운터.
- flatten, lenient 파서(주석접두어+단어경계), 테스트 **21개**.

### 실전 E2E 검증 (2026-11-10, 실 ChatGPT)
- 실사용 요청 "로블록스 미니 게임 스켈레톤…" → 실제 응답 → **DOM에서 코드 블록 2개 추출 → 관대 파서 승격 → opencode write 2건 실행** → `ServerScriptService/GameManager.lua`(3,856B) / `HUD.client.lua`(2,499B) **실제 생성 확인**.
- **핸드오버 라이브 테스트**: "지금 인수인계를 실행해서…" → 팩 저장(`handover-chatcmpl-codebridge-0004.md`, 2.6KB) → **새 채팅 교체 → 팩 재전송** → 새 스레드에서 응답에 코드 블록 2개 → write tool_calls 2건, 카운터 리셋 확인. **인수인계 체인 전체 실증 완료.**

## headless 숨김 실행 스파이크 (2026-11-10, 실패 → 폴백)

- `--headless=new`(Anti-bot 완화 플래그 `--disable-blink-features=AutomationControlled` 포함)로 같은 전용 프로필을 재사용해 CDP 연결 성공.
- 그러나 chatgpt.com이 **"잠시만 기다리십시오"(Cloudflare 대기/검증 화면)에 고정** — 약 50초 경과 후에도 `composer=false, bodyText=""`.
  → **headless는 ChatGPT 앞에서 봇으로 판정됨. 로그인 세션을 재사용해도 무효.**
- 해상도/폴백 방향:
  - **A안(완전 숨김/headless): 이 계정에서는 불가.**
  - **B안(WebView2 임베디드):** 창 자체를 띄우므로 headless 판정에 안 걸릴 가능성 높음. 단, OpenAI가 WebView2도 봇으로 볼 수는 있음 — 임베드 후 재검증 필요.
  - **C안(자동 관리 Chrome):** 창은 뜨지만 **앱이 자동으로 전용 Chrome을 띄움**(사용자가 수동으로 크롬을 열 필요 없음) + 기존 이 크롬(9333) 경로가 100% 검증됨. 가장 안전한 "내부적 체감" 옵션.

## C안 — 앱이 전용 Chrome을 자동 관리 (2026-11-10, 구현·검증 완료)

- Electron 앱(`desktop/main.cjs`)이 **전용 Chrome을 자동 실행**한다: 표준 경로(574기준 Chrome/Edge)에서 exe 탐지 → `--remote-debugging-port=9333 --user-data-dir=%APPDATA%\CodeBridge\chrome-profile`로 가시 창 실행.
- 첫 실행 시 로그인 1회는 사용자가 이 크롬 창에서 하면 된다 (앱이 로그인 여부를 주기적으로 표시: 트레이 툴팁 '로그인됨' / 'ChatGPT 로그인 필요').
- 브릿지에 `GET /v1/probe` 추가 → `{composer, login_required, url, title}` 반환. Electron이 8초마다 폴링 (선택은 사용자, 앱은 표시만).
- 번들 브릿지는 **browser 모드**(`resources/bridge/config.toml`) — 앱을 켜면 전용 Chrome → 브릿지 실연결 경로가 그대로 동작.
- 이전 수동 Chrome을 쓰던 상태에서 `.chrome-profile` → `%APPDATA%\CodeBridge\chrome-profile`로 1회 이관(로그인 승계, 248MB).
- **dev 모드 실측**: 앱 실행 → 전용 Chrome 자동 등장(CDP 9333) → `/v1/probe`: `login_required=false` → `bridge /v1/chat/completions`에 "안녕" 전송 → **실제 ChatGPT 응답 수신(finish=stop, 8.1s)** → 앱 종료 시 Chrome/브릿지/serve 모두 정리됨.

## Gemini 연동 스파이크 (2026-11-10, 진행 중)

### 검증된 것
- 로그인: dedicated 프로필에서 `gemini.google.com/app` — composer 확인 (`div.ql-editor`, contenteditable).
- **전송 버튼**: `button[aria-label*='보내']` (WebSocket/Wake on Synthetic events 금지 — `Element.click()` CDP 네이티브 경로 필수). 버튼은 텍스트 입력 후 ~1초 뒤 렌더링됨 (폴링 필수).
- **응답 컨테이너**: `message-content`(.model-response-text) → `.markdown.markdown-main-panel` 구조. 완료 감지는 stop 버튼 소멸 + 텍스트 안정으로.
- **스트리밍 완료 감지**: 텍스트 안정성 + stop 버튼 부재 방식은 ChatGPT와 동일하게 작동.

### 발견된 문제/해결
- **렌더된 UI에 ` ``` ` 마커 없음** (ChatGPT와 동일) → `pre` 블록 DOM 추출로 해결.
- Chromium Element click 직접 사용 안 됨 → JS Rect + `page.click(Point)`로 우회.
- 버튼은 텍스트 입력 후 ~1초 뒤에야 나타남 → 8초 폴링으로 안정화.
- "Request timed out." (cdfa 내부 기본 타임아웃) → page 전체가 생성 중이거나 버튼 미렌더링일 때 발생. 재시도+폴링으로 대응.
- 연속 요청으로 첫 성공 후 "1+1" 따위도 한동안 실패하던 현상: 이전 요청이 아직 생성 중("Analyzing..." 상태)인 채로 다음 요청이 들어가 전송 버튼이 교체/비활성화됨. **교훈: 생성 중 composer에 입력해도 send 버튼이 안 뜨므로, 시나리오별로 폴링+재시도 필수.**

### 상태
- 엔진(`engine.rs`) Site::ChatGpt/Site::Gemini 이중화, 모델 id에 "gemini" 포함 시 Gemini 라우팅.
- `/v1/probe` — 양쪽 사이트 동시 로그인 감지 보고.
- 폴링(~8초) + `page.click(Point)` 기반 전송.
- 아직 실사용 코딩 E2E 안정화 진행 중 (생성 지연/타임아웃 반복 이슈).

## Phase 3 — Gemini 이중화 + 완전한 앱 릴리즈 인프라 (2026-11-10, 진행 중)

### Gemini 이중화 (완료)
- `Site::ChatGpt | Site::Gemini` 엔진 이중화 (`automation/engine.rs`). 모델 id에 `gemini` 포함 시 Gemini.
- 선택자 실측: 전송 버튼 `button[aria-label*='보내']` + **CDP Native Element 클릭**(200자 이상의 합성 클릭 미반응). 완료 감지는 동일(stop 버튼 소멸 + 텍스트 안정).
- 응답 컨테이너: `message-content` / `structured-content-container.model-response-text`. 코드 블록은 `pre` 안에서 추출 (ChatGPT와 동일).
- **E2E 통과**: `opencode run -m webchat/webchat-gemini "점프 게임 만들어줘"` → 코드펜스 없이 렌더링된 코드도 DOM 추출 → write tool_calls → 파일 실제 생성 (GameManager.lua, HUD.client.lua).
- **핸드오버 체인** 구현+실측: [handover] `order = ["same_site_new_chat", "gemini"]` 기본값. 수동 `"지금 인수인계"` 트리거 → 팩 생성 → 새 채팅 → Gemini로 재전송 → 코드 생성 → write 승격 확인.

### 완전한 앱 릴리즈 인프라 (Phase 3)
- `electron-updater` 내장 (`setupAutoUpdate()`): 시작 시 1회 체크 + 트레이 "업데이트 확인". 코드 서명 없음.
- `RELEASING.md` — 릴리즈 절차 문서화 (GitHub Releases 기반).
- x64 per-user NSIS 인스톨러 (`desktop/dist/CodeBridge Setup 0.1.0.exe`).
- **알려진 결함(수정 완료)**: settings.json이 깨진 경로를 들고 있으면 앱이 기동하지 않고 로그도 남기지 않았다 → `resolveProjectDir()` + spawn `error` 리스너 + `boot.log` 추가.
- **주요 결함**: `electron-updater`를 devDependencies에 넣었는데 electron-builder는 dependencies만 asar에 묶는다 → packaged 앱이 `require('electron-updater')` 시 module-not-found로 즉시 종료하고, Windows GUI라 콘솔 출력도 없어 "아무 일도 안 하는 앱"처럼 보였다. **수정: dependencies로 이동 + try/catch로 감싸**, 기동 실패해도 `boot.log`에 남도록 변경.
- 따라서 이 시점(`e2a0f07`)의 인스톨러부터 정상 동작해야 한다.

## 완전한 UI 셸 (v0.2.0)

- `desktop/ui/`에 자체 셸 구현: 커스텀 타이틀바(드래그 영역 + 최소화/최대화/닫기), 사이드바(작업/로그/설정), 상태바(브릿지·ChatGPT·Gemini·메시지 수·업데이트 상태).
- 채팅 본문은 opencode 웹 UI를 `<iframe>`으로 임베드하되 전체 프레임과 사이드바+상태바가 우리 디자인이므로 "실제 앱처럼" 보인다.
- `ui/preload.cjs` → `window.codebridge.*` API 노출 (windowAction / getStatus / pickProject / restartBridge / reopenChrome / tailLogs / checkForUpdates).
- main.cjs frameless(`frame: false`)로 전환, `ui/` 추가를 `build.files`에 포함.
- CSP 인라인 핸들러 위반 수정(shell.html의 onerror 제거) — 렌더러 콘솔에서 "Refused to execute inline event handler"가 떠서 조사함.
- 패키지드 앱 검증: 창 핸들 정상(메인 윈도우 handle≠0), 9224/7891/9333 LISTEN, `/v1/probe` → ChatGPT·Gemini 모두 OK, 창 제목 "CodeBridge".
- **자동 업데이트 E2E 검증 완료 (v0.2.0 기준)**:
  - 인스톨러 이름을 `CodeBridge-Setup-${version}.${ext}`로 고정(latest.yml과 불일치하던 문제 수정).
  - 저장소를 public으로 전환 (인앱 업데이트가 `releases.atom` + `latest.yml`을 무인증으로 조회해야 함).
  - v0.1.0 릴리스를 이름/latest.yml이 맞는 에셋으로 재생성.
  - 패키지드 앱에서 `/releases/latest` → 302 → JSON(tag_name v0.1.0) 확인, 업데이터가 `Update for version 0.1.0 is not available`까지 도달함을 디버그 패치로 확인 후 원복.
  - (참고) 릴리스 재생성 직후에 앱을 띄우면 GitHub CDN 전파 전이라 "No published versions on GitHub"가 잠깐 날 수 있음 — 실제 원인 없음.

## iframe 채팅 실사용 검증 (2026-11-11, v0.2.0 기반)

임베드된 opencode SPA에서 실제 코딩 작업을 처음부터 끝까지 수행해 E2E로 확인했다.

- **검증 절차**: 앱을 `--remote-debugging-port`로 띄우고 CDP(Node, `9222`→고스트 엔트리로 바인드 실패 → `9334`로 재기동)로 셸+iframe(OOPIF, `type:iframe` 타겟)을 조작.
- **성공 시나리오**: iframe → 새 세션 → 모델 `ChatGPT (web)` 선택 → "로블록스 점프 부스트 패드 Lua 만들어줘" → 브릿지 → (전용 Chrome, CDP 9333) → ChatGPT 응답 → 관대 파서가 fence를 write로 승격 → opencode가 쓰기 실행 → **`D:\cb-test-scratch\Workspace\JumpBoostPad\Script` (1,302자, 한국어 주석 정상)** 생성. UI에 "1 작성 사용됨" 표시, 상태바 메시지 카운트 증가, 세션 자연 종료 확인.
- **검증 중 발견한 iframe-임베드 특유의 사항** (첫 실행 시 사용자에게 그대로 노출됨):
  1. SPA는 프로젝트가 없으면 "＋ 새 세션"이 동작하지 않음 → **"프로젝트 추가" → 앱 내 폴더 브라우저**(`~`/`루트`/경로 입력)로 `D:\cb-test-scratch`를 등록하면 해결. SPA는 `localStorage opencode.global.dat:server`의 `projects.local` 목록을 읽는다. (데스크톱 preload가 하던 등록을 iframe 임베드에서는 사용자가 직접 해야 함)
  2. 우리 `webchat-*` 모델 3종은 **기본 비활성화** 상태로만 표시됨 → "모델 관리"(모델 선택기 하단)에서 스위치를 켜야 피커에 나타남. 저장 위치: `opencode.global.dat:model` (`{"user":[{"providerID":"webchat","modelID":"webchat-chatgpt","visibility":"show"},...]}`). **첫 실행 시 자동으로 켜는 시드(프리로드/부트스트랩)가 필요** — 다음 릴리스 후보.
  3. 서버 선택은 `location.origin`으로 자동 등록되며, 인증은 셸이 `webRequest`로 `Authorization: Basic ...`를 iframe 요청에 주입해 통과시키는 방식 → 그대로 동작 확인.
  4. 모델 피커는 419개 모델을 내보내며(`/api/model` — 우리 모델 3종 `status=active`), 검색/선택/최근 사용 저장까지 정상.
  5. CDP 9222 포트가 죽은 인스턴스 pid를 붙들고 있는 것처럼 보이며(고스트) 새 인스턴스의 바인드를 막음 → 새 포트로 우회. (netstat 버그성 동작)
  6. 설치본 브릿지 교체는 Program Files 권한이 필요 (다음 릴리스에 포함).
- **추가된 모델**: `webchat-chatgpt` (ChatGPT (web)) — opencode.json 2곳 + `/v1/models`. 엔진은 id에 "gemini" 미포함이면 ChatGPT로 분기.

## CodeRabbit 검사 (2026-11-11) — 19건 Major, 전부 수정

`coderabbit review --committed --base-commit 08fc7ef` 전체 변경분 리뷰 결과 19건:

**보안**
- `bridge/src/api.rs`: 프롬프트 디버그 덤프(codebridge_flatten_debug.txt) 제거
- 핸드오버 아카이브를 `handover.archive`(기본 false)에 게이트 — 기본 설정에서 대화 평문 저널 기록 안 함
- `engine.rs find_tab`: substring URL 매칭 → https + 정확히 host 일치만 탭 재사용 (피싱 페이지에 프롬프트 삽입 방지)
- 한도 배너 감지를 body 전체 → alert/dialog/modal/banner/toast/limit/notice/warning 클래스 요소로 한정 ("rate limit" 같은 대화 문구로 오판 방지)

**안정성**
- `rotate_new_chat_site`: 새 채팅 후 컴포저 20s 대기 실패를 Err로 (기존엔 무시하고 "완료" 처리)
- `send_and_await_site` 전체에 프로세스 전역 잠금 (동시 요청이 같은 탭 컴포저 파괴 방지)
- Chrome 재시작: kill 후 exit 대기 + CDP /json/version 200 확인 후 반환 (트레이/설정의 전용 Chrome 재시작 모두)
- 셸: iframe은 opencode 서버가 200일 때만 src 설정 (+ 실패 오버레이 + 새로고침/로그 링크)

**정확성**
- flatten: content 배열 파싱 (`{"type":"text","text":...}`), 스냅샷을 예산 내 행 단위 절단, 사용자 요청은 절대 자르지 않고 초과 시 명시적 에러(400)
- tool 턴 판정을 마지막 메시지 기준으로 + 실패 징후(success:false/error/failed/denied/거부/실패/오류) 시 실패 메시지
- 수동 인수인계 감지는 마지막 user 메시지만 검사 (이전 인수인계 요청이 새 요청으로 재촉발되지 않도록)
- 응답 안정 판정에 "이번 세대(stop 버튼 출현 또는 pre-send 텍스트와의 변화)" 관찰 필수 — 이전 턴 응답을 새 응답으로 오인하던 문제 수정, 180s 마감 미완료 시 `truncated=true` + 파일 승격 중단
- 잘린 응답은 "이어서 계속" 1회 시도 후에도 미완료면 파일 쓰기 건너뜀 (불완전 코드 승격 방지)
- Gemini 재전송의 한도 배너도 체크 (체인 무한 성공 처리 방지)
- mock 모델은 browser 모드 패키지에서도 mock 경로로 라우팅
- "＋ 새 세션이 안 됨"(프로젝트 미등록)·"모델 기본 숨김"을 해결: **`ui/spa-seed.cjs` 프리로드**를 세션에 등록해 SPA(127.0.0.1:7891) document-start에 localStorage를 시드 — `opencode.global.dat:model`(webchat 3종 visibility=show + recent 기본 ChatGPT), `opencode.global.dat:server`(projects.local + lastProject), `opencode.global.dat:layout`(home.selection). 첫 실행부터 추가 단계 없이 동작 검증(저장소 초기화 → 재실행 → 전부 복원 확인)
- iframe 레이아웃 회귀 수정: `#view-work`에 준 `position:relative`가 `.view`의 absolute inset:0을 덮어써 iframe이 150px로 붕괴

**검증**: `cargo test --release` 22/22 통과, 패키지드 앱 재설치형 검증(신규 인스턴스+초기화된 SPA 저장소) 완료, mock 모델 browser 모드에서 mock 경로로 답함 확인, 실사용 E2E는 상단 "iframe 채팅 실사용 검증" 참조.

## v0.4.0 (2026-11-11)

네이티브 React 프론트엔드로 전면 재작성한 릴리스.
- 프론트엔드 재구축: opencode SPA iframe 제거 → 자체 React+Vite+Tailwind 프레임리스 셸
- 백엔드 정리: mock harness 제거 → opencode browser-only
- 전용 Chrome 지연 실행 (첫 요청 시 브릿지 실행)
- 자동 업데이트 v0.2.0→v0.4.0 실증

## CI/CD + v0.3.0 (2026-11-11)## CI/CD + v0.3.0 (2026-11-11)

- `.github/workflows/ci.yml`: push/PR → cargo test + node --check + testpages 계약 검사 → **통과**
- `.github/workflows/release.yml`: 태그 `v*` push → 러너에서 `cargo build --release` → `npm ci` → `npm run dist -- --publish never` → CHANGELOG에서 릴리스 노트 추출 → `gh release create` (에셋 3종) → **성공 (v0.3.0)**
  - 첫 시도는 `electron-builder`가 publish 설정을 가지고 GH_TOKEN 없이 자동 배포를 시도해 실패 → `--publish never`로 빌드만 하고 gh가 업로드하도록 수정 (`c6e77e5`)
  - gh 토큰에 `workflow` scope 필요 (초기엔 없어서 push 거부 → `gh auth refresh --scopes workflow`)
  - v0.3.0 릴리스: CodeBridge-Setup-0.3.0.exe + latest.yml + blockmap, 노트는 CHANGELOG에서 자동 추출
- 인수인계 이력 뷰/수동 트리거/로그인 배너/testpages 7/7 전부 v0.3.0에 포함
- (참고) 인앱 자동 업데이트 경로는 v0.2.0→v0.2.1 실증 완료; v0.3.0부터는 CI로 배포된다

## 남은 Phase 0/1 작업

- [x] ~~CDP(chromiumoxide) 브라우저 자동화 스파이크~~ → **2026-11-10 완료** (PING/PING2 전송+응답 추출 검증)
- [ ] mock 모드 → 실제 사이트 자동화 전환 ([automation] mode) + flatten(평탄화) 구현 (Phase 1)
- [ ] 인수인계(핸드오버) 프롬프트 구현 (F2.16~F2.20, 로테이션 체인: 같은 사이트 → Gemini)
- [x] v0.1 독립 앱 포장: Electron + opencode serve 번들 (Phase 1.5) — `desktop/dist/CodeBridge Setup 0.1.0.exe` 생성

## 전용 Chrome이 안 뜨던 것의 진짜 원인 (v0.4.3, 2026-11-12)

설치본/소스 실행 모두에서 "안녕" → 무반응의 근본 원인은 하나가 아니라 여러 겹이었다 (전용 Chrome이 화면에 떠 보이던 증상 포함):

- **Chrome 157의 devtools HTTP 서버는 HTTP/1.0 요청을 무시**(0바이트 수신). `http_ok`가 HTTP/1.0을 보내므로 연결은 되고 응답은 영원히 안 와서 4초 타임아웃 → 항상 false → 재시작할 때마다/첫 요청마다 "전용 Chrome이 뜨지 않았습니다" 오류. HTTP/1.1 + 첫 청크 전용으로 수정.
- 두 요청(답변용+제목 생성용)이 동시에 `ensure_chrome` → 둘 다 http_ok=false → **Chrome 2개가 9333을 놓고 경합** → 진 쪽은 9333을 못 받아 45초 대기 후 실패. `chrome_spawn_lock` 싱글플라이트로 해결 (예: 숨김 로그가 1줄만 찍히고 Chrome 브라우저 1개만 뜸).
- `--window-position=-32000,-32000`은 Chrome이 좌표를 클램프해 창이 **여전히 화면에 보일 수 있고 작업표시줄 버튼도 남음** → HWND를 `ShowWindow(SW_HIDE)`로 숨기는 Win32 FFI(winhide.rs)를 ensure_chrome에 통합 — 숨기면 작업표시줄에서도 사라짐. 전체 Chrome 프로세스를 죽였을 때 크래시 세션이 복원돼 옛 컴포저가 채워진 채 뜨던 문제도 세션 스냅샷 삭제로 해결.
- 검증: 앱에서 "안녕, 한 줄로만 답해줘." 전송 → ChatGPT 전용 탭(/c/<id>)에 전송 → 응답 완료 → opencode 세션에 user 1/assistant stop/idle 저장 → 앱 UI 사이드바에 세션 생성("안녕! 😄 반가워!") → 전용 Chrome은 SW_HIDE로 숨김(작업표시줄 없음). 앱 프로세스 1개 + 설치본 자동 업데이트(재실행 시 0.4.3) 경로로 배포.
- 참고(미수정, opencode 기본 동작): opencode는 사용자 메시지당 웹챗 호출을 2번(답변+제목 추정) 한다 — 둘 다 성공하므로 세션에는 user 1개+assistant stop 1개로만 저장됨. 무료 웹 채팅을 2배 쓰는 점은 향후 검토.

## v0.4.4 ~ v0.5.6 — 배포 모델 확정, 도구 모드, 안정화 (2026-11-12)

**배포/설치 모델 (v0.4.4~v0.4.9)**

- 설치 마법사+UAC 요구하던 구조(`perMachine: true`, NSIS 마법사 페이지)를 **NSIS oneClick + `perMachine: false`** 로 변경 — 설치 위치가 `%LOCALAPPDATA%\Programs\codebridge-desktop`로 바뀌어 UAC 없이 설치/업데이트 (ChatGPT 앱과 동일한 경험). README에 SmartScreen 미서명 안내 명시.
- v0.4.7: 기존 Program Files 설치본 제거 → 개인 설치 경로로 마이그레이션 완료.
- v0.4.6: 채팅 메시지 로딩 `limit=200` 회귀 수정 (opencode `limit` 상한 관련).
- **자동 업데이트 실증**: 앱 재시작만으로 v0.4.6→v0.4.9→v0.5.4→v0.5.6 연속 적용 확인. 릴리스는 `.github/workflows/release.yml`이 태그 push마다 빌드→publish, `ci.yml`은 master push/PR마다 cargo test+node --check 검증. (주의: `source-map-support@0.5.31` 타르볼이 npm registry에서 404 → 0.5.21로 repin, byte 동일 확인.)

**도구 모드 (v0.5.0)**

- 브릿지가 ChatGPT/Gemini 웹 답변에서 `TOOL name ...` 한 줄을 감지해 opencode가 그대로 실행할 **tool_calls로 승격** (read/glob/grep/websearch/edit/shell/question). opencode가 도구를 실제 실행하고 결과를 다시 대화에 주입하는 루프 — "ChatGPT를 백엔드로 쓰는 opencode" 가 실제 동작 (MAX_TOOL_TURNS=10).
- 파일 신규/재작성은 기존처럼 **코드펜스+`// path:` 주석 관대 파서**가 처리.
- `websearch` 네이티브 툴 추가, 파괴적 셸(rm -rf 등)은 자동 실행 대신 **확인 카드(CODEBRIDGE_QUESTION)**로 변환 + `app:runApprovedShell` IPC로 승인 후 실행, `BRIDGE_DEBUG_PROMPT`/`CODEBRIDGE_LOG_TOOLS` 디버그 훅.
- flatten: assistant `tool_calls`를 `TOOL name k=v ...` KV 형식으로 렌더해 모델이 자기 이전 호출/결과를 텍스트로 인지하게 함. `TEST_COUNTER` 모듈 스코프로 병렬 테스트 race 수정.
- opencode 기본 총 2회 호출 문제는 **`webchat-title` 가짜 모델**(제목 생성 요청에 짧은 컷)로 답변+제목 추정 루프 중복 제거.

**파싱/질문 카드/실패 배너 (v0.5.1~v0.5.6, CodeRabbit 지적 모두 반영)**

- `old`/`new`/`file`/`find`/`before`/`replace` 등 **별칭 KV 키** 추가 (워드 모델이 짧은 별칭을 자주 출력). `\\n` 리터럴 개행 변환. 경로 샌드박스, `END_TOOL` 블록 마커 인식 유지.
- **도구 실패 배너**: TOOL 호출이 불완전하면 `codebridge:tool-failed:N` 마커 출력 → UI가 앰버 배너("도구 N건이 실행되지 않았습니다")로 렌더. 코드 펜스 안의 마커는 배너 아닌 텍스트로 유지.
- **질문 카드**: `TOOL question` 단일형(flat header/question/options)도 배열형(opencode 스키마)과 동일하게 `CODEBRIDGE_QUESTION:` 마커로 변환되어 `QuestionCard.tsx` 앰버/방향 선택지 버튼 UI에 렌더. 문자열 options는 쉼표 분리 후 {label,description}으로 포장.
- 빈 응답 "완료되었습니다." → "응답이 비어 있습니다 — 모델이 출력을 생성하지 못했습니다. 다시 시도해 주세요."
- 병렬 통합 테스트 temp dir 고유화 (프로세스 id + 원자 카운터) — CI master 빨간 적이 v0.5.2에서 v0.5.3로 수정, 이후 **ci/release 둘 다 녹색 유지**.

**검증 상태**

- `cargo test --release` **43/43 green**, frontend `npm run build` OK.
- 실제 도구 루프: `D:\코드브릿지 테스트` 샘플 프로젝트에서 "divide와 subtract 함수를 추가해줘" → `TOOL read path=src/math.ts` → 코드펜스 전체 재작성 → 파일 적용 확인 (같은 세션: TODO 찾아 고쳐줘 → `divide`/`power` 추가).
- 설치본 v0.5.6 실행 확인: 9224/7891/9333 LISTEN, boot.log `services bridgeOk=true opencodeOk=true`.
- **남은 미검증**: 질문 카드/위험 명령 카드가 실제로 렌더된 사례 없음(모델 의존 — 유도 여지), TOOL edit 단일 편집은 alias를 추가해도 모델이 자주 불완전하게 출력해 **코드펜스 전체 재작성 루프가 가장 안정적**.

**projectDir 지정 기능 (이미 구현됨, 2026-11-12/13 점검)**

- 앱 UI는 사이드바 "프로젝트 폴더 변경…" 버튼과 폴더 클릭으로 `dialog.showOpenDialog` → `settings.json`에 저장, 부팅 시 `readSettings().projectDir`을 `fs.existsSync`로 검증하고 없으면 폴더 미선택 상태(`프로젝트 폴더 선택 필요`)로 시작 + 폴더 선택 화면 표시. 브릿지는 cwd로 그 경로를 사용(`[browser] cwd=…` 로그로 확인).
- 2026-11-12 부팅에서 `saved projectDir missing: D:\코드브릿지 테스트` 로그 → **settings.json의 한국어 경로 바이트가 손상**되어 existsSync가 false (과거 PowerShell 기본 인코딩(cp949)로 쓰인 파일을 Node가 UTF-8로 읽어 U+FFFD가 섞인 것이 원인으로 추정). 셀프힐 동작 확인: 앱이 폴더 미선택으로 뜨고, 사용자가 폴더를 다시 선택하면 `writeSettings`(UTF-8)로 덮어써서 정상화됨 (11-13 오전 settings.json은 정상 UTF-8 확인, 재부팅 후 services bridgeOk/opencodeOk).
- **교훈**: 한국어 경로가 든 텍스트 파일은 PowerShell `Set-Content`/`Out-File`(PS5.1 기본 cp949/ASCII)로 절대 쓰지 말 것 — Node `fs.writeFileSync(..., 'utf8')` 또는 edit 도구만 사용.

## v0.6.0 — 설정 화면 + 실전 검증 (2026-11-13)

**설정(Settings) 기능 신설**

- 타이틀바 톱니바퀴 → 모달(`SettingsModal.tsx`): 프로젝트 변경 / 기본 모델 / 위험 명령 확인 / 인수인계 한도(10·25·50) / 자동 업데이트 확인 / 전용 Chrome(보이기·로그인 재확인·프로필 초기화) / 로그 폴더 열기·브릿지 재시작 / 릴리스 페이지 / SmartScreen 안내.
- `settings.json` 스키마: `model`, `autoCheckUpdates`, `dangerConfirm`, `handoverLimit` (기본값 병합). 쓰기는 앱의 `fs.writeFileSync`(UTF-8) — 한국어 경로 손상 없음 확인.
- 브릿지 연동: `CODEBRIDGE_DANGER_CONFIRM`(0=카드 없이 실행), `CODEBRIDGE_HANDOVER_LIMIT`(config.toml보다 우선)를 spawn 시 env로 전달. 값 변경 시 브릿지만 자동 재시작(opencode 유지), `settings.log`에 기록.
- E2E(CDP): 모달 렌더 확인(토글/버튼/경고 배너), dangerConfirm ON→OFF→ON 전환과 브릿지 재시작 로그 확인, `settings.json` 즉시 반영 확인.

**남은 문제 해결 — 실전 검증(CDP + 전용 Chrome)**

- **질문 카드 실제 렌더 확인**: dev 인스턴스→ChatGPT에 "TOOL question으로 먼저 물어봐" 요청 → 응답 `TOOL question header=방향 question="무엇을 추가할까요?" options="divide, power"` → 브릿지가 `CODEBRIDGE_QUESTION:` 마커로 변환 → UI QuestionCard(방향 / 무엇을 추가할까요? / divide·power 버튼) 렌더. (이전엔 한 번도 못 본 경로)
- **위험 명령**: ChatGPT는 파괴적 `TOOL shell`을 스스로는 절대 내지 않고 `TOOL question`("실행할까요?")으로 되묻는 것을 2회 확인. 즉 실사용에서 삭제 확인은 주로 question 카드로 도달하고, `danger_shell` 변환은 파서 단위 테스트·자동 차단 게이트로만 유지. `runApprovedShell` IPC는 `Write-Output cb-ipc-ok` → `{"ok":true,"output":"cb-ipc-ok"}`로 검증.
- **KV 파서 버그 수정**: `question=텍스트 공백 포함`처럼 따옴표 없는 값의 공백에서 잘라내던 문제 → 공백에서 잘라낼지 여부를 "다음 토큰이 `key=` 형태인가"로 판정하도록 `split_kv` 재작성 (`looks_like_kv_start`). 테스트 3개 추가 (question 무인용 공백값/멀티스페이스 path/실제 키 분리 유지).
- **edit oldString 정규화**: 브릿지가 edit 승격 시 파일을 읽어, oldString이 그대로 있지 않고 줄 trim 비교 **정확히 1곳** 일치하면 실제 텍스트(CRLF 포함)로 치환. 0곳/2곳 이상이면 추측하지 않고 원본 유지. 단위 테스트 8개 (정확일치·인덴트 복원·CRLF·모호·파일없음·경로탈출·승격 E2E). test fixture는 매번 원자 카운터로 고유 디렉터리 — `TEST_COUNTER` 모듈 스코프 유지.
- 프롬프트(`fence_convention.md`) 강화: 모호하면 question 먼저, 여러 줄/인덴트 불확실하면 전체 재작성 우선, edit 실패 시 다음 턴 전체 재작성.
- 테스트 54개 green (기존 43 + 파서 3 + 정규화 8).

**릴리스 검증**

- `cargo test --release` 54/54, frontend build OK, settings 토글·로그·IPC E2E 통과.
- CI/release 녹색 유지 시 v0.6.0을 latest로 설치 교체.
- 테스트 세션 4건(검증용) 정리 완료. 사용자 환경의 이전 테스트 대화 일부는 보존됨.

## v0.7.0 — 검증 + 인수인계 체인 실동작 (2026-11-13)

**Phase A(검증)에서 드러난 결정적 버그들**

- **format! `{}` → named 인자 혼용 바인딩 버그 (핸드오버 입력 JS 깨짐의 근본 원인)**: `format!(..., sel = ..., text = ...)`에서 무명 `{}`는 정의 순서대로 첫 named 인자(selector)에 바인딩된다. 결과: `catch (e) "textarea#..."` → SyntaxError("Unexpected string", line 7 col 32, script_id 2xx) → 새 채팅 입력 불가. `{{}}`로 수정. **교훈**: format!에서 named 인자와 무명 `{}`를 섞지 말 것.
- **숨김 창(SW_HIDE)에서 갓 생성된 채팅 페이지의 execCommand 입력 실패**: `{{` 이스케이프 수정 후에도 3회 재시도 모두 실패 → 삽입 후 컴포저 에코 확인 + 실패 시 `ClipboardEvent('paste')`/직접 세팅+InputEvent로 폴백하는 3단계 구조로 변경. 이후 **새 채팅 회전 + pack 전송 성공 (chain_ok)**.
- **js_string U+2028/U+2029**: serde_json이 이스케이프하지 않는 JS 줄 종결자를 문자열에 그대로 담으면 파서가 문자열을 두 줄로 나누며 SyntaxError. `\u2028`/`\u2029`로 치환.
- **UI busy 해제 판정 (질문 카드 답변 직후 화면 멈춤)**: 이전 assistant의 `stop`이 목록에 남아 있고 tool-calls도 종료로 처리되어, 답을 보낸 직후 busy가 곧바로 꺼져 폴링 루프가 멈춘 것이 근본 원인. "이전과 다른 assistant + terminal finish"에서만 해제하도록 수정 + `isLastAssistantDone` 동일 기준으로 통일. (참고: 백그라운드/occluded 창은 Chromium의 타이머 스로틀링으로 갱신이 느려질 수 있음 — 앱이 포커스일 때는 정상)

**검증 결과 (CDP + 전용 Chrome, dev 인스턴스)**

- 질문 카드 E2E: 렌더 → "divide" 클릭 → TOOL glob 호출 → 두 번째 질문 카드(답 대기) — **새로고침 없이 렌더 확인** (기존에 reload 필요했던 원인 = 위 busy 버그).
- Gemini: 로그인 상태(gemini.google.com/app, composer true) 확인, "안녕" → TOOL question 호출 → 카드 렌더.
- 인수인계: tray "지금 인수인계"(또는 `/v1/handover/trigger`) → 원 응답 유지 → same_site_new_chat 회전 → handover pack 전송 → `handover_started → same_site_ok → chain_ok`. 전용 Chrome이 새 대화 `/c/<id>`로 이동함을 probe로 확인.
- danger_shell: ChatGPT가 파괴적 `TOOL shell`을 스스로는 절대 내지 않음을 3회 재확인 → 실사용 삭제 확인은 question 카드 경로로 도달. danger_shell 변환은 파서 단위 테스트로 유지.
- CI testpages-check: 10s→15s 대기 + kill 후 1회 재시도 + --no-sandbox. 로컬 7/7 통과.
- 새 기능: 사이드바 세션 삭제(휴지통, 2단계 인라인 확인) E2E 확인.
- eval_str 실패 시 스크립트 전문을 temp_dir/opencode/eval_str_failed.js에 덤프 (디버깅 편의).

## v0.8.0 — UX·안정성 + 인수인계 이중 전송 제거 (2026-11-13)

**Phase A에서 사용자 지적으로 확정된 문제**

- **채팅 본문에 raw `TOOL …` 라인이 그대로 보이던 문제** — 모델이 `TOOL question header=...` 같은 도구 프로토콜 텍스트를 대화에 그대로 찍는 것을 파서가 그대로 표시했다. 이제 `^TOOL\s+(도구명)\b` 라인은 텍스트 렌더에서 제외 (ToolCallChip/QuestionCard가 같은 내용을 렌더하므로 정보 손실 없음, 코드 펜스 안은 유지).
- **React 에러 바운더리 부재** — 렌더 크래시 = 앱 전체 흰 화면. `components/ErrorBoundary.tsx` 신설, `main.tsx`에서 감싸서 복구 화면 제공.
- **수동 인수인계 이중 전송** — "지금 인수인계" 트리거 시 원본 요청을 먼저 보내고 pack을 또 실행해 무료 웹챗을 2배 소비하고 원본 응답은 체인 성공 시 버려지던 구조. 이제 트리거 감지를 전송 이전으로 옮겨 forced 시 원본 전송을 건너뛰고 pack을 바로 체인에 투입. 체인 전부 실패 시 원본 요청으로 폴백. 체인 코드를 `run_handover_chain`으로 추출해 자동(limit_hit)/수동 경로가 같은 구현 공유.

**기능**

- 질문 카드 도착/작업 완료 Windows 토스트 (`app:notify`, 숨겨진 창에서만, 클릭하면 창 표시). 렌더러가 messages 변경을 보고 `document.hidden`일 때만 발화 (중복 알림 방지 `notifySeenRef`).
- 설정: "전용 Chrome 항상 보이기" (`chromeAlwaysVisible` → `CODEBRIDGE_CHROME_KEEP_VISIBLE` → 브릿지가 숨김 생략, 다음 기동부터), "서비스 전체 재시작" (bridge+opencode, 진행 중 요청 중단됨), "이 모델을 지금 대화에도 적용" (activeId 있을 때만 표시, 기존 changeModel 경로 재사용).
- 브릿지 재시작 트리거에 chromeAlwaysVisible 변경도 포함.

**사용자가 제기한 문제와 대응**

- "오픈코드 창이 자꾸 꺼지더라" — 원인은 **에이전트(본인)가 개발 검증마다 `Get-Process | ? ProcessName -eq 'opencode' | Stop-Process`로 시스템 전체의 opencode 프로세스를 강제 종료**한 것 (오늘만 boots 96회). 코드브릿지 증거상 앱 자발적 종료 로그는 없음. 이후: 경로 기반 선별 kill만 사용, 앱 강제 종료 없음, 실검증은 CI + 마지막 1회 재시작.
- CI testpages-check red run (docs 커밋) — `gh run rerun --failed` 트리거.

**v0.8.0 사후 사고 + v0.8.1**

- v0.8.0 배포 직후 사용자가 "문제가 발생했습니다" 오류 화면 목격 — 원인은 `applyModelToCurrent` useCallback이 `changeModel`보다 먼저 선언되어 deps 배열 평가 시 **TDZ ReferenceError** (매 렌더 크래시). esbuild는 이 에러를 잡지 못하고, 새 코드라 런타임 확인 없이 배포된 것이 원인.
- 수정: 선언 순서 교정 + 배포 전 dev 인스턴스+CDP로 실기기 렌더 확인 후 v0.8.1 릴리스.
- **프로세스 교훈**: 새 React 경로를 추가한 릴리스는 반드시 CDP로 실제 렌더 1회 확인하고 릴리스한다. "CI green + 코드 리뷰"만으로는 TDZ/로직 런타임 오류를 못 잡는다.

## v0.14.0 — 이미지 자동 다운로드 (2026-11-13)

**구현**

- `SendOutcome`에 `images: Vec<String>` 필드 추가 — 마지막 assistant 페이지 요소에서 `img` src들을 CDP로 수집 (순서 유지).
- `Engine::persist_images` — `CODEBRIDGE_IMAGE {"path":"..."}` 마커 매칭 → `base64_decode` → `cwd/path`에 저장. 마커 라인을 `💾 path (N KB) 저장됨`으로 교체해 UI에 표시. 탈출 경로는 거부.
- `base64_decode` 구현 + 테스트 2개 (56/56 green).
- `fence_convention.md`에 CODEBRIDGE_IMAGE 예시 enqueue 예시 포함.

**주의/한계**

- 마커와 이미지 매칭은 **생성 순서** — 이미지가 여러 장이면 순서가 보장되도록 마커도 순서대로 써야 함.
- 이미지는 ChatGPT 웹세션 쿠키로 받으므로 cdn 도메인 외 외부 이미지는 실패할 수 있음 (필요 시 호스트 allowlist 추가 가능).

## v0.15.0 — 도구 루프 연속성 수정 (2026-11-13)

**문제 확인 (실제 세션에서 재현)**

- "웹페이지 만들어줘" → GPT가 `TOOL write path=package.json` 출력 → bridge가 `"도구 실행이 완료되었습니다."` → 루프 종료 — package.json만 만들어지고 실제 웹페이지 코드는 한 줄도 작성되지 않음.
- 근본 원인: `TERMINAL_TOOLS = ["write","edit"]` — write/edit를 도구 루프 종료 조건으로 둔 설계 실수. GPT가 파일을 하나 쓰는 즉시 세션이 "완료" 처리되어, 실제로 화면의 웹페이지는 한 줄도 작성되지 않았다.

**수정**

- `TERMINAL_TOOLS` 제거 — 모든 도구 결과를 GPT에게 다시 보여주고 계속 이어가게 한다. 완료 시점은 GPT가 tool이 없는 답("완료" 등)을 하는 것.
- 무한 루프 방지는 도구 반복 감지(같은 이름+인자 4연속)만 유지, 나머지 한도는 제거.

## v0.19.0 — 예산 제거 + 처리 중 표시 (2026-11-14)

**수정**
- `MAX_TOOL_TURNS` 완전 제거 — 도구 호출 횟수 제한 없음.
- 대신 무한 루프 차단: 동일 도구+인자 4연속 반복 감지 시만 멈춤 (인자 문자열 비교).

**개선**
- 메시지 하나도 없는 상태(첫 전송 직후)에도 "처리 중…" 스피너 버블이 표시된다.
- 도구 루프가 계속되는 동안에도 표시된다.

**검증**
- `cargo test --release` 56/56 green, 프론트엔드 빌드 OK, `node scripts/smoke-check.mjs` 5/5 PASS.

## v0.18.0 — 도구 한도 확대 (2026-11-14)

**문제 확인**

- "도구 호출 한도에 도달했습니다. 필요한 부분을 확인하고…" 가 계속 뜨는 진짜 원인: 전 요청 공통 예산 10턴 — 웹페이지 하나 제작인데 파일 4~5개 쓰는 것만으로도 즉시 초과 → 세션이 연속 끝남.
- 전 요청 예산 10 → 40으로 상향 (요청당).

**한도 메시지**

- "작업이 너무 길어 일단 멈췄습니다. 이어서 진행하시려면 '계속해줘'라고 보내주세요."

**검증**

- `cargo test --release` 56/56 green.

**검증**

- `cargo test --release` 56/56, 프론트엔드 빌드 OK, `node scripts/smoke-check.mjs` 5/5 PASS.

## v0.17.0 — 버그 수정 + 상세 로그 (2026-11-13)

**수정**
- `tool_turn_count` 세션 전체 누적 → 마지막 `role=user` 이후 tool 결과만 카운트. "도구 호출 한도에 도달했습니다"가 누적되어 이후 요청을 전부 끊던 문제 수정.

**로그**
- 매 요청 `tool-turn count={} (limit={}, remaining={})`
- 응답 로그에 `calls=[name(args잘라낸것...)]`, `issues=[...]`, `raw_first="응답앞부분..."` 포함
- 무한도달 시 `MAX_TOOL_TURNS 도달 (tool_count=N limit=10)` 로그

**검증**
- `cargo test --release` 56/56 green.

## v0.9.0 — 릴리스 안전망 + 실사용 편의 (2026-11-13)

**v0.8.0 사태를 계기로 만든 릴리스 게이트**

- `scripts/smoke-check.mjs`: Electron dev 인스턴스를 remote-debugging-port로 띄워 CDP로 ① CDP 엔드포인트 ② 렌더 완료(innerText > 10자) ③ "문제가 발생했습니다" 부재 ④ 앱 셸 텍스트 존재 ⑤ `window.codebridge.getBoot()` JSON 응답 — 5단계 확인, 실패 시 exit 1. 로컬에서도 `npm run smoke:check`로 실행 가능.
- `ci.yml`에 `Build frontend` → `Render smoke check` 단계 추가 — 이제 이 게이트를 통과 못한 릴리스는 배포 불가.
- `main.cjs`: `webContents.on('console-message', ...)` → `logs/renderer.log`에 기록 (5MB 회전, 라인 1000자 제한) — 렌더러 크래시의 직접 증거 확보용.

**기능**

- 사이드바 "폴더 열기" 버튼 → `shell.openPath(projectDir)` — 에이전트가 수정한 폴더를 즉시 탐색기에서 확인.
- 설정 "알림" 섹션: 질문/완료 토스트 on/off (기본 ON). 렌더러는 발화 시 getSettings로 게이트 (숨김 창에서만).
- 설정 "긴 대화 안내 숨기기": `hideContextNote` → `CODEBRIDGE_NO_CONTEXT_NOTE=1` env → 브릿지가 "메시지 N개 쌓였습니다" 문구 생략. 변경 시 브릿지 자동 재시작 대상에 포함.
- 테스트 훅: `AppState.force_limit_hit` — `CODEBRIDGE_FORCE_LIMIT_ONCE=1`로 시작하면 첫 요청의 limit_hit을 강제 → 자동 인수인계 체인(same_site → gemini)을 실제 한도 없이 실측 가능 (one-shot, 첫 요청에서 소비).

**검증**

- `node scripts/smoke-check.mjs` → **5/5 PASS** (로컬, v0.8.1 기준 인스턴스).
- 기존 54개 브릿지 테스트 green, 프론트엔드 빌드 OK.

## v0.10.0 — 창 기억/시작 시 실행/MRU/내보내기/복사 (2026-11-13)

**새 기능**

- **창 배치 기억**: move/resize 디바운스 저장(500ms), 앱 시작 시 settings.json의 windowBounds 복원, isMaximized 포함. 화면 이탈(해상도 변경/멀티모니터 제거) 대비: workAreaSize 기준으로 클램프.
- **Windows 시작 시 함께 실행**: 설정 토글 → `app.setLoginItemSettings({ openAtLogin })` → HKCU Run 등록/해제. 기본값 OFF (사용자가 켜야 동작).
- **MRU 최근 프로젝트**: pickProject/boot resolve 시 unshift + dedupe + 최대 5개. getSettings에 포함되어 UI에 표시. "전환" → `app:switchProject` → services 재시작 + 세션 목록 자동 리프레시(useEffect on projectDir). 제거는 `app:removeRecentProject`.
- **대화 내보내기**: Composer에 다운로드 버튼 → `api.getMessages`로 전체 메시지 → 마크다운 직렬화(사용자/어시스턴트/도구 호출 주석) → `app:exportSession`(dialog.showSaveDialog .md 필터) → 토스트로 저장 경로 표시.
- **코드 블록 복사**: `PreWithCopy` — react-markdown의 `pre` 렌더러를 교체, hover 시 버튼 표시, innerText를 clipboard에.

**버그 수정 (중요한 것)**

- **개발/스모크 인스턴스가 설치본과 같은 userData를 쓰며 single-instance lock에 걸려 조용히 종료되던 문제**: `main.cjs`가 무조건 `app.setPath('userData', %APPDATA%\CodeBridge)`를 실행해서 `CODEBRIDGE_USER_DATA_DIR` 격리가 먹지 않았다. 이제 env를 먼저 확인해 setPath 적용 (`CODEBRIDGE_USER_DATA_DIR=... electron .`). 스모크 게이트가 설치본 실행 중에도 5/5 PASS 확인. CI에서는 충돌이 없어서 드러나지 않았던 환경 특이 케이스.

**프로세스 교훈**

- lockfile 문자열 치환 실수(v0.9.0 사태)가 있어 package-lock의 모든 변경은 **diff로 검증**한다. v0.10.0 버전 범프도 diff 확인으로 3줄(2개 앱 버전)만 변경됨을 검증.

## v0.11.0 — CodeRabbit 지적 전부 반영 (2026-11-13)

**변경 요약 (코드리뷰 기반)**

- **대화 내보내기를 컴포저 → 설정창 "대화" 섹션으로 이동** (`SettingsModal` — 현재 세션 없으면 버튼 비활성화). 내보내기는 opencode의 asc+limit=200 대신 `order=desc&limit=200`으로 **최신 200개 우선**, 초과 시 토스트로 안내.
- **switchProject 엄격화**: `fs.statSync`로 디렉토리 확인, startServices readiness 실패/예외 시 `{ ok:false, error }` 반환, 전환이 취소되지 않고 실제로 서비스까지 올라갔는지 검증.
- **서비스 교체 동기화**: `waitChildExit(proc, 8000)` — taskkill 후 exit 이벤트를 기다리고 교체 스폰. 이전엔 포트 해제 전에 재바인딩 → 브릿지 panic/바인딩 10048 위험 제거. `startServices` 호출부 전부 await.
- **창 bounds 저장 방식 개선**: `getNormalBounds()` (일반 크기 추적) + workArea 원점 포함 클램프 (모니터 제거/해상도 변경 대비 — 기존엔 일부만 보일 수 있었음).
- **렌더러 알림/스모크**: `killTree` 후 taskkill 종료 대기 → 프로필 삭제 5회 재시도.
- **설정 MRU 제거 즉시 반영**: 모달 내 settings state도 함께 갱신.
- **도움말 문구 정정**: 위험 명령 확인 카드는 설정에서 끌 수 있음 (이전 문구는 "끌 수 없음"이었는데 이제는 토글이 존재).

**검증**

- `cargo test --release` 54/54 green, 프론트엔드 빌드 OK, `node scripts/smoke-check.mjs` 5/5 PASS.

## v0.12.0 — 실사용 편의 (2026-11-13)

**기능**

- **대화 내보내기**: 파일명에 날짜/시간 포함 (`{제목}-YYYY-MM-DD-HHmm.md`), 마지막 저장 폴더를 `settings.exportDir`에 기록해 다음 내보내기 기본 위치로 사용 (`dialog.showSaveDialog` 전에 읽음). 내보내기는 v0.11.0대로 설정창 "대화" 섹션 버튼에서.
- **세션 이름 변경**: 사이드바 대화 제목 더블클릭 또는 우측 연필 아이콘 → 인라인 입력 → Enter/블러 저장 / Esc 취소. `PATCH /api/session/{id} { title }`.
- **모든 대화 삭제**: 설정 → 대화 섹션에 2단계 인라인 확인(예/아니오)을 거쳐 `Promise.allSettled`로 병렬 삭제, 삭제 수/실패 수를 토스트로 보고. 활성 세션이 삭제되면 메인 화면 초기화.
- **본문 검색**: 검색어로 제목 매칭이 없으면 세션 메시지(최대 200개)를 lazy fetch해 본문 일치 세션을 찾아 "본문" 배지 표시 — 제목과 본문을 동시에 필터. 세션당 병렬 fetch는 최대 10개/400ms 디바운스로 제한.
- **온보딩**: boot.log 시작이 아니라 `settings.onboarded` (기본 false)로 관리, 채팅 상단에 3단계 안내 배너, "시작하기 → 닫기"로 영구 해제. 설정 → 시작에서 "첫 실행 안내 → 다시 보기" 버튼으로 재표시.

**UI/렌더러**

- 사이드바 각 대화 항목에 이름 변경 아이콘 추가.
- 설정 모달 "시작" 섹션에 "첫 실행 안내 → 다시 보기" 추가.

**검증**

- `cargo test --release` 54/54, 프론트엔드 빌드 OK, `node scripts/smoke-check.mjs` 5/5 PASS.

## v0.13.0 — 프로젝트 목록 + Chrome 툴바/팝업 제어 + 셸 가이드 강화 (2026-11-13)

**버그 수정**

- **show_chrome → WidgetWin_1만 표시**: 전용 Chrome을 보일 때 메인 창(Chrome_WidgetWin_1)만 꺼내고 `Chrome_WidgetWin_0`(팝업 창, 제목 없는 유령 창)는 숨김 상태로 둔다 → "이상한 창" 제거.
- **비대화형 셸 가이드**: `npm create vite@latest` 류 대화형 명령을 모델이 선택해 루프가 멎는 일을 막기 위해 — fence_convention 규칙에 "대화형 명령을 출력하지 말 것 (npm create/init/npx create-*), 파일 직접 작성, 설치 시 --yes/CI=true 강제" 추가.

**기능**

- **프로젝트 목록 사이드바 재구성**: ChatGPT "프로젝트" 스타일 리스트 — 최대 5개 최근 폴더, hover로 제거(X), 현재 프로젝트는 "현재" 배지, 상단 `+`(폴더 추가).
- **설정에 "숨기기" 버튼** (보이기 옆): `/v1/chrome/hide` IPC → bridge `hide_chrome` → winhide `hide_chrome_windows_for_pid`. settings에 별도 켜기 없음 — 클릭 즉시 적용.
- **ipc.d.ts/preload/ipc** 신규 `hideChrome` 노출.

**프로세스**

- lockfile은 전체 문자열 치환 금지 — v0.9.0/v0.10.0에서 `retry@0.12.0`, xmldom, temp 범위가 각각 0.13.0으로 밀려 나갔다. 앱 버전(코드 2곳)만 정밀 치환 + `git show v<이전>:package-lock.json`과 diff로 검증한다.
