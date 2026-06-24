#[macro_use] extern crate log;

mod run_state;
mod app;
mod terminal;
mod screens;
mod config;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use onetagger_autotag::{Tagger, TaggingStatusWrap};

use crate::app::{Action, App, Screen};
use crate::run_state::RunState;
use crate::screens::{home::HomeScreen, autotagger_form::AutotaggerForm};

pub fn run() -> Result<()> {
    info!("TUI starting");
    let mut tui = terminal::init()?;
    let result = run_app(&mut tui);
    terminal::restore()?;
    result
}

fn run_app(tui: &mut terminal::Tui) -> Result<()> {
    let mut app = App::new();
    let mut home = HomeScreen::default();
    let mut form: Option<AutotaggerForm> = None;
    let mut run: Option<RunState> = None;
    let mut rx: Option<crossbeam_channel::Receiver<TaggingStatusWrap>> = None;

    while !app.should_quit {
        // Draw current screen
        tui.draw(|f| {
            let area = f.area();
            match app.current() {
                Screen::Home => home.render(f, area),
                Screen::AutotaggerForm => { if let Some(form) = &form { form.render(f, area); } }
                Screen::Dashboard => { if let Some(run) = &run { screens::dashboard::render(f, area, run); } }
            }
        })?;

        // Drain engine status into the run state (non-blocking). A Disconnected channel
        // means the engine finished — mark the run done.
        if let (Some(rx_ref), Some(run_ref)) = (&rx, &mut run) {
            loop {
                match rx_ref.try_recv() {
                    Ok(wrap) => run_ref.apply(wrap),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => { run_ref.done = true; break; }
                }
            }
        }

        // Input (polled with a tick so the dashboard animates)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                let action = match app.current() {
                    Screen::Home => home.handle_key(key),
                    Screen::AutotaggerForm => form.as_mut().map(|f| f.handle_key(key)).unwrap_or(Action::None),
                    Screen::Dashboard => dashboard_keys(key, run.as_mut()),
                };
                match action {
                    Action::Push(Screen::AutotaggerForm) => {
                        form = Some(AutotaggerForm::new(config::load_defaults()));
                        app.apply_action(Action::Push(Screen::AutotaggerForm));
                    }
                    Action::StartAutotag(config, files) => {
                        let mut state = RunState::new();
                        rx = None; // clear any receiver from a previous run
                        if files.is_empty() {
                            state.done = true;
                        } else {
                            // TODO(SP-later): surface TaggerFinishedData (success/failed m3u paths) in the dashboard
                            rx = Some(Tagger::tag_files(&config, files, Arc::new(Mutex::new(None))));
                        }
                        run = Some(state);
                        app.apply_action(Action::Push(Screen::Dashboard));
                    }
                    other => app.apply_action(other),
                }
            }
        }
    }
    Ok(())
}

/// Dashboard keys: while running, `q` stops; once done, any key returns to the form/home.
fn dashboard_keys(key: event::KeyEvent, run: Option<&mut RunState>) -> Action {
    let done = run.as_ref().map(|r| r.done).unwrap_or(true);
    if done {
        return Action::Pop;
    }
    if let KeyCode::Char('q') = key.code {
        onetagger_autotag::STOP_TAGGING.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(run) = run { run.stopping = true; }
    }
    Action::None
}
