# CodeBridge

무료 웹 채팅(ChatGPT / Gemini)을 "모델"로 사용하는 코딩 에이전트 시스템.
하네스는 오픈코드를 재사용하고, 이 저장소에는 **웹 채팅 브릿지(Rust)**와 **CodeBridge Desktop(Electron)** 앱이 들어간다.

> 상세 계획 문서는 워크스페이스 밖(`.opencode/plan`)에 있다. 저장소에는 코드만 포함한다.

## 현재 상태 (v0.5.3)

- [x] 브릿지(browser 모드): 실 ChatGPT/Gemini 자동화 — flatten, CDP 전송/감지/추출, 관대 파서, 아카이브, 한도 감지, 인수인계 체인
- [x] 관대 파서: 코드펜스+경로 주석 → write tool_calls 승격 (유닛 테스트 22개 포함)
- [x] E2E(실 ChatGPT): 자연어 요청 → 응답 → 파일 생성 (`GameManager.lua`, `HUD.client.lua`, 점프 패드 Lua 등)
- [x] 인수인계 체인: 한도 도달/수동("지금 인수인계" 버튼) → 인계 팩 생성 → 같은 사이트 새 채팅 → Gemini 로테이션 (실증 완료)
- [x] **CodeBridge Desktop**: 네이티브 React 프론트엔드 풀 재작성(프레임리스 커스텀 셸), 전용 Chrome **지연 실행**(첫 요청 시), 자동 업데이트(electron-updater + GitHub Releases, 태그 푸시 시 CI 자동 릴리스)
- [x] opencode.json 자동 시드(모델 표시/프로젝트 등록) — 첫 실행부터 추가 단계 없음
- [x] testpages 회귀 자동화 (`node scripts/testpages-check.mjs`, 7/7)
- [ ] Roblox Studio 실행으로 생성물 검증 (사용자가 실사용으로 수행 중)

## 빠른 시작

```powershell
# 개발 모드 (브릿지 + 앱)
cd codebridge\desktop
npm install
npm start        # 이후 dist\ 인스톨러로 배포 (CI: tag push)

# 브릿지 단독 (browser 모드, 9224)
cd codebridge\bridge
cargo run
```

## CodeBridge Desktop (v0.5.3 — 독립 앱)

`desktop/`는 **Electron + React + Vite + Tailwind**로 만든 독립 설치파일 앱입니다. 앱은 opencode serve(v2 API)를 자식 프로세스로 실행하고, opencode 웹 UI 대신 **자체 프레임리스 셸 UI**를 화면에 표시합니다. 전용 Chrome(CDP 9333)은 **지연 실행** — 앱 첫 요청 때 브릿지가 전용 프로필 Chrome을 띄웁니다. 기본 모드는 **browser(실 ChatGPT/Gemini 자동화)**.

```powershell
cd codebridge\desktop
npm install
npm run dist     # → dist\CodeBridge-Setup-<버전>.exe (NSIS)
npm start        # 개발 모드 (electron)
```

최초 실행 시 프로젝트 폴더를 선택하면, 해당 폴더에 `opencode.json`(webchat→브릿지 9224)이 없을 때만 복사됩니다.
앱 창을 닫아도 트레이의 "CodeBridge"가 백그라운드에서 유지되며, 트레이 메뉴에서 프로젝트 재선택/브릿지 재시작/로그 폴더/종료가 가능합니다.
릴리스는 태그(`v*`) push 시 GitHub Actions가 자동 빌드·배포합니다 (`.github/workflows/release.yml`).

## 설치 시 SmartScreen 경고 (서명 없음 — 무료 배포)

이 앱은 코드 서명을 하지 않으므로(오픈소스, 비용 0) Windows가 설치 파일을 "알 수 없는 게시자"로 표시할 수 있습니다.

1. 설치 파일을 실행하면 "Windows가 PC를 보호했습니다" 파란 경고가 뜹니다.
2. **"추가 정보"** → **"실행"** 을 누르면 정상적으로 설치됩니다.
3. 차등 자동 업데이트도 동일하게 "추가 정보 → 실행"이 필요할 수 있습니다 (새 버전마다 한 번).

왜 서명을 안 하나요? 서명은 유료이고(Microsoft Trusted Signing ~$9.99/월 또는 인증서 구매), 이 프로젝트는 무료 배포를 원칙으로 합니다. 경고는 불편하지만 앱 동작에는 영향이 없으며, 소스 코드와 빌드 절차는 이 저장소에 전부 공개되어 있습니다.

## curl로 헬스체크

```powershell
curl.exe http://127.0.0.1:9224/health
curl.exe http://127.0.0.1:9224/v1/models
```

## 터미널 인코딩 관련

PowerShell에서 한글 로그가 깨지면 `chcp 65001` 또는 `[Console]::OutputEncoding = [Text.Encoding]::UTF8` 사용.
