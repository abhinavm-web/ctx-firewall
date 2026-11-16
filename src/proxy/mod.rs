pub mod client;
pub mod routes;
pub mod sse;

pub use client::{build_downstream_headers, build_forward_headers, is_hop_by_hop};
pub use routes::{health_handler, ProxyState};
pub use sse::{ParsedTokenMetrics, SseChunkParser};

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;

use crate::prune::PruningEngine;
use crate::tui::app::TelemetryEvent;

pub fn build_router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/health", get(routes::health_handler))
        .route("/v1/messages", post(routes::anthropic_messages_handler))
        .route("/messages", post(routes::anthropic_messages_handler))
        .route(
            "/v1/chat/completions",
            post(routes::openai_completions_handler),
        )
        .route(
            "/chat/completions",
            post(routes::openai_completions_handler),
        )
        .with_state(state)
}

/// Convenience router builder for tests and embedded proxy runners.
pub fn create_router(
    upstream_anthropic: String,
    upstream_openai: String,
    telemetry_tx: UnboundedSender<TelemetryEvent>,
) -> Router {
    let client = reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .expect("Failed to initialize HTTP client");

    let pruner = Arc::new(PruningEngine::default());
    let state = Arc::new(ProxyState {
        upstream_anthropic: reqwest::Url::parse(&upstream_anthropic)
            .unwrap_or_else(|_| reqwest::Url::parse("https://api.anthropic.com").unwrap()),
        upstream_openai: reqwest::Url::parse(&upstream_openai)
            .unwrap_or_else(|_| reqwest::Url::parse("https://api.openai.com").unwrap()),
        client,
        pruner,
        telemetry_tx,
        start_time: Instant::now(),
        turn_counter: AtomicU64::new(0),
    });

    build_router(state)
}

pub async fn run_server(
    state: Arc<ProxyState>,
    listener: TcpListener,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
