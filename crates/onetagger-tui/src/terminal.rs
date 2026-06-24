use std::io::{self, Stdout};
use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Enter the alternate screen + raw mode and install a panic hook that restores the terminal
/// before the default panic handler runs (so a crash never leaves the shell in raw mode).
pub fn init() -> Result<Tui> {
    // Install the restoring panic hook first, so a panic during setup still restores the terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen) {
        let _ = restore();
        return Err(e.into());
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok(terminal),
        Err(e) => { let _ = restore(); Err(e.into()) }
    }
}

/// Restore the terminal to its normal state. Safe to call more than once.
pub fn restore() -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    Ok(())
}
