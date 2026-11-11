# CodeBridge Desktop 릴리즈 절차

이 문서는 CodeBridge Desktop을 배포하고, 설치된 앱이 자동으로 업데이트를 받도록 설정하는 방법을 설명한다.

## 현재 상태

- 설치 파일: `desktop/dist/CodeBridge-Setup-<버전>.exe` (NSIS, artifactName 고정, per-user)
- 자동 업데이트: `electron-updater` 내장, 앱 시작 시 1회 + 트레이 "업데이트 확인"
- 배포 채널: **skyb85993/codebridge (PUBLIC), v0.1.0부터, `release` 워크플로우로 릴리스**

## 릴리즈 순서 (CI 자동화, 권장)

1. `desktop/package.json`의 `"version"`을 올리고 커밋
2. `CHANGELOG.md`에 `## v<버전> (YYYY-MM-DD)` 섹션 추가
3. 태그를 만들어 push: `git tag v0.3.0 && git push origin v0.3.0`
4. **GitHub Actions `release` 워크플로우가 자동으로 빌드** → 인스톨러 + `latest.yml` + blockmap 업로드 → 릴리스 생성 (릴리스 노트는 CHANGELOG에서 해당 버전 섹션을 추출)
5. 설치된 앱은 다음 실행 시 자동으로 업데이트를 확인·다운로드·설치한다
   (설치 파일 실행 없이 인앱 업데이트).

### 코드 서명 (선택)

`release` 워크플로우는 시크릿이 있으면 자동으로 서명한다:
- `WIN_CSC_LINK` — .pfx 인증서 (base64 또는 URL)
- `WIN_CSC_KEY_PASSWORD` — pfx 비밀번호

없으면 무서명 빌드 (현재 선택). SmartScreen 경고가 뜰 수 있으며, 사용자 안내는 README의 "설치 시 SmartScreen 경고" 절을 따른다. (사용자가 서명하지 않기로 결정, 2026-11-11)

### 수동 릴리스 (CI 없이)

1. `npm run dist` → `desktop/dist/CodeBridge-Setup-<버전>.exe`
2. `gh release create v<버전> "CodeBridge-Setup-<버전>.exe" "latest.yml" "CodeBridge-Setup-<버전>.exe.blockmap"`

## 업데이트 확인용 설정

- `settings.json` (`%APPDATA%\CodeBridge\settings.json`):
  - `projectDir` — 작업할 프로젝트 폴더 (없으면 첫 실행 시 폴더 선택)
- 로그: `%APPDATA%\CodeBridge\logs\` (bridge / opencode / updater / probe)

## 현재 단계 (v0.3.0 준비)

- 앱이 자체 Chrome을 자동 실행 (전용 프로필, 로그인 1회 필요)
- 브릿지는 browser 모드 (실 ChatGPT/Gemini 자동화)
