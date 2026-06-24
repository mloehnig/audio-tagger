use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem};
use onetagger_autotag::TaggingState;
use crate::run_state::RunState;

pub fn render(frame: &mut Frame, area: Rect, run: &RunState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);

    // Top: progress + counts
    let pct = (run.progress.clamp(0.0, 1.0) * 100.0) as u16;
    let header = format!(
        " {}   matched {}  failed {}  skipped {} ",
        if run.platform.is_empty() { "…" } else { &run.platform },
        run.ok, run.failed, run.skipped
    );
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(
            if run.done { " done " } else if run.stopping { " stopping… " } else { " tagging " }
        ))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(pct)
        .label(header);
    frame.render_widget(gauge, chunks[0]);

    // Bottom: recent results
    let items: Vec<ListItem> = run.recent.iter().take((chunks[1].height as usize).saturating_sub(2)).map(|r| {
        let (icon, color) = match r.state {
            TaggingState::Ok => ("✓", Color::Green),
            TaggingState::Error => ("✗", Color::Red),
            TaggingState::Skipped => ("⊘", Color::Yellow),
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::raw(format!("{:<32} {}", trunc(&r.label, 32), r.detail)),
        ]))
    }).collect();
    let footer = if run.done { " any key = back " } else { " q = stop " };
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(" recent ·{footer}")));
    frame.render_widget(list, chunks[1]);
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n.saturating_sub(1)).collect::<String>() + "…" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_counts_and_recent() {
        let mut run = RunState::new();
        run.ok = 2; run.progress = 0.5;
        run.recent.push_front(crate::run_state::RecentItem {
            state: TaggingState::Ok, label: "No Scrubs".to_string(), detail: "deezer 1.00".to_string(),
        });
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal.draw(|f| render(f, f.area(), &run)).unwrap();
        let content = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("matched 2"));
        assert!(content.contains("No Scrubs"));
    }
}
