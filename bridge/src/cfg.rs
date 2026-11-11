use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ServerCfg {
    // 누락된 필드는 기본값을 쓴다 (프로젝트에 흔한 config.toml과 섞여도 안전)
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct AutomationCfg {
    #[serde(default)]
    pub mode: String,
    /// CDP 연결 포트 (전용 Chrome). 기본 9333
    #[serde(default)]
    pub cdp_port: u16,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct HandoverCfg {
    /// auto: 감지 시 자동 인수인계 / manual: 알림만
    #[serde(default)]
    pub mode: String,
    /// 스레드당 안전 메시지 수 (소프트 리밋). 기본 25
    #[serde(default)]
    pub thread_soft_limit: u32,
    /// 이 사용자 입력마다 아카이브에 기록
    #[serde(default)]
    pub archive: bool,
    /// 인수인계 체인. 기본 ["same_site_new_chat", "gemini"]
    /// 예: ["same_site_new_chat"] (같은 사이트 새 채팅만), ["gemini"] (바로 Gemini로)
    #[serde(default)]
    pub order: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerCfg,
    #[serde(default)]
    pub automation: AutomationCfg,
    #[serde(default)]
    pub handover: HandoverCfg,
}

impl Config {
    /// config.toml을 읽는다. 없거나 파싱 실패 시 기본값 사용 (placeholder 확정).
    pub fn load(path: &str) -> Config {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str::<Config>(&s).unwrap_or_else(|e| {
                println!("[cfg] parse failed ({e}); using defaults");
                Config::default()
            }),
            Err(_) => {
                println!("[cfg] {path} not found; using defaults");
                Config::default()
            }
        };
        if cfg.server.host.is_empty() {
            cfg.server.host = "127.0.0.1".into();
        }
        if cfg.server.port == 0 {
            cfg.server.port = 9224;
        }
        if cfg.automation.mode.is_empty() {
            cfg.automation.mode = "browser".into();
        }
        if cfg.automation.cdp_port == 0 {
            cfg.automation.cdp_port = 9333;
        }
        if cfg.handover.mode.is_empty() {
            cfg.handover.mode = "auto".into();
        }
        if cfg.handover.thread_soft_limit == 0 {
            cfg.handover.thread_soft_limit = 25;
        }
        if cfg.handover.order.is_empty() {
            cfg.handover.order = vec!["same_site_new_chat".into(), "gemini".into()];
        }
        cfg
    }
}
