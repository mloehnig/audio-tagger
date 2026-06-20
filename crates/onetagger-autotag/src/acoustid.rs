use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use anyhow::{anyhow, Error};
use serde::Deserialize;
use lazy_static::lazy_static;
use onetagger_tag::AudioFileFormat;
use onetagger_tagger::{AudioFileInfo, FileTaggedStatus};

lazy_static! {
    /// AcoustID application API key (https://acoustid.org/api-key). None = AcoustID disabled.
    static ref ACOUSTID_KEY: Mutex<Option<String>> = Mutex::new(None);
    /// Last request start, for minimum-interval spacing (AcoustID asks for <= 3 req/s).
    static ref ACOUSTID_LAST: Mutex<Option<Instant>> = Mutex::new(None);
}

/// Set the AcoustID API key. Empty/None disables the AcoustID identifier.
/// The key is trimmed so a stray newline/space (e.g. from a shell or env var) doesn't
/// turn a valid key into an "invalid API key" error.
pub fn configure_acoustid(api_key: Option<String>) {
    *ACOUSTID_KEY.lock().unwrap() = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
}

/// Whether AcoustID is usable (an API key has been configured).
pub fn is_configured() -> bool {
    ACOUSTID_KEY.lock().unwrap().is_some()
}

/// Respect AcoustID's ~3 requests/second guidance.
fn throttle() {
    let min = Duration::from_millis(350);
    let mut last = ACOUSTID_LAST.lock().unwrap();
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < min {
            std::thread::sleep(min - elapsed);
        }
    }
    *last = Some(Instant::now());
}

/// Identify a file via Chromaprint (`fpcalc`) + the AcoustID lookup API.
/// Requires the `fpcalc` tool (Chromaprint) on PATH and a configured API key.
pub fn identify(path: &Path) -> Result<AudioFileInfo, Error> {
    let key = ACOUSTID_KEY.lock().unwrap().clone()
        .ok_or(anyhow!("AcoustID API key not configured"))?;

    // Fingerprint with Chromaprint's fpcalc
    let output = Command::new("fpcalc")
        .arg("-json")
        .arg(path)
        .output()
        .map_err(|e| anyhow!("Failed running fpcalc (is Chromaprint/fpcalc installed and on PATH?): {e}"))?;
    if !output.status.success() {
        return Err(anyhow!("fpcalc failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let fp: FpcalcOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow!("Failed parsing fpcalc output: {e}"))?;

    // Lookup on AcoustID
    throttle();
    let duration = (fp.duration.round() as i64).to_string();
    let client = reqwest::blocking::Client::new();
    let response: AcoustIdResponse = client
        .post("https://api.acoustid.org/v2/lookup")
        .form(&[
            ("client", key.as_str()),
            ("meta", "recordings"),
            ("duration", duration.as_str()),
            ("fingerprint", fp.fingerprint.as_str()),
        ])
        .send()?
        .json()?;

    if response.status != "ok" {
        let msg = response.error.map(|e| e.message).unwrap_or_else(|| response.status.clone());
        return Err(anyhow!("AcoustID error: {msg}"));
    }

    // First result (highest score) with a usable recording wins
    for result in response.results.unwrap_or_default() {
        for rec in result.recordings.unwrap_or_default() {
            if let Some(title) = rec.title {
                let artists: Vec<String> = rec.artists.unwrap_or_default().into_iter().map(|a| a.name).collect();
                info!("Recognized on AcoustID: {:?}: {} - {}", path, title, artists.join(", "));
                return Ok(AudioFileInfo {
                    title: Some(title),
                    artists: AudioFileInfo::parse_artist_tag(artists.iter().map(|s| s.as_str()).collect()),
                    format: AudioFileFormat::from_extension(&path.extension().unwrap_or_default().to_string_lossy())
                        .ok_or(anyhow!("Unknown audio format"))?,
                    path: path.to_owned(),
                    isrc: None,
                    duration: Some(Duration::from_secs_f64(fp.duration).into()),
                    track_number: None,
                    tagged: FileTaggedStatus::Untagged,
                    tags: Default::default(),
                });
            }
        }
    }
    Err(anyhow!("AcoustID: no recording match"))
}

#[derive(Debug, Deserialize)]
struct FpcalcOutput {
    duration: f64,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
struct AcoustIdResponse {
    status: String,
    #[serde(default)]
    results: Option<Vec<AcoustIdResult>>,
    #[serde(default)]
    error: Option<AcoustIdError>,
}

#[derive(Debug, Deserialize)]
struct AcoustIdError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct AcoustIdResult {
    #[serde(default)]
    recordings: Option<Vec<AcoustIdRecording>>,
}

#[derive(Debug, Deserialize)]
struct AcoustIdRecording {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artists: Option<Vec<AcoustIdArtist>>,
}

#[derive(Debug, Deserialize)]
struct AcoustIdArtist {
    name: String,
}
