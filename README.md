# ctx-firewall

**Active LLM context proxy and TUI flamegraph profiler that prunes context rot in-flight while preserving KV-cache hits.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#roadmap--mvp-milestones)
[![Architecture: MVP](https://img.shields.io/badge/Architecture-MVP-orange.svg)](#roadmap--mvp-milestones)

---

## The Problem

Heavy AI users burn \$50+/day and wire up 15+ MCP servers, completely blind to why their coding agent suddenly degrades after 20 turns ("context rot"). Usually, a handful of silent offenders—verbose MCP tool schemas, repeated `read_file` outputs of the same file, and megabytes of ANSI-colored `npm test` stdout—quietly consume 70%+ of the attention budget.

Existing tools like `ContextSpy`, `ContextAnalyzerTerminal`, and transcript visualizers are **read-only (passive)**. They draw a chart telling you that stale reads and bloated tool definitions ate your context window, then leave you to fix it manually.

## The Unclaimed Wedge: How It Works

`ctx-firewall` is an **active, cache-aware context proxy** (`ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`) paired with a live terminal flamegraph (`htop` for your context window):

- **Live Context Flamegraph TUI**: Breaks down every request in real time by System Prompt, MCP Tool Schemas, Stale/Duplicate File Reads, Shell Output Bloat, and Conversation History, alongside a real-time **Context Rot Score**.
- **In-Flight Active Pruning**: Automatically strips ANSI escape spam, truncates redundant test logs, deduplicates superseded `read_file` tool results, and lazy-loads unused MCP tool schemas.
- **KV-Cache Prefix Preservation**: Unlike naive summarizers that mutate the front of the prompt and destroy Anthropic/OpenAI prompt caching (costing 10x more), `ctx-firewall` strictly respects cache-breakpoint prefixes and only compacts outside pinned prefix windows—tracking **"Tokens & Dollars Saved Today"** live in the status bar.

```text
  Coding Agent (Claude Code / Cursor / Codex / Custom)
                         │
            ANTHROPIC_BASE_URL / OPENAI_BASE_URL
                         ▼
  ┌───────────────────────────────────────────────────┐
  │                   ctx-firewall                    │
  │  ┌─────────────────┐   ┌───────────────────────┐  │
  │  │ Live Flamegraph │   │ Cache-Safe Pruner     │  │
  │  │ TUI + Rot Score │   │ - Dedupe file reads   │  │
  │  └─────────────────┘   │ - Strip ANSI/log noise│  │
  │                        │ - Lazy MCP schemas    │  │
  │                        └──────────┬────────────┘  │
  └───────────────────────────────────┼───────────────┘
                                      ▼
                    Upstream LLM API (KV-Cache Intact)
```

### Target CLI Experience (`< 60 second aha moment`)

> Note: Below is the target MVP CLI specification currently under construction.

```bash
# 1. Start the active firewall proxy + live TUI flamegraph
npx ctx-firewall --mode active

# 2. Point any coding agent at the proxy
ANTHROPIC_BASE_URL=http://127.0.0.1:8787 claude

# Live TUI output:
# ┌─ ctx-firewall (Cache Hit: 94.2% | Saved Today: $14.82 / 1.9M tokens) ──────┐
# │ Turn 24 | Context: 82k / 200k (Pruned: 61k) | Rot Score: LOW (14/100)      │
# │ [SYS 6%] [MCP 9% ▼] [STALE FILES 0% (deduped 4x)] [LOGS 11% ▼] [CHAT 74%]  │
# │ ✂ Collapsed 3 duplicate reads of src/server.ts (-18,420 tokens)            │
# │ ✂ Stripped ANSI progress bars from `cargo test` (-9,110 tokens)            │
# └────────────────────────────────────────────────────────────────────────────┘
```

## Roadmap & MVP Milestones

- [ ] **Milestone 1 — Transparent Proxy & Inspector**: Streaming SSE pass-through proxy supporting Anthropic Messages API and OpenAI Chat/Responses API with exact per-block token attribution.
- [ ] **Milestone 2 — TUI Flamegraph Dashboard**: Live terminal UI displaying context breakdown bars, prompt-cache hit ratios, and Context Rot Score.
- [ ] **Milestone 3 — Cache-Safe Active Middleware**: Pluggable pruning rules (duplicate `read_file` tombstoning, ANSI/progress-bar stripping, tail-preserving log folding) with KV-cache prefix boundary validation.
- [ ] **Milestone 4 — Dynamic MCP Schema Gate**: On-demand MCP tool schema router that keeps only active/matched tool definitions in the prompt.

## License

[MIT](LICENSE)
