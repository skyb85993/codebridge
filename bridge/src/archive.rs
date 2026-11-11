//! 대화 아카이브 (F2.14) + 스레드 카운터 상태 (F2.16 사전 감지 기반)
//! 저장 위치: %APPDATA%\CodeBridge\bridge-archive\ (없으면 ./.codebridge/archive)
//! - journal-YYYYMMDD.md  : 요청/응답 전문 기록 (인수인계 재료)
//! - state.json           : 프로젝트별 메시지 카운터 (soft limit 판정용)

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct ProjectEntry {
    #[serde(default)]
    message_count: u32,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    projects: HashMap<String, ProjectEntry>,
}

pub struct Archive {
    dir: PathBuf,
    project_key: String,
}

fn base_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("CodeBridge").join("bridge-archive")
    } else {
        PathBuf::from(".codebridge").join("archive")
    }
}

impl Archive {
    pub fn new(project_dir: &str) -> Self {
        let dir = base_dir();
        let _ = fs::create_dir_all(&dir);
        Archive {
            dir,
            project_key: project_dir.to_string(),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn journal_path(&self) -> PathBuf {
        let today = chrono_like_date();
        self.dir.join(format!("journal-{today}.md"))
    }

    fn load_state(&self) -> State {
        fs::read_to_string(self.state_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &State) {
        if let Ok(s) = serde_json::to_string_pretty(state) {
            let _ = fs::write(self.state_path(), s);
        }
    }

    /// 이 프로젝트의 메시지 카운터를 1 올리고 반환한다.
    pub fn bump_counter(&self) -> u32 {
        let mut state = self.load_state();
        let entry = state.projects.entry(self.project_key.clone()).or_default();
        entry.message_count = entry.message_count.saturating_add(1);
        let v = entry.message_count;
        self.save_state(&state);
        v
    }

    /// 카운터를 0으로 되돌린다 (새 스레드 교체 후).
    pub fn reset_counter(&self) {
        let mut state = self.load_state();
        state.projects.insert(self.project_key.clone(), ProjectEntry::default());
        self.save_state(&state);
    }

    /// 요청/응답을 마크다운으로 기록한다.
    pub fn record(&self, tag: &str, body: &str) {
        let mut f = match OpenOptions::new().create(true).append(true).open(self.journal_path()) {
            Ok(f) => f,
            Err(_) => return,
        };
        let _ = writeln!(f, "\n## [{tag}] {}\n", chrono_like_datetime());
        let _ = writeln!(f, "{body}\n");
        let _ = writeln!(f, "---\n");
    }

    fn handover_log_path(&self) -> PathBuf {
        self.dir.join("handover.log")
    }

    /// 인수인계 메타 이벤트를 JSONL로 기록한다 (대화 본문이 아니라서 archive 설정과 무관하게 항상 기록).
    /// 파일이 64KB를 넘으면 마지막 128줄만 남긴다.
    pub fn log_handover_event(&self, kind: &str, detail: &str) {
        let event = serde_json::json!({
            "at": chrono_like_datetime(),
            "kind": kind,
            "detail": detail,
        });
        let line = match serde_json::to_string(&event) {
            Ok(l) => l,
            Err(_) => return,
        };
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(self.handover_log_path()) {
            let _ = writeln!(f, "{line}");
        }
        // 상한 관리: 64KB 초과 시 마지막 128줄만 보존
        if let Ok(meta) = fs::metadata(self.handover_log_path()) {
            if meta.len() > 64 * 1024 {
                if let Ok(content) = fs::read_to_string(self.handover_log_path()) {
                    let kept: Vec<&str> = content.lines().collect();
                    let keep_from = kept.len().saturating_sub(128);
                    let trimmed = kept[keep_from..].join("\n");
                    let _ = fs::write(self.handover_log_path(), format!("{trimmed}\n"));
                }
            }
        }
    }

    /// 최근 인수인계 이벤트를 시간 순으로 돌려준다 (과거→최근).
    pub fn handover_events(&self, limit: usize) -> Vec<serde_json::Value> {
        let Ok(content) = fs::read_to_string(self.handover_log_path()) else {
            return Vec::new();
        };
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        lines
            .iter()
            .rev()
            .take(limit)
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// 임의의 산출물(인수인계 팩 등)을 저장하고 경로를 돌려준다.
    pub fn write_artifact(&self, name: &str, content: &str) -> Option<PathBuf> {
        let path = self.dir.join(name);
        fs::write(&path, content).ok()?;
        Some(path)
    }
}

/// Hinnant의 civil_from_days — 1970-01-01 이후의 일수를 (y, m, d)로 변환
fn days_to_ymd(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn chrono_like_date() -> String {
    let (y, m, d) = days_to_ymd(now_secs() / 86_400);
    format!("{y:04}{m:02}{d:02}")
}

fn chrono_like_datetime() -> String {
    let s = now_secs();
    let (y, m, d) = days_to_ymd(s / 86_400);
    let secs = s % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn counter_persists_shape() {
        let s = serde_json::to_string_pretty(&super::State::default()).unwrap();
        assert!(s.contains("projects"));
    }

    #[test]
    fn days_to_ymd_known_dates() {
        assert_eq!(super::days_to_ymd(0), (1970, 1, 1));
        // 2026-11-10 (검증된 오프라인 계산값: 20767일)
        assert_eq!(super::days_to_ymd(20767), (2026, 11, 10));
    }
}
