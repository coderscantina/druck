//! Makes a folder of Markdown a project: writes `druck.yaml` beside the author's files, which stay where they are. The
//! first chapter's front matter moves into it, since a project's chapters have none. Nothing else changes, and nothing is
//! written unless the whole of it can be: a front matter the manifest could not read stops it before the first write.

use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use druck::editor::resolved_config::split_front_matter;
use druck::input::MANIFEST;

use super::strings::Key;
use super::{MANUSCRIPT, details, templates, yaml};
use crate::persist::atomic;

const CHAPTERS: &str = "chapters";
const TITLE: &str = "title";
/// Markdown that sits in a project's root for other tools, not for the book.
const NOT_CHAPTERS: [&str; 3] = ["README.md", "CLAUDE.md", "AGENTS.md"];

/// What making a folder a project would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The chapters, relative to the folder, in name order.
    pub chapters: Vec<String>,
    /// The file whose front matter would move.
    pub moves_front_matter_of: Option<String>,
}

impl Offer {
    /// One line saying what it does, so nothing happens that the author did not read.
    pub fn hint(&self) -> String {
        let count = self.chapters.len();
        match &self.moves_front_matter_of {
            Some(file) => crate::t!(
                Key::MakeProjectHintMoves,
                manifest = MANIFEST,
                count = count,
                file = file
            ),
            None => crate::t!(Key::MakeProjectHint, manifest = MANIFEST, count = count),
        }
    }
}

/// The label of the offer.
pub fn label() -> &'static str {
    crate::t!(Key::MakeProject)
}

fn markdown_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            let lower = name.to_lowercase();
            lower.ends_with(".md") && !NOT_CHAPTERS.iter().any(|other| other.eq_ignore_ascii_case(name))
        })
        .collect();
    names.sort();
    names
}

/// What can be made of a folder with no manifest: the files in `manuscript/`, or if it has none, the ones in the folder
/// itself, listed where they stand. `None` if there is no Markdown or it is a project already.
pub fn offer(dir: &Path) -> Option<Offer> {
    if !dir.is_dir() || dir.join(MANIFEST).exists() {
        return None;
    }
    let in_manuscript = markdown_in(&dir.join(MANUSCRIPT));
    let chapters: Vec<String> = if in_manuscript.is_empty() {
        markdown_in(dir)
    } else {
        (in_manuscript.iter())
            .map(|name| format!("{MANUSCRIPT}/{name}"))
            .collect()
    };
    let first = chapters.first()?.clone();
    let text = fs::read_to_string(dir.join(&first)).ok()?;
    let has_front_matter = split_front_matter(&text).ok()?.is_some();
    Some(Offer {
        chapters,
        moves_front_matter_of: has_front_matter.then_some(first),
    })
}

/// A path `value` written in a file in `from`, as written from the project's folder. A path that is not relative stays.
fn from_project(value: &str, from: &Path) -> String {
    if Path::new(value).is_absolute() {
        return value.to_owned();
    }
    let joined = from.join(value);
    let mut parts: Vec<Component<'_>> = Vec::new();
    for part in joined.components() {
        match (part, parts.last()) {
            (Component::CurDir, _) => {}
            (Component::ParentDir, Some(Component::Normal(_))) => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.iter().collect::<PathBuf>().to_string_lossy().replace('\\', "/")
}

/// The front matter with the paths it names written from the project's folder, since it moves there from `from`. A
/// theme's bare name is no path.
fn rebased(front_matter: &str, from: &Path, manifest: &Path) -> Result<String, String> {
    if from.as_os_str().is_empty() || front_matter.is_empty() {
        return Ok(front_matter.to_owned());
    }
    let settings = details::parse(front_matter, manifest)?;
    let theme = settings.theme.filter(|theme| theme.contains('/'));
    let mut text = front_matter.to_owned();
    for (key, value) in [
        ("bibliography", settings.bibliography),
        ("cover", settings.cover),
        ("theme", theme),
    ] {
        if let Some(value) = value {
            let edit = yaml::set(&text, key, &yaml::scalar(&from_project(&value, from)), &[])?;
            text = details::applied(&text, &edit);
        }
    }
    Ok(text)
}

/// The manifest for `chapters`, from the first file's front matter as it was written, plus a title if it has none.
fn manifest_text(front_matter: &str, chapters: &[String], dir: &Path) -> Result<String, String> {
    let with_chapters = {
        let edit = yaml::set_list(front_matter, CHAPTERS, chapters, details::ORDER)?;
        details::applied(front_matter, &edit)
    };
    let found = yaml::entries(&with_chapters, 0..with_chapters.len(), 0)?;
    if found.iter().any(|entry| entry.key == TITLE) {
        Ok(with_chapters)
    } else {
        templates::titled(&with_chapters, dir)
    }
}

/// Writes the manifest, then takes the front matter out of the first file. The manifest goes first and never overwrites
/// one, so at any point the front matter stands in at least one file.
pub fn make_project(dir: &Path, offer: &Offer) -> Result<(), String> {
    let manifest_path = dir.join(MANIFEST);
    let first_path = offer.moves_front_matter_of.as_ref().map(|first| dir.join(first));
    let first_text = match &first_path {
        Some(path) => Some(
            fs::read_to_string(path)
                .map_err(|error| crate::t!(Key::CannotRead, path = path.display(), error = error))?,
        ),
        None => None,
    };
    let split = match &first_text {
        Some(text) => split_front_matter(text)?,
        None => None,
    };
    let front_matter = split.as_ref().map_or("", |split| split.yaml);
    let from = offer
        .moves_front_matter_of
        .as_deref()
        .and_then(|first| Path::new(first).parent());
    let front_matter = rebased(front_matter, from.unwrap_or(Path::new("")), &manifest_path)?;
    let manifest = manifest_text(&front_matter, &offer.chapters, dir)?;
    details::parse(&manifest, &manifest_path)?;
    let describe = |error: std::io::Error| crate::t!(Key::CannotCreate, path = manifest_path.display(), error = error);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => {
                crate::t!(Key::ManifestExists, path = dir.display(), manifest = MANIFEST)
            }
            _ => describe(error),
        })?;
    file.write_all(manifest.as_bytes()).map_err(describe)?;
    if let (Some(path), Some(split)) = (&first_path, &split) {
        let body = split.body.trim_start_matches(['\n', '\r']);
        atomic::write(path, body.as_bytes())
            .map_err(|error| crate::t!(Key::CannotCreate, path = path.display(), error = error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    struct Folder(std::path::PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("druck-adopt-{name}-{}", std::process::id()));
            fs::remove_dir_all(&dir).ok();
            fs::create_dir_all(dir.join(MANUSCRIPT)).expect("folder");
            Self(dir)
        }

        fn write(&self, name: &str, text: &str) {
            fs::write(self.0.join(name), text).expect("file");
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn a_manuscript_folder_becomes_a_project_and_its_front_matter_moves_whole() {
        let folder = Folder::new("manuscript");
        let front = "title: Mein Buch\nlang: de\nmeta:\n  isbn: \"978-3\"\n# a comment of mine\n";
        folder.write("manuscript/02-b.md", "# B\n\nZwei.\n");
        folder.write("manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n\nEins.\n"));
        let offer = offer(&folder.0).expect("has Markdown");
        assert_eq!(offer.chapters, ["manuscript/01-a.md", "manuscript/02-b.md"]);
        assert_eq!(offer.moves_front_matter_of.as_deref(), Some("manuscript/01-a.md"));
        make_project(&folder.0, &offer).expect("made");
        let manifest = fs::read_to_string(folder.0.join(MANIFEST)).expect("written");
        for line in front.lines() {
            assert!(
                manifest.lines().any(|kept| kept == line),
                "{line} stays as written: {manifest}"
            );
        }
        assert!(
            manifest.contains("  - manuscript/01-a.md\n  - manuscript/02-b.md\n"),
            "{manifest}"
        );
        assert_eq!(
            fs::read_to_string(folder.0.join("manuscript/01-a.md")).expect("kept"),
            "# A\n\nEins.\n"
        );
        assert_eq!(
            fs::read_to_string(folder.0.join("manuscript/02-b.md")).expect("kept"),
            "# B\n\nZwei.\n"
        );
        let session = Session::open(&folder.0).expect("opens as a project");
        assert_eq!(session.book().len(), 2);
        assert_eq!(session.details().title, "Mein Buch");
    }

    #[test]
    fn paths_in_the_front_matter_still_name_their_files_from_the_project() {
        let folder = Folder::new("paths");
        let front = "bibliography: ../refs.bib\ncover: images/c.png\ntheme: memoir\n";
        folder.write("manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n"));
        make_project(&folder.0, &offer(&folder.0).expect("has Markdown")).expect("made");
        let manifest = fs::read_to_string(folder.0.join(MANIFEST)).expect("written");
        assert!(manifest.contains("bibliography: refs.bib\n"), "{manifest}");
        assert!(manifest.contains("cover: manuscript/images/c.png\n"), "{manifest}");
        assert!(manifest.contains("theme: memoir\n"), "{manifest}");
    }

    #[test]
    fn markdown_in_the_folder_itself_stays_where_it_is() {
        let folder = Folder::new("root");
        folder.write("b.md", "# B\n");
        folder.write("a.md", "# A\n");
        folder.write("README.md", "# Not a chapter\n");
        let offer = offer(&folder.0).expect("has Markdown");
        assert_eq!(offer.chapters, ["a.md", "b.md"]);
        make_project(&folder.0, &offer).expect("made");
        assert_eq!(fs::read_to_string(folder.0.join("a.md")).expect("kept"), "# A\n");
        assert!(!folder.0.join(MANUSCRIPT).join("a.md").exists());
        assert_eq!(Session::open(&folder.0).expect("opens").book().len(), 2);
    }

    #[test]
    fn a_front_matter_the_manifest_cannot_take_writes_nothing() {
        let folder = Folder::new("broken");
        let text = "---\nlang: [not, a, language\n---\n\n# A\n";
        folder.write("manuscript/01-a.md", text);
        let found = Offer {
            chapters: vec!["manuscript/01-a.md".to_owned()],
            moves_front_matter_of: Some("manuscript/01-a.md".to_owned()),
        };
        assert!(make_project(&folder.0, &found).is_err());
        assert!(!folder.0.join(MANIFEST).exists());
        assert_eq!(
            fs::read_to_string(folder.0.join("manuscript/01-a.md")).expect("kept"),
            text
        );
    }

    #[test]
    fn a_manifest_that_appeared_is_never_overwritten() {
        let folder = Folder::new("exists");
        folder.write("manuscript/01-a.md", "---\ntitle: T\n---\n\n# A\n");
        let found = offer(&folder.0).expect("has Markdown");
        folder.write(MANIFEST, "title: Theirs\n");
        assert!(make_project(&folder.0, &found).is_err());
        assert_eq!(
            fs::read_to_string(folder.0.join(MANIFEST)).expect("kept"),
            "title: Theirs\n"
        );
        assert!(
            fs::read_to_string(folder.0.join("manuscript/01-a.md"))
                .expect("kept")
                .starts_with("---")
        );
    }

    #[test]
    fn a_folder_with_no_markdown_or_a_manifest_has_no_offer() {
        let empty = Folder::new("empty");
        assert_eq!(offer(&empty.0), None);
        let project = Folder::new("project");
        project.write("a.md", "# A\n");
        project.write(MANIFEST, "chapters: [a.md]\n");
        assert_eq!(offer(&project.0), None);
    }
}
