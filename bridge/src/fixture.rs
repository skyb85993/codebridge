/// Phase 0 스파이크용 고정 응답(모의 웹 채팅 출력).
/// 실제 ChatGPT가 답변하는 형태(설명 + 코드펜스 + 마무리)와 동일한 구조다.
pub const INTRO: &str = "로블록스 미니 게임 스켈레톤을 만들어 드릴게요!\n아래 2개의 파일로 구성됩니다.\n\n";
pub const OUTRO: &str =
    "두 파일을 프로젝트 폴더에 저장했으니 확인해 주세요! 점프 높이나 점수 규칙을 바꾸고 싶으면 이어서 말씀 주세요.";

const FENCES: &str = "\
```lua
-- path: ServerScriptService/GameManager.lua
local Players = game:GetService(\"Players\")

local GameManager = {}
GameManager.score = 0

function GameManager:start()
	print(\"[GameManager] 게임 시작!\")
	GameManager.score = 0
end

function GameManager:addScore(points)
	GameManager.score = GameManager.score + points
end

return GameManager
```

```lua
-- path: StarterPlayer/StarterPlayerScripts/HUD.client.lua
local Players = game:GetService(\"Players\")
local playerGui = Players.LocalPlayer:WaitForChild(\"PlayerGui\")

local gui = Instance.new(\"ScreenGui\")
gui.Name = \"MiniGameHUD\"
gui.ResetOnSpawn = false
gui.Parent = playerGui

print(\"[HUD] HUD 초기화 완료\")
```
";

/// 도구 결과가 이미 대화에 포함된 경우의 마무리 텍스트 (재호출 루프 방지)
pub const ACK: &str =
    "파일을 모두 저장했습니다. 수정하거나 이어서 작업할 내용이 있으면 말씀해 주세요.";

pub struct MockReply {
    pub raw: String,
    pub intro: &'static str,
    pub outro: &'static str,
}

/// raw: 파서에 들어갈 실제 응답 텍스트 / intro·outro: 스트림 content에 들어가는 순수 텍스트
pub fn parts() -> MockReply {
    MockReply {
        raw: format!("{INTRO}{FENCES}"),
        intro: INTRO,
        outro: OUTRO,
    }
}
