# User Configuration File (CLI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a hand-editable TOML user-config file (`~/.config/onetagger/config.toml`) that stores Spotify credentials, the AcoustID key, and default autotagger options, layered under CLI flags.

**Architecture:** A new `user_config` module in the CLI crate parses the TOML into a `UserConfig` (secrets + a `Defaults` struct). `main.rs` loads it once and layers it between built-in defaults and CLI flags. Precedence: built-in → `[defaults]` → `--config` file → flags/env. Engine crates are untouched.

**Tech Stack:** Rust, `clap`, `toml` + `serde` (new CLI deps), the existing `directories`-backed `Settings::get_folder()`.

Spec: `docs/superpowers/specs/2026-06-24-user-config-design.md`.

---

### Task 1: Add `toml` and `serde` dependencies

**Files:**
- Modify: `crates/onetagger-cli/Cargo.toml`

- [ ] **Step 1: Add the dependencies**

In the `[dependencies]` section, add these two lines (after `convert_case = "0.8"`):

```toml
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo build -p onetagger-cli`
Expected: builds (no code uses the deps yet; just confirms they resolve).

- [ ] **Step 3: Commit**

```bash
git add crates/onetagger-cli/Cargo.toml Cargo.lock
git commit -m "build(cli): add toml + serde for user config"
```

---

### Task 2: `UserConfig` types + `load`/`path` (with parsing tests)

**Files:**
- Create: `crates/onetagger-cli/src/user_config.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/onetagger-cli/src/user_config.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full() {
        let toml = r#"
acoustid_api_key = "abc"
[spotify]
client_id = "id"
client_secret = "secret"
[defaults]
platforms = ["deezer", "beatport"]
tags = ["title", "artist"]
threads = 8
strictness = 80
output_suffix = ".done"
in_place = true
overwrite = false
enable_shazam = true
force_shazam = false
shazam_concurrency = 2
shazam_interval_ms = 500
include_subfolders = false
"#;
        let cfg: UserConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.acoustid_api_key.as_deref(), Some("abc"));
        let s = cfg.spotify.as_ref().unwrap();
        assert_eq!(s.client_id, "id");
        assert_eq!(s.client_secret, "secret");
        assert_eq!(cfg.defaults.threads, Some(8));
        assert_eq!(cfg.defaults.platforms.as_deref(), Some(&["deezer".to_string(), "beatport".to_string()][..]));
        assert_eq!(cfg.defaults.in_place, Some(true));
        assert_eq!(cfg.defaults.output_suffix.as_deref(), Some(".done"));
    }

    #[test]
    fn parse_empty() {
        let cfg: UserConfig = toml::from_str("").unwrap();
        assert!(cfg.spotify.is_none());
        assert!(cfg.acoustid_api_key.is_none());
        assert!(cfg.defaults.platforms.is_none());
        assert!(cfg.defaults.threads.is_none());
    }

    #[test]
    fn parse_partial_spotify_only() {
        let cfg: UserConfig = toml::from_str("[spotify]\nclient_id = \"x\"\nclient_secret = \"y\"\n").unwrap();
        assert_eq!(cfg.spotify.unwrap().client_id, "x");
        assert!(cfg.defaults.threads.is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p onetagger-cli user_config 2>&1 | head -20`
Expected: FAIL — `UserConfig` / `SpotifyCreds` not found (module not declared / types missing).

> Note: this task's tests will only compile once Task 5 adds `mod user_config;` to `main.rs`. If the test run errors with "file not included in module tree", that is expected here — proceed to implement the types; the test pass is verified in Step 4 after Task 5, OR temporarily add `mod user_config;` to `main.rs` now (Task 5 makes it permanent). Adding the `mod` line now is recommended.

- [ ] **Step 3: Implement the types + load/path**

Prepend (above the `#[cfg(test)]` block) in `crates/onetagger-cli/src/user_config.rs`:

```rust
use std::path::{Path, PathBuf};
use serde::Deserialize;
use onetagger_shared::Settings;

/// Parsed `~/.config/onetagger/config.toml`. All fields optional so partial files work.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    pub spotify: Option<SpotifyCreds>,
    pub acoustid_api_key: Option<String>,
    pub defaults: Defaults,
}

#[derive(Debug, Default, Deserialize)]
pub struct SpotifyCreds {
    pub client_id: String,
    pub client_secret: String,
}

/// Default autotagger options; each used only when the matching CLI flag is omitted.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub platforms: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub threads: Option<u16>,
    pub strictness: Option<u8>,
    pub output_suffix: Option<String>,
    pub in_place: Option<bool>,
    pub overwrite: Option<bool>,
    pub enable_shazam: Option<bool>,
    pub force_shazam: Option<bool>,
    pub shazam_concurrency: Option<usize>,
    pub shazam_interval_ms: Option<u64>,
    pub include_subfolders: Option<bool>,
}

/// Path to the user config file (in the same dir as the Spotify token cache / logs).
pub fn path() -> PathBuf {
    match Settings::get_folder() {
        Ok(dir) => dir.join("config.toml"),
        Err(_) => PathBuf::from("config.toml"),
    }
}

/// Load the user config. Absent file -> defaults. Malformed -> warn + defaults (never fail).
pub fn load() -> UserConfig {
    let p = path();
    if !p.exists() {
        return UserConfig::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                warn!("Failed parsing config {}: {e}. Using defaults.", p.display());
                UserConfig::default()
            }
        },
        Err(e) => {
            warn!("Failed reading config {}: {e}. Using defaults.", p.display());
            UserConfig::default()
        }
    }
}
```

> `warn!` comes from `#[macro_use] extern crate log;` in `main.rs` (crate-wide). The `Path` import is used by Task 4.

- [ ] **Step 4: Run the tests to verify they pass**

(After Task 5's `mod user_config;` exists, or add it now.) Run: `cargo test -p onetagger-cli user_config -- parse_`
Expected: `parse_full`, `parse_empty`, `parse_partial_spotify_only` PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/onetagger-cli/src/user_config.rs crates/onetagger-cli/src/main.rs
git commit -m "feat(cli): UserConfig types + TOML load"
```

---

### Task 3: `Defaults::apply_to` + tag parsing (with test)

**Files:**
- Modify: `crates/onetagger-cli/src/user_config.rs`

- [ ] **Step 1: Write the failing test**

Add inside the `mod tests` block in `user_config.rs`:

```rust
    #[test]
    fn apply_to_sets_fields() {
        use onetagger_tagger::{TaggerConfig, SupportedTag};
        let mut config = TaggerConfig::default();
        let defaults = Defaults {
            platforms: Some(vec!["deezer".to_string()]),
            tags: Some(vec!["title".to_string(), "albumArt".to_string()]),
            strictness: Some(50),
            output_suffix: Some(".x".to_string()),
            overwrite: Some(false),
            enable_shazam: Some(true),
            include_subfolders: Some(false),
            ..Default::default()
        };
        defaults.apply_to(&mut config);
        assert_eq!(config.platforms, vec!["deezer".to_string()]);
        assert_eq!(config.tags, vec![SupportedTag::Title, SupportedTag::AlbumArt]);
        assert_eq!(config.strictness, 0.5);
        assert_eq!(config.output_suffix, ".x");
        assert_eq!(config.overwrite, false);
        assert_eq!(config.enable_shazam, true);
        assert_eq!(config.include_subfolders, false);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p onetagger-cli user_config -- apply_to_sets_fields`
Expected: FAIL — no method `apply_to`.

- [ ] **Step 3: Implement `apply_to` + `parse_tags`**

Add to `user_config.rs` (above the tests), and add the imports at the top of the file:

```rust
use convert_case::{Casing, Case};
use onetagger_tagger::{TaggerConfig, SupportedTag};
```

```rust
/// Parse canonical tag names (e.g. "albumArt") into SupportedTag, warning on unknowns.
pub fn parse_tags(tags: &[String]) -> Vec<SupportedTag> {
    tags.iter().filter_map(|t| {
        match serde_json::from_str(&format!("\"{}\"", t.to_case(Case::Camel))) {
            Ok(tag) => Some(tag),
            Err(_) => { warn!("Invalid tag: {t}"); None }
        }
    }).collect()
}

impl Defaults {
    /// Apply the simple field defaults onto a config. Does NOT handle threads or in_place
    /// (those interact with flags / the 2x-cores fallback and are resolved in get_at_config).
    pub fn apply_to(&self, config: &mut TaggerConfig) {
        if let Some(p) = &self.platforms { config.platforms = p.clone(); }
        if let Some(t) = &self.tags { config.tags = parse_tags(t); }
        if let Some(s) = self.strictness { if s <= 100 { config.strictness = s as f64 / 100.0; } }
        if let Some(s) = &self.output_suffix { config.output_suffix = s.clone(); }
        if let Some(v) = self.overwrite { config.overwrite = v; }
        if let Some(v) = self.enable_shazam { config.enable_shazam = v; }
        if let Some(v) = self.force_shazam { config.force_shazam = v; }
        if let Some(v) = self.include_subfolders { config.include_subfolders = v; }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p onetagger-cli user_config -- apply_to_sets_fields`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/onetagger-cli/src/user_config.rs
git commit -m "feat(cli): Defaults::apply_to + tag parsing"
```

---

### Task 4: Config template + `write_template` (0600)

**Files:**
- Modify: `crates/onetagger-cli/src/user_config.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    #[test]
    fn template_parses_to_defaults() {
        // The shipped template is fully commented, so it parses to an all-default config.
        let cfg: UserConfig = toml::from_str(TEMPLATE).unwrap();
        assert!(cfg.spotify.is_none());
        assert!(cfg.acoustid_api_key.is_none());
        assert!(cfg.defaults.platforms.is_none());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p onetagger-cli user_config -- template_parses`
Expected: FAIL — `TEMPLATE` not found.

- [ ] **Step 3: Implement TEMPLATE + write_template + perms**

Add to `user_config.rs` (above tests):

```rust
/// Commented starter template written by `config init`.
pub const TEMPLATE: &str = r#"# OneTagger CLI configuration
# Uncomment and edit. CLI flags override these; flags > this file > built-in defaults.

# [spotify]
# client_id = ""
# client_secret = ""

# acoustid_api_key = ""

# [defaults]
# platforms = ["deezer", "beatport"]
# tags = ["title", "artist", "album", "genre", "bpm", "label", "isrc", "albumArt"]
# threads = 8
# strictness = 80          # 0-100
# output_suffix = ".tagged"
# in_place = false
# overwrite = true
# enable_shazam = true
# force_shazam = false
# shazam_concurrency = 3
# shazam_interval_ms = 350
# include_subfolders = true
"#;

/// Write the template to the config path. Refuses to overwrite unless `force`. Sets 0600 on Unix.
pub fn write_template(force: bool) -> Result<PathBuf, anyhow::Error> {
    let p = path();
    if p.exists() && !force {
        anyhow::bail!("Config already exists at {} (use --force to overwrite)", p.display());
    }
    std::fs::write(&p, TEMPLATE)?;
    set_owner_only(&p);
    Ok(p)
}

#[cfg(unix)]
fn set_owner_only(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)) {
        warn!("Failed setting 0600 on {}: {e}", p.display());
    }
}
#[cfg(not(unix))]
fn set_owner_only(_p: &Path) {}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p onetagger-cli user_config`
Expected: all `user_config` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/onetagger-cli/src/user_config.rs
git commit -m "feat(cli): config template + write_template (0600)"
```

---

### Task 5: Declare module, load at startup, add `config` subcommand

**Files:**
- Modify: `crates/onetagger-cli/src/main.rs`

- [ ] **Step 1: Declare the module**

In `crates/onetagger-cli/src/main.rs`, under the existing `mod spotify_auth;` line, add:

```rust
mod user_config;
```

- [ ] **Step 2: Add the `ConfigAction` enum and `Config` variant**

In `main.rs`, add this enum right after the `enum Actions { ... }` block closes:

```rust
#[derive(Subcommand, Debug, Clone)]
enum ConfigAction {
    /// Print the path to the user config file
    Path,
    /// Write a commented config template
    Init {
        /// Overwrite an existing config file
        #[clap(long)]
        force: bool,
    },
}
```

Inside `enum Actions { ... }`, add this variant (e.g. after the `Renamer { .. }` variant):

```rust
    /// Manage the user configuration file (~/.config/onetagger/config.toml)
    Config {
        #[clap(subcommand)]
        action: ConfigAction,
    },
```

- [ ] **Step 3: Load user config at startup and handle `config`**

In `fn main()`, after the two early-return blocks (`--autotagger-config` / `--audiofeatures-config`) and before `match &action {` / wherever `action` is matched, add:

```rust
    let user_config = user_config::load();
```

Add this match arm to the main `match` over actions (alongside the others):

```rust
        Actions::Config { action } => {
            match action {
                ConfigAction::Path => println!("{}", user_config::path().display()),
                ConfigAction::Init { force } => match user_config::write_template(*force) {
                    Ok(p) => info!("Wrote config template to {}", p.display()),
                    Err(e) => { error!("{e}"); std::process::exit(1); }
                },
            }
        },
```

> If `action` is consumed by value in the match (`let action = cli.action.unwrap();` then `match &action`), keep using `&action`; the `Config { action }` binds `action: &ConfigAction`, so `*force` and matching `ConfigAction::Path` work as written.

- [ ] **Step 4: Build and verify the subcommand + tests**

Run: `cargo build -p onetagger-cli && cargo test -p onetagger-cli user_config`
Expected: builds; all `user_config` tests pass.

Run: `./target/debug/onetagger-cli config path`
Expected: prints `…/.config/onetagger/config.toml`.

Run: `./target/debug/onetagger-cli config init && ls -l "$(./target/debug/onetagger-cli config path)"`
Expected: "Wrote config template to …"; file mode shows `-rw-------` (0600). Run `config init` again → error "Config already exists … (use --force …)".

- [ ] **Step 5: Commit**

```bash
git add crates/onetagger-cli/src/main.rs
git commit -m "feat(cli): config subcommand (path/init) + load user config"
```

---

### Task 6: Layer user config into `get_at_config` (defaults + Spotify creds)

**Files:**
- Modify: `crates/onetagger-cli/src/main.rs`

- [ ] **Step 1: Change `get_at_config` signature and call site**

Change the method signature from:

```rust
    pub fn get_at_config(&self) -> Result<TaggerConfig, Error> {
```
to:
```rust
    pub fn get_at_config(&self, user: &user_config::UserConfig) -> Result<TaggerConfig, Error> {
```

In the `Actions::Autotagger` handler arm, change the call from `action.get_at_config()` to:

```rust
            let config = action.get_at_config(&user_config).expect("Failed loading config file!");
```

- [ ] **Step 2: Apply `[defaults]` onto the base config**

In `get_at_config`, immediately after the base config is built (after the `let mut config = if let Some(config_path) = config { … } else { TaggerConfig::custom_default() };` block and the `config.path = Some(path.to_owned());` line), insert:

```rust
                // Layer user-config defaults under the CLI flags (flags applied below still win).
                // Skipped when an explicit --config file is given: that file is authoritative
                // (precedence: built-in < [defaults] < --config < flags).
                if !has_config_file {
                    user.defaults.apply_to(&mut config);
                }
```

- [ ] **Step 3: Make threads + in_place honor `[defaults]`**

Replace the existing threads block:

```rust
                if let Some(threads) = threads {
                    config.threads = *threads;
                } else if !has_config_file {
                    config.threads = onetagger_shared::default_thread_count() as u16;
                }
```
with (when no `--config` file: flag > `[defaults].threads` > 2x-cores fallback):
```rust
                if let Some(threads) = threads {
                    config.threads = *threads;
                } else if !has_config_file {
                    config.threads = user.defaults.threads
                        .unwrap_or_else(|| onetagger_shared::default_thread_count() as u16);
                }
```

Replace the existing preserve_original line:

```rust
                config.preserve_original = !*in_place;
```
with (a config `in_place = true` also enables overwriting the original, but only when no `--config` file is in play):
```rust
                let cfg_in_place = !has_config_file && user.defaults.in_place.unwrap_or(false);
                config.preserve_original = !(*in_place || cfg_in_place);
```

- [ ] **Step 4: Populate Spotify creds from `[spotify]`**

Add `SpotifyConfig` to the existing `onetagger_tagger` import line in `main.rs`:

```rust
use onetagger_tagger::{TaggerConfig, AudioFileInfo, SupportedTag, is_tagged_output_path, SpotifyConfig};
```

In `get_at_config`, just before `return Ok(config);`, add:

```rust
                // Spotify credentials from user config (unless a --config file already set them)
                if config.spotify.is_none() {
                    if let Some(s) = &user.spotify {
                        if !s.client_id.is_empty() && !s.client_secret.is_empty() {
                            config.spotify = Some(SpotifyConfig {
                                client_id: s.client_id.clone(),
                                client_secret: s.client_secret.clone(),
                            });
                        }
                    }
                }
```

- [ ] **Step 5: Build and verify defaults are applied**

Run: `cargo build -p onetagger-cli`
Expected: builds.

Verify end to end:

```bash
printf '[defaults]\nplatforms = ["deezer"]\nthreads = 5\ntags = ["genre"]\n' > "$(./target/debug/onetagger-cli config path)"
mkdir -p /tmp/uc && : > /tmp/uc/x.mp3
./target/debug/onetagger-cli autotagger --path /tmp/uc --dry-run --changes /tmp/uc/c.json 2>&1 | grep -oE 'platforms: \[[^]]*\]|threads: [0-9]+|tags: \[[^]]*\]'
```
Expected: shows `platforms: ["deezer"]`, `threads: 5`, `tags: [Genre]` (from config, since no flags passed). Then confirm a flag still wins:
```bash
./target/debug/onetagger-cli autotagger --path /tmp/uc -j 9 --dry-run --changes /tmp/uc/c.json 2>&1 | grep -oE 'threads: [0-9]+'
```
Expected: `threads: 9`.

- [ ] **Step 6: Commit**

```bash
git add crates/onetagger-cli/src/main.rs
git commit -m "feat(cli): layer user-config defaults + spotify creds into get_at_config"
```

---

### Task 7: Wire Shazam/AcoustID defaults + optional Spotify auth creds

**Files:**
- Modify: `crates/onetagger-cli/src/main.rs`

- [ ] **Step 1: Shazam throttle + AcoustID from config**

In the `Actions::Autotagger` handler, replace:

```rust
            onetagger_autotag::configure_shazam(shazam_concurrency.unwrap_or(3), shazam_interval_ms.unwrap_or(350));
            // Enable AcoustID fallback if a key is provided (flag or ACOUSTID_API_KEY env var)
            let acoustid_key = acoustid_api_key.clone().or_else(|| std::env::var("ACOUSTID_API_KEY").ok());
            onetagger_autotag::configure_acoustid(acoustid_key);
```
with:
```rust
            onetagger_autotag::configure_shazam(
                shazam_concurrency.or(user_config.defaults.shazam_concurrency).unwrap_or(3),
                shazam_interval_ms.or(user_config.defaults.shazam_interval_ms).unwrap_or(350),
            );
            // AcoustID key: flag > ACOUSTID_API_KEY env > user config
            let acoustid_key = acoustid_api_key.clone()
                .or_else(|| std::env::var("ACOUSTID_API_KEY").ok())
                .or_else(|| user_config.acoustid_api_key.clone())
                .filter(|k| !k.trim().is_empty());
            onetagger_autotag::configure_acoustid(acoustid_key);
```

- [ ] **Step 2: Make `authorize-spotify` creds optional with config fallback**

Change the `AuthorizeSpotify` clap variant fields from required `String` to optional:

```rust
    /// Authorize Spotify and cache the token
    AuthorizeSpotify {
        /// Spotify Client ID (falls back to the user config [spotify] section)
        #[clap(long)]
        client_id: Option<String>,

        /// Spotify Client Secret (falls back to the user config [spotify] section)
        #[clap(long)]
        client_secret: Option<String>,
    },
```

Replace the `AuthorizeSpotify` handler arm body:

```rust
        Actions::AuthorizeSpotify { client_id, client_secret } => {
            let (auth_url, client) = Spotify::generate_auth_url(&client_id, &client_secret).expect("Failed generating auth URL!");
            println!("\nPlease go to the following URL and authorize OneTagger:\n{auth_url}\n");
            // Start a minimal local callback server to capture the redirect, then authorize
            spotify_auth::spawn_callback_server();
            let _spotify = Spotify::auth_server(client).expect("Spotify authentication failed!");
            info!("Successfully authorized Spotify!");
            std::process::exit(0);
        },
```
with:
```rust
        Actions::AuthorizeSpotify { client_id, client_secret } => {
            // Resolve creds: flags override the user config [spotify] section
            let id = client_id.clone().or_else(|| user_config.spotify.as_ref().map(|s| s.client_id.clone()));
            let secret = client_secret.clone().or_else(|| user_config.spotify.as_ref().map(|s| s.client_secret.clone()));
            let (id, secret) = match (id, secret) {
                (Some(i), Some(s)) if !i.is_empty() && !s.is_empty() => (i, s),
                _ => {
                    error!("Missing Spotify credentials. Pass --client-id/--client-secret or set [spotify] in {}", user_config::path().display());
                    std::process::exit(1);
                }
            };
            let (auth_url, client) = Spotify::generate_auth_url(&id, &secret).expect("Failed generating auth URL!");
            println!("\nPlease go to the following URL and authorize OneTagger:\n{auth_url}\n");
            spotify_auth::spawn_callback_server();
            let _spotify = Spotify::auth_server(client).expect("Spotify authentication failed!");
            info!("Successfully authorized Spotify!");
            std::process::exit(0);
        },
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p onetagger-cli`
Expected: builds.

```bash
printf '[defaults]\nshazam_concurrency = 1\nshazam_interval_ms = 700\n' > "$(./target/debug/onetagger-cli config path)"
./target/debug/onetagger-cli authorize-spotify 2>&1 | grep -i "Missing Spotify credentials"
```
Expected: the "Missing Spotify credentials" error (no creds anywhere) — confirms the optional-flag path and config lookup run without panicking. (Full auth needs real creds + browser.)

- [ ] **Step 4: Commit**

```bash
git add crates/onetagger-cli/src/main.rs
git commit -m "feat(cli): shazam/acoustid defaults + optional spotify auth creds from config"
```

---

### Task 8: README note + final verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the config file**

In `README.md`, add a short "Configuration" section (after the "Usage" heading’s intro). Use this content:

```markdown
### Configuration

Defaults and credentials can be stored in `~/.config/onetagger/config.toml` (see
`onetagger-cli config path`). Create a commented template with `onetagger-cli config init`,
then edit it. CLI flags always override the file. Example:

​```toml
[spotify]
client_id = "..."
client_secret = "..."
acoustid_api_key = "..."

[defaults]
platforms = ["deezer", "beatport"]
tags = ["title", "artist", "genre", "bpm"]
threads = 8
​```
```

(Remove the zero-width `​` characters around the inner code fence — they are only here to keep this plan's markdown intact. Use a normal ```` ```toml ```` fence.)

- [ ] **Step 2: Full verification**

Run: `cargo build -p onetagger-cli && cargo test -p onetagger-cli`
Expected: builds; all tests pass (including the `user_config` and `shazam` tests).

Run: `rm -f "$(./target/debug/onetagger-cli config path)"` then `./target/debug/onetagger-cli autotagger --path /tmp/uc --dry-run --changes /tmp/uc/c.json 2>&1 | grep -oE 'threads: [0-9]+'`
Expected: `threads: <2x cores>` (config absent → falls back to default), confirming the no-config path still works.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document user config file"
```

---

## Notes for the implementer

- **Precedence recap:** built-in defaults → `[defaults]` → `--config` file → CLI flags. Implemented by skipping the entire `[defaults]` layer (`apply_to`, plus the `threads`/`in_place` defaults) when `has_config_file` is true, so an explicit `--config` file is authoritative; flags are applied last and always win. Spotify creds from `[spotify]` fill in only when `config.spotify` is still unset.
- **Boolean flags** only turn things on (clap can't see "passed false"); a config-enabled bool resolves as `config || flag`. To disable for one run, edit the config.
- **Out of scope:** `config set` editor, `authorize-spotify --save`, audiofeatures cred fallback, OS keyring.
