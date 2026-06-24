use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use crate::app::{Action, Screen};

/// Menu entries. SP1 enables only Auto-tag and Quit; others are present but inert this SP.
pub const ITEMS: [&str; 8] = [
    "Auto-tag", "Audio Features", "Apply changes", "Find Unprocessed",
    "Rename", "Authorize Spotify", "Settings", "Quit",
];

#[derive(Default)]
pub struct HomeScreen {
    pub selected: usize,
}

impl HomeScreen {
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up => { if self.selected > 0 { self.selected -= 1; } Action::None }
            KeyCode::Down => { if self.selected + 1 < ITEMS.len() { self.selected += 1; } Action::None }
            KeyCode::Enter => match ITEMS[self.selected] {
                "Auto-tag" => Action::Push(Screen::AutotaggerForm),
                "Settings" => Action::Push(Screen::Settings),
                "Quit" => Action::Quit,
                _ => Action::None, // enabled in later sub-projects
            },
            KeyCode::Char('q') => Action::Quit,
            _ => Action::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = ITEMS.iter().enumerate().map(|(i, label)| {
            let style = if i == self.selected {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!("  {label}"), style)))
        }).collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" OneTagger "));
        frame.render_widget(list, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_menu_items() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let screen = HomeScreen::default();
        terminal.draw(|f| screen.render(f, f.area())).unwrap();
        let content = terminal.backend().buffer().content().iter()
            .map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("Auto-tag"));
        assert!(content.contains("Quit"));
        assert!(content.contains("OneTagger"));
    }

    #[test]
    fn enter_on_autotag_pushes_form() {
        let mut screen = HomeScreen::default(); // selected = 0 = Auto-tag
        let action = screen.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(action, Action::Push(Screen::AutotaggerForm)));
    }

    #[test]
    fn down_then_q_quits() {
        let mut screen = HomeScreen::default();
        screen.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(screen.selected, 1);
        assert!(matches!(screen.handle_key(KeyEvent::from(KeyCode::Char('q'))), Action::Quit));
    }
}
