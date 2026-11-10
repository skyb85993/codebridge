use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

mod api;
mod archive;
mod automation;
mod cfg;
mod fixture;
mod harness;
mod sse;
mod translate;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<cfg::Config>,
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
        .with_state(AppState { cfg: Arc::new(cfg) });

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr} failed: {e}"));

    println!("CodeBridge bridge | mode={mode} | http://{addr}");
    axum::serve(listener, app).await.expect("serve failed");
}
