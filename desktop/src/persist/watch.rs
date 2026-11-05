//! The folder watcher and the wait between a change and acting on it. Holds no gpui types.
//!
//! The watcher only says which paths may have changed. What they hold is read afterward and compared by content, so
//! the app's own writes and an editor's rename or delete-and-recreate come out the same as anything else.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::atomic::is_temporary;

/// How long a path must stay quiet before it is read, so a burst of events is one read.
pub const QUIET: Duration = Duration::from_millis(100);
/// A path that keeps changing is read after this anyway.
const MOST: Duration = Duration::from_secs(1);
/// How long a file may be gone, or new, before that counts. Saving by delete and create, a `git checkout`, and the two
/// halves of a rename pass within this.
pub const GRACE: Duration = Duration::from_millis(500);

const FOLDERS: [&str; 3] = ["manuscript", "notes", "research"];
/// What a project's folder holds that no book is set from: its output.
const OUTPUT: &str = "build";

/// What the watcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Path(PathBuf),
    /// Too much changed to name, such as a folder moved. Everything needs a look.
    Everything,
    /// A file that is no text of the book, such as an image or a bibliography, which a layout that failed may have lacked.
    Resource,
}

/// Which paths matter.
#[derive(Debug, Default)]
struct Scope {
    /// The document's folder as given, and as the file system reports it. They differ under a symlink.
    root: PathBuf,
    real_root: PathBuf,
    /// A project watches its folder for new files. A single file watches only what is listed.
    project: bool,
    /// Files that matter wherever they are, by real path, with the path the session knows them by.
    files: HashMap<PathBuf, PathBuf>,
    /// Files a book names that are not its text, by real path, wherever they are.
    resources: HashSet<PathBuf>,
}

impl Scope {
    fn signal(&self, path: &Path) -> Option<Signal> {
        if is_temporary(path) {
            return None;
        }
        if let Some(known) = self.files.get(path) {
            return Some(Signal::Path(known.clone()));
        }
        if self.resources.contains(path) {
            return Some(Signal::Resource);
        }
        if !self.project {
            return None;
        }
        if path == self.real_root {
            return Some(Signal::Everything);
        }
        let relative = path.strip_prefix(&self.real_root).ok()?;
        let top = relative.components().next()?.as_os_str();
        let hidden = relative
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if !FOLDERS.iter().any(|folder| OsStr::new(folder) == top) {
            if relative == Path::new(druck::input::MANIFEST) {
                return Some(Signal::Path(self.root.join(relative)));
            }
            let quiet = hidden || top.to_string_lossy().starts_with('.') || top == OsStr::new(OUTPUT);
            return (!quiet).then_some(Signal::Resource);
        }
        let text = (relative.extension())
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt"));
        match relative.extension() {
            // A name without an extension here is a folder that came or went, and its files with it, unannounced.
            None if !hidden => Some(Signal::Everything),
            _ if text && !hidden => Some(Signal::Path(self.root.join(relative))),
            _ if !hidden => Some(Signal::Resource),
            _ => None,
        }
    }
}

/// Watches a document's folder. Filters on the watcher's own thread and stays silent otherwise, so an idle folder and a
/// busy `.git` cost the app nothing.
pub struct Watcher {
    inner: RecommendedWatcher,
    scope: Arc<Mutex<Scope>>,
    watched: HashSet<PathBuf>,
}

impl Watcher {
    /// Starts watching. `root` is the project's folder, or a single file's. `signal` runs on the watcher's thread.
    pub fn start(root: &Path, project: bool, signal: impl Fn(Signal) + Send + 'static) -> notify::Result<Self> {
        let scope = Arc::new(Mutex::new(Scope {
            root: root.to_owned(),
            real_root: fs::canonicalize(root)?,
            project,
            files: HashMap::new(),
            resources: HashSet::new(),
        }));
        let filter = scope.clone();
        let mut inner = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else {
                signal(Signal::Everything);
                return;
            };
            if event.need_rescan() {
                signal(Signal::Everything);
                return;
            }
            let Ok(scope) = filter.lock() else {
                return;
            };
            for path in &event.paths {
                if let Some(found) = scope.signal(path) {
                    signal(found);
                }
            }
        })?;
        if project {
            inner.watch(root, RecursiveMode::Recursive)?;
        }
        Ok(Self {
            inner,
            scope,
            watched: HashSet::new(),
        })
    }

    /// Sets the files that matter by name: a project's chapters, wherever the manifest lists them, the theme, a single
    /// file. A symlink's target or a file outside the folder gets a watch of its own, on its folder, which survives the
    /// file being replaced. `resources` are watched the same way and report only as [`Signal::Resource`].
    pub fn set_files(&mut self, files: &[PathBuf], resources: &[PathBuf]) {
        let (root, project) = match self.scope.lock() {
            Ok(scope) => (scope.real_root.clone(), scope.project),
            Err(_) => return,
        };
        let mut known = HashMap::new();
        let mut folders = HashSet::new();
        for file in files {
            let real = real_path(file);
            if let Some(folder) = real.parent()
                && !(project && folder.starts_with(&root))
            {
                folders.insert(folder.to_owned());
            }
            known.insert(real, file.clone());
            // The name itself may be replaced, whatever it points to.
            let parent = file.parent().and_then(|folder| fs::canonicalize(folder).ok());
            if let (Some(parent), Some(name)) = (parent, file.file_name()) {
                known.insert(parent.join(name), file.clone());
            }
        }
        let mut named = HashSet::new();
        for file in resources {
            let real = real_path(file);
            if let Some(folder) = real.parent()
                && !(project && folder.starts_with(&root))
            {
                folders.insert(folder.to_owned());
            }
            named.insert(real);
        }
        if let Ok(mut scope) = self.scope.lock() {
            scope.files = known;
            scope.resources = named;
        }
        for folder in folders.difference(&self.watched).cloned().collect::<Vec<_>>() {
            if self.inner.watch(&folder, RecursiveMode::NonRecursive).is_ok() {
                self.watched.insert(folder);
            }
        }
        for folder in self.watched.difference(&folders).cloned().collect::<Vec<_>>() {
            self.inner.unwatch(&folder).ok();
            self.watched.remove(&folder);
        }
    }
}

/// A file's real path, also when the file or its target does not exist yet.
fn real_path(file: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(file) {
        return real;
    }
    match (
        file.parent().and_then(|folder| fs::canonicalize(folder).ok()),
        file.file_name(),
    ) {
        (Some(folder), Some(name)) => folder.join(name),
        _ => file.to_owned(),
    }
}

/// Paths that changed, waiting to be read. Each waits until it has been quiet for [`QUIET`], or, if it is settled only
/// by waiting, [`GRACE`].
#[derive(Debug, Default)]
pub struct Waiting {
    paths: HashMap<PathBuf, (Instant, Instant)>,
    everything: bool,
    resource: bool,
}

/// What is ready to read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ready {
    pub paths: Vec<PathBuf>,
    pub everything: bool,
    pub resource: bool,
}

impl Waiting {
    pub fn note(&mut self, signal: Signal, now: Instant) {
        match signal {
            Signal::Path(path) => {
                let times = self.paths.entry(path).or_insert((now, now));
                times.1 = now;
            }
            Signal::Everything => self.everything = true,
            Signal::Resource => self.resource = true,
        }
    }

    /// While a path waits for its grace, everything waits with it, so a rename's two halves and the manifest that names
    /// the new one are taken in together.
    fn due(&self, times: (Instant, Instant), plain: bool) -> Instant {
        if plain {
            (times.1 + QUIET).min(times.0 + MOST)
        } else {
            times.1 + GRACE
        }
    }

    fn grace_ends(&self, plain: &impl Fn(&Path) -> bool) -> Option<Instant> {
        (self.paths.iter())
            .filter(|(path, _)| !plain(path))
            .map(|(_, times)| self.due(*times, false))
            .max()
    }

    /// How long until something may be ready. `plain` says a path is a known file that exists, so it needs no grace.
    pub fn wait(&self, now: Instant, plain: impl Fn(&Path) -> bool) -> Option<Duration> {
        if self.everything || self.resource {
            return Some(Duration::ZERO);
        }
        if let Some(ends) = self.grace_ends(&plain) {
            return Some(ends.saturating_duration_since(now));
        }
        (self.paths.values())
            .map(|times| self.due(*times, true))
            .min()
            .map(|due| due.saturating_duration_since(now))
    }

    /// Takes what is ready.
    pub fn take(&mut self, now: Instant, plain: impl Fn(&Path) -> bool) -> Ready {
        let graced = self.grace_ends(&plain).is_some_and(|ends| ends <= now);
        if self.grace_ends(&plain).is_some() && !graced {
            return Ready {
                paths: Vec::new(),
                everything: std::mem::take(&mut self.everything),
                resource: std::mem::take(&mut self.resource),
            };
        }
        let mut ready = Vec::new();
        for (path, times) in std::mem::take(&mut self.paths) {
            if graced || self.due(times, true) <= now {
                ready.push(path);
            } else {
                self.paths.insert(path, times);
            }
        }
        ready.sort();
        Ready {
            paths: ready,
            everything: std::mem::take(&mut self.everything),
            resource: std::mem::take(&mut self.resource),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn scope() -> Scope {
        Scope {
            root: PathBuf::from("/link/book"),
            real_root: PathBuf::from("/real/book"),
            project: true,
            files: HashMap::from([(PathBuf::from("/themes/mine.json"), PathBuf::from("/themes/mine.json"))]),
            resources: HashSet::from([PathBuf::from("/shared/refs.bib")]),
        }
    }

    fn found(path: &str) -> Option<Signal> {
        scope().signal(Path::new(path))
    }

    fn path(path: &str) -> Option<Signal> {
        Some(Signal::Path(PathBuf::from(path)))
    }

    #[test]
    fn the_document_s_files_pass_under_the_name_the_session_uses() {
        assert_eq!(found("/real/book/manuscript/a.md"), path("/link/book/manuscript/a.md"));
        assert_eq!(found("/real/book/notes/sub/a.txt"), path("/link/book/notes/sub/a.txt"));
        assert_eq!(found("/real/book/druck.yaml"), path("/link/book/druck.yaml"));
        assert_eq!(found("/themes/mine.json"), path("/themes/mine.json"));
    }

    #[test]
    fn everything_else_is_silent() {
        for quiet in [
            "/real/book/.git/index",
            "/real/book/build/book.pdf",
            "/real/book/manuscript/.a.md.12.druck-tmp",
            "/real/book/manuscript/.a.md.swp",
            "/real/book/.druck/notes.json",
            "/elsewhere/manuscript/a.md",
        ] {
            assert_eq!(found(quiet), None, "{quiet}");
        }
    }

    #[test]
    fn a_file_a_layout_may_lack_reports_as_a_resource() {
        for resource in [
            "/real/book/literatur.bib",
            "/real/book/assets/cover.png",
            "/real/book/manuscript/picture.png",
            "/shared/refs.bib",
        ] {
            assert_eq!(found(resource), Some(Signal::Resource), "{resource}");
        }
    }

    #[test]
    fn a_folder_that_moves_means_everything() {
        assert_eq!(found("/real/book/manuscript"), Some(Signal::Everything));
        assert_eq!(found("/real/book"), Some(Signal::Everything));
    }

    #[test]
    fn a_burst_of_events_is_one_read_after_it_goes_quiet() {
        let now = Instant::now();
        let mut waiting = Waiting::default();
        let file = PathBuf::from("/a.md");
        let plain = |_: &Path| true;
        waiting.note(Signal::Path(file.clone()), now);
        waiting.note(Signal::Path(file.clone()), now + QUIET / 2);
        assert_eq!(waiting.take(now + QUIET, plain), Ready::default());
        assert_eq!(waiting.wait(now + QUIET, plain), Some(QUIET / 2));
        let ready = waiting.take(now + QUIET * 2, plain);
        assert_eq!(ready.paths, vec![file]);
        assert_eq!(waiting.wait(now, plain), None);
    }

    #[test]
    fn a_file_that_keeps_changing_is_read_anyway() {
        let now = Instant::now();
        let mut waiting = Waiting::default();
        let plain = |_: &Path| true;
        for step in 0..=(MOST.as_millis() as u64 / 50) {
            waiting.note(
                Signal::Path(PathBuf::from("/a.md")),
                now + Duration::from_millis(step * 50),
            );
        }
        assert_eq!(waiting.take(now + MOST, plain).paths.len(), 1);
    }

    #[test]
    fn a_missing_file_waits_for_the_grace() {
        let now = Instant::now();
        let mut waiting = Waiting::default();
        let missing = |_: &Path| false;
        waiting.note(Signal::Path(PathBuf::from("/a.md")), now);
        assert!(waiting.take(now + QUIET * 2, missing).paths.is_empty());
        assert_eq!(waiting.take(now + GRACE, missing).paths.len(), 1);
    }

    #[test]
    fn a_known_file_waits_with_a_rename_in_progress() {
        let now = Instant::now();
        let mut waiting = Waiting::default();
        let plain = |path: &Path| path == Path::new("/druck.yaml");
        waiting.note(Signal::Path(PathBuf::from("/druck.yaml")), now);
        waiting.note(Signal::Path(PathBuf::from("/old.md")), now);
        assert!(waiting.take(now + QUIET * 2, plain).paths.is_empty());
        assert_eq!(waiting.take(now + GRACE, plain).paths.len(), 2);
    }

    #[test]
    fn the_real_watcher_survives_a_rename_over_and_ignores_the_rest() {
        let dir = std::env::temp_dir().join(format!("druck-watch-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("manuscript")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        let chapter = dir.join("manuscript/a.md");
        fs::write(&chapter, "one").unwrap();
        let (send, receive) = mpsc::channel();
        let watcher = Watcher::start(&dir, true, move |signal| {
            send.send(signal).ok();
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        for round in 0..2 {
            let temporary = dir.join(format!("manuscript/.a.md.{round}.tmp"));
            fs::write(&temporary, format!("round {round}")).unwrap();
            fs::rename(&temporary, &chapter).unwrap();
            fs::write(dir.join(".git/index"), "noise").unwrap();
            let expected = Signal::Path(dir.join("manuscript/a.md"));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match receive.recv_timeout(left) {
                    Ok(signal) if signal == expected => break,
                    Ok(Signal::Path(other)) => assert!(!other.ends_with(".git/index"), "noise passed"),
                    Ok(Signal::Everything | Signal::Resource) => {}
                    Err(_) => panic!("no event in round {round}"),
                }
            }
            while receive.recv_timeout(Duration::from_millis(100)).is_ok() {}
        }
        drop(watcher);
        fs::remove_dir_all(dir).ok();
    }
}
