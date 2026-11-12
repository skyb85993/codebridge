//! 인수인계 팩 생성 (F2.17 v1, 결정적 템플릿 — 로컬 LLM 미사용).
//! 새 스레드의 첫 메시지로 보내 "이어서 해 줘" 루프를 재개시킨다.

pub const FENCE_HEADER: &str = include_str!("../../../prompts/fence_convention.md");

/// 인수인계 문서를 만든다.
/// - project_dir: 프로젝트 루트 (이름 표기용)
/// - last_user: 마지막 사용자 요청 전문
/// - tool_results: 도구 결과 요약 (이미 병합된 문자열)
/// - snapshot: harness::flatten::snapshot 이 반환한 파일 목록
pub fn build_pack(
    project_dir: &str,
    last_user: &str,
    tool_results: &str,
    snapshot: &str,
) -> String {
    let name = project_dir
        .trim_end_matches(['\\', '/'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("project")
        .to_string();
    let tool_results = if tool_results.trim().is_empty() {
        "(아직 도구 실행 결과 없음)".to_string()
    } else {
        tool_results.chars().take(1500).collect()
    };
    format!(
        "[인수인계 — 이전 대화에서 이어받음]\n\
         1. 프로젝트: {name}\n\
         2. 마지막 사용자 요청:\n{last_user}\n\
         3. 이미 만든/수정한 파일 (현재 워크스페이스 스냅샷):\n{snapshot}\n\
         4. 직전 도구 실행 결과·상태:\n{tool_results}\n\
         5. 이번에 할 일: 위 요청을 이어서 진행해 주세요. 진행 상황은 이 대화에서 이어서 작업합니다.\n\n\
         {fence}\n",
        name = name,
        last_user = last_user,
        snapshot = snapshot,
        tool_results = tool_results,
        fence = FENCE_HEADER.trim(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn pack_contains_key_sections() {
        let p = super::build_pack(
            "D:/Roblox/MyGame",
            "미니 게임 만들어줘",
            "write가 성공했습니다",
            "- ServerScriptService/GameManager.lua (295 B)\n- HUD.client.lua (255 B)",
        );
        assert!(p.contains("[인수인계"));
        assert!(p.contains("미니 게임 만들어줘"));
        assert!(p.contains("GameManager.lua"));
        assert!(p.contains("write가 성공했습니다"));
        assert!(p.contains("에이전트"));
        assert!(p.contains("MyGame"));
    }
}
