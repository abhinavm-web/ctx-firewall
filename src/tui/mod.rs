pub mod app;
pub mod stacked_bar;
pub mod ui;

pub use app::{AppState, ContextBreakdown, PruneRule, TelemetryEvent};

use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::error::Error;
use std::io::stdout;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn run_tui(
    mut rx: UnboundedReceiver<TelemetryEvent>,
    mut app: AppState,
) -> Result<(), Box<dyn Error>> {
    // 1. Setup terminal in raw mode
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    // 2. Panic hook to ensure terminal restoration
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        );
        orig_hook(panic_info);
    }));

    // 3. Main event loop
    let mut event_stream = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(100));

    while app.is_running {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        tokio::select! {
            Some(telem) = rx.recv() => {
                app.handle_telemetry(telem);
            }
            Some(Ok(event)) = event_stream.next() => {
                if let Event::Key(key_event) = event {
                    app.handle_key(key_event);
                }
            }
            _ = ticker.tick() => {
                app.on_tick();
            }
        }
    }

    // 4. Clean restoration
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    )?;
    terminal.show_cursor()?;

    Ok(())
}

pub async fn run_headless_logger(mut rx: UnboundedReceiver<TelemetryEvent>) {
    tracing::info!("Headless monitoring active (logging telemetry)");
    while let Some(event) = rx.recv().await {
        match event {
            TelemetryEvent::RequestReceived {
                turn_id,
                model,
                breakdown,
                rot_score,
            } => {
                tracing::info!(
                    turn_id,
                    model = %model,
                    total_tokens = breakdown.total_tokens,
                    rot_score,
                    "Request received"
                );
            }
            TelemetryEvent::PruneApplied {
                rule,
                tokens_saved,
                bytes_saved,
                description,
                ..
            } => {
                tracing::info!(
                    rule = %rule,
                    tokens_saved,
                    bytes_saved,
                    "{}",
                    description
                );
            }
            TelemetryEvent::StreamFinished {
                cache_hit,
                cached_tokens,
                output_tokens,
                latency_ms,
                ..
            } => {
                tracing::info!(
                    cache_hit,
                    cached_tokens,
                    output_tokens,
                    latency_ms,
                    "Stream finished"
                );
            }
            TelemetryEvent::StatusUpdate(msg) => {
                tracing::info!("{}", msg);
            }
        }
    }
}
