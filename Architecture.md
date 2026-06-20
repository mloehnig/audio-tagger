# OneTagger — Architecture Review

## What the project is

**OneTagger** is a cross-platform desktop application for **tagging music libraries**, aimed primarily at **DJs**. Its core value proposition: pull rich, accurate metadata from music databases and write it into your audio files in a format DJ software understands.

**Goal:** automate and streamline music metadata management — fetch tags (genre, BPM, key, label, release date, cover art, lyrics, ISRC, etc.) from many online sources, and provide manual editing tools for the cases automation can't cover.

It supports **MP3, AIFF, FLAC, M4A (AAC/ALAC), WAV, OGG**, and integrates with **Beatport, Traxsource, Juno Download, Discogs, MusicBrainz, Spotify, Deezer, iTunes, Bandcamp, Musixmatch, Beatsource, BPMSupreme**, plus **Shazam** audio fingerprinting.

The five user-facing tools:
1. **Auto Tag** — batch metadata fetching from platforms
2. **Audio Features** — Spotify danceability/energy/etc. by ISRC
3. **Quick Tag** — keyboard-driven fast tagging (mood/genre/energy)
4. **Tag Editor** — detailed single-file editor
5. **Renamer** — template-DSL batch file renaming

## High-level architecture

It's a **Rust backend + Vue 3 SPA frontend**, bridged by a **local WebSocket server** — Tauri-style, but built directly on **Wry/Tao** rather than Tauri.

```
┌─────────────────────────────────────────────────────────┐
│  Desktop window (Wry/Tao webview)  OR  plain browser     │
│  ┌───────────────────────────────────────────────────┐  │
│  │  Vue 3 + Quasar SPA (client/, ~11k LOC)            │  │
│  │  Singleton OneTagger class holds all state (Refs)  │  │
│  └────────────────────┬──────────────────────────────┘  │
└───────────────────────┼─────────────────────────────────┘
                        │ WebSocket JSON (Action enum) + HTTP for assets/art/audio
              127.0.0.1:36913
┌───────────────────────┴─────────────────────────────────┐
│  Axum web server (onetagger-ui)                          │
│  Dispatches 40+ Action message types to subsystems       │
└──┬────────┬─────────┬──────────┬─────────┬───────────────┘
   │        │         │          │         │
 autotag  renamer  player    tag I/O   playlist
   │
   ├── onetagger-tagger     (core traits + Track/Config + matching)
   └── onetagger-platforms  (12 platform integrations)
```

**Two binaries share the same engine:**
- `onetagger` (GUI) — spawns the Axum server in a thread, opens a Wry webview pointing at it. Has a `--server` headless mode too.
- `onetagger-cli` — runs autotagger/audiofeatures/renamer/spotify-auth directly, no UI.

## Crate breakdown (workspace of 11 crates)

| Crate | LOC | Role |
|---|---|---|
| `onetagger-platforms` | 5.9k | The 12 metadata source integrations |
| `onetagger-tag` | 2.4k | Format-agnostic tag read/write |
| `onetagger-autotag` | 2.1k | The auto-tagging orchestration engine |
| `onetagger-ui` | 1.3k | Axum server + WebSocket protocol |
| `onetagger-tagger` | 1.3k | **Core contract crate** — traits, `Track`, `TaggerConfig`, matching utils |
| `onetagger-renamer` | 1.3k | Template DSL parser + rename engine |
| `onetagger-player` | 0.6k | Multi-format audio playback (rodio) |
| `onetagger-cli` | 0.4k | CLI binary |
| `onetagger` | 0.3k | GUI binary (Wry webview) |
| `onetagger-shared` | 0.2k | Settings, logging, constants (port 36913) |
| `onetagger-playlist` | 0.1k | M3U parsing |

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

**5. Renamer DSL.** A genuine little language: `%variable%.property().function()` with 30 variables, properties (`.first()`), and functions (`replace`, `camelot`, `pad`, `join`…), complete with autocomplete and syntax highlighting served to the frontend.

**6. Frontend state.** No Vuex/Pinia — a single `OneTagger` singleton (`onetagger.ts`, 765 LOC) holds all state as Vue `Ref`s and is the sole WebSocket bridge. Components call `get1t()` and override event hooks (`onTaggingDone`, etc.). Simple, but that file is a god-object.

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

**CLI is non-destructive by default; the GUI/engine default is unchanged (in-place).**
`TaggerConfig::default()` sets `preserve_original = false` and `dry_run = false`
(`onetagger-tagger/src/lib.rs`), so existing GUI/server behavior is identical to before.
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

## State of the project (abandoned)

- Last real feature work landed around the **1.7.0 changelog (Aug 2023)**.
- Commits since are pure maintenance: the Aug 2025 cluster is "Fix beatport", "Fix JunoDownload", "Update dependencies", "MacOS fixes", "Fix vite version" — keeping it *compiling and not-broken* against shifting platform APIs and toolchains, not adding anything.
- The Feb 2026 commit is just "CI rebuild".
- This is the classic decay mode for scraper-based tools: platforms (Beatport especially) change their sites/APIs and break the integrations faster than a solo maintainer can patch.
- The `example/` folder contains messy real-world test MP3s (e.g. `-  - mo money mo problems.mp3.mp3`) — useful fixtures if revived.

## Architectural assessment

**Strengths**
- **Clean separation via the contract crate.** `onetagger-tagger` as a dependency-light hub of traits/types is textbook — platforms, engine, and UI all depend on it, not on each other.
- **Format abstraction is the right shape** — `Field` enum + per-format `TagImpl` keeps format quirks contained.
- **One engine, two frontends** (GUI + CLI) falls out naturally from the server-based design.
- **Genuine extensibility** via the FFI plugin system.

**Weaknesses / risks (relevant to any revival)**
- **Scraper fragility is structural**, not incidental. Several platforms parse `__NEXT_DATA__` out of HTML (Beatport) — guaranteed to break on site redesigns. This is *the* reason maintenance dominated and the project stalled.
- **FFI plugin layer is unsafe-heavy** (`Symbol<'static>` transmutes, raw pointer boxing) — a correctness/soundness liability.
- **Frontend god-object**: a 765-line singleton owning all state and the socket switch is hard to test and evolve.
- **Untyped WS protocol**: a 40-variant `Action` enum on one side and a giant JS `switch` on the other, with no shared schema — easy to drift.
- **No visible test suite** in the crates; for a tool whose core risk is matching accuracy and parser breakage, that's a notable gap.

**If reviving it**, the highest-leverage moves would be:
1. Replace HTML-scraping platforms with official APIs where they exist.
2. Add integration tests around `MatchingUtils` and each platform's parser using recorded fixtures.
3. Reconsider whether the FFI plugin system earns its unsafe complexity vs. a simpler subprocess/WASM plugin model.

## Tech stack

- **Backend:** Rust (workspace, resolver 2), Tokio + Axum web server, Wry/Tao webview, rodio (audio), libloading (FFI plugins), songrec (Shazam).
- **Frontend:** Vue 3 + Quasar 2 + TypeScript, Vite 6, vue-router, axios; embedded into the binary at compile time via `include_dir!`.
- **Tag I/O libraries:** id3, metaflac, mp4ameta, lofty.
- **Platforms:** Windows, macOS, Linux (and an Android build referenced in the 1.6.0 changelog).
