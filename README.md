# CodeBridge

무료 웹 채팅(ChatGPT / Gemini)을 "모델"로 사용하는 코딩 에이전트 시스템.
하네스는 오픈코드를 재사용하고, 이 저장소에는 **웹 채팅 브릿지(Rust)**와 **CodeBridge Desktop(Electron)** 앱이 들어간다.

> 상세 계획 문서는 워크스페이스 밖(`.opencode/plan`)에 있다. 저장소에는 코드만 포함한다.

## 현재 상태 (v0.3.0)

- [x] 브릿지(browser 모드): 실 ChatGPT/Gemini 자동화 — flatten, CDP 전송/감지/추출, 관대 파서, 아카이브, 한도 감지, 인수인계 체인
- [x] 관대 파서: 코드펜스+경로 주석 → write tool_calls 승격 (유닛 테스트 22개 포함)
- [x] E2E(실 ChatGPT): 자연어 요청 → 응답 → 파일 생성 (`GameManager.lua`, `HUD.client.lua`, 점프 패드 Lua 등)
- [x] 인수인계 체인: 한도 도달/수동("지금 인수인계" 버튼) → 인계 팩 생성 → 같은 사이트 새 채팅 → Gemini 로테이션 (실증 완료)
- [x] **CodeBridge Desktop**: 프레임리스 커스텀 셸(타이틀바/사이드바/상태바 + iframe 내 opencode 웹 UI), 전용 Chrome 자동 실행(9333), 자동 업데이트(electron-updater + GitHub Releases, v0.3.0부터 CI 자동 릴리스)
- [x] 인수인계 이력 뷰 + 로그인 필요 배너 + 첫 실행 자동 시드(webchat 모델 표시/프로젝트 등록)
- [x] testpages 회귀 자동화 (`node scripts/testpages-check.mjs`, 7/7)
- [ ] Roblox Studio 실행으로 생성물 검증 (사용자가 실사용으로 수행 중)

## 빠른 시작

```powershell
# 개발 모드 (브릿지 + 앱)
cd codebridge\desktop
npm install
npm start        # 이후 dist\ 인스톨러로 배포 (CI: tag push)

# 브릿지 단독 (mock 모드, 9224)
cd codebridge\bridge
cargo run
```

## CodeBridge Desktop (v0.3.0 — 독립 앱)

`desktop/`는 **Electron**으로 만든 독립 설치파일 앱입니다. 앱이 전용 Chrome(CDP 9333, 로그인된 전용 프로필)을 자동 실행하고, opencode serve(공식 웹 UI+v2 API)와 브릿지를 자식 프로세스로 띄웁니다. 기본 모드는 **browser(실 ChatGPT/Gemini 자동화)**, 번들 설정은 `desktop/resources/bridge/config.toml`에서 변경 가능합니다. UI는 자체 프레임리스 셸(커스텀 타이틀바/사이드바/상태바) + iframe 내 opencode 웹 UI 구조입니다.

```powershell
cd codebridge\desktop
npm install
npm run dist     # → dist\CodeBridge-Setup-<버전>.exe (NSIS)
npm start        # 개발 모드 (electron)
```

최초 실행 시 프로젝트 폴더를 선택하면, 해당 폴더에 `opencode.json`(webchat→브릿지 9224)이 없을 때만 복사됩니다.
앱 창을 닫아도 트레이의 "CodeBridge"가 백그라운드에서 유지되며, 트레이 메뉴에서 프로젝트 재선택/브릿지 재시작/로그 폴더/종료가 가능합니다.
릴리스는 태그(`v*`) push 시 GitHub Actions가 자동 빌드·배포합니다 (`.github/workflows/release.yml`).

## curl로 헬스체크

```powershell
curl.exe http://127.0.0.1:9224/health
curl.exe http://127.0.0.1:9224/v1/models
```

## 터미널 인코딩 관련

PowerShell에서 한글 로그가 깨지면 `chcp 65001` 또는 `[Console]::OutputEncoding = [Text.Encoding]::UTF8` 사용.
