use serde::Deserialize;
use onetagger_shared::Settings;

/// Subset of ~/.config/onetagger/config.toml `[defaults]` the TUI pre-fills forms from.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TuiDefaults {
    pub platforms: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub threads: Option<u16>,
    pub enable_shazam: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileRoot {
    defaults: TuiDefaults,
}

/// Load `[defaults]` from the user config file; returns defaults if absent/malformed.
pub fn load_defaults() -> TuiDefaults {
    let path = match Settings::get_folder() { Ok(d) => d.join("config.toml"), Err(_) => return TuiDefaults::default() };
    let Ok(text) = std::fs::read_to_string(&path) else { return TuiDefaults::default() };
    match toml::from_str::<FileRoot>(&text) {
        Ok(root) => root.defaults,
        Err(e) => { warn!("Failed parsing {}: {e}", path.display()); TuiDefaults::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_defaults_subset() {
        let root: FileRoot = toml::from_str("[defaults]\nplatforms=[\"deezer\"]\nthreads=5\nenable_shazam=true\n").unwrap();
        assert_eq!(root.defaults.platforms.as_deref(), Some(&["deezer".to_string()][..]));
        assert_eq!(root.defaults.threads, Some(5));
        assert_eq!(root.defaults.enable_shazam, Some(true));
    }
    #[test]
    fn empty_is_default() {
        let root: FileRoot = toml::from_str("").unwrap();
        assert!(root.defaults.platforms.is_none());
    }
}
