use clap::Parser;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use ctx_firewall::config::{Cli, Commands, ServeArgs};
use ctx_firewall::metrics::{RotScoreCalculator, TokenEstimator};
use ctx_firewall::proxy::{run_server, ProxyState};
use ctx_firewall::prune::{PruneConfig, PruningEngine};
use ctx_firewall::tui::{run_headless_logger, run_tui, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Serve(args)) => run_serve(args).await?,
        Some(Commands::Check(args)) => run_check(args).await?,
        Some(Commands::Stats(args)) => run_stats(args).await?,
        None => run_serve(cli.default_serve).await?,
    }

    Ok(())
}

async fn run_serve(args: ServeArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if args.no_tui {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::INFO.into()),
            )
            .init();
    }

    let (telemetry_tx, telemetry_rx) = mpsc::unbounded_channel();

    let prune_config = PruneConfig {
        enable_ansi_strip: !args.no_ansi,
        enable_log_folding: !args.no_fold,
        enable_dedup: !args.no_dedup,
        enable_lazy_mcp: !args.no_lazy_mcp,
        ..Default::default()
    };

    let client = reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()?;

    let upstream_anthropic = reqwest::Url::parse(&args.upstream_anthropic)?;
    let upstream_openai = reqwest::Url::parse(&args.upstream_openai)?;

    let state = Arc::new(ProxyState {
        upstream_anthropic,
        upstream_openai,
        client,
        pruner: Arc::new(PruningEngine::new(prune_config)),
        telemetry_tx,
        start_time: Instant::now(),
        turn_counter: AtomicU64::new(0),
    });

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;

    if args.no_tui {
        tracing::info!("ctx-firewall listening on http://{}", addr);
        tokio::spawn(run_server(state, listener));
        run_headless_logger(telemetry_rx).await;
    } else {
        tokio::spawn(run_server(state, listener));
        let app = AppState::new(args.port);
        let _ = run_tui(telemetry_rx, app).await;
    }

    Ok(())
}

async fn run_check(
    args: ctx_firewall::config::CheckArgs,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let raw_content = match args.file {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            eprintln!("No file provided. Please specify --file <path>");
            return Ok(());
        }
    };
    let content = raw_content.trim_start_matches('\u{feff}');

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        let breakdown = TokenEstimator::analyze_anthropic_request(&json);
        let engine = PruningEngine::default();
        let (_, report) = engine.prune_anthropic_payload(json.clone(), 1)?;
        let stale_tokens = if report.rules_applied.iter().any(|r| r == "file_dedup") {
            breakdown.file_content_tokens / 2
        } else {
            0
        };
        let rot_score = RotScoreCalculator::calculate(
            stale_tokens,
            breakdown.logs_tokens,
            breakdown.tools_tokens,
            breakdown.total_tokens,
        );
        println!("Context Breakdown:");
        println!("  System Tokens:   {}", breakdown.system_tokens);
        println!("  Tools Tokens:    {}", breakdown.tools_tokens);
        println!("  File Tokens:     {}", breakdown.file_content_tokens);
        println!("  Logs Tokens:     {}", breakdown.logs_tokens);
        println!("  Chat Tokens:     {}", breakdown.history_tokens);
        println!("  Total Tokens:    {}", breakdown.total_tokens);
        println!("Pruning Simulation:");
        println!("  Original Tail:   {} B", report.original_tail_bytes);
        println!("  Pruned Tail:     {} B", report.pruned_tail_bytes);
        println!("  Tokens Saved:    ~{}", report.estimated_tokens_saved);
        println!("  Rules Triggered: {}", if report.rules_applied.is_empty() { "none".to_string() } else { report.rules_applied.join(", ") });
        println!("  Prefix SHA256:   {}", report.sealed_prefix_hash);
        println!(
            "Rot Score: {}/100 [{}]",
            rot_score,
            RotScoreCalculator::level(rot_score).as_str()
        );
    } else {
        println!("File is not valid JSON. Total bytes: {}", content.len());
    }

    Ok(())
}

async fn run_stats(
    args: ctx_firewall::config::StatsArgs,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("ctx-firewall statistics:");
    if let Some(session) = args.session {
        println!("Session: {}", session);
    }
    println!("Query live proxy via GET http://localhost:8080/health");
    Ok(())
}
