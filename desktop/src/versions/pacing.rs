//! When a file's local history gets its next snapshot: at a pause, not too often, and not never during long typing. Each
//! edit only notes the time, so a key costs a hash lookup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Typing has stopped for this long.
const PAUSE: Duration = Duration::from_secs(5);
/// A file gets at most one snapshot in this time, so a writer who stops every few seconds does not fill the disk.
const LEAST_GAP: Duration = Duration::from_secs(30);
/// Typing without a pause still gets a snapshot this often.
const MOST_GAP: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct Pacing {
    /// Files edited since their last snapshot, with the time of that first edit.
    waiting: HashMap<PathBuf, Instant>,
    /// When each file last had a snapshot.
    taken: HashMap<PathBuf, Instant>,
    edited: Option<Instant>,
}

/// What the timer should do.
#[derive(Debug, PartialEq, Eq)]
pub enum Due {
    Idle,
    Wait(Duration),
    Take(Vec<PathBuf>),
}

impl Pacing {
    /// Notes an edit. Returns whether this is the file's first edit this run, so what it held before can be kept.
    pub fn edited(&mut self, path: &Path, now: Instant) -> bool {
        self.edited = Some(now);
        if self.waiting.contains_key(path) {
            return false;
        }
        self.waiting.insert(path.to_owned(), now);
        !self.taken.contains_key(path)
    }

    /// Records a snapshot taken for another reason, or that the file has nothing older to keep.
    pub fn taken(&mut self, path: &Path, now: Instant) {
        self.taken.insert(path.to_owned(), now);
    }

    pub fn next(&mut self, now: Instant) -> Due {
        let Some(edited) = self.edited else {
            return Due::Idle;
        };
        if self.waiting.is_empty() {
            return Due::Idle;
        }
        let due_at = |since: Instant, last: Option<&Instant>| {
            let earliest = last.map_or(since, |last| *last + LEAST_GAP);
            (edited + PAUSE).max(earliest).min(since + MOST_GAP)
        };
        let mut take = Vec::new();
        let mut soonest = None;
        for (path, since) in &self.waiting {
            let due = due_at(*since, self.taken.get(path));
            if due <= now {
                take.push(path.clone());
            } else if soonest.is_none_or(|soonest| due < soonest) {
                soonest = Some(due);
            }
        }
        if !take.is_empty() {
            for path in &take {
                self.waiting.remove(path);
                self.taken.insert(path.clone(), now);
            }
            take.sort();
            return Due::Take(take);
        }
        soonest.map_or(Due::Idle, |soonest| Due::Wait(soonest - now))
    }

    /// Everything still waiting, for a quit.
    pub fn drain(&mut self) -> Vec<PathBuf> {
        self.waiting.drain().map(|(path, _)| path).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pause_takes_a_snapshot_and_not_before() {
        let start = Instant::now();
        let mut pacing = Pacing::default();
        let path = Path::new("/a.md");
        assert_eq!(pacing.next(start), Due::Idle);
        assert!(pacing.edited(path, start));
        assert_eq!(
            pacing.next(start + Duration::from_secs(1)),
            Due::Wait(Duration::from_secs(4))
        );
        assert_eq!(pacing.next(start + PAUSE), Due::Take(vec![path.to_owned()]));
        assert_eq!(pacing.next(start + PAUSE), Due::Idle);
    }

    #[test]
    fn snapshots_keep_a_least_gap() {
        let start = Instant::now();
        let mut pacing = Pacing::default();
        let path = Path::new("/a.md");
        pacing.edited(path, start);
        assert!(matches!(pacing.next(start + PAUSE), Due::Take(_)));
        let later = start + PAUSE + Duration::from_secs(1);
        assert!(!pacing.edited(path, later));
        let paused = later + PAUSE;
        assert!(matches!(pacing.next(paused), Due::Wait(_)));
        assert!(matches!(pacing.next(start + PAUSE + LEAST_GAP), Due::Take(_)));
    }

    #[test]
    fn long_typing_still_gets_a_snapshot() {
        let start = Instant::now();
        let mut pacing = Pacing::default();
        let path = Path::new("/a.md");
        let mut now = start;
        while now < start + MOST_GAP {
            pacing.edited(path, now);
            assert!(matches!(pacing.next(now), Due::Wait(_)));
            now += Duration::from_secs(1);
        }
        pacing.edited(path, now);
        assert!(matches!(pacing.next(now), Due::Take(_)));
    }
}
