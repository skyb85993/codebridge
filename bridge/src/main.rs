use std::sync::{
    atomic::AtomicBool,
    Arc,
};

use axum::{
    routing::{get, post},
    Router,
};

mod api;
mod archive;
mod automation;
mod cfg;
mod harness;
mod sse;
mod translate;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<cfg::Config>,
    /// "지금 인수인계" 버튼을 눌렀는지 (one-shot). 다음 browser-mode 요청이 소비한다.
    pub manual_handover: Arc<AtomicBool>,
    /// 테스트 훅: CODEBRIDGE_FORCE_LIMIT_ONCE=1로 시작하면 첫 요청을 한도 배너로 강제한다.
    pub force_limit_hit: Arc<AtomicBool>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let cfg_path = std::env::var("CODEBRIDGE_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let cfg = cfg::Config::load(&cfg_path);
    let mode = cfg.automation.mode.clone();
    let addr = format!("{}:{}", cfg.server.host, cfg.server.port);

    let app = Router::new()
        .route("/health", get(api::health))
        .route("/v1/models", get(api::list_models))
        .route("/v1/probe", get(api::probe_web))
        .route("/v1/chat/completions", post(api::chat_completions))
        .route("/v1/handover/trigger", post(api::trigger_handover))
        .route("/v1/handover/events", get(api::handover_events))
        .route("/v1/chrome/show", get(api::show_chrome))
        .route("/v1/chrome/hide", get(api::hide_chrome))
        .with_state(AppState {
            cfg: Arc::new(cfg),
            manual_handover: Arc::new(AtomicBool::new(false)),
            force_limit_hit: Arc::new(AtomicBool::new(
                std::env::var("CODEBRIDGE_FORCE_LIMIT_ONCE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
            )),
        });

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr} failed: {e}"));

    println!("CodeBridge bridge | mode={mode} | http://{addr}");
    axum::serve(listener, app).await.expect("serve failed");
}
