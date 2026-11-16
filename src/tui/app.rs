use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub use crate::metrics::ContextBreakdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneRule {
    AnsiStripping,
    LogFolding,
    FileDeduplication,
    LazyMcpSchema,
}

impl std::fmt::Display for PruneRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PruneRule::AnsiStripping => write!(f, "ANSI"),
            PruneRule::LogFolding => write!(f, "FOLD"),
            PruneRule::FileDeduplication => write!(f, "DEDUP"),
            PruneRule::LazyMcpSchema => write!(f, "MCP"),
        }
    }
}

impl PruneRule {
    pub fn from_str_name(name: &str) -> Self {
        match name {
            "ansi_strip" | "ANSI" => PruneRule::AnsiStripping,
            "log_folding" | "FOLD" => PruneRule::LogFolding,
            "file_dedup" | "DEDUP" => PruneRule::FileDeduplication,
            "lazy_mcp" | "MCP" => PruneRule::LazyMcpSchema,
            _ => PruneRule::FileDeduplication,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityLogEntry {
    pub timestamp: String,
    pub rule: PruneRule,
    pub tokens_saved: usize,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    RequestReceived {
        turn_id: u64,
        model: String,
        breakdown: ContextBreakdown,
        rot_score: u8,
    },
    PruneApplied {
        turn_id: u64,
        rule: PruneRule,
        tokens_saved: usize,
        bytes_saved: usize,
        description: String,
    },
    StreamFinished {
        turn_id: u64,
        cache_hit: bool,
        cached_tokens: usize,
        uncached_tokens: usize,
        output_tokens: usize,
        latency_ms: u64,
    },
    StatusUpdate(String),
}

pub struct AppState {
    pub is_running: bool,
    pub is_paused: bool,
    pub start_time: Instant,
    pub uptime: Duration,
    pub active_model: String,
    pub current_turn: u64,
    pub port: u16,

    // Latest request breakdown
    pub breakdown: ContextBreakdown,
    pub current_rot_score: u8,

    // Cumulative metrics
    pub total_requests: u64,
    pub cache_hits: u64,
    pub total_tokens_saved: usize,
    pub total_bytes_saved: usize,
    pub total_usd_saved: f64,
    pub total_cached_tokens: usize,
    pub total_uncached_tokens: usize,

    // Activity log with scroll
    pub activity_log: VecDeque<ActivityLogEntry>,
    pub max_log_entries: usize,
    pub log_scroll_offset: usize,
}

impl AppState {
    pub fn new(port: u16) -> Self {
        Self {
            is_running: true,
            is_paused: false,
            start_time: Instant::now(),
            uptime: Duration::ZERO,
            active_model: "claude-3-5-sonnet".to_string(),
            current_turn: 0,
            port,
            breakdown: ContextBreakdown::default(),
            current_rot_score: 0,
            total_requests: 0,
            cache_hits: 0,
            total_tokens_saved: 0,
            total_bytes_saved: 0,
            total_usd_saved: 0.0,
            total_cached_tokens: 0,
            total_uncached_tokens: 0,
            activity_log: VecDeque::with_capacity(250),
            max_log_entries: 200,
            log_scroll_offset: 0,
        }
    }

    pub fn on_tick(&mut self) {
        self.uptime = self.start_time.elapsed();
    }

    pub fn handle_telemetry(&mut self, event: TelemetryEvent) {
        match event {
            TelemetryEvent::RequestReceived {
                turn_id,
                model,
                breakdown,
                rot_score,
            } => {
                self.current_turn = turn_id;
                self.active_model = model;
                self.breakdown = breakdown;
                self.current_rot_score = rot_score;
                self.total_requests += 1;
            }
            TelemetryEvent::PruneApplied {
                rule,
                tokens_saved,
                bytes_saved,
                description,
                ..
            } => {
                self.total_tokens_saved += tokens_saved;
                self.total_bytes_saved += bytes_saved;
                // Anthropic Sonnet benchmark: $3.00 per MTok uncached
                self.total_usd_saved += (tokens_saved as f64 / 1_000_000.0) * 3.00;

                let now = chrono::Local::now().format("%H:%M:%S").to_string();
                if self.activity_log.len() >= self.max_log_entries {
                    self.activity_log.pop_front();
                }
                self.activity_log.push_back(ActivityLogEntry {
                    timestamp: now,
                    rule,
                    tokens_saved,
                    description,
                });
            }
            TelemetryEvent::StreamFinished {
                cache_hit,
                cached_tokens,
                uncached_tokens,
                ..
            } => {
                if cache_hit {
                    self.cache_hits += 1;
                }
                self.total_cached_tokens += cached_tokens;
                self.total_uncached_tokens += uncached_tokens;
                // Anthropic cache discount: saving $2.70 per MTok on cached tokens
                self.total_usd_saved += (cached_tokens as f64 / 1_000_000.0) * 2.70;
            }
            TelemetryEvent::StatusUpdate(_) => {}
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.is_running = false,
            KeyCode::Char('p') => self.is_paused = !self.is_paused,
            KeyCode::Char('r') => self.reset_stats(),
            KeyCode::Char('c') => self.activity_log.clear(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_scroll_offset = self.log_scroll_offset.saturating_add(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn reset_stats(&mut self) {
        self.total_requests = 0;
        self.cache_hits = 0;
        self.total_tokens_saved = 0;
        self.total_bytes_saved = 0;
        self.total_usd_saved = 0.0;
        self.total_cached_tokens = 0;
        self.total_uncached_tokens = 0;
        self.activity_log.clear();
        self.log_scroll_offset = 0;
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.cache_hits as f64 / self.total_requests as f64) * 100.0
        }
    }
}
