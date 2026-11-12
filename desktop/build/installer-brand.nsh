; CodeBridge — 다크 브랜드 NSIS 정의 (UTF-8 BOM)
; installer.nsi보다 먼저 로드되어 MUI2 기본값보다 우선 적용된다.

!define MUI_BGCOLOR "0F1626"      ; 앱 배경과 같은 다크 네이비
!define MUI_TEXTCOLOR "E2E8F0"    ; 밝은 회색
!define MUI_INSTFILESPAGE_COLORS "E2E8F0 0F1626"  ; 설치 진행 페이지(로그/프로그레스) 배경

; 환영 페이지 — Hermes 스타일: 히어로 이미지 + 타이틀 + 불릿
!define MUI_WELCOMEPAGE_TITLE "CodeBridge 설치"
!define MUI_WELCOMEPAGE_TEXT "ChatGPT·Gemini 무료 웹 채팅을 모델로 사용하는 AI 코딩 에이전트입니다.$\n$\n· 파일은 프로젝트 폴더에 바로 생성/수정됩니다$\n· 전용 Chrome은 보이지 않게 숨겨집니다$\n· 자동 업데이트 · 새 채팅 로테이션 지원"

!macro customWelcomePage
  !insertmacro MUI_PAGE_WELCOME
!macroend

; 마침 페이지 — 설치 완료 안내
!define MUI_FINISHPAGE_TITLE "설치가 완료됐어요"
!define MUI_FINISHPAGE_TEXT "CodeBridge를 설치했어요.$\n마침을 누르면 앱이 바로 실행됩니다."
