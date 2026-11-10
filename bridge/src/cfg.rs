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
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerCfg,
    #[serde(default)]
    pub automation: AutomationCfg,
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
            cfg.automation.mode = "mock".into();
        }
        cfg
    }
}
