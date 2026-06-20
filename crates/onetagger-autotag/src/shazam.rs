use anyhow::Error;
use std::path::Path;
use std::thread::Builder;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use crossbeam_channel::{bounded, Sender, Receiver};
use lazy_static::lazy_static;
use onetagger_player::rodio::source::UniformSourceIterator;
use serde::{Serialize, Deserialize};
use songrec::SignatureGenerator;
use onetagger_player::AudioSources;

// Global Shazam rate limit, decoupled from the tagging thread count. Shazam runs on the
// tagging worker threads (up to `--threads`), which can fire many concurrent requests at
// Shazam's unofficial endpoint and trigger 429s. These globals cap concurrency and space
// out requests regardless of how many tagging threads exist.
static SHAZAM_CONCURRENCY: AtomicUsize = AtomicUsize::new(3);
static SHAZAM_INTERVAL_MS: AtomicU64 = AtomicU64::new(350);

lazy_static! {
    /// Token bucket used as a semaphore. Holds `SHAZAM_CONCURRENCY` permits (read once, at first use).
    static ref SHAZAM_SLOTS: (Sender<()>, Receiver<()>) = {
        let n = SHAZAM_CONCURRENCY.load(Ordering::SeqCst).max(1);
        let (tx, rx) = bounded(n);
        for _ in 0..n { let _ = tx.send(()); }
        (tx, rx)
    };
    /// Start time of the most recent request, for minimum-interval spacing.
    static ref SHAZAM_LAST: Mutex<Option<Instant>> = Mutex::new(None);
}

/// Configure the global Shazam rate limit. Must be called before the first recognition
/// (the concurrency value is read once when the permit pool is first created).
pub fn configure_shazam(concurrency: usize, interval_ms: u64) {
    SHAZAM_CONCURRENCY.store(concurrency.max(1), Ordering::SeqCst);
    SHAZAM_INTERVAL_MS.store(interval_ms, Ordering::SeqCst);
}

/// RAII permit returned to the bucket on drop.
struct ShazamPermit;
impl Drop for ShazamPermit {
    fn drop(&mut self) { let _ = SHAZAM_SLOTS.0.send(()); }
}

/// Acquire a concurrency permit and enforce the minimum spacing between request starts.
/// Held for the duration of the network request (the permit is returned when dropped).
fn shazam_throttle() -> ShazamPermit {
    // Limit how many requests are in flight at once
    SHAZAM_SLOTS.1.recv().expect("Shazam permit pool closed");
    // Space out request starts globally
    let interval = Duration::from_millis(SHAZAM_INTERVAL_MS.load(Ordering::SeqCst));
    if !interval.is_zero() {
        let mut last = SHAZAM_LAST.lock().unwrap();
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < interval {
                std::thread::sleep(interval - elapsed);
            }
        }
        *last = Some(Instant::now());
    }
    ShazamPermit
}

pub struct Shazam;

impl Shazam {
    /// Recognize song on Shazam from path, returns Track, Duration
    pub fn recognize_from_file(path: impl AsRef<Path>) -> Result<(ShazamTrack, u128), Error> {
        // Load file
        let source = AudioSources::from_path(path)?;
        let duration = source.duration();
        let conv = UniformSourceIterator::new(source.get_source()?, 1, 16000);
        // Get 12s part from middle
        let buffer = if duration >= 12000 {
            // ((duration / 1000) * 16KHz) / 2 (half duration) - (6 * 16KHz) seconds.
            conv.skip((duration * 8 - 96000) as usize).take(16000 * 12).collect::<Vec<i16>>()
        } else {
            conv.collect::<Vec<i16>>()
        };
        // Calculating singnature requires 6MB stack, because it allocates >2MB of buffers for some reason
        let signature = Builder::new()
            .stack_size(1024 * 1024 * 6)
            .spawn(move || { SignatureGenerator::make_signature_from_buffer(&buffer) })
            .unwrap()
            .join()
            .unwrap();
        // Throttle the network request (permit held for the duration of the call)
        let response = {
            let _permit = shazam_throttle();
            songrec::recognize_song_from_signature(&signature, 0)
        }.map_err(|e| anyhow!("{e:?}"))?;
        let response: ShazamResponse = serde_json::from_value(response)?;
        let track = response.track.ok_or(anyhow!("Shazam returned no matches!"))?;
        Ok((track, duration))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamResponse {
    pub timestamp: u64,
    pub tagid: String,
    pub track: Option<ShazamTrack>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamTrack {
    pub albumadamid: Option<String>,
    pub artists: Option<Vec<ShazamSmall>>,
    pub genres: Option<ShazamGenres>,
    pub images: Option<ShazamImages>,
    pub isrc: Option<String>,
    pub key: String,
    pub sections: Vec<ShazamSection>,
    /// Song title
    pub title: String,
    /// Artist
    pub subtitle: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamSmall {
    pub adamid: String,
    pub id: String
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamGenres {
    pub primary: Option<String>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamImages {
    pub background: String,
    pub coverart: String,
    pub coverarthq: String
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShazamSection {
    MetaSection {
        metadata: Vec<ShazamMetadataSection>
    },
    ArtistSection {
        id: String,
        name: String,
        tabname: String,
        // Has to == "ARTIST"
        #[serde(rename = "type")]
        _type: String
    },
    Other {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShazamMetadataSection {
    pub text: String,
    pub title: String
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The throttle should space out request starts by at least the configured interval.
    #[test]
    fn throttle_spaces_requests() {
        configure_shazam(1, 200);
        let start = Instant::now();
        for _ in 0..3 {
            // Permit is acquired and dropped each iteration (token returned to the pool)
            let _permit = shazam_throttle();
        }
        let elapsed = start.elapsed();
        // 3 starts spaced by 200ms => ~400ms minimum (the first request is immediate)
        assert!(elapsed >= Duration::from_millis(380), "throttle too fast: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(2000), "throttle unexpectedly slow: {elapsed:?}");
    }
}