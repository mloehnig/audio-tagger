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

use std::path::{Path, PathBuf};

/// Commented starter shown when the user has no config.toml yet.
pub const CONFIG_TEMPLATE: &str = "# OneTagger configuration\n\n# [spotify]\n# client_id = \"\"\n# client_secret = \"\"\n\n# acoustid_api_key = \"\"\n\n# [defaults]\n# platforms = [\"deezer\"]\n# tags = [\"title\", \"artist\", \"genre\", \"bpm\"]\n# threads = 8\n";

/// Path to the user config file.
pub fn config_path() -> PathBuf {
    match Settings::get_folder() {
        Ok(d) => d.join("config.toml"),
        Err(_) => PathBuf::from("config.toml"),
    }
}

/// Current config text, or the commented template if the file doesn't exist.
pub fn config_text() -> String {
    std::fs::read_to_string(config_path()).unwrap_or_else(|_| CONFIG_TEMPLATE.to_string())
}

/// Write `text` to `path` and set 0600 on Unix. Factored out so it is unit-testable.
pub fn write_with_perms(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)?;
    set_owner_only(path);
    Ok(())
}

/// Save config text to the user config path.
pub fn save(text: &str) -> std::io::Result<()> {
    write_with_perms(&config_path(), text)
}

#[cfg(unix)]
fn set_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_with_perms_writes_and_chmods() {
        let dir = std::env::temp_dir().join(format!("ot_tui_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        write_with_perms(&path, "hello = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello = 1\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
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
