# Design: Interactive TUI — architecture & roadmap

## Context

OneTagger is now CLI-only (the desktop GUI and web UI were removed). As a middle ground for
users who still want something more visual than flags, we're adding a **terminal UI (TUI)** —
ultimately a full interactive front-end that can drive every subcommand (auto-tag, audio
features, apply, find-unprocessed, rename, authorize Spotify, settings), with a live progress
dashboard for long-running operations.

This document is the **overarching architecture + roadmap** for the whole TUI. It is large, so
it is decomposed into sub-projects (SP1–SP4); **each sub-project gets its own spec and
implementation plan**. SP1 is detailed here enough to proceed to a plan; SP2–SP4 are scoped at
a high level and will be specced when reached.

## Decisions (from brainstorming)

- **Style:** full ratatui dashboard (rich, full-screen), not just a progress bar.
- **Code location:** a new library crate `crates/onetagger-tui`, depended on by `onetagger-cli`
  (the CLI stays thin and just launches it).
- **Entry point:** a single `onetagger-cli tui` subcommand that opens the home menu; everything
  is driven from inside the TUI (no per-command `--tui` flags).
- **Interactivity:** live view + a quit/stop key (`q`) that stops tagging via the existing
  `STOP_TAGGING` atomic. No pause (engine has no pause support).
- **Navigation/state model:** screen-stack + central `App` state + `Action` messages (Elm-ish).

## Architecture

### Crate & entry
- New `crates/onetagger-tui` (lib) with `pub fn run() -> anyhow::Result<()>`. Depends on the
  engine crates (`onetagger-autotag`, `onetagger-tagger`, `onetagger-shared`, `onetagger-renamer`,
  `onetagger-platforms`, `onetagger-playlist`) and `ratatui` + `crossterm`.
- `onetagger-cli` gains a `Tui` clap subcommand whose handler calls `onetagger_tui::run()`.
- `onetagger-cli`'s user-config logic (`user_config`) is reused for form defaults. To avoid a
  cyclic dependency (cli → tui), the shared user-config + `TaggerConfig`-building helpers that
  both the CLI and TUI need will live where both can reach them: either moved into
  `onetagger-tui` (and the CLI depends on tui) or a small shared module. **SP1 decision:** the
  TUI reads `config.toml` via its own thin loader reusing `onetagger_shared::Settings::get_folder()`
  and the `toml`/serde types; the CLI's `user_config` is not imported by the TUI (no cycle).
  (If duplication becomes painful, a later refactor can extract a `onetagger-config` crate.)

### Terminal lifecycle & logging
- On start: enter the alternate screen and raw mode (crossterm). A RAII `TerminalGuard` (Drop)
  plus a panic hook **always restore** the terminal (leave raw mode / leave alternate screen),
  so a crash never leaves the user's shell broken.
- While the TUI is active, application logs must not write to the terminal. `onetagger_shared`
  logging gains a **file-only mode** (no stderr console chain; keep the `onetagger.log` file
  chain). The TUI initializes logging in this mode. Engine `info!/warn!/error!` then go to the
  log file only; the TUI surfaces what the user needs on screen.

### App core (ratatui + crossterm)
- `App { stack: Vec<Screen>, state: AppState, should_quit: bool }`.
  - `Screen` is an enum (Home, AutotaggerForm, RunDashboard, ChangesReview, AudioFeaturesForm,
    Renamer, AuthorizeSpotify, Settings, FilePicker). The stack enables push/pop navigation.
  - `AppState` holds cross-screen data (selected path, in-progress form values, the active
    `RunState`, loaded config defaults, last results).
- Event loop:
  1. `terminal.draw(|f| render(f, &app))`.
  2. Poll crossterm events with a tick timeout (~100 ms).
  3. On a key event: route to the top screen's `handle_key`, which returns `Option<Action>`.
  4. On each tick: drain any active engine status channel into `state` (keeps the dashboard
     animating without input).
  5. Apply `Action`s centrally (push/pop screen, start a run, set state, quit).
- Global keys: `Esc`/`q` pop the current screen (quit at Home); `Ctrl-C` aborts.

### Engine integration
- A form builds a `TaggerConfig` (or `AudioFeaturesConfig`) from its fields, pre-filled from
  `config.toml` `[defaults]`.
- The Run dashboard launches the engine — `Tagger::tag_files(...)` /
  `AudioFeatures::start_tagging(...)`, both returning `Receiver<TaggingStatusWrap>` — and the
  event loop drains the receiver each tick into:
  `RunState { progress: f64, ok: u32, failed: u32, skipped: u32, platform: String,
  recent: VecDeque<TaggingStatus>, started_at: Instant, done: bool }`.
- `q` in the dashboard sets `STOP_TAGGING`; the run ends, the dashboard shows the final summary.

### Rendering & UX
- Consistent header (context/title) and footer (keybind hints). Color-coded ✓/✗/⊘. Unicode
  box-drawing matching the approved mockup. Responsive layout; a minimum-size guard renders a
  "terminal too small" message rather than panicking.

### Testing
- Keep `render` thin and side-effect-free. Put logic in pure, unit-testable functions:
  - a reducer `apply_status(&mut RunState, TaggingStatusWrap)` (counts, progress, recent ring),
  - form validation / `TaggerConfig` construction,
  - navigation (`apply_action(&mut App, Action)`).
- Use ratatui's `TestBackend` for buffer-snapshot tests of key widgets (dashboard, home menu).

## Sub-project roadmap

Each is independently shippable and gets its own spec → plan → implementation.

### SP1 — Foundation + first vertical slice (next)
The crate + app shell + the minimal path to actually run a tag job from the TUI:
- `crates/onetagger-tui` scaffold; `onetagger-cli tui` subcommand.
- Terminal lifecycle (`TerminalGuard` + panic hook) and `onetagger_shared` file-only logging mode.
- App core: `App`, `Screen` enum, `Action`, event loop with tick + channel draining, global keys.
- **Home menu** (full list rendered; non-SP1 entries may be present-but-stubbed).
- **Autotagger form** — a basic but usable form (path, platforms, tags, threads, dry-run,
  enable-shazam) pre-filled from `config.toml` defaults.
- **Run dashboard** — the approved live view, wired to `Tagger::tag_files` and
  `AudioFeatures::start_tagging`; `q` stops.
- **Settings screen** — an in-TUI raw-TOML editor for `config.toml` (`tui-textarea` editable
  text area; Ctrl-S saves at `0600`, Esc cancels; pre-fills a commented template when absent).
- Tests: `apply_status` reducer, navigation, a dashboard `TestBackend` snapshot, config save round-trip.
Delivers: "open the TUI, edit your config, and configure + run an auto-tag (or audio-features)
job with a live dashboard."

### SP2 — Full forms, pickers, more commands
- Reusable file/directory **picker** widget.
- Complete option forms (all autotagger/audiofeatures fields incl. AcoustID key, strictness,
  in-place, output suffix, force-shazam, shazam throttle).
- **Authorize Spotify** screen (show URL, run callback server, wait, report).
- (A structured per-field Settings editor may build on SP1's raw-TOML editor here.)

### SP3 — Interactive dry-run review & apply
- After a dry-run, a **changes review** screen: browse per-file proposed tag changes, edit
  values, toggle files in/out, then Apply (reusing `ChangesDocument`).
- **Find Unprocessed** view.

### SP4 — Renamer + polish
- **Renamer** screen: template entry with a live preview list.
- Theming, small-terminal handling, keybinding help overlay, final polish.

## Error handling (whole TUI)
- Terminal always restored (guard + panic hook).
- Engine/IO errors become on-screen messages or a results row; the event loop never crashes on
  a single failure.
- Bad user input in forms is validated before launching a run (clear inline message).

## Out of scope (for the whole TUI)
- Mouse support (keyboard-first; can revisit).
- Remote/headless operation (the TUI requires a TTY; non-TTY users keep the plain CLI).
- Re-introducing the web UI or any HTTP server.

## Notes
- SP2–SP4 sections above are intentionally high-level; each will be expanded into its own
  design spec before implementation. Only SP1 is detailed enough to write an implementation
  plan now.
