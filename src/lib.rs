//! ctx-firewall: Active LLM context proxy and TUI flamegraph profiler.
//!
//! Prunes context rot (redundant file reads, ANSI escapes, test logs)
//! in-flight while strictly preserving KV-cache prompt prefix boundaries.

pub mod config;
pub mod metrics;
pub mod models;
pub mod proxy;
pub mod prune;
pub mod tui;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
