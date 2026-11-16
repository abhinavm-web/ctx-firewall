use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::metrics::{RotScoreCalculator, TokenEstimator};
use crate::proxy::client::{build_downstream_headers, build_forward_headers};
use crate::proxy::sse::SseChunkParser;
use crate::prune::PruningEngine;
use crate::tui::app::{PruneRule, TelemetryEvent};

pub struct ProxyState {
    pub upstream_anthropic: reqwest::Url,
    pub upstream_openai: reqwest::Url,
    pub client: reqwest::Client,
    pub pruner: Arc<PruningEngine>,
    pub telemetry_tx: tokio::sync::mpsc::UnboundedSender<TelemetryEvent>,
    pub start_time: Instant,
    pub turn_counter: AtomicU64,
}

pub async fn health_handler(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let body = serde_json::json!({
        "status": "ok",
        "uptime_secs": uptime,
        "version": env!("CARGO_PKG_VERSION"),
        "upstream_anthropic": state.upstream_anthropic.to_string(),
        "upstream_openai": state.upstream_openai.to_string(),
    });
    (StatusCode::OK, axum::Json(body))
}

pub async fn anthropic_messages_handler(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_proxy_turn(state, "/v1/messages", true, headers, body).await
}

pub async fn openai_completions_handler(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_proxy_turn(state, "/v1/chat/completions", false, headers, body).await
}

#[derive(Default)]
struct StreamStats {
    cache_hit: bool,
    cached_tokens: usize,
    uncached_tokens: usize,
    output_tokens: usize,
}

struct StreamGuard {
    telemetry_tx: tokio::sync::mpsc::UnboundedSender<TelemetryEvent>,
    turn_id: u64,
    start_time: Instant,
    stats: Arc<Mutex<StreamStats>>,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        if let Ok(st) = self.stats.lock() {
            let latency_ms = self.start_time.elapsed().as_millis() as u64;
            let _ = self.telemetry_tx.send(TelemetryEvent::StreamFinished {
                turn_id: self.turn_id,
                cache_hit: st.cache_hit,
                cached_tokens: st.cached_tokens,
                uncached_tokens: st.uncached_tokens,
                output_tokens: st.output_tokens,
                latency_ms,
            });
        }
    }
}

async fn handle_proxy_turn(
    state: Arc<ProxyState>,
    subpath: &str,
    is_anthropic: bool,
    headers: HeaderMap,
    raw_body: Bytes,
) -> Response {
    let start_time = Instant::now();
    let turn_id = state.turn_counter.fetch_add(1, Ordering::SeqCst) + 1;

    let base_url = if is_anthropic {
        &state.upstream_anthropic
    } else {
        &state.upstream_openai
    };

    let target_url = match base_url.join(subpath.trim_start_matches('/')) {
        Ok(u) => u,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid upstream URL configuration").into_response();
        }
    };

    let mut model_name = if is_anthropic {
        "claude-3-5-sonnet".to_string()
    } else {
        "gpt-4o".to_string()
    };

    // 1. Ingest JSON & Context Analysis
    let mut original_tokens = 0usize;

    let (final_body_bytes, tokens_saved) = match serde_json::from_slice::<Value>(&raw_body) {
        Ok(json_payload) => {
            if let Some(m) = json_payload.get("model").and_then(|m| m.as_str()) {
                model_name = m.to_string();
            }

            let breakdown = if is_anthropic {
                TokenEstimator::analyze_anthropic_request(&json_payload)
            } else {
                TokenEstimator::analyze_openai_request(&json_payload)
            };
            original_tokens = breakdown.total_tokens;

            // 2. Active Pruning with KV-Cache Guard
            let prune_res = if is_anthropic {
                state.pruner.prune_anthropic_payload(json_payload, turn_id)
            } else {
                state.pruner.prune_openai_payload(json_payload, turn_id)
            };

            match prune_res {
                Ok((pruned_val, report)) => {
                    let rot_score = RotScoreCalculator::calculate(
                        report.estimated_tokens_saved,
                        0,
                        0,
                        breakdown.total_tokens,
                    );

                    // Send RequestReceived telemetry
                    let _ = state.telemetry_tx.send(TelemetryEvent::RequestReceived {
                        turn_id,
                        model: model_name.clone(),
                        breakdown,
                        rot_score,
                    });

                    // Send PruneApplied telemetry for each rule
                    for rule_name in &report.rules_applied {
                        let rule = PruneRule::from_str_name(rule_name);
                        let _ = state.telemetry_tx.send(TelemetryEvent::PruneApplied {
                            turn_id,
                            rule,
                            tokens_saved: report.estimated_tokens_saved,
                            bytes_saved: report
                                .original_tail_bytes
                                .saturating_sub(report.pruned_tail_bytes),
                            description: format!("Pruned {}", rule_name),
                        });
                    }

                    let bytes = match serde_json::to_vec(&pruned_val) {
                        Ok(b) => Bytes::from(b),
                        Err(_) => raw_body.clone(),
                    };
                    (bytes, report.estimated_tokens_saved)
                }
                Err(err) => {
                    tracing::warn!("Pruning skipped due to error: {}", err);
                    (raw_body.clone(), 0)
                }
            }
        }
        Err(_) => (raw_body.clone(), 0),
    };

    // 3. Build Forward Headers
    let host_str = target_url.host_str().unwrap_or("localhost").to_string();
    let forward_headers = build_forward_headers(&headers, &host_str, final_body_bytes.len());

    // 4. Dispatch Upstream
    let upstream_req = state
        .client
        .post(target_url)
        .headers(forward_headers)
        .body(final_body_bytes);

    let upstream_resp = match upstream_req.send().await {
        Ok(r) => r,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("ctx-firewall upstream error: {err}"),
            )
                .into_response();
        }
    };

    let status = upstream_resp.status();
    let resp_headers = build_downstream_headers(upstream_resp.headers());

    // 5. Zero-Latency Stream Tap
    let sse_parser = Arc::new(Mutex::new(SseChunkParser::new()));
    let stream_stats = Arc::new(Mutex::new(StreamStats {
        cache_hit: false,
        cached_tokens: 0,
        uncached_tokens: original_tokens.saturating_sub(tokens_saved),
        output_tokens: 0,
    }));

    let guard = Arc::new(StreamGuard {
        telemetry_tx: state.telemetry_tx.clone(),
        turn_id,
        start_time,
        stats: stream_stats.clone(),
    });

    let byte_stream = upstream_resp.bytes_stream().map(move |item| match item {
        Ok(chunk) => {
            let _ = &guard; // Retain guard in closure
            if let Ok(mut parser) = sse_parser.lock() {
                for metrics in parser.feed(&chunk) {
                    if let Ok(mut st) = stream_stats.lock() {
                        if let Some(cr) = metrics.cache_read_tokens {
                            if cr > 0 {
                                st.cache_hit = true;
                                st.cached_tokens = cr;
                            }
                        }
                        if let Some(out) = metrics.output_tokens {
                            st.output_tokens = out;
                        }
                    }
                }
            }
            Ok::<Bytes, std::io::Error>(chunk)
        }
        Err(e) => Err(std::io::Error::other(e)),
    });

    let body = Body::from_stream(byte_stream);
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = resp_headers;

    response
}
