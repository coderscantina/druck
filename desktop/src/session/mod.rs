//! A document session: the document in one window, from a project folder or a single file. Holds no gpui types.
//!
//! It owns every chapter's source buffer, with its undo history, caret, and scroll place, and the settings. It makes
//! the joined text the preview requests and the map from chapter offsets to joined offsets. The engine still reads
//! `druck.yaml` and checks the chapter list on each layout, through [`joined::LiveResolver`], so there is one set of
//! manifest rules. The session's own edits to the manifest are text splices, see [`yaml`].
//!
//! A project's chapter is a file. A single file's chapter is a section: it splits at level 1 headings in view only, and
//! its sections' texts joined are the file. Notes, research, and files not in the book open in the same editor and
//! are never built.
//!
//! The editor view holds the session, since every key needs the active buffer. Others read it through
//! `EditorView::session` and change it through `EditorView::update_session`, which keeps the view in step.

pub mod details;
pub mod disk;
mod joined;
mod outline;
pub mod templates;
#[cfg(test)]
mod tests;
pub(crate) mod yaml;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use druck::editor::engine;
use druck::editor::resolved_config::FrontMatter;
use druck::input::{MANIFEST, ThemeDirs};
use druck::resource::{FileResolver, Origin, Resolver, normalize};

pub use self::details::{Details, Field, LANGUAGES, MetaRow};
use self::disk::Synced;
pub use self::joined::{ChapterMap, Joined, LiveResolver};
pub use self::outline::Kind as RowKind;
use self::outline::{Outline, outline, sections, slug, title_from_file};
pub use self::templates::{STARTERS, Starter, TEMPLATES, Template};
use crate::persist::atomic::{self, Outcome};
use crate::source::buffer::EditError;
use crate::source::{BlockKind, Blocks, Buffer, Edit, EditKind, Selection};

const MANUSCRIPT: &str = "manuscript";
const NOTES: &str = "notes";
const RESEARCH: &str = "research";
/// A key in the settings that lists chapters.
const CHAPTERS: &str = "chapters";
/// The title of a single file's text before its first level 1 heading.
const FRONT_MATTER: &str = "Front matter";

/// A chapter, for as long as the session lives. Moving it in the book does not change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChapterId(u32);

impl ChapterId {
    /// A number that stays this chapter's for the session, for element ids.
    pub fn number(self) -> usize {
        self.0 as usize
    }
}

/// Which list a chapter shows under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// In the book, and built.
    Book,
    /// A Markdown file in `manuscript/` the chapter list does not name.
    NotInBook,
    Notes,
    Research,
    /// `druck.yaml` itself, shown while the book cannot be read from it.
    Manifest,
}

/// Where the galley was in a chapter, to come back to.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChapterView {
    pub selection: Selection,
    /// The block at the top of the viewport and how far into it, once the chapter has been shown.
    pub scroll: Option<(usize, f32)>,
}

/// What reading a chapter's file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    /// Not read yet: notes and research are read when opened.
    Later,
    Done,
    /// Listed but not there. Saving creates it.
    Missing,
    /// Cannot be edited, such as text that is not UTF-8. Saving never touches it.
    Failed(String),
}

pub struct Chapter {
    path: PathBuf,
    /// The path as the manifest lists it, or relative to the project folder.
    listed: String,
    group: Group,
    buffer: Buffer,
    read: Read,
    /// The buffer's revision that is on disk.
    saved: u64,
    pub view: ChapterView,
    outline: Outline,
}

impl Chapter {
    fn new(path: PathBuf, listed: String, group: Group, buffer: Buffer, read: Read) -> Self {
        let outline = outline(buffer.text());
        let saved = buffer.revision();
        Self {
            path,
            listed,
            group,
            buffer,
            read,
            saved,
            view: ChapterView::default(),
            outline,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn group(&self) -> Group {
        self.group
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    /// The first heading's words, without their Markdown.
    pub fn heading(&self) -> Option<&str> {
        self.outline.title.as_deref()
    }

    /// Why this chapter has no text to show, if so.
    pub fn problem(&self) -> Option<String> {
        match &self.read {
            Read::Missing => Some(format!("{} is not there yet. Saving creates it.", self.listed)),
            Read::Failed(problem) => Some(problem.clone()),
            Read::Later | Read::Done => None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.revision() != self.saved
    }

    /// Whether the buffer holds the file's text. A note or research file is read when first opened.
    pub fn is_read(&self) -> bool {
        self.read != Read::Later
    }
}

/// A chapter's line in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: ChapterId,
    pub title: String,
    pub kind: RowKind,
    /// The chapter's number in the book, for a numbered chapter.
    pub number: Option<u32>,
    /// The file is listed but missing, or cannot be read.
    pub broken: bool,
}

/// The sidebar's lists, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contents {
    pub book: Vec<Row>,
    pub not_in_book: Vec<Row>,
    pub notes: Vec<Row>,
    pub research: Vec<Row>,
    pub manifest: Vec<Row>,
}

/// What the document is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Single { path: PathBuf },
    Project { dir: PathBuf },
}

pub struct Session {
    kind: Kind,
    chapters: BTreeMap<ChapterId, Chapter>,
    /// The chapters of the book in reading order.
    book: Vec<ChapterId>,
    active: ChapterId,
    next_id: u32,
    /// Added to the sum of the book's buffer revisions, so a revision never repeats.
    revision_base: i64,
    /// The settings as last read, or why they cannot be.
    settings: Result<FrontMatter, String>,
    details: Details,
    /// A single file whose split changed has text on no buffer's undo stack that is not on disk.
    resplit_unsaved: bool,
    resolver: FileResolver,
    /// What each file last agreed with, the base of a merge. See [`disk`].
    synced: HashMap<PathBuf, Synced>,
    /// Files whose text must not reach the disk yet: a person is deciding about them.
    held: HashSet<PathBuf>,
}

impl Session {
    /// Opens a project folder, its `druck.yaml`, or a single Markdown file. A document with problems still opens, so
    /// they can be fixed in the editor; only a path with nothing to show is an error.
    pub fn open(path: &Path) -> Result<Self, String> {
        let working_dir =
            std::env::current_dir().map_err(|error| format!("cannot find the working directory: {error}"))?;
        let path = normalize(&working_dir.join(path));
        if !path.exists() {
            return Err(format!("cannot open {}: no such file or folder", path.display()));
        }
        if path.is_dir() {
            return Self::open_project(&path);
        }
        if path.file_name().is_some_and(|name| name == MANIFEST)
            && let Some(dir) = path.parent()
        {
            return Self::open_project(dir);
        }
        Self::open_single(&path)
    }

    fn empty(kind: Kind) -> Self {
        Self {
            kind,
            chapters: BTreeMap::new(),
            book: Vec::new(),
            active: ChapterId(0),
            next_id: 0,
            revision_base: 0,
            settings: Ok(FrontMatter::default()),
            details: Details::default(),
            resplit_unsaved: false,
            resolver: FileResolver::new(),
            synced: HashMap::new(),
            held: HashSet::new(),
        }
    }

    fn add(&mut self, chapter: Chapter) -> ChapterId {
        let id = ChapterId(self.next_id);
        self.next_id += 1;
        self.synced
            .entry(chapter.path.clone())
            .or_insert_with(|| Synced::new(chapter.text(), matches!(chapter.read, Read::Done | Read::Later)));
        self.chapters.insert(id, chapter);
        id
    }

    fn open_single(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let text = String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text.", path.display()))?;
        let mut session = Self::empty(Kind::Single { path: path.to_owned() });
        session.synced.insert(path.to_owned(), Synced::new(&text, true));
        session.split_into_sections(&text);
        session.active = session.book[0];
        session.refresh_details();
        Ok(session)
    }

    /// Makes one chapter per section of `text`.
    fn split_into_sections(&mut self, text: &str) {
        let Kind::Single { path } = &self.kind else {
            return;
        };
        let path = path.clone();
        for range in sections(text) {
            let chapter = Chapter::new(
                path.clone(),
                String::new(),
                Group::Book,
                Buffer::new(&text[range]),
                Read::Done,
            );
            let id = self.add(chapter);
            self.book.push(id);
        }
    }

    fn open_project(dir: &Path) -> Result<Self, String> {
        let manifest = dir.join(MANIFEST);
        if !manifest.exists() {
            return Err(format!(
                "{} has no {MANIFEST}. A project folder needs one, or open a Markdown file.",
                dir.display()
            ));
        }
        let mut session = Self::empty(Kind::Project { dir: dir.to_owned() });
        session.read_manifest();
        Ok(session)
    }

    /// Reads the manifest and lines the chapters up with it. Chapters already open keep their buffers. Call it after
    /// `druck.yaml` changed outside the session's own edits.
    pub fn read_manifest(&mut self) {
        let Kind::Project { dir } = &self.kind else {
            return;
        };
        let dir = dir.clone();
        let manifest = dir.join(MANIFEST);
        let before = self.revision();
        let text = fs::read_to_string(&manifest);
        match &text {
            Ok(text) => self.set_synced(&manifest, text.as_str().into(), true),
            Err(_) => {
                if let Some(synced) = self.file_base(&manifest) {
                    self.set_synced(&manifest, synced.base, false);
                }
            }
        }
        self.settings = text
            .map_err(|error| format!("cannot read {MANIFEST}: {error}"))
            .and_then(|text| details::parse(&text, &manifest));
        let listed: Vec<String> = match &self.settings {
            Ok(settings) => (settings.chapters.iter().flatten())
                .map(|chapter| chapter.value.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
        for id in std::mem::take(&mut self.book) {
            if let Some(chapter) = self.chapters.get_mut(&id) {
                chapter.group = Group::NotInBook;
            }
        }
        for name in listed {
            let path = normalize(&dir.join(&name));
            if !path.starts_with(&dir) || self.book.iter().any(|id| self.chapters[id].path == path) {
                continue; // The engine reports these against the manifest's line.
            }
            let id = self.find_or_read(&path, name, Group::Book);
            self.set_read(id);
            self.known_mut(id).group = Group::Book;
            self.book.push(id);
        }
        self.find_files(MANUSCRIPT, Group::NotInBook);
        self.find_files(NOTES, Group::Notes);
        self.find_files(RESEARCH, Group::Research);
        self.show_manifest_if_needed(&manifest);
        self.set_revision_above(before);
        self.refresh_details();
    }

    /// The chapter at `path`, or a new one with its file read.
    fn find_or_read(&mut self, path: &Path, listed: String, group: Group) -> ChapterId {
        if let Some((id, _)) = self.chapters.iter().find(|(_, chapter)| chapter.path == path) {
            return *id;
        }
        let (buffer, read) = self.read_file(path, &listed);
        self.add(Chapter::new(path.to_owned(), listed, group, buffer, read))
    }

    fn read_file(&self, path: &Path, listed: &str) -> (Buffer, Read) {
        let Kind::Project { dir } = &self.kind else {
            return (Buffer::new(""), Read::Later);
        };
        match self.resolver.read(&Origin::Document(dir.clone()), listed) {
            Ok(bytes) => match Buffer::from_bytes(bytes.to_vec()) {
                Ok(buffer) => (buffer, Read::Done),
                Err(_) => (
                    Buffer::new(""),
                    Read::Failed(format!("{} is not UTF-8 text.", path.display())),
                ),
            },
            Err(druck::resource::ResolveError::NotFound(_)) => (Buffer::new(""), Read::Missing),
            Err(error) => (
                Buffer::new(""),
                Read::Failed(format!("cannot read {}: {error}", path.display())),
            ),
        }
    }

    /// Adds text files found below `folder` that the session does not have yet. Files of the manuscript are read now, for
    /// their titles. Notes and research are read when opened.
    fn find_files(&mut self, folder: &str, group: Group) {
        let Kind::Project { dir } = &self.kind else {
            return;
        };
        let dir = dir.clone();
        let mut files = self
            .resolver
            .list(&Origin::Document(dir.clone()), folder)
            .unwrap_or_default();
        files.retain(|file| {
            let extension = Path::new(file).extension().and_then(|extension| extension.to_str());
            let markdown = extension.is_some_and(|extension| {
                extension.eq_ignore_ascii_case("md")
                    || (group != Group::NotInBook && extension.eq_ignore_ascii_case("txt"))
            });
            markdown
                && !self
                    .chapters
                    .values()
                    .any(|chapter| chapter.path == normalize(&dir.join(file)))
        });
        files.sort();
        for file in files {
            let path = normalize(&dir.join(&file));
            let (buffer, read) = if group == Group::NotInBook {
                self.read_file(&path, &file)
            } else {
                (Buffer::new(""), Read::Later)
            };
            self.add(Chapter::new(path, file, group, buffer, read));
        }
    }

    /// While the book cannot be read from the manifest, the manifest is what opens, so it can be fixed here.
    fn show_manifest_if_needed(&mut self, manifest: &Path) {
        let existing = self
            .chapters
            .iter()
            .find(|(_, chapter)| chapter.group == Group::Manifest);
        let existing = existing.map(|(id, _)| *id);
        if !self.book.is_empty() {
            if self.chapters.contains_key(&self.active) && Some(self.active) != existing {
                if let Some(id) = existing {
                    self.chapters.remove(&id);
                }
                return;
            }
            self.active = self.book[0];
            if let Some(id) = existing {
                self.chapters.remove(&id);
            }
            return;
        }
        let id = existing.unwrap_or_else(|| {
            let (buffer, read) = self.read_file(manifest, MANIFEST);
            self.add(Chapter::new(
                manifest.to_owned(),
                MANIFEST.to_owned(),
                Group::Manifest,
                buffer,
                read,
            ))
        });
        self.active = id;
    }

    // Reading

    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// The folder relative paths in the document start from.
    pub fn base_dir(&self) -> PathBuf {
        match &self.kind {
            Kind::Project { dir } => dir.clone(),
            Kind::Single { path } => path.parent().map(Path::to_owned).unwrap_or_default(),
        }
    }

    /// The folder or file the document was opened from, for the window title.
    pub fn location(&self) -> &Path {
        match &self.kind {
            Kind::Project { dir } => dir,
            Kind::Single { path } => path,
        }
    }

    pub fn book(&self) -> &[ChapterId] {
        &self.book
    }

    pub fn active(&self) -> ChapterId {
        self.active
    }

    pub fn chapter(&self, id: ChapterId) -> Option<&Chapter> {
        self.chapters.get(&id)
    }

    fn known(&self, id: ChapterId) -> &Chapter {
        self.chapters.get(&id).expect("a chapter id comes from this session")
    }

    fn known_mut(&mut self, id: ChapterId) -> &mut Chapter {
        self.chapters
            .get_mut(&id)
            .expect("a chapter id comes from this session")
    }

    pub fn buffer(&self, id: ChapterId) -> &Buffer {
        &self.known(id).buffer
    }

    pub fn active_buffer(&self) -> &Buffer {
        &self.known(self.active).buffer
    }

    pub fn active_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.known_mut(self.active).buffer
    }

    pub fn active_chapter(&self) -> &Chapter {
        self.known(self.active)
    }

    pub fn active_chapter_mut(&mut self) -> &mut Chapter {
        self.known_mut(self.active)
    }

    /// The revision of what the pages are set from. It changes with any edit to a chapter in the book and any change
    /// to the book's order or settings, and never with an edit to a note.
    pub fn revision(&self) -> u64 {
        let edits: u64 = self.book.iter().map(|id| self.known(*id).buffer.revision()).sum();
        (self.revision_base + edits as i64) as u64
    }

    fn bump_revision(&mut self) {
        self.revision_base += 1;
    }

    /// After buffers came or went: makes the revision the next one after `before`.
    fn set_revision_above(&mut self, before: u64) {
        let edits: u64 = self.book.iter().map(|id| self.known(*id).buffer.revision()).sum();
        self.revision_base = before as i64 + 1 - edits as i64;
    }

    pub fn details(&self) -> &Details {
        &self.details
    }

    /// Why the document's settings cannot be read.
    pub fn problem(&self) -> Option<&str> {
        self.settings.as_ref().err().map(String::as_str)
    }

    /// Reads the settings of a single file again, after its first section changed. A project's come from the file.
    pub(crate) fn refresh_details(&mut self) {
        if let Kind::Single { path } = &self.kind {
            let path = path.clone();
            let text = self.buffer(self.book[0]).text();
            let yaml = druck::editor::resolved_config::split_front_matter(text)
                .map(|split| split.map_or(String::new(), |split| split.yaml.to_owned()));
            self.settings = yaml.and_then(|yaml| details::parse(&yaml, &path));
        }
        self.details = match &self.settings {
            Ok(settings) => details::read(settings),
            Err(problem) => Details {
                problem: Some(problem.clone()),
                ..Details::default()
            },
        };
        if self.details.title.is_empty() {
            self.details.title = self
                .location()
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
    }

    /// The sidebar's lists.
    pub fn contents(&self) -> Contents {
        let mut number = 0;
        let mut contents = Contents {
            book: (self.book.iter())
                .map(|id| {
                    let chapter = self.known(*id);
                    let numbered = chapter.outline.kind == RowKind::Numbered;
                    number += u32::from(numbered);
                    let mut row = self.row(*id, chapter);
                    row.number = numbered.then_some(number);
                    row
                })
                .collect(),
            ..Contents::default()
        };
        for (id, chapter) in &self.chapters {
            let row = self.row(*id, chapter);
            match chapter.group {
                Group::Book => {}
                Group::NotInBook => contents.not_in_book.push(row),
                Group::Notes => contents.notes.push(row),
                Group::Research => contents.research.push(row),
                Group::Manifest => contents.manifest.push(row),
            }
        }
        contents
    }

    /// The title a chapter's row shows.
    pub fn title_of(&self, id: ChapterId) -> String {
        self.chapters
            .get(&id)
            .map(|chapter| self.row(id, chapter).title)
            .unwrap_or_default()
    }

    fn row(&self, id: ChapterId, chapter: &Chapter) -> Row {
        let stem = || {
            chapter
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let title = match chapter.group {
            Group::Notes | Group::Research => self.note_title(chapter),
            Group::Manifest => MANIFEST.to_owned(),
            Group::Book | Group::NotInBook => match (&chapter.outline.title, &self.kind) {
                (Some(title), _) => title.clone(),
                (None, Kind::Single { .. }) => FRONT_MATTER.to_owned(),
                (None, Kind::Project { .. }) => title_from_file(&stem()),
            },
        };
        Row {
            id,
            title,
            kind: chapter.outline.kind,
            number: None,
            broken: matches!(chapter.read, Read::Missing | Read::Failed(_)),
        }
    }

    /// A note's name: its path below its folder, without the extension.
    fn note_title(&self, chapter: &Chapter) -> String {
        let relative = Path::new(&chapter.listed);
        let below: PathBuf = relative.components().skip(1).collect();
        below.with_extension("").to_string_lossy().into_owned()
    }

    /// The theme file the document names, if any.
    pub fn theme_file(&self) -> Option<PathBuf> {
        let dirs = self.theme_dirs();
        match &self.kind {
            Kind::Single { path } => {
                engine::document_theme(&self.resolver, path, self.buffer(self.book[0]).text(), &dirs)
            }
            Kind::Project { dir } => {
                let theme = self.settings.as_ref().ok()?.theme.as_ref()?;
                Some(engine::theme_file(&self.resolver, Path::new(theme), dir, &dirs))
            }
        }
    }

    /// The text of the bibliography the document names, if it has one.
    pub fn bibliography_text(&self) -> Option<String> {
        let file = self.settings.as_ref().ok()?.bibliography.as_ref()?;
        fs::read_to_string(self.base_dir().join(file)).ok()
    }

    /// An image in the document's usual folders, written as the engine reads it, from the document's folder. For the
    /// slash menu.
    pub fn first_image(&self) -> Option<String> {
        const IMAGES: [&str; 4] = ["png", "jpg", "jpeg", "svg"];
        let base = self.base_dir();
        let chapter_dir = self.active_chapter().path().parent()?.to_owned();
        for folder in [base.join("assets"), base.join("images"), chapter_dir] {
            let mut files: Vec<PathBuf> = fs::read_dir(&folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| IMAGES.contains(&extension))
                })
                .collect();
            files.sort();
            if let Some(file) = files.first() {
                return Some(relative_path(&base, file));
            }
        }
        None
    }

    /// Where a bare theme name is looked up for this document.
    pub fn theme_dirs(&self) -> ThemeDirs {
        ThemeDirs {
            project: match &self.kind {
                Kind::Project { dir } => Some(dir.clone()),
                Kind::Single { .. } => None,
            },
            shared: FileResolver::shared_themes(),
        }
    }

    // Offsets

    /// The whole document as one text, with what the preview needs to read it.
    pub fn joined(&self) -> Joined {
        let project = matches!(self.kind, Kind::Project { .. });
        let size: usize = self.book.iter().map(|id| self.known(*id).buffer.len() + 2).sum();
        let mut text = String::with_capacity(size);
        let mut map = ChapterMap::default();
        for id in &self.book {
            let chapter = self.known(*id);
            if chapter.read != Read::Done {
                continue; // The engine reports a missing or unreadable file.
            }
            let start = text.len();
            text.push_str(chapter.buffer.text());
            map.push(*id, chapter.path.clone(), start..text.len());
            if project {
                if !text.ends_with('\n') && text.len() > start {
                    text.push('\n');
                }
                text.push('\n');
            }
        }
        Joined {
            text,
            map,
            revision: self.revision(),
        }
    }

    /// The chapter and its offset for a place in a file, as a source map's position says it.
    pub fn locate(&self, file: &Path, offset: usize) -> Option<(ChapterId, usize)> {
        match &self.kind {
            Kind::Single { path } => {
                if path != file {
                    return None;
                }
                let mut start = 0;
                for id in &self.book {
                    let length = self.known(*id).buffer.len();
                    if offset < start + length || Some(id) == self.book.last() {
                        return Some((*id, (offset - start).min(length)));
                    }
                    start += length;
                }
                None
            }
            Kind::Project { .. } => {
                let id = self.book.iter().find(|id| self.known(**id).path == file)?;
                Some((*id, offset.min(self.known(*id).buffer.len())))
            }
        }
    }

    /// The file and its offset for a place in a chapter of the book, as a source map names it. A note has none.
    pub fn file_position(&self, id: ChapterId, offset: usize) -> Option<(PathBuf, usize)> {
        let chapter = self.chapters.get(&id).filter(|chapter| chapter.group == Group::Book)?;
        let before: usize = match self.kind {
            Kind::Single { .. } => {
                let at = self.book.iter().position(|other| *other == id)?;
                self.book[..at]
                    .iter()
                    .map(|other| self.known(*other).buffer.len())
                    .sum()
            }
            Kind::Project { .. } => 0,
        };
        Some((chapter.path.clone(), before + offset))
    }

    /// Where on the pages a caret in a chapter shows, as a file and offset. A comment, front matter, or blank lines set
    /// nothing, so a caret there shows at the text after them, not at the end of the chapter before.
    pub fn page_position(&self, id: ChapterId, offset: usize) -> Option<(PathBuf, usize)> {
        let blocks = Blocks::parse(self.chapters.get(&id)?.buffer.text());
        let list = blocks.as_slice();
        let target = list[blocks.index_at(offset)..].iter().find(|block| {
            !matches!(
                block.kind,
                BlockKind::Comment | BlockKind::FrontMatter | BlockKind::Blank
            )
        });
        let start = list[blocks.index_at(offset)].range.start;
        let shown = match target {
            Some(block) if block.range.start != start => block.range.start,
            _ => offset,
        };
        self.file_position(id, shown)
    }

    // Changing the text

    /// Makes `id` the active chapter, reading its file if that is still to do.
    pub fn set_active(&mut self, id: ChapterId) {
        self.load(id);
        self.active = id;
    }

    /// Reads a note's or research file if that is still to do, so its text can be edited without opening it.
    pub fn load(&mut self, id: ChapterId) {
        if self.known(id).read == Read::Later {
            let listed = self.known(id).listed.clone();
            let path = self.known(id).path.clone();
            let (buffer, read) = self.read_file(&path, &listed);
            let chapter = self.known_mut(id);
            chapter.saved = buffer.revision();
            chapter.buffer = buffer;
            chapter.read = read;
            chapter.outline = outline(chapter.buffer.text());
            let synced = Synced::new(chapter.buffer.text(), chapter.read == Read::Done);
            self.synced.insert(path.clone(), synced);
        }
    }

    /// Applies edits to a chapter as one undo step, moving its saved caret with the text. For anything outside the
    /// editor's own typing that changes a chapter's text.
    pub fn edit_chapter(&mut self, id: ChapterId, edits: Vec<Edit>, kind: EditKind) -> Result<(), EditError> {
        let chapter = self.known_mut(id);
        let before = chapter.view.selection;
        let after = Selection {
            anchor: moved(before.anchor, &edits),
            head: moved(before.head, &edits),
        };
        chapter.buffer.transact(kind, edits, before, after, Instant::now())?;
        chapter.view.selection = after;
        chapter.outline = outline(chapter.buffer.text());
        Ok(())
    }

    /// Reads a chapter's title and kind again after its text changed. Returns whether the sidebar should change.
    pub fn refresh_outline(&mut self, id: ChapterId) -> bool {
        let chapter = self.known_mut(id);
        let new = outline(chapter.buffer.text());
        if new == chapter.outline {
            return false;
        }
        chapter.outline = new;
        true
    }

    /// Whether any text in the session differs from its file.
    pub fn is_dirty(&self) -> bool {
        self.resplit_unsaved || self.chapters.values().any(Chapter::is_dirty)
    }

    /// Writes what changed, now. Each file that changed is written whole, and no other file is touched. A file that
    /// changed on disk since it was read is not overwritten: this is an error, and the merge deals with it. Returns how
    /// many files were written.
    pub fn save(&mut self) -> Result<usize, String> {
        self.resplit();
        let mut written = 0;
        for outgoing in self.outgoing() {
            let path = outgoing.path.display();
            match outgoing.write() {
                Ok(Outcome::Written) => {}
                Ok(Outcome::Changed(_)) => return Err(format!("{path} changed on disk, so it was not saved.")),
                Err(error) => return Err(format!("cannot save {path}: {error}")),
            }
            self.written(&outgoing);
            written += 1;
        }
        Ok(written)
    }

    /// Brings a single file's sections in line with its level 1 headings, once a heading came or went. A section whose
    /// text stayed keeps its buffer. One that changed starts a new undo history, so call this when the caret leaves a
    /// chapter or on a save, not while typing. Returns whether the sections changed.
    pub fn resplit(&mut self) -> bool {
        if !matches!(self.kind, Kind::Single { .. }) {
            return false;
        }
        let joined = self.joined();
        let ranges = sections(&joined.text);
        let same = ranges.len() == self.book.len()
            && (ranges.iter().zip(&self.book)).all(|(range, id)| range.len() == self.known(*id).buffer.len());
        if same {
            return false;
        }
        let before = self.revision();
        let caret = joined
            .map
            .to_joined(self.active, self.known(self.active).view.selection.head)
            .unwrap_or(0);
        let mut old = std::mem::take(&mut self.book);
        let mut changed = false;
        for range in ranges {
            let piece = &joined.text[range];
            let kept = old.iter().position(|id| self.known(*id).buffer.text() == piece);
            let id = match kept {
                Some(at) => old.remove(at),
                None => {
                    changed = true;
                    let path = self.location().to_owned();
                    self.add(Chapter::new(
                        path,
                        String::new(),
                        Group::Book,
                        Buffer::new(piece),
                        Read::Done,
                    ))
                }
            };
            self.book.push(id);
        }
        for id in old {
            self.resplit_unsaved |= self.known(id).is_dirty();
            self.chapters.remove(&id);
        }
        self.resplit_unsaved |= changed;
        let joined_after = self.joined();
        if let Some((id, offset)) = joined_after.map.chapter_at(caret) {
            self.active = id;
            self.known_mut(id).view.selection = Selection::caret(offset);
        }
        self.set_revision_above(before);
        true
    }

    // Changing the book

    /// The project's folder, or why this document has no book to change.
    fn project_dir(&self, doing: &str) -> Result<PathBuf, String> {
        match &self.kind {
            Kind::Project { dir } => Ok(dir.clone()),
            Kind::Single { .. } => Err(format!("A single file cannot {doing}: its order is its text.")),
        }
    }

    /// Writes the current chapter list to `druck.yaml`.
    fn write_chapter_list(&mut self) -> Result<(), String> {
        let dir = self.project_dir("change its chapter list")?;
        let manifest = dir.join(MANIFEST);
        let wanted: Vec<String> = self.book.iter().map(|id| self.known(*id).listed.clone()).collect();
        self.patch_manifest(
            &manifest,
            |text| yaml::set_list(text, CHAPTERS, &wanted, &[]).map(Some),
            |settings| {
                let found = settings.chapters.iter().flatten().map(|chapter| &chapter.value);
                found.eq(wanted.iter())
            },
        )
    }

    /// Moves a chapter so it stands before the one now at `to`, or last if `to` is the length of the book.
    pub fn move_chapter(&mut self, id: ChapterId, to: usize) -> Result<(), String> {
        let Some(from) = self.book.iter().position(|other| *other == id) else {
            return Err("Only a chapter in the book can move.".to_owned());
        };
        let target = if from < to { to - 1 } else { to }.min(self.book.len() - 1);
        if target == from {
            return Ok(());
        }
        let order = self.book.clone();
        self.book.remove(from);
        self.book.insert(target, id);
        self.write_chapter_list().inspect_err(|_| self.book = order)
    }

    /// Takes a chapter out of the book. Its file stays, in the "Not in book" list.
    pub fn remove_from_book(&mut self, id: ChapterId) -> Result<(), String> {
        self.project_dir("take out a section")?;
        if self.book.len() == 1 && self.book[0] == id {
            return Err("A book needs at least one chapter.".to_owned());
        }
        let before = self.revision();
        let order = self.book.clone();
        self.book.retain(|other| *other != id);
        if let Err(error) = self.write_chapter_list() {
            self.book = order;
            return Err(error);
        }
        self.known_mut(id).group = Group::NotInBook;
        self.set_revision_above(before);
        Ok(())
    }

    /// Adds a file from "Not in book" at the end of the book.
    pub fn add_to_book(&mut self, id: ChapterId) -> Result<(), String> {
        self.project_dir("add a file")?;
        self.set_read(id);
        let before = self.revision();
        self.book.push(id);
        if let Err(error) = self.write_chapter_list() {
            self.book.pop();
            return Err(error);
        }
        self.known_mut(id).group = Group::Book;
        self.set_revision_above(before);
        Ok(())
    }

    /// Reads a chapter's file now if it is still to be read.
    fn set_read(&mut self, id: ChapterId) {
        let active = self.active;
        self.set_active(id);
        self.active = active;
    }

    /// Renames a chapter. Its title comes from its first heading, so that is what changes, as an undoable edit. A
    /// project's chapter without a heading has its title from its file, so the file is renamed instead.
    pub fn rename(&mut self, id: ChapterId, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A name cannot be empty.".to_owned());
        }
        if let Some(heading) = self.known(id).outline.heading.clone() {
            return self
                .edit_chapter(id, vec![Edit::new(heading, name)], EditKind::Discrete)
                .map_err(|error| error.to_string());
        }
        let dir = self.project_dir("rename a section without a heading")?;
        let chapter = self.known(id);
        let extension = chapter
            .path
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned());
        let extension = extension.unwrap_or_else(|| "md".to_owned());
        let folder = chapter.path.parent().map(Path::to_owned).unwrap_or_else(|| dir.clone());
        let target = folder.join(format!("{}.{extension}", slug(name)));
        if target.exists() {
            return Err(format!("{} already exists.", target.display()));
        }
        let old = chapter.path.clone();
        let old_listed = chapter.listed.clone();
        let listed = target
            .strip_prefix(&dir)
            .map_err(|_| "A chapter must stay in the project folder.".to_owned())?
            .to_string_lossy()
            .into_owned();
        fs::rename(&old, &target).map_err(|error| format!("cannot rename {}: {error}", old.display()))?;
        self.move_synced(&old, &target);
        let chapter = self.known_mut(id);
        chapter.path = target.clone();
        chapter.listed = listed;
        if self.known(id).group != Group::Book {
            return Ok(());
        }
        self.write_chapter_list().inspect_err(|_| {
            fs::rename(&target, &old).ok();
            self.move_synced(&target, &old);
            let chapter = self.known_mut(id);
            chapter.path = old.clone();
            chapter.listed = old_listed.clone();
        })
    }

    /// Adds a section from a template after `after`, and makes it active. In a project it is a new file listed in
    /// `druck.yaml`. In a single file it is text at the end of that section, as an undoable edit.
    pub fn new_section(
        &mut self,
        template: &Template,
        name: Option<&str>,
        after: ChapterId,
    ) -> Result<ChapterId, String> {
        let name = name.map(str::trim).filter(|name| !name.is_empty());
        let markdown = self.fill_missing(template.markdown(name));
        let Kind::Project { dir } = self.kind.clone() else {
            return self.insert_in_file(&markdown, after);
        };
        let folder = dir.join(MANUSCRIPT);
        let base = slug(name.unwrap_or(template.slug));
        let (path, listed) = (1..)
            .map(|attempt| {
                let file = if attempt == 1 {
                    format!("{base}.md")
                } else {
                    format!("{base}-{attempt}.md")
                };
                (folder.join(&file), format!("{MANUSCRIPT}/{file}"))
            })
            .find(|(path, _)| !path.exists())
            .expect("some name is free");
        fs::create_dir_all(&folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
        atomic::write(&path, markdown.as_bytes())
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        let before = self.revision();
        let id = self.add(Chapter::new(
            path.clone(),
            listed,
            Group::Book,
            Buffer::new(markdown),
            Read::Done,
        ));
        let at = self
            .book
            .iter()
            .position(|other| *other == after)
            .map_or(self.book.len(), |at| at + 1);
        self.book.insert(at, id);
        if let Err(error) = self.write_chapter_list() {
            self.book.remove(at);
            self.chapters.remove(&id);
            self.synced.remove(&path);
            fs::remove_file(&path).ok();
            return Err(error);
        }
        self.set_revision_above(before);
        self.active = id;
        Ok(id)
    }

    /// Writes out the placeholders this document has no value for, since the engine rejects those.
    fn fill_missing(&self, mut markdown: String) -> String {
        let Ok(settings) = &self.settings else {
            return markdown;
        };
        let values = [
            ("{title}", "Title", settings.title.is_some()),
            ("{subtitle}", "Subtitle", settings.subtitle.is_some()),
            ("{author}", "Author", settings.author.is_some()),
        ];
        for (placeholder, words, set) in values {
            if !set {
                markdown = markdown.replace(placeholder, words);
            }
        }
        markdown
    }

    fn move_synced(&mut self, from: &Path, to: &Path) {
        if let Some(synced) = self.synced.remove(from) {
            self.synced.insert(to.to_owned(), synced);
        }
    }

    fn insert_in_file(&mut self, markdown: &str, after: ChapterId) -> Result<ChapterId, String> {
        let text = self.buffer(after).text();
        let lead = if text.is_empty() || text.ends_with("\n\n") {
            ""
        } else if text.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let end = text.len();
        let caret = end + lead.len();
        self.edit_chapter(
            after,
            vec![Edit::new(end..end, format!("{lead}{markdown}"))],
            EditKind::Discrete,
        )
        .map_err(|error| error.to_string())?;
        self.active = after;
        self.known_mut(after).view.selection = Selection::caret(caret);
        self.resplit();
        Ok(self.active)
    }
}

/// `file` as a path from `folder`, with forward slashes, as Markdown writes it.
fn relative_path(folder: &Path, file: &Path) -> String {
    let from: Vec<_> = folder.components().collect();
    let to: Vec<_> = file.components().collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let ups = std::iter::repeat_n("..".to_owned(), from.len() - shared);
    let downs = to[shared..]
        .iter()
        .map(|part| part.as_os_str().to_string_lossy().into_owned());
    ups.chain(downs).collect::<Vec<_>>().join("/")
}

/// An offset after edits sorted by position, whose ranges refer to the text before them.
fn moved(offset: usize, edits: &[Edit]) -> usize {
    let mut delta = 0isize;
    for edit in edits {
        if offset < edit.range.start {
            break;
        }
        if offset <= edit.range.end {
            return (edit.range.start as isize + delta) as usize + edit.text.len();
        }
        delta += edit.text.len() as isize - edit.range.len() as isize;
    }
    (offset as isize + delta) as usize
}
