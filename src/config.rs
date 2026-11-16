use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "ctx-firewall",
    about = "Active LLM context proxy and TUI flamegraph profiler that prunes context rot in-flight while preserving KV-cache hits.",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub default_serve: ServeArgs,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Starts the active context firewall proxy server
    Serve(ServeArgs),
    /// Checks a JSON request payload or log file for context rot
    Check(CheckArgs),
    /// Displays cumulative context rot and prompt caching statistics
    Stats(StatsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    /// Upstream Anthropic API URL
    #[arg(long, default_value = "https://api.anthropic.com")]
    pub upstream_anthropic: String,

    /// Upstream OpenAI API URL
    #[arg(long, default_value = "https://api.openai.com")]
    pub upstream_openai: String,

    /// Disables the interactive TUI and runs in headless logging mode
    #[arg(long)]
    pub no_tui: bool,

    /// Disables ANSI escape sequence stripping
    #[arg(long)]
    pub no_ansi: bool,

    /// Disables log folding
    #[arg(long)]
    pub no_fold: bool,

    /// Disables duplicate file read deduplication
    #[arg(long)]
    pub no_dedup: bool,

    /// Disables lazy MCP tool schema optimization
    #[arg(long)]
    pub no_lazy_mcp: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Path to a JSON request payload to analyze for rot
    #[arg(short, long)]
    pub file: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct StatsArgs {
    /// Session ID to inspect
    #[arg(short, long)]
    pub session: Option<String>,
}
