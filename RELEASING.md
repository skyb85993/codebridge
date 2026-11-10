# CodeBridge Desktop 릴리즈 절차

이 문서는 CodeBridge Desktop을 배포하고, 설치된 앱이 자동으로 업데이트를 받도록 설정하는 방법을 설명한다.

## 현재 상태

- 설치 파일: `desktop/dist/CodeBridge Setup <버전>.exe` (NSIS, per-user)
- 자동 업데이트: `electron-updater` 내장 (`desktop/main.cjs`의 `setupAutoUpdate()`)
  - 앱 시작 시 1회 + 트레이 메뉴 "업데이트 확인"으로 수동 확인 가능
  - GitHub Releases의 `latest`를 기준으로 `latest.yml`을 확인
  - 코드 서명 없음 → SmartScreen 경고가 뜰 수 있다
- 배포 채널: 미설정 (GitHub 저장소/Release를 사용자가 만들어야 동작)

## 릴리즈 순서

1. **GitHub 저장소 생성** (예: `https://github.com/<your-id>/codebridge`)
2. `desktop/package.json`의 `build.publish`를 실제 저장소로 수정:
   ```jsonc
   "publish": [{ "provider": "github", "owner": "<your-id>", "repo": "codebridge" }]
   ```
3. 버전 올리기: `desktop/package.json`의 `"version"`을 올리고 커밋
4. 태그 푸시 후 `npm run dist -- --publish always`로 빌드하면 GitHub Release에
   설치 파일 + `latest.yml`이 자동 업로드됨.
   (gh CLI 또는 GitHub 토큰 필요, `GH_TOKEN` 환경변수)
5. 이후 설치된 앱은 다음 실행 시 자동으로 업데이트를 확인·다운로드·설치한다
   (설치 파일 실행 없이 인앱 업데이트).

## 업데이트 확인용 설정

- `settings.json` (`%APPDATA%\CodeBridge\settings.json`):
  - `projectDir` — 작업할 프로젝트 폴더 (없으면 첫 실행 시 폴더 선택)
- 로그: `%APPDATA%\CodeBridge\logs\` (bridge / opencode / updater / probe)

## 현재 단계 (v0.1.0)

- 앱이 자체 Chrome을 자동 실행 (전용 프로필, 로그인 1회 필요)
- 브릿지는 browser 모드 (실 ChatGPT/Gemini 자동화)
- 배포 채널 미설정 — 설치 파일을 직접 복사해 쓰거나, 위 절차로 GitHub에 올린 뒤 업데이트 활성화 필요
