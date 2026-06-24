<p align='center'>
    <img alt='Logo' src='https://raw.githubusercontent.com/Marekkon5/onetagger/master/assets/onetagger-logo-github.png'>
</p>
<h1 align='center'>Cross-platform music tagger for DJs — CLI</h1>

<hr>

A command-line music tagger. It identifies tracks (by existing tags, filename, or audio
fingerprint via **Shazam**/**AcoustID**) and fetches metadata from **Beatport, Traxsource,
Juno Download, Discogs, MusicBrainz, Spotify, Deezer, iTunes, Bandcamp, Musixmatch, Beatsource
and BPMSupreme**, then writes the tags into your files. It can also fetch Spotify's Audio
Features by ISRC, and rename files from a template.

Supported formats: **MP3, AIFF, FLAC, M4A (AAC, ALAC), WAV, OGG**.

> This is a **CLI-only** fork — the desktop GUI and web UI have been removed. Everything runs
> from `onetagger-cli`.

## Building

Requires [rustup](https://rustup.rs). No Node.js / frontend toolchain is needed.

**Linux** — install build deps, then build:
```
sudo apt install -y lld autogen libasound2-dev pkg-config make libssl-dev gcc g++
cargo build --release -p onetagger-cli
```

**macOS / Windows:**
```
cargo build --release -p onetagger-cli
```

The binary is at `target/release/onetagger-cli`.

Optional: install `fpcalc` (Chromaprint) to enable AcoustID fingerprinting
(e.g. `sudo apt install libchromaprint-tools`).

## Usage

```
onetagger-cli <command> [options]
```

Commands: `autotagger`, `apply`, `unprocessed`, `audiofeatures`, `authorize-spotify`, `renamer`.
Run `onetagger-cli <command> --help` for all options.

### Auto-tagging

```
onetagger-cli autotagger \
  --path /path/to/music \
  --platforms deezer,beatport \
  --enable-shazam \
  --tags title,artist,album,genre,bpm,label,releaseDate,isrc,albumArt
```

- **Safe by default:** writes the result to a copy beside each file
  (`song.mp3` → `song.tagged.mp3`), leaving the original untouched. Pass `--in-place` to
  overwrite originals (destructive).
- `--platforms` is a comma-separated fallback order. No-auth platforms include `deezer`,
  `beatport`, `musicbrainz`, `itunes`, `junodownload`. `spotify` requires authorization (below).
- `--enable-shazam` identifies untagged files by audio fingerprint. The identifier chain is
  **Shazam → AcoustID** (AcoustID runs when `--acoustid-api-key` is set and `fpcalc` is
  installed). Shazam requests are throttled (`--shazam-concurrency`, `--shazam-interval-ms`).
- `-j` / `--threads` sets parallelism (default: **2× CPU cores**).

### Preview, edit, apply (dry-run workflow)

Generate a JSON plan of proposed tag changes without touching audio, edit it by hand, then apply:

```
onetagger-cli autotagger --path /music --platforms deezer --enable-shazam \
  --dry-run --changes changes.json
# review/edit changes.json ...
onetagger-cli apply --changes changes.json        # writes .tagged copies (--in-place for originals)
```

Dry-run writes incrementally and is **resumable**: re-running skips already-matched files and
retries the rest, and it saves progress on `Ctrl-C`/`SIGTERM`. List what's left with:

```
onetagger-cli unprocessed --changes changes.json --path /music   # prints JSON
```

### Spotify

Register an app at the [Spotify dashboard](https://developer.spotify.com/dashboard) with
**Web API** enabled and redirect URI `http://127.0.0.1:36913/spotify`, then authorize once
(this starts a tiny local callback server and caches the token):

```
onetagger-cli authorize-spotify --client-id <ID> --client-secret <SECRET>
```

For Spotify auto-tagging, the credentials also go in a `--config` JSON file's
`spotify: { clientId, clientSecret }` block.

## Credits
SongRec (Shazam support) — https://github.com/marin-m/SongRec

## License
See [LICENSE](LICENSE).
