# ctx-firewall

**Active LLM context proxy and TUI flamegraph profiler that prunes context rot in-flight while preserving KV-cache hits.**

[![CI](https://github.com/abhinavm-web/ctx-firewall/actions/workflows/ci.yml/badge.svg)](https://github.com/abhinavm-web/ctx-firewall/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/abhinavm-web/ctx-firewall)](https://github.com/abhinavm-web/ctx-firewall/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](Cargo.toml)

<p align="center">
  <img src="assets/demo.svg" width="860" alt="ctx-firewall terminal flamegraph demo">
</p>

---

## Overview

Heavy AI coding sessions degrade after 15–25 turns ("context rot"). A handful of silent offenders—repeated `read_file` outputs of the same file, megabytes of ANSI-colored test runner output, and dozens of unused MCP tool schemas—consume 70%+ of the context window.

Existing context profilers are passive viewers: they draw a chart showing your context is bloated and leave you to manually compact it. Worse, naive prompt compressors mutate early messages in the conversation history, invalidating Anthropic and OpenAI **KV-cache prefixes** and multiplying input token costs by up to 10x.

`ctx-firewall` is a drop-in local HTTP/SSE proxy (`ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`) with a live `ratatui` terminal dashboard that actively prunes context rot in-flight while enforcing strict cache-prefix immutability:

- **Duplicate File Read Deduplication**: Detects repeated reads of the same file across turns and replaces superseded copies outside the frozen cache prefix with compact tombstones (`[ctx-firewall: duplicate read of <path> elided]`).
- **ANSI & Progress-Bar Stripping**: Strips terminal escape sequences, carriage-return spinner frames, and redundant build output from tool results.
- **Head/Tail Log Folding**: Compresses multi-thousand-line test or compiler outputs while preserving the header context and failing tail stack traces.
- **Lazy MCP Schema Stubbing**: Compresses unreferenced MCP tool JSON schemas into lightweight stubs until referenced in the active conversation window.
- **Cryptographic KV-Cache Prefix Guard**: Computes SHA-256 checksums over `cache_control` / static prefix boundaries and guarantees byte-for-byte prefix preservation before forwarding upstream.

```text
  Coding Agent (Claude Code / Cursor / Codex / Custom)
                         |
            ANTHROPIC_BASE_URL / OPENAI_BASE_URL
                         v
  +---------------------------------------------------+
  |                   ctx-firewall                    |
  |  +-----------------+   +-----------------------+  |
  |  | Ratatui Live    |   | Cache-Safe Pruner     |  |
  |  | Flamegraph TUI  |   | - Dedupe file reads   |  |
  |  | + Rot Score     |   | - Strip ANSI/log noise|  |
  |  +-----------------+   | - Fold verbose stdout |  |
  |                        | - Lazy MCP schemas    |  |
  |                        +----------+------------+  |
  +-----------------------------------|---------------+
                                      v
                    Upstream LLM API (KV-Cache Intact)
```

## Installation & 30-Second Showcase

Install prebuilt binaries with one command, download directly from [GitHub Releases](https://github.com/abhinavm-web/ctx-firewall/releases/latest), or build from source:

```bash
# macOS (Apple Silicon) & Linux (x86_64)
curl -fsSL https://raw.githubusercontent.com/abhinavm-web/ctx-firewall/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/abhinavm-web/ctx-firewall/main/install.ps1 | iex
```

Or run from source:

```bash
git clone https://github.com/abhinavm-web/ctx-firewall.git
cd ctx-firewall

# Run the self-contained KV-cache-safe pruning showcase
cargo run --example showcase

# Or install the proxy & TUI binary globally
cargo install --path .
```

## Usage

### 1. Start the active firewall proxy with live TUI flamegraph

```bash
# Starts proxy on port 8080 with interactive TUI flamegraph
ctx-firewall serve --port 8080

# Or run in headless mode for CI / background daemon usage
ctx-firewall serve --port 8080 --no-tui
```

### 2. Point your coding agent at the proxy

```bash
# Claude Code / Anthropic clients
ANTHROPIC_BASE_URL=http://127.0.0.1:8080 claude

# Codex / OpenAI clients
OPENAI_BASE_URL=http://127.0.0.1:8080/v1 codex
```

### 3. Audit a captured request payload offline

```bash
# Analyze a captured Anthropic or OpenAI JSON request for context rot and token savings
ctx-firewall check --file request.json

# View cumulative token and cost savings
ctx-firewall stats
```

### Selective Pruning Flags

Each pruning pass can be toggled independently:

```bash
ctx-firewall serve \
  --port 8080 \
  --upstream-anthropic https://api.anthropic.com \
  --upstream-openai https://api.openai.com \
  --no-ansi      # disable ANSI stripping
  --no-fold      # disable log folding
  --no-dedup     # disable duplicate file read deduplication
  --no-lazy-mcp  # disable lazy MCP schema stubbing
```

## Architecture

```text
src/
├── config.rs           # Clap CLI & Serve/Check/Stats configuration
├── models/
│   ├── anthropic.rs    # Anthropic Messages API & cache_control models
│   └── openai.rs       # OpenAI Chat Completions & tool models
├── prune/
│   ├── ansi.rs         # CSI/OSC escape sequence & carriage-return stripper
│   ├── dedup.rs        # Superseded file-read detector & tombstone rewriter
│   ├── logs.rs         # Head/tail log folder for large tool outputs
│   ├── mcp.rs          # Context-aware MCP tool schema stubber
│   └── cache_guard.rs  # SHA-256 KV-cache prefix boundary verifier
├── proxy/
│   ├── routes.rs       # Axum endpoints (/v1/messages, /v1/chat/completions, /health)
│   ├── client.rs       # Upstream streaming HTTP forwarder
│   └── sse.rs          # Server-Sent Events usage & cache telemetry parser
├── metrics/
│   ├── rot.rs          # Context Rot Score (0-100) calculator
│   └── pricing.rs      # Token & dollar savings estimator
└── tui/
    ├── app.rs          # Interactive terminal state & event loop
    ├── ui.rs           # Dashboard layout, metrics cards, and activity log
    └── stacked_bar.rs  # Horizontal context flamegraph bar widget
```

## Multi-Turn A/B Benchmark & Safety Guarantees

Reproduce on your machine:

```bash
cargo run --release --example benchmark
```

**Built-In Safety Guarantees:**
- **Session-Stateful KV-Cache Pinning**: Pruned tool outputs are cached by `(tool_use_id, sha256)` so subsequent turns stay byte-identical to earlier turns across the upstream prefix (`SealedPrefixGuard` verifies SHA-256 immutability on every turn).
- **Immediate Re-Read Recovery**: If an agent receives a deduplication tombstone on Turn `N` and immediately calls `read_file` for the same file on Turn `N+1`, `ctx-firewall` detects the pushback and passes the full unpruned file through.
- **No Blind Truncation on File Reads**: Log folding applies strictly to shell/command outputs (`bash`, `run_command`), never silently dropping lines inside `read_file` source reads.

| Metric (15-Turn Coding Session) | Unpruned Baseline | `ctx-firewall` | Delta / Status |
| :--- | ---: | ---: | :--- |
| Billed Input Tokens | 130,074 | 51,562 | **-60.4%** |
| Eligible Prefix Cache Hits | — | 13 / 13 turns | **100.0% preserved** |
| Immediate Re-Read Recovery | — | Full file restored on retry | **Verified** |
| Proxy Overhead | 0 ms | 8.21 ms total | **0.55 ms / turn** |

## Development & Verification

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Related Tools (Agent Systems Toolkit)

| Repository | Focus |
| :--- | :--- |
| **[ctx-firewall](https://github.com/abhinavm-web/ctx-firewall)** | Active LLM context proxy & Ratatui flamegraph profiler that prunes context rot without breaking KV-cache |
| **[agent-fork](https://github.com/abhinavm-web/agent-fork)** | OS-level Copy-on-Write time-travel debugger & parallel trajectory forker for coding agents |
| **[diffmem](https://github.com/abhinavm-web/diffmem)** | Zero-touch "Diff-of-Diffs" episodic failure memory across Claude Code, Cursor, and AGENTS.md |
| **[reddiff](https://github.com/abhinavm-web/reddiff)** | Zero-prose adversarial diff breaker: runnable crash PoC or silence |
| **[jitpipe](https://github.com/abhinavm-web/jitpipe)** | JIT-compiled semantic Unix pipes (`sgrep`, `sawk`, `sjq`) streaming at native Rust speed |

## License

[MIT](LICENSE)
