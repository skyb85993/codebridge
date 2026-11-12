; CodeBridge — 다크 브랜드 NSIS 정의
; installer.nsi보다 먼저 로드되어 MUI2의 기본 색상보다 우선 적용된다.
; (설치 마법사 자체의 .rc 리소스는 수정하지 않으므로 버튼 등 OS 컨트롤은 기본 테마 유지)

!define MUI_BGCOLOR "0F1626"    ; 앱 배경과 같은 다크 네이비
!define MUI_TEXTCOLOR "E2E8F0"  ; 밝은 회색 텍스트

; 설치 첫 화면인 환영 페이지 추가 — 좌측에 브랜드 사이드바, 우측에 안내 텍스트
; (기존 oneClick=false 흐름은 설치봄(INSTFILES) 페이지부터만 표시되어 브랜드 노출이 적었음)
!macro customWelcomePage
  !insertmacro MUI_PAGE_WELCOME
!macroend
