# OneTagger — Architecture Review

## What the project is

**OneTagger** is a cross-platform **command-line** tool for **tagging music libraries**, aimed primarily at **DJs**. Its core value proposition: pull rich, accurate metadata from music databases and write it into your audio files in a format DJ software understands.

> This is a **CLI-only** fork. The original project shipped a desktop GUI (Wry webview) and a Vue/Quasar web UI served by an Axum server (`onetagger-ui`); those have been removed. All functionality runs through the `onetagger-cli` binary.

**Goal:** automate and streamline music metadata management — fetch tags (genre, BPM, key, label, release date, cover art, lyrics, ISRC, etc.) from many online sources.

It supports **MP3, AIFF, FLAC, M4A (AAC/ALAC), WAV, OGG**, and integrates with **Beatport, Traxsource, Juno Download, Discogs, MusicBrainz, Spotify, Deezer, iTunes, Bandcamp, Musixmatch, Beatsource, BPMSupreme**, plus **Shazam** and **AcoustID** audio fingerprinting.

The CLI subcommands:
1. **autotagger** — batch metadata fetching from platforms (with `--dry-run`)
2. **apply** — write a reviewed/edited changes file produced by `--dry-run`
3. **unprocessed** — list files not yet successfully tagged (JSON)
4. **audiofeatures** — Spotify danceability/energy/etc. by ISRC
5. **authorize-spotify** — cache a Spotify OAuth token
6. **renamer** — template-DSL batch file renaming

## High-level architecture

A single Rust binary (`onetagger-cli`) drives a shared engine of library crates:

```
onetagger-cli  (clap subcommands: autotagger / apply / unprocessed / audiofeatures /
   │            authorize-spotify / renamer)
   │
   ├── onetagger-autotag    orchestration: identify → match → write, dry-run/apply, throttles
   │     ├── onetagger-tagger     core traits + Track/TaggerConfig + matching utils
   │     ├── onetagger-platforms  12 metadata source integrations
   │     ├── onetagger-tag        format-agnostic tag read/write
   │     └── onetagger-player     audio decode (duration + Shazam sampling)
   ├── onetagger-renamer    template-DSL file renaming
   ├── onetagger-playlist   M3U parsing
   └── onetagger-shared     settings, logging, constants

Spotify OAuth uses a tiny built-in TcpListener callback on 127.0.0.1:36913
(crates/onetagger-cli/src/spotify_auth.rs) — no web server / SPA.
```

## Crate breakdown (workspace of 9 crates)

| Crate | Role |
|---|---|
| `onetagger-platforms` | The 12 metadata source integrations |
| `onetagger-tag` | Format-agnostic tag read/write |
| `onetagger-autotag` | The auto-tagging orchestration engine (+ identifier chain, changes/dry-run) |
| `onetagger-tagger` | **Core contract crate** — traits, `Track`, `TaggerConfig`, matching utils |
| `onetagger-renamer` | Template DSL parser + rename engine |
| `onetagger-player` | Multi-format audio decode (rodio + lofty) |
| `onetagger-cli` | The CLI binary (all user-facing commands) |
| `onetagger-shared` | Settings, logging, constants (port 36913), thread defaults |
| `onetagger-playlist` | M3U parsing |

## The key design ideas

**1. Two-layer tag abstraction.**
`onetagger-tag` exposes a `TagImpl` trait implemented per format (`ID3Tag`, `FLACTag`, `MP4Tag`, `VorbisTag`), unified behind a `Tag` enum. A canonical `Field` enum (Title, Artist, BPM, Key…) maps to each format's native tag names via `.id3()`/`.vorbis()`/`.mp4()`. This is the clean isolation point that lets the rest of the app think in fields, not formats.

**2. The `onetagger-tagger` contract crate.**
Everything pivots on two traits:
- `AutotaggerSourceBuilder` — factory + UI metadata (`PlatformInfo`, custom options, auth requirements)
- `AutotaggerSource` — `match_track()` → `extend_track()` → `get_album()`

Every platform implements these. The engine never knows about Beatport specifically.

**3. Matching strategy (`MatchingUtils`).** Tiered, by reason:
- `ID` (1.0) — existing platform ID in tags
- `ISRC` (1.0) — exact ISRC lookup
- `Fuzzy` (0.0–1.0) — multi-step title cleaning + Levenshtein on title/artist, thresholded by `config.strictness`
- `Album` (1.0) — from album track listing

**4. Plugin system via FFI.** Beyond the 12 built-in platforms, `onetagger-autotag/repo.rs` dynamically loads `.dll/.so/.dylib` plugins from a platforms folder using `libloading`, with a `_1T_PLATFORM_COMPATIBILITY` version handshake and the `create_plugin!` macro generating the C-ABI boilerplate. Real extensibility, though it relies on lifetime transmutes and raw pointers at the boundary.

**5. Renamer DSL.** A genuine little language: `%variable%.property().function()` with 30 variables, properties (`.first()`), and functions (`replace`, `camelot`, `pad`, `join`…), with autocomplete and syntax-highlighting support in the parser.

## Data flow (auto-tag)

1. **Input** — local audio file → `AudioFileInfo` extracted (title, artists, format, duration, ISRC). Falls back to **Shazam** fingerprinting if tags are missing and it's enabled.
2. **Matching** — `TaggerConfig` lists platforms to query; each platform's `match_track()` returns scored `Vec<TrackMatch>`; `MatchingUtils::match_track()` does fuzzy title/artist matching against `config.strictness`.
3. **Enrichment** — best match → mutable `Track`; `extend_track()` fetches extra metadata; results from multiple platforms merged via `Track::merge()`.
4. **Writing** — `Track` fields → canonical `Field` enum → native tag names → `TagImpl::set_field()` → `save_file()`. Album art downloaded; optional file move + M3U write + post-command.

## Tag writing: how files are modified

Tag writing all funnels through one method, `TrackImpl::write_to_file`
(`onetagger-autotag/src/lib.rs:78`, called from `tag_track`, album mode, and multiplatform
merge — plus the parallel `AudioFeatures::write_to_path`). It returns a `FileChanges`
(the before→after tag diff) and its behavior is governed by three `TaggerConfig` fields:
`preserve_original`, `output_suffix` (default `.tagged`), and `dry_run`.

**CLI is non-destructive by default; the engine default stays in-place.**
`TaggerConfig::default()` sets `preserve_original = false` and `dry_run = false`
(`onetagger-tagger/src/lib.rs`) — the conservative default for library/engine callers.
The **CLI** sets `preserve_original = true` unless `--in-place` is passed.

- **Safe default (CLI):** the tagged result is written to a copy beside the original —
  `song.mp3` → `song.tagged.mp3` (`tagged_output_path` in `onetagger-tagger/src/lib.rs`).
  The original is left byte-identical. On reruns, `*.tagged.*` files are skipped as inputs
  (`is_tagged_output_path`).
- **Opt back into in-place:** `--in-place` overwrites the originals (the old behavior). Destructive.
- **Overwrite is ON by default.** `TaggerConfig.overwrite` defaults to `true`
  (`onetagger-tagger/src/lib.rs`), so existing tag values of the enabled tags are replaced.
  Set `"overwrite": false` in a config to only fill empty fields (plus any in `overwrite_tags`).
- **Only the configured tags are written.** The **default `tags` set is narrow**:
  `Genre, BPM, Style, Label, ReleaseDate` — **Title and Artist are NOT written by default**.
  Widen with `--tags` (e.g. `title,artist,album,key,isrc,albumArt`).
- **Non-matches are left untouched**, recorded in `~/.config/onetagger/runs/failed-*.m3u`
  (successes in `success-*.m3u`).

### Dry-run / apply workflow (preview + manual edit)

The CLI decouples matching from writing so changes can be reviewed and hand-edited:

1. `onetagger-cli autotagger --path … --dry-run [--changes out.json]` runs the full
   identify+match but writes **no audio**. It emits a JSON `ChangesDocument`
   (`onetagger-autotag/src/changes.rs`): per file, the `output_path`, optional `artUrl`, and a
   `changes` map of **raw tag/frame name → {old, new}** (computed by diffing
   `TagImpl::all_tags()` before/after applying the matched track in memory). The embedded
   `config` makes apply reproducible.
2. The user edits `out.json` (change any `new` value, delete entries, etc.).
3. `onetagger-cli apply --changes out.json [--in-place]` writes exactly those values
   (`set_raw(new, overwrite=true)`, plus art fetched from `artUrl`), to the `.tagged` copy by
   default or the original with `--in-place`. No platform re-querying — fast and offline.
   Apply runs **in parallel** (each entry writes its own file and fetches its own art, so
   there's no shared state) using `std::thread::scope` with an `AtomicUsize` work-stealing
   index (`ChangesDocument::apply`). Parallelism = `apply -j/--threads <N>`, defaulting to
   **2× CPU cores** (`onetagger_shared::default_thread_count`), always capped at the number of
   files to write. The autotagger's `-j/--threads` likewise defaults to 2× CPU cores when
   unset (unless a `--config` file specifies a thread count).

`onetagger-cli unprocessed --changes out.json --path <dir>` lists, as pretty JSON on stdout,
the audio files under `<dir>` that do **not** have a successful (matched) entry in the changes
file — i.e. exactly what a `--dry-run` resume would still process (never-seen + previously
failed). Generated `.tagged` copies are excluded. Useful for checking remaining work before
re-running. (Logs go to **stderr**, so stdout is clean JSON — pipeable into `jq`/`python`.)

Because the JSON keys are raw frame names straight from `all_tags()`, edits round-trip
losslessly through `set_raw`.

**Durability & resume (CLI dry-run, `run_dry_run` in `onetagger-cli/src/main.rs`):** matching
can be slow and rate-limited (Shazam especially), so a dry-run never relies on a single
write at the end:

- **Incremental writes:** the changes file is rewritten every `--save-every` files
  (default 25), on normal completion, and on interrupt. Writes are **atomic** (temp file +
  rename) so a kill mid-write can't corrupt the JSON.
- **Signal flush:** a `ctrlc` handler catches **SIGINT (Ctrl-C) and SIGTERM**, flushes the
  results collected so far, and exits 0. `SIGKILL` (`kill -9`) cannot be caught by any
  process — the periodic writes are the backstop for that case.
- **Resume:** if the `--changes` file already exists, it is loaded first; files that already
  have a successful match are **skipped**, while previously **unmatched/failed** files are
  reprocessed (so a rate-limit interruption is recoverable). Re-running the same command
  picks up where it left off instead of reprocessing the whole directory.

## Fingerprint identification chain (Shazam + AcoustID)

When a file can't be identified from its tags, OneTagger identifies it by **audio
fingerprint**. This runs through a fallback **chain** (`onetagger-autotag/src/identifier.rs`,
`identify()`), trying each provider in order and moving to the next when one fails:

1. **Shazam** (`shazam.rs`) — always available, no setup; reverse-engineered via `songrec`.
2. **AcoustID** (`acoustid.rs`) — only when an API key is configured. Uses Chromaprint's
   `fpcalc` tool to fingerprint the file, then the AcoustID lookup API, which resolves to
   MusicBrainz recordings (title/artist). Requires `fpcalc` on PATH and a free AcoustID
   application key (`--acoustid-api-key` or the `ACOUSTID_API_KEY` env var).

The chain is entered at the same points Shazam was before (the `--force-shazam` path and the
tags-failed fallback, both gated by `--enable-shazam`). `TaggingStatus.used_shazam` is set
only when Shazam specifically produced the match. Adding another provider (e.g. a RapidAPI
Shazam endpoint) is just another arm in `identify()`.

Ordering note: Shazam is tried first, so a rate-limited Shazam still runs (and slowly fails)
before AcoustID is attempted. If AcoustID is your primary goal (e.g. to avoid Shazam's limits
entirely), trying AcoustID first would be the better order — currently a follow-up, not wired.

## Shazam rate limiting

Shazam recognition (`onetagger-autotag/src/shazam.rs`) runs **inside the tagging worker
threads**, so without limits it fires up to `--threads` (default 16) concurrent requests at
Shazam's unofficial endpoint and gets 429-throttled. songrec retries a 429 ~5× with growing
backoff (8–40s) then gives up, marking the file failed.

A **process-wide limiter**, independent of the thread count, sits in front of the network
call (`shazam_throttle`):
- A **concurrency cap** (crossbeam bounded-channel used as a semaphore) limits how many
  Shazam requests are in flight at once.
- A **minimum interval** (`SHAZAM_LAST` mutex) spaces out request *starts* globally.

Both are tunable from the CLI — `--shazam-concurrency` (default **3**) and
`--shazam-interval-ms` (default **350**) — applied via `configure_shazam(..)` before tagging
starts (the permit pool reads the concurrency once, at first use). Platform matching stays
fully parallel; only Shazam is throttled. Combined with dry-run resume, a run that still gets
throttled can simply be re-run to finish the remaining files.

Mitigations beyond the throttle: prefer `--enable-shazam` + `--parse-filename` over
`--force-shazam` so Shazam is only a fallback. A longer-term alternative to Shazam is
**AcoustID + Chromaprint** (free, bulk-friendly, resolves via the existing MusicBrainz
integration).

## State of the project (abandoned)

- Last real feature work landed around the **1.7.0 changelog (Aug 2023)**.
- Commits since are pure maintenance: the Aug 2025 cluster is "Fix beatport", "Fix JunoDownload", "Update dependencies", "MacOS fixes", "Fix vite version" — keeping it *compiling and not-broken* against shifting platform APIs and toolchains, not adding anything.
- The Feb 2026 commit is just "CI rebuild".
- This is the classic decay mode for scraper-based tools: platforms (Beatport especially) change their sites/APIs and break the integrations faster than a solo maintainer can patch.
- The `example/` folder contains messy real-world test MP3s (e.g. `-  - mo money mo problems.mp3.mp3`) — useful fixtures if revived.

## Architectural assessment

**Strengths**
- **Clean separation via the contract crate.** `onetagger-tagger` as a dependency-light hub of traits/types is textbook — platforms and engine depend on it, not on each other. (Removing the UI was low-friction precisely because of this: the engine crates never depended on `onetagger-ui`.)
- **Format abstraction is the right shape** — `Field` enum + per-format `TagImpl` keeps format quirks contained.
- **Single CLI over a shared engine** — one binary, no GUI/web-server surface to maintain.
- **Genuine extensibility** via the FFI plugin system.

**Weaknesses / risks**
- **Scraper fragility is structural**, not incidental. Several platforms parse `__NEXT_DATA__` out of HTML (Beatport) — guaranteed to break on site redesigns. This is *the* reason maintenance dominated and the project stalled.
- **FFI plugin layer is unsafe-heavy** (`Symbol<'static>` transmutes, raw pointer boxing) — a correctness/soundness liability.
- **Identification depends on unofficial endpoints** — Shazam via `songrec` (reverse-engineered, rate-limited); AcoustID is the open/official fallback but needs `fpcalc` + a key.
- **Thin test coverage** in the crates; for a tool whose core risk is matching accuracy and parser breakage, that's a notable gap.

**If reviving it**, the highest-leverage moves would be:
1. Replace HTML-scraping platforms with official APIs where they exist.
2. Add integration tests around `MatchingUtils` and each platform's parser using recorded fixtures.
3. Reconsider whether the FFI plugin system earns its unsafe complexity vs. a simpler subprocess/WASM plugin model.

## Tech stack

- **Core:** Rust (workspace, resolver 2), clap (CLI), rodio + lofty (audio decode), reqwest (HTTP), libloading (FFI plugins), songrec (Shazam), AcoustID via `fpcalc` (Chromaprint).
- **Tag I/O libraries:** id3, metaflac, mp4ameta, lofty.
- **Platforms:** Windows, macOS, Linux. No Node.js / frontend toolchain required to build.
