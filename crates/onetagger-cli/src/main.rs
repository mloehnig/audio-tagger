#[macro_use] extern crate log;
#[macro_use] extern crate onetagger_shared;

mod spotify_auth;
mod user_config;

use anyhow::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use clap::{Parser, Subcommand};
use convert_case::{Casing, Case};
use onetagger_platforms::spotify::Spotify;
use onetagger_renamer::{RenamerConfig, Renamer, TemplateParser};
use onetagger_shared::{VERSION, COMMIT};
use onetagger_autotag::audiofeatures::{AudioFeaturesConfig, AudioFeatures};
use onetagger_autotag::{Tagger, TaggerConfigExt, AudioFileInfoImpl, ChangeEntry, ChangesDocument, TaggingState, TaggingStatusWrap};
use onetagger_tagger::{TaggerConfig, AudioFileInfo, SupportedTag, is_tagged_output_path};
use std::collections::HashMap;

fn main() {
    let cli = Cli::parse();

    // Default configs
    if cli.autotagger_config {
        let config = serde_json::to_string_pretty(&TaggerConfig::custom_default()).expect("Failed serializing default config!");
        println!("{config}");
        return;
    }
    if cli.audiofeatures_config {
        let config = serde_json::to_string_pretty(&AudioFeaturesConfig::default()).expect("Failed serializing config!");
        println!("{config}");
        return;
    }

    if cli.action.is_none() {
        println!("No action. Use onetagger-cli --help to get print help.");
        return;
    }

    // Setup logging
    onetagger_shared::setup();
    info!("\n\nStarting OneTagger v{VERSION} Commit: {COMMIT} OS: {}\n\n", std::env::consts::OS);


    let action = cli.action.unwrap();
    match &action {
        Actions::Autotagger { path, dry_run, changes, save_every, shazam_concurrency, shazam_interval_ms, acoustid_api_key, .. } => {
            let config = action.get_at_config().expect("Failed loading config file!");
            debug!("{:?}", config);

            // Configure the global Shazam rate limit before any recognition starts
            onetagger_autotag::configure_shazam(shazam_concurrency.unwrap_or(3), shazam_interval_ms.unwrap_or(350));
            // Enable AcoustID fallback if a key is provided (flag or ACOUSTID_API_KEY env var)
            let acoustid_key = acoustid_api_key.clone().or_else(|| std::env::var("ACOUSTID_API_KEY").ok());
            onetagger_autotag::configure_acoustid(acoustid_key);

            // Get files
            let mut files = if path.is_file() {
                onetagger_playlist::get_files_from_playlist_file(path).expect("Not a valid playlist file")
            } else {
                AudioFileInfo::get_file_list(&path, config.include_subfolders)
            };
            // Don't re-ingest previously written `.tagged` copies
            if config.preserve_original {
                files.retain(|f| !is_tagged_output_path(f, &config.output_suffix));
            }

            if *dry_run {
                let out_path = changes.clone().unwrap_or_else(|| PathBuf::from("onetagger-changes.json"));
                run_dry_run(config, files, out_path, save_every.unwrap_or(25).max(1));
            } else {
                let rx = Tagger::tag_files(&config, files, Arc::new(Mutex::new(None)));
                let start = timestamp!();
                for status in rx {
                    debug!("{status:?}");
                }
                info!("Tagging finished, took: {} seconds.", (timestamp!() - start) / 1000);
            }
        },
        // Apply changes from a changes file produced by `autotagger --dry-run`
        Actions::Apply { changes, in_place, threads } => {
            let file = File::open(changes).expect("Failed opening changes file!");
            let doc: ChangesDocument = serde_json::from_reader(file).expect("Failed parsing changes file!");
            let results = doc.apply(*in_place, *threads);
            let (mut ok, mut failed) = (0, 0);
            for r in &results {
                match &r.result {
                    Ok(_) => { ok += 1; info!("Applied: {} -> {}", r.path.display(), r.output_path.display()); },
                    Err(e) => { failed += 1; error!("Failed applying to {}: {e}", r.path.display()); }
                }
            }
            info!("Apply finished: {ok} ok, {failed} failed.");
        },
        // List audio files under a directory that aren't yet successfully processed in a changes file
        Actions::Unprocessed { changes, path, no_subfolders } => {
            let file = File::open(changes).expect("Failed opening changes file!");
            let doc: ChangesDocument = serde_json::from_reader(file).expect("Failed parsing changes file!");

            // Paths that already have a successful match (these are considered "processed")
            let matched: std::collections::HashSet<PathBuf> = doc.files.iter()
                .filter(|f| f.matched)
                .map(|f| canonical(&f.path))
                .collect();

            // Gather candidate audio files (mirror the autotagger's input handling)
            let mut files = if path.is_file() {
                onetagger_playlist::get_files_from_playlist_file(path).expect("Not a valid playlist file")
            } else {
                AudioFileInfo::get_file_list(path, !*no_subfolders)
            };
            // Never treat generated `.tagged` copies as inputs
            files.retain(|f| !is_tagged_output_path(f, &doc.config.output_suffix));

            let total = files.len();
            let mut unprocessed: Vec<PathBuf> = files.into_iter()
                .filter(|f| !matched.contains(&canonical(f)))
                .collect();
            unprocessed.sort();

            let report = serde_json::json!({
                "directory": path,
                "changesFile": changes,
                "totalFiles": total,
                "processed": total - unprocessed.len(),
                "unprocessed": unprocessed.len(),
                "files": unprocessed,
            });
            println!("{}", serde_json::to_string_pretty(&report).expect("Failed serializing report"));
        },
        Actions::Audiofeatures { path, config, client_id, client_secret, no_subfolders } => {
            let file = File::open(config).expect("Failed reading config file!");
            let config: AudioFeaturesConfig = serde_json::from_reader(&file).expect("Failed parsing config file!");
            // Cli subfolders override
            let mut subfolders = config.include_subfolders;
            if *no_subfolders {
                subfolders = false;
            }
            // Auth spotify
            let spotify = Spotify::try_cached_token(client_id, client_secret)
                .expect("Spotify unauthorized, please run the authorize-spotify option or login to Spotify in UI at least once!");

            // Get files
            let files = if path.is_file() {
                onetagger_playlist::get_files_from_playlist_file(path).expect("Not a valid playlist file")
            } else {
                AudioFileInfo::get_file_list(&path, subfolders)
            };

            let rx = AudioFeatures::start_tagging(config, spotify, files);
            let start = timestamp!();
            for status in rx {
                debug!("{status:?}");
            }
            info!("Tagging finished, took: {} seconds.", (timestamp!() - start) / 1000);
        },
        // Spotify OAuth flow
        Actions::AuthorizeSpotify { client_id, client_secret } => {
            let (auth_url, client) = Spotify::generate_auth_url(&client_id, &client_secret).expect("Failed generating auth URL!");
            println!("\nPlease go to the following URL and authorize OneTagger:\n{auth_url}\n");
            // Start a minimal local callback server to capture the redirect, then authorize
            spotify_auth::spawn_callback_server();
            let _spotify = Spotify::auth_server(client).expect("Spotify authentication failed!");
            info!("Successfully authorized Spotify!");
            std::process::exit(0);
        },
        // Renamer
        Actions::Renamer { path, output, template, copy, no_subfolders, preview, overwrite, separator, keep_subfolders } => {
            let config = RenamerConfig {
                path: path.to_owned(),
                out_dir: output.to_owned(),
                template: template.to_string(),
                copy: *copy,
                subfolders: !*no_subfolders,
                overwrite: *overwrite,
                separator: separator.to_string(),
                keep_subfolders: *keep_subfolders,
            };
            let mut renamer = Renamer::new(TemplateParser::parse(&template));
            let files = AudioFileInfo::load_files_iter(&config.path, config.subfolders, None, None);
            let names = renamer.generate(files, &config).expect("Failed generating filenames!");

            // Only preview
            if *preview {
                for (i, (from, to)) in names.iter().enumerate() {
                    println!("{}. {:?} -> {:?}", i + 1, from, to);
                }
                return;
            }

            renamer.rename(&names, &config).expect("Failed renaming!");
        },
    }
}

/// Run a dry-run: identify+match every file but write no audio. The proposed changes are
/// written to `out_path` incrementally (every `save_every` files, on completion, and on
/// SIGINT/SIGTERM). If `out_path` already exists, previously-matched files are skipped and
/// only unmatched/failed/new files are reprocessed (resume). Note: SIGKILL cannot be caught.
fn run_dry_run(config: TaggerConfig, mut files: Vec<PathBuf>, out_path: PathBuf, save_every: u32) {
    // Resume: load existing results, keep them, skip files that already matched
    let mut entries: HashMap<PathBuf, ChangeEntry> = HashMap::new();
    let mut pre_matched = 0usize;
    for e in load_existing_changes(&out_path) {
        if e.matched { pre_matched += 1; }
        entries.insert(e.path.clone(), e);
    }
    let before = files.len();
    files.retain(|f| !entries.get(f).map(|e| e.matched).unwrap_or(false));
    let skipped = before - files.len();
    if skipped > 0 {
        info!("Resuming from {}: {skipped} files already matched, {} to (re)process.", out_path.display(), files.len());
    }
    if files.is_empty() {
        write_changes(&out_path, &config, &entries);
        info!("Nothing to process - all files already matched in {}.", out_path.display());
        return;
    }

    // Shared state so the processing loop and the signal handler can both flush
    let state = Arc::new(Mutex::new(entries));

    // Flush and exit on Ctrl-C / SIGTERM. (SIGKILL / `kill -9` cannot be intercepted.)
    {
        let state = state.clone();
        let config = config.clone();
        let out_path = out_path.clone();
        let _ = ctrlc::set_handler(move || {
            onetagger_autotag::STOP_TAGGING.store(true, std::sync::atomic::Ordering::SeqCst);
            let entries = state.lock().unwrap();
            warn!("Interrupted - saving {} results to {} ...", entries.len(), out_path.display());
            write_changes(&out_path, &config, &entries);
            std::process::exit(0);
        });
    }

    let rx = Tagger::tag_files(&config, files, Arc::new(Mutex::new(None)));
    let start = timestamp!();
    let mut processed = 0u32;
    for status in rx {
        debug!("{status:?}");
        let entry = change_entry_from_status(&status);
        {
            let mut entries = state.lock().unwrap();
            match entries.get(&entry.path) {
                // Keep an existing matched entry over a later unmatched one
                Some(existing) if existing.matched && !entry.matched => {},
                _ => { entries.insert(entry.path.clone(), entry); }
            }
        }
        processed += 1;
        if processed % save_every == 0 {
            let entries = state.lock().unwrap();
            write_changes(&out_path, &config, &entries);
            info!("Saved progress: {} files recorded in {}", entries.len(), out_path.display());
        }
    }

    // Final flush
    let entries = state.lock().unwrap();
    write_changes(&out_path, &config, &entries);
    let matched = entries.values().filter(|e| e.matched).count();
    info!("Dry run complete in {}s: {} files recorded ({matched} matched, {pre_matched} pre-existing). Wrote {}",
        (timestamp!() - start) / 1000, entries.len(), out_path.display());
    info!("Review/edit it, then run: onetagger-cli apply --changes {}", out_path.display());
}

/// Canonicalize a path for comparison, falling back to the raw path if it can't be resolved.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Load entries from an existing changes file (returns empty on missing/unparseable file).
fn load_existing_changes(out_path: &Path) -> Vec<ChangeEntry> {
    if !out_path.exists() {
        return vec![];
    }
    match File::open(out_path) {
        Ok(f) => match serde_json::from_reader::<_, ChangesDocument>(f) {
            Ok(doc) => doc.files,
            Err(e) => { warn!("Existing changes file {} couldn't be parsed ({e}); starting fresh.", out_path.display()); vec![] }
        },
        Err(e) => { warn!("Couldn't open existing changes file {} ({e}); starting fresh.", out_path.display()); vec![] }
    }
}

/// Atomically write the changes document (temp file + rename) so a kill mid-write can't corrupt it.
fn write_changes(out_path: &Path, config: &TaggerConfig, entries: &HashMap<PathBuf, ChangeEntry>) {
    let mut files: Vec<ChangeEntry> = entries.values().cloned().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let doc = ChangesDocument::new(config.clone(), files);
    let tmp = out_path.with_extension("tmp");
    let file = match File::create(&tmp) {
        Ok(f) => f,
        Err(e) => { error!("Failed creating temp changes file {}: {e}", tmp.display()); return; }
    };
    if let Err(e) = serde_json::to_writer_pretty(file, &doc) {
        error!("Failed writing changes file: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, out_path) {
        error!("Failed finalizing changes file {}: {e}", out_path.display());
    }
}

/// Build a ChangeEntry (for the dry-run changes document) from a tagging status
fn change_entry_from_status(wrap: &TaggingStatusWrap) -> ChangeEntry {
    let s = &wrap.status;
    let matched = matches!(s.status, TaggingState::Ok);
    let (output_path, art_url, changes) = match &s.changes {
        Some(fc) => (fc.output_path.clone(), fc.art_url.clone(), fc.changes.clone()),
        None => (s.path.clone(), None, Default::default()),
    };
    ChangeEntry {
        path: s.path.clone(),
        output_path,
        matched,
        platform: Some(wrap.platform.clone()),
        accuracy: s.accuracy,
        message: s.message.clone(),
        art_url,
        changes,
    }
}


#[derive(Parser, Debug, Clone)]
#[clap(version)]
struct Cli {
    /// What should OneTagger do
    #[clap(subcommand)]
    action: Option<Actions>,
    
    /// Prints the default Autotagger config and exits
    #[clap(long)]
    autotagger_config: bool,

    /// Prints the default Audio Features config and exits
    #[clap(long)]
    audiofeatures_config: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum Actions {
    /// Start Autotagger in CLI mode
    Autotagger {
        /// Path to music files (overrides config)
        #[clap(short, long)]
        path: PathBuf,

        /// Specify a path to config file
        #[clap(short, long)]
        config: Option<PathBuf>,

        /// Comma separated list of platforms to use. For custom platforms use the library filename
        #[clap(short = 'P', long)]
        platforms: Option<String>,

        /// Comma separated list of tags to use
        #[clap(short, long)]
        tags: Option<String>,

        /// Use ID3v2.4 instead of IDv2.3 for MP3/AIFF files
        #[clap(long)]
        id3v24: bool,

        /// Overwrite the existing tags in the track
        #[clap(long)]
        overwrite: bool,

        /// How many threads to use for the searching & matching process (default: 2x CPU cores)
        #[clap(short = 'j', long)]
        threads: Option<u16>,

        /// How strict should the matching be? Use: 0 - 100, Default: 80 (%).
        #[clap(long)]
        strictness: Option<u8>,

        /// Writes a cover.jpg into the folder
        #[clap(long)]
        album_art_file: bool,

        /// Merge new genres with existing ones
        #[clap(long)]
        merge_genres: bool,

        /// Write the key tag in CAMELOT format
        #[clap(long)]
        camelot: bool,

        /// Write title tag without version (ie. remix)
        #[clap(long)]
        short_title: bool,

        /// Match the song duration as well (WARNING: very strict)
        #[clap(long)]
        match_duration: bool,

        /// If duration matching is enabled, how big the difference in durations can be (in seconds)
        #[clap(long)]
        max_duration_difference: Option<u64>,

        /// Use platform specific ID tags to get exact matches
        #[clap(long)]
        match_by_id: bool,

        /// Try to indentify the track on Shazam if title & artist tags are missing
        #[clap(long)]
        enable_shazam: bool,

        /// Always try to indentify the track on Shazam
        #[clap(long)]
        force_shazam: bool,

        /// Skip tracks that have 1T_TAGGEDDATE tag
        #[clap(long)]
        skip_tagged: bool,

        /// Try to get title & artist from filename if the tags are missing
        #[clap(long)]
        parse_filename: bool,

        /// Template for parse_filename option. Example: `%track$. %artists% - %title%`
        #[clap(long)]
        filename_template: Option<String>,

        /// Don't include subfolders
        #[clap(long)]
        no_subfolders: bool,

        /// Write only year instead of full date
        #[clap(long)]
        only_year: bool,

        /// Tag on multiple platforms instead of the default fallback mode
        #[clap(long)]
        multiplatform: bool,

        /// Modify the original files in place instead of writing `.tagged` copies (DESTRUCTIVE)
        #[clap(long)]
        in_place: bool,

        /// Don't write any files; instead compute the proposed tag changes and save them to a JSON file
        #[clap(long)]
        dry_run: bool,

        /// Output path for the --dry-run changes JSON (default: onetagger-changes.json)
        #[clap(long)]
        changes: Option<PathBuf>,

        /// During --dry-run, write the changes file every N processed files (default: 25)
        #[clap(long)]
        save_every: Option<u32>,

        /// Max concurrent Shazam requests (rate-limit protection; default: 3)
        #[clap(long)]
        shazam_concurrency: Option<usize>,

        /// Minimum milliseconds between Shazam requests (rate-limit protection; default: 350)
        #[clap(long)]
        shazam_interval_ms: Option<u64>,

        /// AcoustID API key — enables AcoustID as a fingerprint fallback after Shazam.
        /// Requires the `fpcalc` (Chromaprint) tool on PATH. Falls back to the ACOUSTID_API_KEY env var.
        #[clap(long)]
        acoustid_api_key: Option<String>,
    },
    /// Apply tag changes from a changes file produced by `autotagger --dry-run`
    Apply {
        /// Path to the changes JSON file
        #[clap(short, long)]
        changes: PathBuf,

        /// Modify the original files in place instead of writing `.tagged` copies (DESTRUCTIVE)
        #[clap(long)]
        in_place: bool,

        /// Max files to write in parallel (default: 2x CPU cores)
        #[clap(short = 'j', long)]
        threads: Option<usize>,
    },
    /// List audio files in a directory that are not yet successfully processed in a changes file (prints JSON)
    Unprocessed {
        /// Path to the changes JSON file
        #[clap(short, long)]
        changes: PathBuf,

        /// Directory (or playlist file) of audio files to check
        #[clap(short, long)]
        path: PathBuf,

        /// Don't include subfolders
        #[clap(long)]
        no_subfolders: bool,
    },
    /// Start Audio Features in CLI mode
    Audiofeatures {
        /// Path to music files (overrides config)
        #[clap(short, long)]
        path: PathBuf,

        /// Specify a path to config file
        #[clap(short, long)]
        config: String,

        /// Spotify Client ID
        #[clap(long)]
        client_id: String,

        /// Spotify Client Secret
        #[clap(long)]
        client_secret: String,

        /// Don't include subfolders
        #[clap(long)]
        no_subfolders: bool,
    },
    /// Authorize Spotify and cache the token
    AuthorizeSpotify {
        /// Spotify Client ID
        #[clap(long)]
        client_id: String,

        /// Spotify Client Secret
        #[clap(long)]
        client_secret: String,
    },
    Renamer {
        /// Path to input files
        #[clap(long, short)]
        path: PathBuf,

        /// Output directory
        #[clap(long, short)]
        output: Option<PathBuf>,

        /// New filename template
        #[clap(long, short)]
        template: String,

        /// Copy files instead of moving
        #[clap(long)]
        copy: bool,

        /// Exclude subfolders 
        #[clap(long)]
        no_subfolders: bool,

        /// Don't actually affect files, only generate new names
        #[clap(long)]
        preview: bool,

        /// Overwrite files
        #[clap(long)]
        overwrite: bool,

        /// Multiple values separator
        #[clap(long, default_value = ", ")]
        separator: String,

        /// Keep original subfolders
        #[clap(long)]
        keep_subfolders: bool,
    },
}

/// For easily generating CLI -> config
macro_rules! config_option {
    ($target:expr, $t:tt) => {
        if *$t {
            $target.$t = *$t;
        }
    };
    ($target:expr, $($t:tt),+) => {
        $(config_option!($target, $t);)+
    }
}

impl Actions {
    //. Create tagger config
    pub fn get_at_config(&self) -> Result<TaggerConfig, Error> {
        match self {
            Actions::Autotagger { path, config, platforms, tags, id3v24,
                overwrite, threads, strictness, album_art_file, merge_genres, camelot,
                short_title, match_duration, max_duration_difference, match_by_id, enable_shazam, force_shazam,
                skip_tagged, parse_filename, filename_template, no_subfolders, only_year, multiplatform,
                in_place, dry_run, changes: _, save_every: _, shazam_concurrency: _, shazam_interval_ms: _, acoustid_api_key: _ } => {

                // Load config
                let has_config_file = config.is_some();
                let mut config = if let Some(config_path) = config {
                    let config = serde_json::from_reader(&File::open(config_path)?)?;
                    config
                } else {
                    TaggerConfig::custom_default()
                };

                // Overrides
                config.path = Some(path.to_owned());
                if let Some(platforms) = platforms {
                    config.platforms = platforms.split(",").map(String::from).collect();
                }
                // Tags
                if let Some(tags) = tags {
                    let tags: Vec<SupportedTag> = tags
                        .split(",")
                        .filter_map(|t| {
                            match serde_json::from_str(&format!("\"{}\"", t.to_case(Case::Camel))) {
                                Ok(tag) => Some(tag),
                                Err(_) => {
                                    warn!("Invalid tag: {t}");
                                    None
                                },
                            }
                        })
                        .collect();
                    config.tags = tags;
                }
                // Boolean options
                config_option!(config, id3v24, overwrite, album_art_file, merge_genres, camelot, short_title, match_duration,
                    match_by_id, enable_shazam, force_shazam, skip_tagged, parse_filename, only_year, multiplatform);
                // Remaining options
                // Threads: -j wins; otherwise default to 2x CPU cores (unless a config file set it)
                if let Some(threads) = threads {
                    config.threads = *threads;
                } else if !has_config_file {
                    config.threads = onetagger_shared::default_thread_count() as u16;
                }
                if let Some(strictness) = strictness {
                    if *strictness > 100 {
                        warn!("Invalid stricness!");
                    } else {
                        config.strictness = *strictness as f64 / 100.0;
                    }
                }
                if let Some(mdd) = max_duration_difference {
                    config.max_duration_difference = *mdd;
                }
                if let Some(template) = filename_template {
                    config.filename_template = Some(template.to_string());
                }
                if *no_subfolders {
                    config.include_subfolders = false;
                }
                // Non-destructive by default in the CLI: write to a `.tagged` copy unless --in-place
                config.preserve_original = !*in_place;
                config.dry_run = *dry_run;
                return Ok(config);
            },
            _ => unreachable!()
        }
    }
}

