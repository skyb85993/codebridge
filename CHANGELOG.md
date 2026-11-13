# 릴리즈 노트

## v0.8.1 (2026-11-13)

긴급 수정 (v0.8.0 렌더링 크래시):

- **`applyModelToCurrent`가 `changeModel`보다 먼저 선언되어 `useCallback` deps 배열 평가 시 TDZ ReferenceError** → 앱이 매 렌더마다 죽던 것을 순서 교정으로 수정. v0.8.0에 잘못 배포된 릴리스입니다
- 배포 전 실기기 렌더 확인 완료 (dev 인스턴스 + CDP) — "무엇을 만들까요?" 화면 정상 렌더 확인

## v0.8.0 (2026-11-13)

UX·안정성 릴리스:

**UI**
- **`TOOL …` 원시 라인을 채팅 본문에서 숨김** — ToolCallChip/QuestionCard만 렌더 (코드 펜스 안은 그대로 유지). 모델이 찍은 `TOOL question header=...` 같은 생 텍스트가 화면에 남지 않습니다
- **React 에러 바운더리** — 렌더 크래시 시 흰 화면 대신 "일시적인 오류가 발생했습니다" + 다시 불러오기/로그 폴더 열기
- **질문 카드/작업 완료 Windows 알림** — 앱이 숨겨져 있는 동안 답변을 기다리거나 작업이 끝나면 토스트 (클릭하면 창 표시)

**브릿지**
- **인수인계 수동 트리거 이중 전송 제거** — "지금 인수인계" 시 원본 요청 대신 handover pack을 바로 보내고 그 결과를 응답으로 사용. 예전에는 원본+pack 둘 다 소비했습니다. 체인 전부 실패 시 원본 요청으로 폴백
- 인수인계 체인 로직을 `run_handover_chain`으로 추출해 자동/수동 경로가 같은 코드를 공유

**설정**
- "전용 Chrome 항상 보이기" 토글 (다음 Chrome 기동부터 적용)
- "서비스 전체 재시작" 버튼 (브릿지+opencode, 진행 중인 요청은 중단됨)
- "이 모델을 지금 대화에도 적용 →" — 기본 모델 변경을 진행 중인 세션에도 적용
- Chrome 보이기 설정 변경도 브릿지 자동 재시작 대상에 포함

**CI**
- testpages-check Chrome 기동 실패 시 kill 후 1회 재시도 + 대기 15초 + --no-sandbox (master의 red run도 재실행 완료)

## v0.7.0 (2026-11-13)

검증·안정화 릴리스 — 인수인계 체인을 실제로 동작시키고, 진행 중인 답변이 화면에 나타나지 않던 버그를 잡았습니다:

**결정적 버그 수정**
- **핸드오버 입력 JS 깨짐 수정** — Rust `format!`에서 named 인자 `{sel}` 다음에 온 무명 `{}`가 첫 번째 named 인자(selector)로 바인딩되어 `catch (e) "textarea#prompt-textarea..."` 형태가 되던 것을 `{{}}`로 이스케이프 수정. (숨김 창 새 채팅에서 입력이 불가능하던 근본 원인)
- **숨김 창 입력 3단계 폴백** — `execCommand('insertText')` → `ClipboardEvent('paste')` → 직접 세팅+`InputEvent` 순서로 시도하고, **삽입 후 컴포저 내용을 읽어 실제로 반영됐는지 확인**(최대 3회 재시도). 새 채팅 직후 입력이 적용되지 않던 문제 해결
- **`js_string` U+2028/U+2029 이스케이프** — JSON이 이스케이프하지 않는 JS 줄 종결자를 그대로 넘기면 문자열이 중간에서 끊겨 SyntaxError가 나는 고전적 함정 수정
- **UI busy 판정 버그** — assistant `tool-calls`(루프 중간)와 이전 assistant의 `stop`(직전에 보낸 질문의 종료)을 종료로 오인해 폴링 루프가 멈추던 문제 수정. 이제 "이전과 다른 assistant의 terminal finish"에서만 해제 → 질문 카드에 답한 뒤에도 새 답변(도구 호출 포함)이 **새로고침 없이** 렌더됨. 백그라운드 스로틀링 환경에선 지연될 수 있음

**기능**
- 사이드바 세션 삭제 버튼 (호버 시 표시, 인라인 2단계 확인) + `api.deleteSession` (`DELETE /api/session/{id}`)

**검증 (실측)**
- **질문 카드 전체 루프** — 카드 렌더 → "divide" 클릭 → `TOOL glob` 호출 → 두 번째 질문 카드 자동 렌더 (새로고침 없이)
- **Gemini 실측** — 로그인 상태 확인, `TOOL question` 호출 → 카드 렌더 (ChatGPT와 동일 경로)
- **인수인계 체인** — 수동 트리거 → 원 응답 + 새 채팅 자동 회전 + handover pack 전송 → `chain_ok` (전용 Chrome이 새 대화로 이동)
- danger_shell 확인 카드는 모델이 자발적으로 호출하지 않음을 3회 확인 (실사용은 question 카드로 도달); 파서 변환은 단위 테스트 유지

**인프라**
- `testpages-check` flake 완화: CDP 기동 대기 10초→15초 + Chrome kill 후 1회 재시도 + `--no-sandbox`

## v0.6.0 (2026-11-13)

설정 화면 + 남은 문제 해결 릴리스:

**설정 (타이틀바 톱니바퀴 → 모달)**
- 새 대화의 기본 모델 (ChatGPT/Gemini), 위험 명령 확인 토글(기본 ON, OFF 시 앰버 경고), 인수인계 스레드 한도 10/25/50(브릿지에 `CODEBRIDGE_HANDOVER_LIMIT` env로 전달), 자동 업데이트 확인 토글 + 수동 "지금 업데이트 확인", 전용 Chrome 보이기/로그인 재확인/프로필 초기화(백업으로 이동), 로그 폴더 열기, 브릿지 재시작, 릴리스 페이지, 버전/SmartScreen 안내
- 설정은 `settings.json`(UTF-8)에 저장, 위험 명령/인수인계 변경 시 브릿지만 자동 재시작 (opencode 유지). `settings.json`은 한국어 경로도 정상 저장 확인

**남은 문제 해결**
- **질문 카드 실전 검증 완료** — CDP로 ChatGPT에 강제 유도 요청 → `TOOL question` → `CODEBRIDGE_QUESTION:` 마커 → QuestionCard 렌더 확인 (divide/power 선택지)
- **위험 명령 확인 실전 검증** — ChatGPT는 파괴적 `TOOL shell`을 스스로 내지 않고 `TOOL question`("실행할까요?")으로 먼저 물어보는 것을 확인. 파서의 `danger_shell` 변환 경로는 단위 테스트로 유지
- **`TOOL question` 값에 공백+따옴표 없는 경우**: KV splitter가 다음 토큰이 `key=` 형태일 때만 자르도록 수정 — `question=무엇을 추가할까요`처럼 공백 든 값도 보존됨
- **edit oldString 정규화**: 파일을 읽어 oldString이 파일에 그대로 있지 않으면 줄 단위 trim 비교로 **정확히 1곳**만 일치할 때 실제 텍스트로 치환 (CRLF 보존, 모호하면 추측 안 함) — 모델이 인덴트를 틀려도 edit 승격 성공
- `fence_convention.md` 강화: 모호하면 question 먼저, 여러 줄 수정은 전체 재작성, edit 실패 시 다음 턴 전체 재작성, 되돌리기 어려운 명령은 shell 확인 카드
- `CODEBRIDGE_DANGER_CONFIRM=0`이면 브릿지가 파괴적 명령을 확인 카드 없이 그대로 실행 (설정 토글과 연동)

**검증**: cargo test 54개 green, frontend build OK, E2E(CDP)로 설정 모달 토글·로그 파일 기록·질문 카드·runApprovedShell IPC(`Write-Output` → `{"ok":true,"output":"cb-ipc-ok"}`) 확인

## v0.5.6 (2026-11-12)

CodeRabbit 리뷰 지적 모두 수정:

- **단일형 `TOOL question header=... question=... options="A,B"` 에서 header/question/options 필드가 모두 마커로 실제 전달** — 문자열 옵션은 쉼표 분리 → `{label,description}` 객체로 변환 (이전에는 header="질문", options=[]로 떨어져 모델이 보낸 정보를 UI가 못 받았음)
- 병렬 테스트 race 수정에서 **공유 TEST_COUNTER로 이동** — 두 prompt-build 테스트가 같은 카운터를 써서 `cb_flat_<pid>_0` 충돌 없이 실행
- 테스트 추가 1개 (배열형 question 스키마 마커 전달됨) — 총 43 green

## v0.5.5 (2026-11-12)

도구 확대 업데이트:

- `TOOL websearch {"query":"..."}` — 웹 검색 도구를 TOOL로도 호출 가능 (코드 조정 도움 등)
- 테스트 커버리지 7개 추가 (총 42개 green) — websearch/glob/grep/edit 등 모든 TOOL 라인 승격 확인:
  websearch, glob, grep, edit (oldString/alias 포함), question 라인, shell alias `cmd=`
- fence_convention.md: websearch 예시 추가

## v0.5.4 (2026-11-12)

npm registry에서 `source-map-support@0.5.31` 타르볼이 내려왔던 CI 실패 복구:

- `node_modules/source-map-support`를 0.5.21(0.5.31과 byte 동일)로 재고정 — lockfile 다시 설치 성공 확인

## v0.5.3 (2026-11-12)

CI 마스터 통합 테스트 한 건 실패로 인한 안정화 릴리스:

- 병렬 실행 시 같은 프로세스 id + 같은 temp_dir 이름이 공유되어 실패하던 `build_contains_header_and_user_request` 테스트 수정 — 프로세스 id + 원자적 카운터로 디렉터리 고유화

## v0.5.2 (2026-11-12)

CodeRabbit 리뷰(v0.5.0 이후 누적분, 모든 지적 수정 완료):

- 빈 응답이 "완료되었습니다."가 아니라 "모델이 출력을 생성하지 못했습니다. 다시 시도해 주세요." 로 표시됨 (`api.rs`)
- 코드 펜스 안의 `codebridge:tool-failed:N` 마커는 배너가 아닌 코드 텍스트로 렌더됨 (`MessageRow.tsx` 펜스 추적)

## v0.5.1 (2026-11-12)

하드닝 + 피드백 개선:

- **도구 호출 실패 안내 UI**: TOOL 호출이 완전히 실행되지 못하면 `codebridge:tool-failed:N` 마커가 텍스트에 추가되고, 앱이 이를 `! 도구 N건이 실행되지 않았습니다` 배너로 렌더합니다
- **parser: 도구 호출 aliases 추가** — `old`/`new`/`find`/`before`/`file`/`filepath` 등을 받아 `oldString`/`newString`/`path`로 정규화합니다 (워드 모델이 짧은 별칭을 더 자주 출력합니다)
- **`\n` in argument values**: 모델이 `\n` 문자를 리터럴 개행으로 변환합니다
- flatten: 조립 후 디버그용 `BRIDGE_DEBUG_PROMPT=/path`를 강제할 때 마지막에 환경변수 참조해서 완성 프롬프트를 파일로 덤프 (성능에 영향 없음)
- Cargo 35개 테스트 green, 엄격 버전 미적용된 JSON이 빈 문자열이면 `완료되었습니다.`로 답변합니다

## v0.5.0 (2026-11-12)

**전체 도구 모드** — 코드브릿지가 코드 에이전트로서 파일/셸/질문 도구를 실제로 사용합니다.

- 코드 수정 시 **모델이 `TOOL read/edit/glob/grep/shell` 한 줄씩 사용** — 브릿지가 해당 도구를 OpenAI tool_calls로 승격하고, opencode가 직접 실행 — 결과를 프롬프트에 넣어 모델이 다음 단계를 결정
- **KV형 인자 형식** `TOOL edit path=a.ts old="hello" new="bye"` 와 JSON형 `TOOL read {"path":"a.ts"}` 둘 다 지원
- **질문 도구**(proactive question): 모델이 방향을 물으면 UA된 대로 "어떻게 하지?" 카드로 렌더링하고 "선택: …" 메시지로 응답
- **파괴적 명령 가드**: `rm -rf`, `dd`, `mkfs`, `Shutdown`, `reg delete`, `format`, `diskpart`, `schtasks`, `bcdedit` 등이 감지되면 실행 대신 **사용자 확인 카드**로 변형 — 사용자가 명시적으로 승인해야만 앱이 직접 실행
- **다단계 도구 루프**: read→edit→verify 최대 10회까지 한 턴에 여러 도구 사용 가능
- **`webfetch` 권한 복원**: shell/read/glob/grep/edit/write/question/skill/webfetch 모두 활성
- **효율성 개선**: 단일 API 호출로 제목 생성 인터셉트, 다단계 대화 창 기록 지원
- **Opencode 세션 표시**: 사이드바에 메시지 타입 tool-calls 표시
- **코드 사이즈 축소**: flatten 프롬프트에서 대화 구조를 넣어 모델이 자신의 출력을 보고 이어가게 함
- `BRIDGE_DEBUG_PROMPT=/path` 설정하면 전송한 프롬프트를 파일로 덤프 (디버그용)
- Cargo 단위 테스트 35개 — TOOL 호출 파서/진단/경로 샌드박스/파괴적 명령 감지 모두 포함

## v0.4.9 (2026-11-12)

CodeRabbit 리뷰(v0.4.6 이후 누적분, 6건)를 모두 반영한 하드닝 릴리스:

- **전용 Chrome 프로세스 식별 정확화**: WMI 폴백이 `chrome-profile` 부분문자열만 검사하던 것을 정확한 `--user-data-dir=...CodeBridge\chrome-profile` 경로로 일치시키고, **pid 파일/스크린샷 모두 사용 전 `chrome.exe` + 정확한 프로필 경로인지 검증** (Windows PID 재활용으로 인한 무관한 Chrome 종료 방지)
- **`winhide.rs` 전역 상태 경주 해결**: `TARGET_PID`/`ACTION`/`DONE_COUNT` 전역 변수를 뮤텍스로 직렬화 (hide/show 동시 호출 시 핸들 콜백이 뒤섞이던 문제)
- **`show_chrome_visible`을 hide와 동일한 `chrome_spawn_lock`으로 직렬화**하고 이중 체크 — 두 show 요청이 동시에 도착해도 Chrome 1개만 뜨고 pid 파일 갱신도 한 번만 일어남
- `app:showChrome` 타임아웃 20s → 60s (콜드 스타트 상한 45s보다 여유 있게)
- **사이드바 로그인 배너 실패 표시**: `showChrome()` 결과가 실패면 배너에 에러를 표시하고, 버튼 라벨을 고정 timeout이 아닌 실제 promise 완료로 관리

## v0.4.8 (2026-11-12)

중요 수정/개선:

- **메시지 1개당 ChatGPT 2회 호출되던 문제 해결** — opencode의 숨겨진 `title` 에이전트가 메인 모델로 폴백되어 웹 채팅을 두 번 부르던 것. 브릿지에 `webchat-title` 가짜 모델을 추가하고 사용자 요청의 첫 줄을 제목으로 즉시 응답하도록 처리 (ChatGPT를 부르지 않음). 템플릿/사용자 프로젝트의 `opencode.json`에 `small_model: "webchat/webchat-title"` 추가
- **로그인 필요 시 전용 Chrome을 바로 꺼낼 수 있는 배너 + 버튼** — 사이드바에 "웹 채팅 로그인이 필요해요 / 전용 Chrome 열기" 버튼 (probe가 login_required일 때 표시). 버튼은 pid 파일→tasklist→WMI 순으로 전용 Chrome을 찾아 `SW_SHOW`+화면 안으로 이동시킴 (없으면 보이는 창으로 새로 실행)
- **앱을 끄면 숨겨진 Chrome도 함께 종료** — `before-quit`에서 pid 파일/ WMI로 전용 Chrome 프로세스를 taskkill. 기존 Chrome은 그대로 유지
- 전용 Chrome spawn 시 pid를 `%APPDATA%CodeBridge\dedicated-chrome.pid`에 기록 (show/kill의 기준)
- `GET /v1/chrome/show?pid=N` — pid 기반 show 엔드포인트 추가

## v0.4.7 (2026-11-12)

설치/업데이트를 ChatGPT 앱처럼 — 설치 마법사를 완전히 제거:

- **NSIS oneClick 설치로 전환** — 마법사/진행 페이지 없이 조용히 설치·업데이트 (업데이트는 "업데이트 버튼 → 다운로드 → 재시작"으로 완료, 아무 창도 뜨지 않음)
- **개인 사용자 설치로 전환** — `%LOCALAPPDATA%\Programs\CodeBridge`에 설치되어 관리자 승인(UAC)도 사라짐. 기존 Program Files(머신) 설치는 제거하지 않으면 중복되므로 수동 제거 안내
- **창을 닫으면 앱이 백그라운드에 남지 않고 완전히 종료** — 기존 "트레이에 백그라운드 유지" 동작 제거
- `bridge.log`가 5MB를 넘으면 다음 실행 시 자동으로 `bridge.log.1`로 회전 (크로미엄 WS 경고 스팸으로 21MB까지 늘어나던 문제 완화)

## v0.4.6 (2026-11-12)

중요 수정 — 채팅 화면에 메시지가 전혀 뜨지 않던 문제:

- **`api.getMessages`의 `limit=300`이 opencode의 최대 `limit`(200)을 초과**해 `GET /api/session/:id/message`가 **HTTP 400**으로 거부됐고, 응답이 항상 `[]`로 간주되어 채팅/스트리밍 기록이 전부 보이지 않았음 → `limit=200`으로 수정. 서버에는 정상적으로 저장되고 있었으므로 이제 히스토리/답장이 모두 보임.
- 서버의 실제 파라미터 제한은 `/api/session?order=desc&limit=60`(목록, OK) 기준도 200이므로 동일하게 검토함.

## v0.4.5 (2026-11-12)

설치 마법사를 Hermes 스타일 다크 디자인으로 전면 개선:

- **환영 페이지**: 다크 네이비 배경 + 밝은 타이틀("CodeBridge 설치") + 불릿 형태 기능 소개(ChatGPT·Gemini 무료 웹 채팅 / 파일은 프로젝트 폴더에 / 전용 Chrome 숨김 / 자동 업데이트) — `MUI_WELCOMEPAGE_TITLE`/`MUI_WELCOMEPAGE_TEXT`
- **설치 진행 페이지도 다크 배경으로**: `MUI_INSTFILESPAGE_COLORS`로 로그/프로그레스 영역 배경을 다크 네이비로 (기존 흰색)
- **마침 페이지**: "설치가 완료됐어요 / 마침을 누르면 앱이 바로 실행됩니다" 커스텀 문구 + 다크 배경/텍스트
- 설치 마법사는 앱의 온보딩·업데이트 배너와 같은 딥 네이비(#0F1626) 팔레트로 통일

## v0.4.4 (2026-11-12)

설치 마법사를 다크 브랜드로 개선:

- **설치 옵션 선택(모든 사용자/전용) 페이지 제거** — `perMachine: true`로 강제(기존에도 `C:\Program Files\CodeBridge`에 머신별 설치되어 있었음)
- **폴더 선택 페이지 제거** — `allowToChangeInstallationDirectory: false`
- **환영 페이지 추가** — 다크 네이비 배경 + 밝은 텍스트 + 풀블리드 브랜드 사이드바(로고/단어마크/태그라인 "무료 AI 코딩 에이전트")
- 사이드바/헤더 이미지 전면 재제작(다크 네이비 + 시안→민트 글로우), 언인스톨러 마법사도 동일한 다크 브랜드 적용 (`uninstallerSidebar`)
- `installer-brand.nsh`로 `MUI_BGCOLOR`/`MUI_TEXTCOLOR` 다크 테마 + `customWelcomePage` 매크로 주입 (헤더/진행 페이지 비트맵 유지)

## v0.4.3 (2026-11-12)

중요 수정 — 전용 Chrome이 응답하면 다음 요청이 정상 동작하도록 브릿지 수정:

- **Chrome 157의 devtools HTTP 서버가 HTTP/1.0 요청에는 응답하지 않는 문제** — `http_ok`가 HTTP/1.1 + 첫 청크만 읽도록 변경. 기존에는 Chrome이 떠 있어도 `read_to_end`로 영원히 기다려 4초 타임아웃 → 항상 "전용 Chrome이 뜨지 않았습니다" 오류가 나던 근본 원인.
- **전용 Chrome 실행 race** — 두 요청(답변용/제목 생성용)이 동시에 도착하면 동시에 Chrome을 띄워 디버그 포트(9333)를 두고 경합해 한쪽이 45초 대기 후 실패하던 문제 — `chrome_spawn_lock` 싱글플라이트로 직렬화.
- **전용 Chrome 창이 작업표시줄/창 목록에 남던 문제** — 화면 밖 좌표로 띄우던 기존 방식은 Chrome이 좌표를 클램프해 창이 보일 수 있으므로, `ShowWindow(SW_HIDE)`로 Chrome 브라우저 프로세스의 보이는 창을 숨기도록 변경. 숨기면 작업표시줄에서도 사라진다.
- 전용 Chrome을 새로 띄울 때 크래시 세션 스냅샷(`Default/Sessions*`)을 삭제해 옛 컴포저에 채워진 채 멈춰 있던 탭이 복원되지 않도록 수정.
- `find_tab`이 페이지 매칭까지 최대 12초 재시도 후에만 새 탭을 만들어 동일 chatgpt.com 탭이 중복 생성되던 문제 수정.
- 전용 Chrome 기동 대기 20초 → 45초 (콜드 스타트 여유).

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

