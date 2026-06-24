use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::TextArea;
use crate::app::Action;
use crate::config;

/// In-TUI raw-TOML editor for the user config file.
pub struct SettingsScreen {
    textarea: TextArea<'static>,
    status: String,
}

impl SettingsScreen {
    pub fn new() -> SettingsScreen {
        let lines: Vec<String> = config::config_text().lines().map(|l| l.to_string()).collect();
        let mut textarea = TextArea::new(lines);
        textarea.set_block(Block::default().borders(Borders::ALL).title(" Settings — config.toml "));
        SettingsScreen { textarea, status: String::from("Ctrl-S save · Esc back") }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.code == KeyCode::Esc {
            return Action::Pop;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            let text = self.textarea.lines().join("\n");
            self.status = match config::save(&text) {
                Ok(_) => format!("saved {}", config::config_path().display()),
                Err(e) => format!("save failed: {e}"),
            };
            return Action::None;
        }
        self.textarea.input(key);
        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        frame.render_widget(&self.textarea, chunks[0]);
        let footer = Paragraph::new(Line::from(format!(" {}", self.status)))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(footer, chunks[1]);
    }
}
