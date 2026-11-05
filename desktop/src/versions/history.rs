//! Local history: what each file held at each pause, in the app data folder and never near the project. Holds no gpui
//! types.
//!
//! `history/blobs/ab/cdef...` holds each text once, named by its Git blob id, as plain files a person can read.
//! `history/files/<id>.jsonl` is one file's log: a line with its path, then one line per snapshot. A snapshot writes its
//! blob by rename first and appends its line second, in one write, so a crash leaves at worst a line that does not parse
//! or a blob nothing names. Reading skips the first, an append starts on a line of its own after one, and thinning clears
//! the second.
//!
//! A file is known by its path. When a path has no log, [`History::follow`] looks for the log of a file that is gone:
//! one that once held this exact text, or one whose folder another file's move has shown to have moved. What it cannot
//! place stays where it is, and the timeline names it.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::persist::atomic;

const DAY: i64 = 24 * 60 * 60 * 1000;
const HOUR: i64 = 60 * 60 * 1000;
/// Everything goes after this long.
const KEEP_DAYS: i64 = 30;
/// Up to this age every snapshot stays. Then the last of each hour, and after [`HOURLY_DAYS`] the last of each day.
const ALL_DAYS: i64 = 1;
const HOURLY_DAYS: i64 = 7;
/// A blob nothing names is left alone for this long, as a snapshot may be between writing it and naming it.
const BLOB_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const THINNED_STAMP: &str = "thinned";

/// One saved state of a file, oldest first in a log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Milliseconds since 1970.
    #[serde(rename = "t")]
    pub time: i64,
    /// The blob's name.
    #[serde(rename = "h")]
    pub hash: String,
    /// Why it was taken, when it was not a pause.
    #[serde(rename = "l", default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A log's first line.
#[derive(Serialize, Deserialize)]
struct Header {
    path: String,
}

/// A log whose file is not at its path any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub path: PathBuf,
    log: PathBuf,
}

/// What [`History::follow`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Followed {
    /// The folder that moved and where to, for each move it recognized.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// How many files' histories came along.
    pub files: usize,
    /// Files with no history, each with a log that could be theirs by its name alone.
    pub unplaced: Vec<(PathBuf, Orphan)>,
}

/// The local history of every book on this machine.
pub struct History {
    root: PathBuf,
    /// One writer at a time, so thinning cannot clear a blob a snapshot is about to name.
    writing: Mutex<()>,
}

/// The time now, as snapshots store it.
pub fn now_ms() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    since.as_millis() as i64
}

/// The Git blob id of some bytes, which names them in the store.
pub fn blob_id(bytes: &[u8]) -> String {
    gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, bytes)
        .map(|id| id.to_string())
        .unwrap_or_default()
}

impl History {
    /// The history under the app's data folder `data`.
    pub fn new(data: &Path) -> Self {
        Self {
            root: data.join("history"),
            writing: Mutex::new(()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.writing.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn files(&self) -> PathBuf {
        self.root.join("files")
    }

    fn log_of(&self, path: &Path) -> PathBuf {
        self.files()
            .join(format!("{}.jsonl", blob_id(path.as_os_str().as_encoded_bytes())))
    }

    fn blob(&self, hash: &str) -> PathBuf {
        let (folder, name) = hash.split_at(hash.len().min(2));
        self.root.join("blobs").join(folder).join(name)
    }

    /// Records what a file holds now. A text equal to the last snapshot and no label records nothing.
    pub fn snapshot(&self, path: &Path, text: &str, label: Option<&str>, time: i64) -> io::Result<()> {
        let hash = blob_id(text.as_bytes());
        let _writing = self.lock();
        let log = self.log_of(path);
        if label.is_none() && read_log(&log).last().is_some_and(|last| last.hash == hash) {
            return Ok(());
        }
        let blob = self.blob(&hash);
        if !blob.exists() {
            fs::create_dir_all(blob.parent().unwrap_or(&self.root))?;
            atomic::write_quick(&blob, text.as_bytes())?;
        }
        fs::create_dir_all(self.files())?;
        let mut lines = String::new();
        if !log.exists() {
            lines.push_str(&header(path));
        } else if !ends_in_line_break(&log)? {
            lines.push('\n');
        }
        lines.push_str(&line(&Snapshot {
            time,
            hash,
            label: label.map(str::to_owned),
        }));
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?
            .write_all(lines.as_bytes())
    }

    /// A file's versions, oldest first. Snapshots of one text in a row count as one, keeping the first label among
    /// them, so a label before an agent turn that left this file alone adds no mark.
    pub fn versions(&self, path: &Path) -> Vec<Snapshot> {
        let mut versions: Vec<Snapshot> = Vec::new();
        for snapshot in read_log(&self.log_of(path)) {
            match versions.last_mut() {
                Some(last) if last.hash == snapshot.hash => {
                    last.label = last.label.take().or(snapshot.label);
                }
                _ => versions.push(snapshot),
            }
        }
        versions.retain(|version| self.blob(&version.hash).exists());
        versions
    }

    /// A version's exact bytes.
    pub fn text(&self, hash: &str) -> io::Result<String> {
        fs::read_to_string(self.blob(hash))
    }

    /// Moves a file's log along with the file, when the app itself saw the move.
    pub fn moved(&self, from: &Path, to: &Path) -> io::Result<()> {
        let _writing = self.lock();
        let (old, new) = (self.log_of(from), self.log_of(to));
        if !old.exists() || new.exists() {
            return Ok(());
        }
        self.move_log(&old, to)
    }

    /// Gives a log to the file now at `to`, when a person said it is that file's.
    pub fn claim(&self, orphan: &Orphan, to: &Path) -> io::Result<()> {
        let _writing = self.lock();
        if self.log_of(to).exists() {
            return Ok(());
        }
        self.move_log(&orphan.log, to)
    }

    fn move_log(&self, log: &Path, to: &Path) -> io::Result<()> {
        let mut text = header(to);
        for snapshot in read_log(log) {
            text.push_str(&line(&snapshot));
        }
        atomic::write(&self.log_of(to), text.as_bytes())?;
        fs::remove_file(log)
    }

    /// Every log whose path holds no file any more.
    fn orphans(&self) -> Vec<Orphan> {
        let mut found = Vec::new();
        for entry in fs::read_dir(self.files()).into_iter().flatten().flatten() {
            let log = entry.path();
            if let Some(path) = read_path(&log)
                && !path.exists()
            {
                found.push(Orphan { path, log });
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }

    /// Looks for the history of each of `files` that has none: first the log of a file that is gone and once held this
    /// exact text, which also shows which folder moved, then logs at the same place under a folder that moved. What is
    /// left and shares a name comes back as `unplaced`.
    pub fn follow(&self, files: &[PathBuf]) -> Followed {
        let _writing = self.lock();
        let mut followed = Followed::default();
        let mut lost: Vec<PathBuf> = files
            .iter()
            .filter(|file| !self.log_of(file).exists())
            .cloned()
            .collect();
        if lost.is_empty() {
            return followed;
        }
        let mut orphans = self.orphans();
        if orphans.is_empty() {
            return followed;
        }
        // A log goes to a file only by its name and its last text: a file that once held the same text, such as a new
        // starter chapter or an empty note, is not the one that moved.
        let mut last: HashMap<(OsString, String), usize> = HashMap::new();
        for (at, orphan) in orphans.iter().enumerate() {
            if let (Some(name), Some(snapshot)) = (orphan.path.file_name(), read_log(&orphan.log).pop()) {
                last.insert((name.to_owned(), snapshot.hash), at);
            }
        }
        let mut taken = HashSet::new();
        lost.retain(|file| {
            let (Some(name), Ok(bytes)) = (file.file_name(), fs::read(file)) else {
                return true;
            };
            let Some(&at) = last.get(&(name.to_owned(), blob_id(&bytes))) else {
                return true;
            };
            if !taken.insert(at) || self.move_log(&orphans[at].log, file).is_err() {
                return true;
            }
            followed.files += 1;
            let moved = moved_folders(&orphans[at].path, file);
            if !followed.moved.contains(&moved) {
                followed.moved.push(moved);
            }
            false
        });
        let mut at = 0;
        orphans.retain(|_| {
            at += 1;
            !taken.contains(&(at - 1))
        });
        for (from, to) in &followed.moved {
            lost.retain(|file| {
                let place = file.strip_prefix(to).ok().map(|inner| from.join(inner));
                let Some(found) = place.and_then(|place| orphans.iter().position(|orphan| orphan.path == place)) else {
                    return true;
                };
                if self.move_log(&orphans[found].log, file).is_err() {
                    return true;
                }
                orphans.remove(found);
                followed.files += 1;
                false
            });
        }
        for file in lost {
            if let Some(orphan) = orphans
                .iter()
                .find(|orphan| orphan.path.file_name() == file.file_name())
            {
                followed.unplaced.push((file, orphan.clone()));
            }
        }
        followed
    }

    /// Thins every log by age and clears what nothing names. At most once a day; returns whether it ran.
    pub fn thin_daily(&self, now: i64) -> bool {
        let stamp = self.root.join(THINNED_STAMP);
        let last = fs::read_to_string(&stamp)
            .ok()
            .and_then(|text| text.trim().parse::<i64>().ok());
        if last.is_some_and(|last| now - last < DAY) {
            return false;
        }
        self.thin(now);
        atomic::write_quick(&stamp, now.to_string().as_bytes()).ok();
        true
    }

    /// Thins every log by age, removes a log with nothing left, and clears blobs nothing names.
    pub fn thin(&self, now: i64) {
        let _writing = self.lock();
        let mut named = HashSet::new();
        for entry in fs::read_dir(self.files()).into_iter().flatten().flatten() {
            let log = entry.path();
            let Some(path) = read_path(&log) else {
                continue;
            };
            let snapshots = read_log(&log);
            let kept = thin(&snapshots, now);
            if kept.is_empty() {
                fs::remove_file(&log).ok();
                continue;
            }
            if kept.len() != snapshots.len() {
                let mut text = header(&path);
                for snapshot in &kept {
                    text.push_str(&line(snapshot));
                }
                if atomic::write_quick(&log, text.as_bytes()).is_err() {
                    named.extend(snapshots.into_iter().map(|snapshot| snapshot.hash));
                    continue;
                }
            }
            named.extend(kept.into_iter().map(|snapshot| snapshot.hash));
        }
        for folder in fs::read_dir(self.root.join("blobs")).into_iter().flatten().flatten() {
            for blob in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
                let hash = format!(
                    "{}{}",
                    folder.file_name().to_string_lossy(),
                    blob.file_name().to_string_lossy()
                );
                let old_enough = (blob.metadata().and_then(|metadata| metadata.modified()))
                    .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age >= BLOB_GRACE));
                if !named.contains(&hash) && old_enough {
                    fs::remove_file(blob.path()).ok();
                }
            }
        }
    }

    /// Whether this is the first time a folder is asked to become a repo, which only happens once.
    pub fn first_asking(&self, book: &Path) -> bool {
        let stamp = self
            .root
            .join("asked")
            .join(blob_id(book.as_os_str().as_encoded_bytes()));
        if stamp.exists() {
            return false;
        }
        fs::create_dir_all(stamp.parent().unwrap_or(&self.root)).ok();
        fs::write(stamp, "").ok();
        true
    }
}

/// Which snapshots stay at their age: all within a day, the last of each hour within a week, the last of each day within
/// 30 days. A labelled one stays through the week, since it marks a moment the author may look for.
fn thin(snapshots: &[Snapshot], now: i64) -> Vec<Snapshot> {
    let mut kept: Vec<Snapshot> = Vec::new();
    let mut last_bucket = None;
    for snapshot in snapshots.iter().rev() {
        let age = now - snapshot.time;
        if age > KEEP_DAYS * DAY {
            continue;
        }
        let bucket = if age <= ALL_DAYS * DAY {
            None
        } else if age <= HOURLY_DAYS * DAY {
            Some(snapshot.time / HOUR)
        } else {
            Some(snapshot.time / DAY)
        };
        let labelled = snapshot.label.is_some() && age <= HOURLY_DAYS * DAY;
        if bucket.is_none() || labelled || bucket != last_bucket {
            kept.push(snapshot.clone());
        }
        last_bucket = bucket;
    }
    kept.reverse();
    kept
}

fn header(path: &Path) -> String {
    let path = path.to_string_lossy().into_owned();
    format!("{}\n", to_json(&Header { path }))
}

fn line(snapshot: &Snapshot) -> String {
    format!("{}\n", to_json(snapshot))
}

/// Strings and numbers, which always serialize.
fn to_json(record: &impl Serialize) -> String {
    serde_json::to_string(record).unwrap_or_default()
}

fn read_path(log: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(log).ok()?;
    let header: Header = serde_json::from_str(text.lines().next()?).ok()?;
    Some(PathBuf::from(header.path))
}

/// A log's snapshots. A line a crash cut short does not parse and is skipped.
fn read_log(log: &Path) -> Vec<Snapshot> {
    let Ok(text) = fs::read_to_string(log) else {
        return Vec::new();
    };
    (text.lines())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Whether the log's last write finished its line. One a crash cut short would glue the next record to it.
fn ends_in_line_break(log: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(log)?;
    if file.seek(SeekFrom::End(0))? == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

/// The folders that differ, once the part of two paths they share at their ends is taken off.
fn moved_folders(from: &Path, to: &Path) -> (PathBuf, PathBuf) {
    let (old, new): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    let shared = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let folder = |all: &[Component]| all[..all.len() - shared].iter().collect::<PathBuf>();
    (folder(&old), folder(&new))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (PathBuf, History) {
        let dir = std::env::temp_dir().join(format!("druck-history-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let history = History::new(&dir.join("data"));
        (dir, history)
    }

    #[test]
    fn a_line_cut_by_a_crash_loses_only_itself() {
        let (dir, history) = store("cut");
        let path = dir.join("a.md");
        let now = now_ms();
        history.snapshot(&path, "first", None, now).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(history.log_of(&path))
            .unwrap()
            .write_all(b"{\"t\":123,\"h\":\"ab")
            .unwrap();
        history.snapshot(&path, "second", None, now + 1).unwrap();
        let versions = history.versions(&path);
        assert_eq!(versions.len(), 2);
        assert_eq!(history.text(&versions[1].hash).unwrap(), "second");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_log_keeps_its_format() {
        let (dir, history) = store("format");
        let path = dir.join("a.md");
        history.snapshot(&path, "text", Some("label"), 5).unwrap();
        history.snapshot(&path, "more", None, 6).unwrap();
        let log = fs::read_to_string(history.log_of(&path)).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines[0], format!("{{\"path\":\"{}\"}}", path.display()));
        assert!(lines[1].starts_with("{\"t\":5,\"h\":") && lines[1].ends_with(",\"l\":\"label\"}"));
        assert!(lines[2].starts_with("{\"t\":6,\"h\":") && !lines[2].contains("\"l\""));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_week_old_snapshot_comes_back_byte_exact() {
        let (dir, history) = store("exact");
        let path = dir.join("a.md");
        let text = "Größe \u{2014} und\r\nkein Zeilenende am Schluss\t";
        let now = now_ms();
        history.snapshot(&path, text, None, now - 7 * DAY).unwrap();
        history.snapshot(&path, "später", None, now).unwrap();
        history.thin(now);
        let versions = history.versions(&path);
        assert_eq!(versions.len(), 2);
        assert_eq!(history.text(&versions[0].hash).unwrap(), text);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_same_text_again_adds_nothing_and_a_label_adds_no_mark() {
        let (dir, history) = store("same");
        let path = dir.join("a.md");
        history.snapshot(&path, "one", None, 1).unwrap();
        history.snapshot(&path, "one", None, 2).unwrap();
        assert_eq!(read_log(&history.log_of(&path)).len(), 1);
        history.snapshot(&path, "one", Some("Before agent turn"), 3).unwrap();
        history.snapshot(&path, "two", None, 4).unwrap();
        let versions = history.versions(&path);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].label.as_deref(), Some("Before agent turn"));
        assert_eq!(versions[1].label, None);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn age_thins_to_hours_then_days_then_nothing() {
        let now = 100 * DAY;
        let at = |time: i64, label: Option<&str>| Snapshot {
            time,
            hash: time.to_string(),
            label: label.map(str::to_owned),
        };
        let snapshots = [
            at(now - 31 * DAY, None),
            at(now - 20 * DAY - 2 * HOUR, None),
            at(now - 20 * DAY - HOUR, None),
            at(now - 3 * DAY - 2 * 60_000, None),
            at(now - 3 * DAY - 60_000, None),
            at(now - 3 * DAY - 5 * HOUR, Some("Before agent turn")),
            at(now - HOUR - 60_000, None),
            at(now - HOUR, None),
        ];
        let mut sorted = snapshots.to_vec();
        sorted.sort_by_key(|snapshot| snapshot.time);
        let kept: Vec<i64> = thin(&sorted, now).iter().map(|snapshot| now - snapshot.time).collect();
        assert!(!kept.contains(&(31 * DAY)), "older than 30 days goes");
        assert_eq!(kept.iter().filter(|age| **age > 7 * DAY).count(), 1, "one a day");
        assert!(kept.contains(&(3 * DAY + 5 * HOUR)), "a label stays through the week");
        assert_eq!(
            kept.iter()
                .filter(|age| **age >= 3 * DAY && **age <= 3 * DAY + HOUR)
                .count(),
            1,
            "one an hour"
        );
        assert!(
            kept.contains(&(HOUR + 60_000)) && kept.contains(&HOUR),
            "all within a day"
        );
    }

    #[test]
    fn thinning_clears_only_blobs_nothing_names() {
        let (dir, history) = store("sweep");
        let now = 100 * DAY;
        let path = dir.join("a.md");
        history.snapshot(&path, "too old", None, now - 40 * DAY).unwrap();
        history.snapshot(&path, "kept", None, now).unwrap();
        let old = history.blob(&blob_id(b"too old"));
        let year_ago = SystemTime::now() - Duration::from_secs(365 * 24 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(year_ago)
            .unwrap();
        history.thin(now);
        assert!(!old.exists());
        assert!(history.blob(&blob_id(b"kept")).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn history_follows_a_moved_book_and_says_what_it_cannot_place() {
        let (dir, history) = store("move");
        let old = dir.join("old/shelf/book");
        let new = dir.join("new/book");
        fs::create_dir_all(old.join("manuscript")).unwrap();
        fs::write(old.join("manuscript/a.md"), "chapter a").unwrap();
        fs::write(old.join("manuscript/b.md"), "chapter b").unwrap();
        fs::write(old.join("manuscript/c.md"), "chapter c").unwrap();
        for (name, text) in [("a", "chapter a"), ("b", "chapter b"), ("c", "chapter c")] {
            history
                .snapshot(&old.join(format!("manuscript/{name}.md")), text, None, 1)
                .unwrap();
        }
        let elsewhere = dir.join("elsewhere/d.md");
        history.snapshot(&elsewhere, "gone", None, 1).unwrap();

        fs::create_dir_all(dir.join("new")).unwrap();
        fs::rename(&old, &new).unwrap();
        // b changed after its last snapshot, so only its place finds it.
        fs::write(new.join("manuscript/b.md"), "chapter b, edited elsewhere").unwrap();
        fs::write(new.join("d.md"), "new text with an old name").unwrap();
        let files = [
            new.join("manuscript/a.md"),
            new.join("manuscript/b.md"),
            new.join("manuscript/c.md"),
            new.join("d.md"),
        ];
        let followed = history.follow(&files);
        assert_eq!(followed.files, 3);
        assert_eq!(followed.moved, vec![(dir.join("old/shelf"), dir.join("new"))]);
        assert_eq!(history.versions(&files[0]).len(), 1);
        assert_eq!(history.versions(&files[1]).len(), 1);
        assert_eq!(history.text(&history.versions(&files[1])[0].hash).unwrap(), "chapter b");
        assert_eq!(followed.unplaced.len(), 1);
        assert_eq!(followed.unplaced[0].0, files[3]);
        assert_eq!(followed.unplaced[0].1.path, elsewhere);
        history.claim(&followed.unplaced[0].1, &files[3]).unwrap();
        assert_eq!(history.text(&history.versions(&files[3])[0].hash).unwrap(), "gone");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_takes_no_log_that_only_once_held_its_text_or_has_another_name() {
        let (dir, history) = store("claim");
        let deleted = dir.join("deleted/book/manuscript/01-first-chapter.md");
        history.snapshot(&deleted, "# The first chapter\n", None, 1).unwrap();
        history
            .snapshot(&deleted, "# The first chapter\n\nWritten on.\n", None, 2)
            .unwrap();
        let other = dir.join("deleted/book/notes/idea.md");
        history.snapshot(&other, "", None, 1).unwrap();
        fs::create_dir_all(dir.join("new/manuscript")).unwrap();
        let starter = dir.join("new/manuscript/01-first-chapter.md");
        fs::write(&starter, "# The first chapter\n").unwrap();
        let empty = dir.join("new/manuscript/empty.md");
        fs::write(&empty, "").unwrap();
        let followed = history.follow(&[starter.clone(), empty.clone()]);
        assert_eq!(followed.files, 0);
        assert!(history.versions(&starter).is_empty());
        assert!(history.versions(&empty).is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_rename_the_app_saw_keeps_the_log() {
        let (dir, history) = store("rename");
        let (from, to) = (dir.join("a.md"), dir.join("b.md"));
        history.snapshot(&from, "one", None, 1).unwrap();
        history.moved(&from, &to).unwrap();
        assert!(history.versions(&from).is_empty());
        assert_eq!(history.versions(&to).len(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_id_is_gits_blob_id() {
        assert_eq!(blob_id(b"hello\n"), "ce013625030ba8dba906f756967f9e9ca394464a");
    }
}
