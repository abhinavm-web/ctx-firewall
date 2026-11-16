use ctx_firewall::tui::{
    app::{AppState, ContextBreakdown, PruneRule, TelemetryEvent},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn test_render_full_dashboard() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut app = AppState::new(8787);
    app.handle_telemetry(TelemetryEvent::RequestReceived {
        turn_id: 24,
        model: "claude-3-5-sonnet".to_string(),
        breakdown: ContextBreakdown {
            system_tokens: 1200,
            tools_tokens: 1800,
            file_content_tokens: 4400,
            logs_tokens: 2200,
            history_tokens: 10400,
            total_tokens: 20000,
        },
        rot_score: 14,
    });

    app.handle_telemetry(TelemetryEvent::PruneApplied {
        turn_id: 24,
        rule: PruneRule::FileDeduplication,
        tokens_saved: 18420,
        bytes_saved: 70000,
        description: "Collapsed 3 duplicate reads of src/server.ts".to_string(),
    });

    app.handle_telemetry(TelemetryEvent::StreamFinished {
        turn_id: 24,
        cache_hit: true,
        cached_tokens: 16000,
        uncached_tokens: 4000,
        output_tokens: 250,
        latency_ms: 420,
    });

    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let content = format!("{:?}", buffer);

    assert!(content.contains("ctx-firewall"));
    assert!(content.contains("8787"));
    assert!(content.contains("Cache Hit"));
    assert!(content.contains("LOW"));
    assert!(content.contains("DEDUP"));
    assert!(content.contains("Collapsed 3 duplicate reads"));
}

#[test]
fn test_render_small_terminal_resilience() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = AppState::new(8787);

    let result = terminal.draw(|f| ui::draw(f, &mut app));
    assert!(result.is_ok(), "Small terminal render panicked!");
}

#[test]
fn test_render_rot_score_levels() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    for (score, expected_tag) in [(10, "LOW"), (45, "MEDIUM"), (85, "HIGH")] {
        let mut app = AppState::new(8787);
        app.current_rot_score = score;
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(buffer.contains(expected_tag), "Failed for rot score {}", score);
    }
}

#[test]
fn test_render_activity_log_scrolling() {
    let backend = TestBackend::new(100, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = AppState::new(8080);

    for i in 1..=10 {
        app.handle_telemetry(TelemetryEvent::PruneApplied {
            turn_id: i,
            rule: PruneRule::AnsiStripping,
            tokens_saved: i as usize * 100,
            bytes_saved: i as usize * 400,
            description: format!("Action item #{}", i),
        });
    }

    // Scroll up
    app.log_scroll_offset = 2;
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    let buffer = format!("{:?}", terminal.backend().buffer());
    assert!(buffer.contains("Action item"));
}
