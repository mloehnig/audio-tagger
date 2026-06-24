#[macro_use] extern crate log;

mod run_state;
mod app;
mod terminal;
mod screens;

/// Entry point for the interactive TUI. (Stub — wired up in later tasks.)
pub fn run() -> anyhow::Result<()> {
    info!("TUI starting");
    Ok(())
}
