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
