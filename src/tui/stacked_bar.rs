use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

pub fn build_stacked_bar_spans<'a>(
    width: usize,
    segments: &[(&'a str, f64, Color)],
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut allocated = 0;

    for (label, pct, color) in segments {
        if *pct <= 0.0 || width == 0 {
            continue;
        }
        let seg_width = ((*pct / 100.0) * width as f64).round() as usize;
        let seg_width = seg_width.min(width.saturating_sub(allocated));
        if seg_width == 0 {
            continue;
        }

        allocated += seg_width;
        let text = if seg_width >= label.len() + 4 {
            format!(" [{:.0}%] ", pct)
        } else {
            "█".repeat(seg_width)
        };

        let padded = if text.len() < seg_width {
            let pad = seg_width - text.len();
            format!("{}{}", text, " ".repeat(pad))
        } else {
            text.chars().take(seg_width).collect::<String>()
        };

        spans.push(Span::styled(
            padded,
            Style::default()
                .fg(Color::Black)
                .bg(*color)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if allocated < width {
        spans.push(Span::styled(
            "░".repeat(width - allocated),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stacked_bar_total_width() {
        let spans = build_stacked_bar_spans(
            50,
            &[
                ("SYS", 20.0, Color::Magenta),
                ("FILES", 40.0, Color::Yellow),
                ("CHAT", 40.0, Color::Cyan),
            ],
        );
        let total_chars: usize = spans.iter().map(|s| s.content.len()).sum();
        assert_eq!(total_chars, 50);
    }
}
