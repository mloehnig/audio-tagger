use std::collections::VecDeque;
use std::time::Instant;
use onetagger_autotag::{TaggingState, TaggingStatusWrap};

pub const RECENT_MAX: usize = 200;

/// One row in the dashboard's "recent" list.
pub struct RecentItem {
    pub state: TaggingState,
    pub label: String,
    pub detail: String,
}

/// Live state of a tagging run, updated from the engine status stream.
pub struct RunState {
    pub progress: f64,
    pub ok: u32,
    pub failed: u32,
    pub skipped: u32,
    pub platform: String,
    pub recent: VecDeque<RecentItem>,
    pub started_at: Instant,
    pub done: bool,
    pub stopping: bool,
}

impl RunState {
    pub fn new() -> RunState {
        RunState {
            progress: 0.0, ok: 0, failed: 0, skipped: 0,
            platform: String::new(),
            recent: VecDeque::with_capacity(RECENT_MAX),
            started_at: Instant::now(),
            done: false, stopping: false,
        }
    }

    pub fn total(&self) -> u32 { self.ok + self.failed + self.skipped }

    /// Fold one engine status into the state.
    pub fn apply(&mut self, wrap: TaggingStatusWrap) {
        self.progress = wrap.progress;
        if !wrap.platform.is_empty() {
            self.platform = wrap.platform.clone();
        }
        match wrap.status.status {
            TaggingState::Ok => self.ok += 1,
            TaggingState::Error => self.failed += 1,
            TaggingState::Skipped => self.skipped += 1,
        }
        let label = wrap.status.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| wrap.status.path.to_string_lossy().to_string());
        let detail = match wrap.status.status {
            TaggingState::Ok => match wrap.status.accuracy {
                Some(a) => format!("{} {:.2}", wrap.platform, a),
                None => wrap.platform.clone(),
            },
            _ => wrap.status.message.clone().unwrap_or_default(),
        };
        if self.recent.len() == RECENT_MAX {
            self.recent.pop_back();
        }
        self.recent.push_front(RecentItem { state: wrap.status.status, label, detail });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use onetagger_autotag::{TaggingState, TaggingStatus, TaggingStatusWrap};

    fn wrap(state: TaggingState, name: &str, progress: f64) -> TaggingStatusWrap {
        TaggingStatusWrap {
            platform: "deezer".to_string(),
            progress,
            status: TaggingStatus {
                status: state,
                path: PathBuf::from(format!("/music/{name}")),
                message: None,
                accuracy: Some(1.0),
                used_shazam: false,
                release_id: None,
                reason: None,
                changes: None,
            },
        }
    }

    #[test]
    fn counts_and_progress() {
        let mut s = RunState::new();
        s.apply(wrap(TaggingState::Ok, "a.mp3", 0.5));
        s.apply(wrap(TaggingState::Error, "b.mp3", 0.75));
        s.apply(wrap(TaggingState::Skipped, "c.mp3", 1.0));
        assert_eq!(s.ok, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.skipped, 1);
        assert_eq!(s.total(), 3);
        assert_eq!(s.progress, 1.0);
        assert_eq!(s.platform, "deezer");
        assert_eq!(s.recent.len(), 3);
    }

    #[test]
    fn recent_is_bounded() {
        let mut s = RunState::new();
        for i in 0..(RECENT_MAX + 50) {
            s.apply(wrap(TaggingState::Ok, &format!("{i}.mp3"), 1.0));
        }
        assert_eq!(s.recent.len(), RECENT_MAX);
    }
}
