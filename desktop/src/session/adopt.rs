//! Makes a folder of Markdown a project: writes `druck.yaml` beside the author's files, which stay where they are. The
//! first chapter's front matter moves into it, since a project's chapters have none. Nothing else changes, and nothing is
//! written unless the whole of it can be: a front matter the manifest could not read stops it before the first write, and
//! the manifest is written whole or not at all, so a failure leaves nothing that blocks trying again.

use std::fs;
use std::path::{Component, Path, PathBuf};

use druck::editor::resolved_config::split_front_matter;
use druck::input::MANIFEST;

use super::strings::Key;
use super::{MANUSCRIPT, details, templates, yaml};
use crate::persist::atomic::{self, Outcome};

const CHAPTERS: &str = "chapters";
const TITLE: &str = "title";
/// Markdown that sits in a project's root for other tools, not for the book.
const NOT_CHAPTERS: [&str; 3] = ["README.md", "CLAUDE.md", "AGENTS.md"];
/// How many folders above the project to look for a file the front matter names.
const SEARCH_UP: usize = 3;

/// What making a folder a project would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The chapters, relative to the folder, in name order.
    pub chapters: Vec<String>,
    /// The file whose front matter would move.
    pub moves_front_matter_of: Option<String>,
    /// Paths that front matter names and that could not be found, as they would be written.
    pub missing: Vec<String>,
}

impl Offer {
    /// One line saying what it does, so nothing happens that the author did not read.
    pub fn hint(&self) -> String {
        let count = self.chapters.len();
        let does = match &self.moves_front_matter_of {
            Some(file) => crate::t!(
                Key::MakeProjectHintMoves,
                manifest = MANIFEST,
                chapters = chapters(count),
                file = file
            ),
            None => crate::t!(Key::MakeProjectHint, manifest = MANIFEST, chapters = chapters(count)),
        };
        if self.missing.is_empty() {
            does
        } else {
            let missing = crate::t!(Key::MakeProjectMissing, paths = self.missing.join(", "));
            format!("{does} {missing}")
        }
    }

    /// One line saying what was written, for after it ran.
    pub fn done(&self) -> String {
        let count = self.chapters.len();
        match &self.moves_front_matter_of {
            Some(file) => crate::t!(
                Key::MadeProjectMoved,
                manifest = MANIFEST,
                chapters = chapters(count),
                file = file
            ),
            None => crate::t!(Key::MadeProject, manifest = MANIFEST, chapters = chapters(count)),
        }
    }
}

/// "1 chapter" or "3 chapters".
fn chapters(count: usize) -> String {
    match count {
        1 => crate::t!(Key::OneChapter).to_owned(),
        count => crate::t!(Key::Chapters, count = count),
    }
}

/// The label of the offer.
pub fn label() -> &'static str {
    crate::t!(Key::MakeProject)
}

/// Why a folder with an offer did not open, by the folder's name.
pub fn reason(dir: &Path) -> String {
    let folder = dir
        .file_name()
        .map_or_else(|| dir.display().to_string(), |name| name.to_string_lossy().into_owned());
    crate::t!(Key::NoManifestInFolder, folder = folder, manifest = MANIFEST)
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
    let split = split_front_matter(&text).ok()?;
    let missing = (split.as_ref())
        .and_then(|split| named_paths(split.yaml, parent_of(&first), dir).ok())
        .into_iter()
        .flatten()
        .filter(|named| !named.found)
        .map(|named| named.path)
        .collect();
    Some(Offer {
        chapters,
        moves_front_matter_of: split.is_some().then_some(first),
        missing,
    })
}

fn parent_of(file: &str) -> &Path {
    Path::new(file).parent().unwrap_or(Path::new(""))
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

/// A path the front matter names.
struct Named {
    key: &'static str,
    /// As it goes in the manifest, from the project's folder.
    path: String,
    found: bool,
}

/// Where a path written in a file in `from` is, from the project's folder `dir`: from that file's folder, then from the
/// project's, since a build script may have read it from there, then by name in the folders above the project. With none
/// of these it stays as written from the file's folder.
fn located(value: &str, from: &Path, dir: &Path) -> Named {
    let written = from_project(value, from);
    let name = Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let above = (1..=SEARCH_UP).filter_map(|levels| Some(format!("{}{}", "../".repeat(levels), name.as_deref()?)));
    let found = [written.clone(), from_project(value, Path::new(""))]
        .into_iter()
        .chain(above)
        .find(|candidate| dir.join(candidate).is_file());
    Named {
        key: "",
        found: found.is_some(),
        path: found.unwrap_or(written),
    }
}

/// The paths a front matter in a file in `from` names. A theme's bare name is no path.
fn named_paths(front_matter: &str, from: &Path, dir: &Path) -> Result<Vec<Named>, String> {
    if front_matter.is_empty() {
        return Ok(Vec::new());
    }
    let settings = details::parse(front_matter, &dir.join(MANIFEST))?;
    let theme = settings.theme.filter(|theme| theme.contains('/'));
    let named = [
        ("bibliography", settings.bibliography),
        ("cover", settings.cover),
        ("theme", theme),
    ];
    Ok(named
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| Named {
                key,
                ..located(&value, from, dir)
            })
        })
        .collect())
}

/// The front matter with the paths it names written from the project's folder, since it moves there from `from`.
fn rebased(front_matter: &str, from: &Path, dir: &Path) -> Result<String, String> {
    let mut text = front_matter.to_owned();
    for named in named_paths(front_matter, from, dir)? {
        let edit = yaml::set(&text, named.key, &yaml::scalar(&named.path), &[])?;
        text = details::applied(&text, &edit);
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
    let from = offer.moves_front_matter_of.as_deref().map_or(Path::new(""), parent_of);
    let front_matter = rebased(front_matter, from, dir)?;
    let manifest = manifest_text(&front_matter, &offer.chapters, dir)?;
    details::parse(&manifest, &manifest_path)?;
    let written = atomic::write_if(&manifest_path, manifest.as_bytes(), None)
        .map_err(|error| crate::t!(Key::CannotCreate, path = manifest_path.display(), error = error))?;
    if written != Outcome::Written {
        return Err(crate::t!(
            Key::ManifestExists,
            path = dir.display(),
            manifest = MANIFEST
        ));
    }
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
        fs::create_dir_all(folder.0.join("manuscript/images")).expect("folder");
        folder.write("manuscript/images/c.png", "");
        folder.write("refs.bib", "");
        folder.write("manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n"));
        let found = offer(&folder.0).expect("has Markdown");
        assert!(found.missing.is_empty());
        make_project(&folder.0, &found).expect("made");
        let manifest = fs::read_to_string(folder.0.join(MANIFEST)).expect("written");
        assert!(manifest.contains("bibliography: refs.bib\n"), "{manifest}");
        assert!(manifest.contains("cover: manuscript/images/c.png\n"), "{manifest}");
        assert!(manifest.contains("theme: memoir\n"), "{manifest}");
    }

    /// The real book: `../literatur.bib` was read from the project's folder by its build script, and stands beside it.
    #[test]
    fn a_path_written_from_the_project_still_finds_its_file() {
        let folder = Folder::new("beside");
        let project = folder.0.join("book");
        fs::create_dir_all(project.join(MANUSCRIPT)).expect("folder");
        folder.write("literatur.bib", "");
        let front = "title: T\nbibliography: ../literatur.bib\n";
        folder.write("book/manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n"));
        let found = offer(&project).expect("has Markdown");
        assert!(found.missing.is_empty());
        make_project(&project, &found).expect("made");
        let manifest = fs::read_to_string(project.join(MANIFEST)).expect("written");
        assert!(manifest.contains("bibliography: \"../literatur.bib\"\n"), "{manifest}");
    }

    #[test]
    fn a_file_named_from_elsewhere_is_found_in_the_folders_above() {
        let folder = Folder::new("above");
        let project = folder.0.join("series/book");
        fs::create_dir_all(project.join(MANUSCRIPT)).expect("folder");
        folder.write("refs.bib", "");
        let front = "bibliography: sources/refs.bib\n";
        folder.write("series/book/manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n"));
        make_project(&project, &offer(&project).expect("has Markdown")).expect("made");
        let manifest = fs::read_to_string(project.join(MANIFEST)).expect("written");
        assert!(manifest.contains("bibliography: \"../../refs.bib\"\n"), "{manifest}");
    }

    #[test]
    fn the_offer_says_what_it_cannot_find_before_anything_is_written() {
        let folder = Folder::new("missing");
        let front = "bibliography: ../gone.bib\n";
        folder.write("manuscript/01-a.md", &format!("---\n{front}---\n\n# A\n"));
        let found = offer(&folder.0).expect("has Markdown");
        assert_eq!(found.missing, ["gone.bib"]);
        assert!(found.hint().contains("gone.bib"), "{}", found.hint());
        assert!(!folder.0.join(MANIFEST).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_nothing_that_blocks_a_retry() {
        use std::os::unix::fs::PermissionsExt as _;
        let folder = Folder::new("retry");
        folder.write("manuscript/01-a.md", "---\ntitle: T\n---\n\n# A\n");
        let found = offer(&folder.0).expect("has Markdown");
        let set_mode = |mode| fs::set_permissions(&folder.0, fs::Permissions::from_mode(mode)).expect("mode");
        set_mode(0o555);
        let failed = make_project(&folder.0, &found);
        set_mode(0o755);
        assert!(failed.is_err());
        assert!(!folder.0.join(MANIFEST).exists());
        make_project(&folder.0, &found).expect("the retry works");
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
            missing: Vec::new(),
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
