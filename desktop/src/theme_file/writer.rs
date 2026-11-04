//! Putting a theme on disk without holding up the window. Each change is checked at once, where it is made, by resolving
//! the theme as a render would. What is left is slow: typesetting a probe document in the new text, which finds what
//! resolving cannot, and the write with its `F_FULLFSYNC`. Both run on one thread, in the order the changes were made, so
//! two quick edits can never land the other way round, and a text the probe refuses is never written.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use super::validate::{self, Problem};

/// A theme text on its way to a file.
#[derive(Debug, Clone)]
pub struct Commit {
    pub path: PathBuf,
    pub text: String,
}

impl Commit {
    /// Typesets the probe in the text, and writes the text if that works. A text that does not typeset is still written
    /// when the one the file holds does not either, so a theme that was broken before can be mended in steps. `disk` is
    /// the text the file holds.
    pub fn run(&self, disk: &str) -> Result<(), Problem> {
        if let Err(problem) = validate::renders(&self.path, &self.text)
            && validate::renders(&self.path, disk).is_ok()
        {
            return Err(problem);
        }
        crate::persist::atomic::write(&self.path, self.text.as_bytes()).map_err(|error| Problem {
            property: None,
            message: format!("{}: {error}", self.path.display()),
        })
    }
}

/// What became of a commit.
pub struct Done {
    pub commit: Commit,
    pub result: Result<(), Problem>,
}

enum Message {
    Write {
        commit: Commit,
        disk: String,
    },
    /// Answers once everything before it has been written.
    Flush(mpsc::Sender<()>),
}

/// The thread that writes themes. It ends, after what it was given, when the writer is dropped.
pub struct Writer {
    jobs: mpsc::Sender<Message>,
}

impl Writer {
    /// `report` is called on the writer's thread for each commit, in order.
    pub fn new(report: impl Fn(Done) + Send + 'static) -> Self {
        let (jobs, queue) = mpsc::channel::<Message>();
        thread::Builder::new()
            .name("druck-theme-writer".to_owned())
            .spawn(move || {
                // The text each file holds after the writes so far. A commit that failed leaves it as it was, so the
                // next one is judged against what is really there.
                let mut held: HashMap<PathBuf, String> = HashMap::new();
                for message in queue {
                    match message {
                        Message::Write { commit, disk } => {
                            let disk = held.get(&commit.path).cloned().unwrap_or(disk);
                            let result = commit.run(&disk);
                            if result.is_ok() {
                                held.insert(commit.path.clone(), commit.text.clone());
                            }
                            report(Done { commit, result });
                        }
                        Message::Flush(reply) => {
                            reply.send(()).ok();
                        }
                    }
                }
            })
            .expect("a thread for writing themes");
        Self { jobs }
    }

    /// Queues a commit. `disk` is what the file held when the change was made, for a file the writer has not written yet.
    pub fn submit(&self, commit: Commit, disk: String) {
        self.jobs.send(Message::Write { commit, disk }).ok();
    }

    /// Waits until everything submitted so far has been written, for a program that is about to end.
    pub fn flush(&self) {
        let (reply, answer) = mpsc::channel();
        if self.jobs.send(Message::Flush(reply)).is_ok() {
            answer.recv().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::theme_file::tests::Scratch;

    const BASE: &str = "{ \"version\": 1 }\n";

    fn commit(path: &std::path::Path, size: u32) -> Commit {
        Commit {
            path: path.to_owned(),
            text: format!("{{ \"version\": 1, \"styles\": {{ \"body\": {{ \"size\": \"{size}pt\" }} }} }}\n"),
        }
    }

    #[test]
    fn quick_edits_land_in_the_order_they_were_made() {
        let scratch = Scratch::new();
        let path = scratch.write("t.json", BASE);
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let writer = Writer::new(move |done| seen.lock().unwrap().push((done.commit.text, done.result.is_ok())));
        let all: Vec<Commit> = (10..20).map(|size| commit(&path, size)).collect();
        for each in &all {
            writer.submit(each.clone(), BASE.to_owned());
        }
        writer.flush();
        let order = order.lock().unwrap();
        let asked: Vec<&String> = all.iter().map(|each| &each.text).collect();
        assert_eq!(order.iter().map(|(text, _)| text).collect::<Vec<_>>(), asked);
        assert!(order.iter().all(|(_, written)| *written));
        assert_eq!(fs::read_to_string(&path).unwrap(), all.last().unwrap().text);
    }

    #[test]
    fn a_text_the_probe_refuses_is_never_written_and_the_next_is_judged_against_the_file() {
        let scratch = Scratch::new();
        let path = scratch.write("t.json", BASE);
        let results = Arc::new(Mutex::new(Vec::new()));
        let seen = results.clone();
        let writer = Writer::new(move |done| seen.lock().unwrap().push(done.result.is_ok()));
        // Resolves, but names a font file that is not there, which the typesetter cannot load.
        let broken = Commit {
            path: path.clone(),
            text: "{ \"version\": 1, \"fonts\": { \"Ghost\": { \"regular\": \"fonts/ghost.otf\" } }, \"styles\": { \"body\": { \"font\": \"Ghost\" } } }\n".to_owned(),
        };
        let on_top = Commit {
            path: path.clone(),
            text: broken.text.replace(
                "\"body\": { \"font\": \"Ghost\" }",
                "\"body\": { \"font\": \"Ghost\", \"size\": \"12pt\" }",
            ),
        };
        writer.submit(broken, BASE.to_owned());
        writer.submit(on_top, BASE.to_owned());
        writer.flush();
        assert_eq!(*results.lock().unwrap(), [false, false]);
        assert_eq!(fs::read_to_string(&path).unwrap(), BASE);
    }
}
