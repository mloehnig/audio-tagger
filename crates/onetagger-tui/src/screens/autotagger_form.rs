use std::path::PathBuf;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use onetagger_tagger::{TaggerConfig, SupportedTag};
use onetagger_autotag::{TaggerConfigExt, AudioFileInfoImpl};
use onetagger_tagger::AudioFileInfo;
use convert_case::{Casing, Case};
use crate::app::Action;
use crate::config::TuiDefaults;

/// A minimal Auto-tag form: edit the music path, platforms (comma list), and a dry-run toggle.
/// Other options come from config defaults / built-ins (fuller form is a later sub-project).
pub struct AutotaggerForm {
    pub path: String,
    pub platforms: String,
    pub dry_run: bool,
    pub field: usize, // 0=path, 1=platforms, 2=dry_run
    defaults: TuiDefaults,
}

impl AutotaggerForm {
    pub fn new(defaults: TuiDefaults) -> AutotaggerForm {
        let platforms = defaults.platforms.clone().unwrap_or_else(|| vec!["beatport".to_string()]).join(", ");
        AutotaggerForm { path: String::new(), platforms, dry_run: true, field: 0, defaults }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::Pop,
            KeyCode::Up => { if self.field > 0 { self.field -= 1; } Action::None }
            KeyCode::Down | KeyCode::Tab => { if self.field < 2 { self.field += 1; } Action::None }
            KeyCode::Char(' ') if self.field == 2 => { self.dry_run = !self.dry_run; Action::None }
            KeyCode::Backspace => {
                match self.field { 0 => { self.path.pop(); } 1 => { self.platforms.pop(); } _ => {} }
                Action::None
            }
            KeyCode::Char(c) => {
                match self.field { 0 => self.path.push(c), 1 => self.platforms.push(c), _ => {} }
                Action::None
            }
            KeyCode::Enter => self.start(),
            _ => Action::None,
        }
    }

    /// Build a TaggerConfig + file list and emit the start action (None if path empty/invalid).
    fn start(&self) -> Action {
        if self.path.trim().is_empty() { return Action::None; }
        let mut config = TaggerConfig::custom_default();
        config.platforms = self.platforms.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if let Some(threads) = self.defaults.threads { config.threads = threads; }
        if let Some(enable) = self.defaults.enable_shazam { config.enable_shazam = enable; }
        if let Some(tags) = &self.defaults.tags {
            let parsed: Vec<SupportedTag> = tags.iter().filter_map(|t| {
                match serde_json::from_str(&format!("\"{}\"", t.to_case(Case::Camel))) {
                    Ok(tag) => Some(tag),
                    Err(_) => { warn!("Invalid tag in config: {t}"); None }
                }
            }).collect();
            if !parsed.is_empty() { config.tags = parsed; }
        }
        config.dry_run = self.dry_run;
        config.preserve_original = true; // safe default in the TUI, like the CLI
        let files = AudioFileInfo::get_file_list(&self.path, config.include_subfolders);
        Action::StartAutotag(Box::new(config), files)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let dry = if self.dry_run { "[x]" } else { "[ ]" };
        let mark = |i: usize| if self.field == i { ">" } else { " " };
        let style = |i: usize| if self.field == i { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default() };
        let lines = vec![
            Line::styled(format!("{} Path:      {}", mark(0), self.path), style(0)),
            Line::styled(format!("{} Platforms: {}", mark(1), self.platforms), style(1)),
            Line::styled(format!("{} Dry-run:   {}", mark(2), dry), style(2)),
            Line::from(""),
            Line::from("Enter = run · Space = toggle dry-run · Esc = back"),
        ];
        let p = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Auto-tag "));
        frame.render_widget(p, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TuiDefaults;

    fn form(path: &str, platforms: &str) -> AutotaggerForm {
        let mut f = AutotaggerForm::new(TuiDefaults::default());
        f.path = path.to_string();
        f.platforms = platforms.to_string();
        f
    }

    #[test]
    fn empty_path_does_nothing() {
        assert!(matches!(form("", "deezer").start(), Action::None));
    }

    #[test]
    fn builds_config_with_trimmed_platforms() {
        match form(".", "deezer, , beatport ").start() {
            Action::StartAutotag(config, _files) => {
                assert_eq!(config.platforms, vec!["deezer".to_string(), "beatport".to_string()]);
            }
            _ => panic!("expected StartAutotag"),
        }
    }
}
