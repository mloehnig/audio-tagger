use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use anyhow::Error;
use reqwest::StatusCode;
use serde::{Serialize, Deserialize};
use onetagger_tag::{Tag, CoverType};
use onetagger_tagger::TaggerConfig;

/// Change to a single tag/frame: its current values vs the proposed new values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    #[serde(default)]
    pub old: Vec<String>,
    pub new: Vec<String>,
}

/// The set of tag changes computed for one file by `Track::write_to_file`.
/// Keys in `changes` are raw tag/frame names (exactly what `TagImpl::all_tags` returns
/// and `set_raw` writes), so the JSON round-trips losslessly and is hand-editable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChanges {
    /// Where the result will be / was written (original path, or the `.tagged` copy).
    pub output_path: PathBuf,
    /// Album art URL to fetch when applying (art is binary so it is not part of `changes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art_url: Option<String>,
    pub changes: BTreeMap<String, FieldChange>,
}

/// One file's entry in a [`ChangesDocument`]. Adds match metadata on top of [`FileChanges`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    pub path: PathBuf,
    pub output_path: PathBuf,
    pub matched: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art_url: Option<String>,
    #[serde(default)]
    pub changes: BTreeMap<String, FieldChange>,
}

/// A complete plan of changes produced by `--dry-run`, applied by the `apply` subcommand.
/// The embedded `config` makes `apply` reproducible (separators, id3 version, output suffix...).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesDocument {
    pub version: u32,
    pub config: TaggerConfig,
    pub files: Vec<ChangeEntry>,
}

/// Result of applying a single entry.
pub struct ApplyResult {
    pub path: PathBuf,
    pub output_path: PathBuf,
    pub result: Result<(), Error>,
}

impl ChangesDocument {
    pub const VERSION: u32 = 1;

    /// Build a document from collected per-file entries and the config used.
    pub fn new(config: TaggerConfig, files: Vec<ChangeEntry>) -> ChangesDocument {
        ChangesDocument { version: Self::VERSION, config, files }
    }

    /// Apply every matched entry's changes to disk.
    /// When `in_place` is true the original file is modified; otherwise the entry's
    /// `output_path` (the `.tagged` copy) is written, leaving the original untouched.
    pub fn apply(&self, in_place: bool) -> Vec<ApplyResult> {
        let mut results = vec![];
        for entry in &self.files {
            // Nothing to do for unmatched files or entries with no changes
            if !entry.matched || (entry.changes.is_empty() && entry.art_url.is_none()) {
                continue;
            }
            let target = if in_place { entry.path.clone() } else { entry.output_path.clone() };
            let result = self.apply_entry(entry, &target);
            results.push(ApplyResult { path: entry.path.clone(), output_path: target, result });
        }
        results
    }

    /// Apply one entry to `target`.
    fn apply_entry(&self, entry: &ChangeEntry, target: &PathBuf) -> Result<(), Error> {
        // Write to a copy: duplicate the original first so the tags are written into the copy
        if target != &entry.path {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&entry.path, target)?;
        }

        let mut tag_wrap = Tag::load_file(target, true)?;
        tag_wrap.set_separators(&self.config.separators);
        if let Tag::ID3(t) = &mut tag_wrap {
            t.set_id3v24(self.config.id3v24);
        }

        {
            let tag = tag_wrap.tag_mut();
            for (raw, change) in &entry.changes {
                tag.set_raw(raw, change.new.clone(), true);
            }
        }

        // Album art (binary, fetched fresh from the stored URL)
        if let Some(url) = entry.art_url.as_ref() {
            match download_art(url) {
                Ok(Some(data)) => {
                    tag_wrap.tag_mut().set_art(CoverType::CoverFront, "image/jpeg", Some("Cover"), data);
                }
                Ok(None) => warn!("Invalid album art for {:?}", entry.path),
                Err(e) => warn!("Failed downloading album art for {:?}: {e}", entry.path),
            }
        }

        tag_wrap.tag_mut().save_file(target)?;
        Ok(())
    }
}

/// Diff two `all_tags()` snapshots. Returns only entries whose values changed (added or modified).
pub fn diff_tags(before: &HashMap<String, Vec<String>>, after: &HashMap<String, Vec<String>>) -> BTreeMap<String, FieldChange> {
    let mut changes = BTreeMap::new();
    for (key, new_values) in after {
        let old_values = before.get(key).cloned().unwrap_or_default();
        if &old_values != new_values {
            changes.insert(key.clone(), FieldChange { old: old_values, new: new_values.clone() });
        }
    }
    changes
}

/// Download album art, returning None for invalid/too-small responses. Mirrors `Track::download_art`.
pub fn download_art(url: &str) -> Result<Option<Vec<u8>>, Error> {
    let response = reqwest::blocking::get(url)?;
    if response.status() != StatusCode::OK {
        return Ok(None);
    }
    if let Some(cl) = response.content_length() {
        if cl < 4096 {
            return Ok(None);
        }
    }
    Ok(Some(response.bytes()?.to_vec()))
}
