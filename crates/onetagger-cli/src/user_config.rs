use std::path::{Path, PathBuf};
use serde::Deserialize;
use onetagger_shared::Settings;
use convert_case::{Casing, Case};
use onetagger_tagger::{TaggerConfig, SupportedTag};

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
        if let Some(s) = self.strictness {
            if s <= 100 { config.strictness = s as f64 / 100.0; }
            else { warn!("Invalid strictness in config: {s} (must be 0-100)"); }
        }
        if let Some(s) = &self.output_suffix { config.output_suffix = s.clone(); }
        if let Some(v) = self.overwrite { config.overwrite = v; }
        if let Some(v) = self.enable_shazam { config.enable_shazam = v; }
        if let Some(v) = self.force_shazam { config.force_shazam = v; }
        if let Some(v) = self.include_subfolders { config.include_subfolders = v; }
    }
}

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
    let p = Settings::get_folder()?.join("config.toml");
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

    #[test]
    fn template_parses_to_defaults() {
        // The shipped template is fully commented, so it parses to an all-default config.
        let cfg: UserConfig = toml::from_str(TEMPLATE).unwrap();
        assert!(cfg.spotify.is_none());
        assert!(cfg.acoustid_api_key.is_none());
        assert!(cfg.defaults.platforms.is_none());
    }
}
