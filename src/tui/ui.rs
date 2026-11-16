use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::tui::app::{AppState, PruneRule};
use crate::tui::stacked_bar::build_stacked_bar_spans;

pub fn draw(f: &mut Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(4), // KPI cards
            Constraint::Length(7), // Flamegraph / Breakdown
            Constraint::Min(6),    // Activity Log
            Constraint::Length(1), // Keybindings Footer
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_kpis(f, app, chunks[1]);
    draw_breakdown(f, app, chunks[2]);
    draw_activity_log(f, app, chunks[3]);
    draw_footer(f, chunks[4]);
}

fn draw_header(f: &mut Frame, app: &AppState, area: Rect) {
    let status_span = if app.is_paused {
        Span::styled(
            " PAUSED ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ACTIVE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    };

    let title_line = Line::from(vec![
        Span::styled(
            "ctx-firewall ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ Status: "),
        status_span,
        Span::raw(format!(
            " │ Port: {} │ Model: {} │ Uptime: {:02}:{:02}:{:02}",
            app.port,
            app.active_model,
            app.uptime.as_secs() / 3600,
            (app.uptime.as_secs() % 3600) / 60,
            app.uptime.as_secs() % 60
        )),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let p = Paragraph::new(title_line).block(block);
    f.render_widget(p, area);
}

fn draw_kpis(f: &mut Frame, app: &AppState, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    // KPI 1: Cache Hit Rate
    let hit_rate = app.cache_hit_rate();
    let hit_color = if hit_rate >= 80.0 {
        Color::Green
    } else if hit_rate >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    };
    let kpi1 = Paragraph::new(vec![
        Line::from(Span::styled(
            "Cache Hit Rate",
            Style::default().fg(Color::Gray),
        )),
        Line::from(vec![
            Span::styled(
                format!("{:.1}% ", hit_rate),
                Style::default().fg(hit_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({}/{})", app.cache_hits, app.total_requests),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(kpi1, cols[0]);

    // KPI 2: Tokens Saved
    let kpi2 = Paragraph::new(vec![
        Line::from(Span::styled(
            "Tokens Saved",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            format!("{} tok", format_number(app.total_tokens_saved)),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(kpi2, cols[1]);

    // KPI 3: USD Saved Today
    let kpi3 = Paragraph::new(vec![
        Line::from(Span::styled(
            "USD Saved Today",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            format!("${:.2}", app.total_usd_saved),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(kpi3, cols[2]);

    // KPI 4: Context Rot Score
    let rot = app.current_rot_score;
    let (rot_label, rot_color) = if rot <= 25 {
        ("LOW", Color::Green)
    } else if rot <= 60 {
        ("MEDIUM", Color::Yellow)
    } else {
        ("HIGH", Color::Red)
    };
    let kpi4 = Paragraph::new(vec![
        Line::from(Span::styled(
            "Context Rot Score",
            Style::default().fg(Color::Gray),
        )),
        Line::from(vec![
            Span::styled(
                format!("{}/100 ", rot),
                Style::default().fg(rot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("[{}]", rot_label), Style::default().fg(rot_color)),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(kpi4, cols[3]);
}

fn draw_breakdown(f: &mut Frame, app: &AppState, area: Rect) {
    let b = &app.breakdown;
    let total = b.total_tokens.max(1);
    let capacity = 200_000usize;

    let sys_pct = (b.system_tokens as f64 / total as f64) * 100.0;
    let tools_pct = (b.tools_tokens as f64 / total as f64) * 100.0;
    let files_pct = (b.file_content_tokens as f64 / total as f64) * 100.0;
    let logs_pct = (b.logs_tokens as f64 / total as f64) * 100.0;
    let hist_pct = (b.history_tokens as f64 / total as f64) * 100.0;

    let title = format!(
        " Context Breakdown (Turn #{} │ {} / {} tokens │ Headroom: {} tokens) ",
        app.current_turn,
        format_number(b.total_tokens),
        format_number(capacity),
        format_number(capacity.saturating_sub(b.total_tokens))
    );

    let inner_width = area.width.saturating_sub(2) as usize;
    let bar_spans = build_stacked_bar_spans(
        inner_width,
        &[
            ("SYS", sys_pct, Color::Magenta),
            ("MCP", tools_pct, Color::Blue),
            ("FILES", files_pct, Color::Yellow),
            ("LOGS", logs_pct, Color::Red),
            ("CHAT", hist_pct, Color::Cyan),
        ],
    );

    let legend_line = Line::from(vec![
        Span::styled("■ SYS: ", Style::default().fg(Color::Magenta)),
        Span::raw(format!("{:.1}% ({})  ", sys_pct, format_number(b.system_tokens))),
        Span::styled("■ MCP: ", Style::default().fg(Color::Blue)),
        Span::raw(format!("{:.1}% ({})  ", tools_pct, format_number(b.tools_tokens))),
        Span::styled("■ FILES: ", Style::default().fg(Color::Yellow)),
        Span::raw(format!(
            "{:.1}% ({})  ",
            files_pct,
            format_number(b.file_content_tokens)
        )),
        Span::styled("■ LOGS: ", Style::default().fg(Color::Red)),
        Span::raw(format!("{:.1}% ({})  ", logs_pct, format_number(b.logs_tokens))),
        Span::styled("■ CHAT: ", Style::default().fg(Color::Cyan)),
        Span::raw(format!("{:.1}% ({})", hist_pct, format_number(b.history_tokens))),
    ]);

    let content = vec![
        Line::raw(""),
        Line::from(bar_spans),
        Line::raw(""),
        legend_line,
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(Color::DarkGray));

    let p = Paragraph::new(content).block(block);
    f.render_widget(p, area);
}

fn draw_activity_log(f: &mut Frame, app: &AppState, area: Rect) {
    let title = format!(
        " Active Pruning Stream (Recent Actions: {}) ",
        app.activity_log.len()
    );
    let rows: Vec<Row> = app
        .activity_log
        .iter()
        .rev()
        .skip(app.log_scroll_offset)
        .map(|entry| {
            let (rule_color, rule_str) = match entry.rule {
                PruneRule::FileDeduplication => (Color::Yellow, "✂ DEDUP"),
                PruneRule::AnsiStripping => (Color::Magenta, "✂ ANSI "),
                PruneRule::LogFolding => (Color::Cyan, "✂ FOLD "),
                PruneRule::LazyMcpSchema => (Color::Blue, "✂ MCP  "),
            };

            Row::new(vec![
                Span::styled(&entry.timestamp, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    rule_str,
                    Style::default().fg(rule_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("-{} tok", format_number(entry.tokens_saved)),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(&entry.description),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10), // Time
        Constraint::Length(10), // Rule
        Constraint::Length(14), // Saved
        Constraint::Min(20),    // Details
    ];

    let table = Table::new(rows, widths).block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(table, area);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let p = Paragraph::new(Span::styled(
        " [q] Quit  [p] Pause/Resume  [r] Reset Stats  [c] Clear Log  [↑/↓] Scroll Log",
        Style::default().fg(Color::DarkGray),
    ))
    .alignment(Alignment::Center);
    f.render_widget(p, area);
}

pub fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
