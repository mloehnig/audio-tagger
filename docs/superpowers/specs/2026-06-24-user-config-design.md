# Design: User configuration file (CLI)

## Context

OneTagger is now a CLI-only tool. There is already a per-user data directory —
`onetagger_shared::Settings::get_folder()` uses the `directories` crate (the Rust analog of
Go's `os.UserConfigDir`) and resolves to `~/.config/onetagger` (and platform equivalents),
where the Spotify token cache, logs, and `runs/` already live.

What's missing is a way to persist **user choices and secrets** so they don't have to be
re-supplied on every invocation:

- **Secrets** — Spotify `clientId`/`clientSecret` (today only the OAuth *token* is cached, not
  the app credentials) and the AcoustID API key.
- **Generic defaults** — the options a user always passes (platforms, tags, thread count,
  strictness, output suffix, Shazam throttle, etc.).

This adds a hand-editable user-config file and wires it into the CLI as a defaults layer.

## Decisions (from brainstorming)

- **Secrets:** stored in plaintext in the config file, written with `0600` permissions on
  Unix (the conventional CLI approach; works headless/SSH/CI).
- **Format:** TOML, hand-editable.
- **Scope:** purely a CLI-layer concern — no changes to the engine crates.

## File

- **Path:** `<Settings::get_folder()>/config.toml` → `~/.config/onetagger/config.toml` on
  Linux (macOS/Windows equivalents via `directories`).
- **Permissions:** when OneTagger writes the file, set mode `0600` on Unix
  (`std::os::unix::fs::PermissionsExt`, behind `#[cfg(unix)]`). No-op on Windows.

### Schema

```toml
[spotify]
client_id = "..."
client_secret = "..."

acoustid_api_key = "..."

[defaults]                 # every key optional; used only when the matching flag is omitted
platforms = ["deezer", "beatport"]
tags = ["title", "artist", "genre", "bpm", "label", "isrc", "albumArt"]
threads = 8
strictness = 80            # 0–100
output_suffix = ".tagged"
in_place = false
overwrite = true
enable_shazam = true
force_shazam = false
shazam_concurrency = 3
shazam_interval_ms = 350
include_subfolders = true
```

## Precedence

Lowest → highest, each layer overriding the previous where it specifies a value:

1. Built-in defaults (`TaggerConfig::custom_default()`).
2. `[defaults]` from `config.toml`.
3. `--config <file>` (explicit full TaggerConfig), when given.
4. CLI flags / env vars.

So personal defaults fill in whatever isn't passed; an explicit flag always wins. Because the
existing `get_at_config` only applies a flag override when the flag is `Some`/the bool is set,
layering `[defaults]` *before* the flag pass yields this precedence naturally.

**Boolean flags** (`--in-place`, `--enable-shazam`, `--force-shazam`, `--overwrite`,
`--no-subfolders`) can only turn an option *on* — clap can't distinguish "passed false" from
"unset". So a bool resolves as `config_default || flag` (config can set the default on; the
flag can additionally turn it on). To turn a config-enabled bool back *off* for a run, edit
the config — there is intentionally no negating flag. `in_place` specifically maps to
`preserve_original = !(flag || defaults.in_place)`.

### Secret wiring

- **Spotify:** `authorize-spotify`'s `--client-id` / `--client-secret` become **optional**,
  falling back to `[spotify]`; error only if neither flag nor config supplies them. For
  autotagging, `TaggerConfig.spotify` is populated from `[spotify]` unless a `--config` file's
  `spotify` block already sets it.
- **AcoustID:** `--acoustid-api-key` flag → `ACOUSTID_API_KEY` env → `acoustid_api_key` from
  config (first present wins), passed to `onetagger_autotag::configure_acoustid`.

## New `config` subcommand

- `config path` — print the resolved config-file path (stdout).
- `config init` — write a commented template to the path with `0600`; refuse to overwrite an
  existing file unless `--force`.

Everything else is hand-edited. (Out of scope for v1: a `config set <key> <value>` editor and
an `authorize-spotify --save` that persists supplied creds — both noted as easy follow-ups.)

## Components

- **`crates/onetagger-cli/src/user_config.rs`** (new):
  - `struct UserConfig { spotify: Option<SpotifyCreds>, acoustid_api_key: Option<String>,
    defaults: Option<Defaults> }` (serde `Deserialize`, `Default`).
  - `struct SpotifyCreds { client_id, client_secret }`.
  - `struct Defaults { platforms, tags, threads, strictness, output_suffix, in_place,
    overwrite, enable_shazam, force_shazam, shazam_concurrency, shazam_interval_ms,
    include_subfolders }` — all `Option<_>`.
  - `fn path() -> PathBuf` (via `Settings::get_folder()`), `fn load() -> UserConfig`
    (absent → `Default`; malformed → warn + `Default`), `fn write_template(force: bool)`,
    a `const TEMPLATE: &str`.
- **`crates/onetagger-cli/src/main.rs`:** load `UserConfig` once at startup; thread it into
  - `get_at_config` — apply `[defaults]` onto the base config before the existing flag pass,
    and populate `config.spotify` from `[spotify]` when unset;
  - the AcoustID configuration call (flag → env → config);
  - `authorize-spotify` (optional creds);
  - add the `config` subcommand (`path`, `init`).
- **Dependencies:** add `toml` and `serde` (derive) to `crates/onetagger-cli/Cargo.toml`.
- **No engine-crate changes.**

## Error handling

- Missing config file → silently use built-in defaults.
- Malformed TOML → log a warning (to stderr) and continue with built-in defaults; never
  hard-fail a tagging run because of the config file.
- Unknown keys → ignored (serde defaults), so older binaries tolerate newer config files.
- `config init` on an existing file without `--force` → clear error, no overwrite.

## Testing

- **Unit (`user_config.rs`):** parse a full config, a partial config (only `[spotify]`; only
  some `[defaults]`), an empty file, and malformed TOML (→ `Default`, no panic).
- **Unit (merge/precedence):** a flag overrides a `[defaults]` value; a `[defaults]` value
  overrides the built-in; an absent flag + absent config → built-in.
- **Manual E2E:** `config init` → edit `[defaults]` + `[spotify]` → run `autotagger` with no
  flags and confirm the defaults are applied (inspect the logged `TaggerConfig`); run
  `authorize-spotify` with no creds and confirm it uses the stored ones; verify the file is
  `0600`.

## Out of scope (v1)

- `config set` key/value editor; `authorize-spotify --save`.
- Storing secrets in an OS keyring.
- Migrating/Reusing the old GUI `settings.json` (the `ui` blob) — left untouched.
- Per-directory/project config files.
