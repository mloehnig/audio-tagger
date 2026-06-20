use std::path::Path;
use anyhow::{anyhow, Error};
use onetagger_tagger::AudioFileInfo;
use crate::acoustid;
use crate::AudioFileInfoImpl;

/// Which identifier produced the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierKind {
    Shazam,
    AcoustId,
}

/// Try to identify an unknown file by audio fingerprint, using each available provider in
/// order and falling back to the next when one fails. Shazam first (no setup required),
/// then AcoustID (only when an API key is configured). Returns the matched info and which
/// provider produced it.
pub fn identify(path: &Path) -> Result<(AudioFileInfo, IdentifierKind), Error> {
    let mut errors: Vec<String> = vec![];

    // 1. Shazam (always available)
    match AudioFileInfo::shazam(path) {
        Ok(info) => return Ok((info, IdentifierKind::Shazam)),
        Err(e) => errors.push(format!("shazam: {e}")),
    }

    // 2. AcoustID (only if an API key was configured)
    if acoustid::is_configured() {
        match acoustid::identify(path) {
            Ok(info) => return Ok((info, IdentifierKind::AcoustId)),
            Err(e) => {
                warn!("AcoustID identify failed: {e}");
                errors.push(format!("acoustid: {e}"));
            }
        }
    } else {
        debug!("AcoustID skipped (no API key configured)");
    }

    Err(anyhow!("All identifiers failed ({})", errors.join("; ")))
}
