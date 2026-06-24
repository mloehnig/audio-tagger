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
