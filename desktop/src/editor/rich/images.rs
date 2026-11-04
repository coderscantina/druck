//! Images as text and as files: reading and writing a figure's Markdown, copying a chosen file into the document, and
//! listing what the document holds. A figure is a paragraph that is one image, as the engine requires.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use super::Key;
use crate::source::Edit;

/// Where a figure goes on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// Where it is written.
    #[default]
    Here,
    Top,
    Bottom,
}

impl Placement {
    fn word(self) -> Option<&'static str> {
        match self {
            Self::Here => None,
            Self::Top => Some("top"),
            Self::Bottom => Some("bottom"),
        }
    }
}

/// One image paragraph, as ranges into its block's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    caption: Range<usize>,
    path: Range<usize>,
    /// Everything up to and including the `)` or the `}` of its attributes.
    pub whole: Range<usize>,
    /// The `fig:name` after `#`.
    pub label: Option<Range<usize>>,
    pub placement: Placement,
}

/// The `]` closing the `[` before `from`, past nested brackets and escapes.
fn closing_bracket(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut depth, mut at) = (0usize, from);
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            b'[' => depth += 1,
            b']' if depth == 0 => return Some(at),
            b']' => depth -= 1,
            _ => {}
        }
        at += 1;
    }
    None
}

impl Figure {
    /// The figure `block_text` is, if all of it is one image with an optional `{...}`. Anything else stays text, and the
    /// engine reports what is wrong with it.
    pub fn parse(block_text: &str) -> Option<Self> {
        if !block_text.starts_with("![") {
            return None;
        }
        let caption_end = closing_bracket(block_text, 2)?;
        let rest = block_text[caption_end..].strip_prefix("](")?;
        let path_start = caption_end + 2;
        let (path, after_path) = if let Some(inner) = rest.strip_prefix('<') {
            let close = inner.find('>')?;
            if !inner[close + 1..].starts_with(')') {
                return None;
            }
            (path_start + 1..path_start + 1 + close, path_start + 1 + close + 2)
        } else {
            let close = rest.find(')')?;
            if rest[..close].is_empty() || rest[..close].contains(char::is_whitespace) {
                return None;
            }
            (path_start..path_start + close, path_start + close + 1)
        };
        let mut figure = Self {
            caption: 2..caption_end,
            path,
            whole: 0..after_path,
            label: None,
            placement: Placement::Here,
        };
        let after = &block_text[after_path..];
        let trimmed = after.trim_end_matches(['\n', '\r', ' ', '\t']);
        if trimmed.is_empty() {
            return Some(figure);
        }
        let inner = trimmed.trim_start().strip_prefix('{')?.strip_suffix('}')?;
        let inner_start = after_path + (trimmed.len() - trimmed.trim_start().len()) + 1;
        for word in inner.split_whitespace() {
            let at = word.as_ptr() as usize - inner.as_ptr() as usize + inner_start;
            if let Some(label) = word.strip_prefix('#') {
                figure.label = Some(at + 1..at + 1 + label.len());
            } else {
                figure.placement = match word {
                    "placement=top" => Placement::Top,
                    "placement=bottom" => Placement::Bottom,
                    _ => return None,
                };
            }
        }
        figure.whole.end = after_path + trimmed.len();
        Some(figure)
    }

    pub fn caption<'a>(&self, block_text: &'a str) -> &'a str {
        &block_text[self.caption.clone()]
    }

    pub fn path<'a>(&self, block_text: &'a str) -> &'a str {
        &block_text[self.path.clone()]
    }

    /// The label without its `fig:`.
    pub fn label_name<'a>(&self, block_text: &'a str) -> &'a str {
        self.label.as_ref().map_or("", |label| {
            let label = &block_text[label.clone()];
            label.strip_prefix("fig:").unwrap_or(label)
        })
    }

    /// The edit, within a block at `start`, that gives the figure this caption, label name, and placement.
    pub fn edit(
        &self,
        block_text: &str,
        start: usize,
        caption: &str,
        label_name: &str,
        placement: Placement,
    ) -> Option<Edit> {
        let path = self.path(block_text);
        let written = write(path, caption, label_name, placement);
        let old = &block_text[self.whole.clone()];
        (old != written).then(|| crate::editor::commands::smallest_edit(start, old, &written))
    }
}

/// How the engine reads a path with spaces.
fn written_path(path: &str) -> String {
    if path.contains(char::is_whitespace) {
        format!("<{path}>")
    } else {
        path.to_owned()
    }
}

/// A figure's Markdown.
pub fn write(path: &str, caption: &str, label_name: &str, placement: Placement) -> String {
    let path = written_path(path);
    let mut attributes = Vec::new();
    if !label_name.trim().is_empty() {
        attributes.push(format!("#fig:{}", label_name.trim()));
    }
    if let Some(word) = placement.word() {
        attributes.push(format!("placement={word}"));
    }
    let braces = if attributes.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", attributes.join(" "))
    };
    format!("![{caption}]({path}){braces}")
}

/// Whether the engine reads this kind of file.
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "svg"))
}

/// A file's natural size in points, or why the engine would refuse it. Reads and decodes the file, so it is for the moment
/// a file is chosen, not for each frame. See [`size_of`].
pub fn inspect(path: &Path) -> Result<(f32, f32), String> {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("");
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    druck::editor::rich::image_size(bytes, extension)
}

/// A file's size for showing it: from the header of a raster file, which is quick, as pixels.
pub fn size_of(path: &Path) -> Result<(f32, f32), String> {
    let is_svg = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if is_svg {
        return inspect(path);
    }
    let reader = image::ImageReader::open(path).map_err(|error| error.to_string())?;
    let reader = reader.with_guessed_format().map_err(|error| error.to_string())?;
    let (width, height) = reader.into_dimensions().map_err(|error| error.to_string())?;
    Ok((width as f32, height as f32))
}

/// A path as the document writes it: relative to `base`, with forward slashes.
pub fn written_relative(base: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(base).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A name free in `folder`, reserved by creating the file: `name.png`, then `name-2.png`. A file with the same bytes under
/// the wanted name is the same image and is used again.
fn copy_into(folder: &Path, source: &Path) -> Result<PathBuf, String> {
    let stem = source.file_stem().map_or("image".into(), |stem| stem.to_string_lossy());
    let extension = source
        .extension()
        .map_or("".into(), |extension| extension.to_string_lossy());
    let bytes = fs::read(source).map_err(|error| format!("{}: {error}", source.display()))?;
    fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    for number in 1u32.. {
        let name = if number == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{number}.{extension}")
        };
        let target = folder.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .map_err(|error| format!("{}: {error}", target.display()))?;
                return Ok(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&target).is_ok_and(|existing| existing == bytes) {
                    return Ok(target);
                }
            }
            Err(error) => return Err(format!("{}: {error}", target.display())),
        }
    }
    unreachable!("some name is free")
}

/// Takes a chosen image into the document and returns its path as the document writes it. The original stays where it
/// is. A file already inside the document is used where it is. A project copies into `assets/`, and a single file's
/// document beside itself. What the engine would refuse is refused here, with its reason.
pub fn bring_in(base: &Path, is_project: bool, source: &Path) -> Result<String, String> {
    if !is_supported(source) {
        return Err(crate::t!(
            Key::ImageUnsupported,
            name = source.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    let source = source
        .canonicalize()
        .map_err(|error| format!("{}: {error}", source.display()))?;
    inspect(&source).map_err(|why| format!("{}: {why}", source.display()))?;
    let base = base
        .canonicalize()
        .map_err(|error| format!("{}: {error}", base.display()))?;
    if let Some(written) = written_relative(&base, &source) {
        return Ok(written);
    }
    let folder = if is_project { base.join("assets") } else { base.clone() };
    let copied = copy_into(&folder, &source)?;
    Ok(written_relative(&base, &copied).expect("the copy is inside the document"))
}

/// One image of the document for the Images tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// As the document writes it.
    pub path: String,
    pub file: PathBuf,
    /// How many times the book's text uses it.
    pub uses: usize,
    /// The text uses it and no file is there.
    pub missing: bool,
}

/// Every image the text uses, and every image in the document's image folders, sorted by path.
pub fn list(base: &Path, is_project: bool, used: impl Iterator<Item = String>) -> Vec<Listed> {
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    for path in used {
        *uses.entry(path).or_default() += 1;
    }
    let mut files: BTreeSet<String> = BTreeSet::new();
    let folders: &[&str] = if is_project { &["assets", "images"] } else { &[""] };
    for folder in folders {
        collect(base, &base.join(folder), is_project, &mut files);
    }
    let every: BTreeSet<String> = uses.keys().cloned().chain(files).collect();
    every
        .into_iter()
        .map(|path| {
            let file = base.join(&path);
            Listed {
                uses: uses.get(&path).copied().unwrap_or(0),
                missing: !file.is_file(),
                path,
                file,
            }
        })
        .collect()
}

fn collect(base: &Path, folder: &Path, recurse: bool, files: &mut BTreeSet<String>) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recurse {
                collect(base, &path, recurse, files);
            }
        } else if is_supported(&path)
            && let Some(written) = written_relative(base, &path)
        {
            files.insert(written);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf,
        0xc0, 0xf0, 0x1f, 0, 5, 0, 1, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
        0x60, 0x82,
    ];

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("druck-images-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("folder");
        dir.canonicalize().expect("canonical")
    }

    #[test]
    fn a_figure_reads_its_parts_and_only_a_whole_image_is_one() {
        let text = "![Größe »x« [1]](<a b.png>){#fig:groß placement=top}\n";
        let figure = Figure::parse(text).expect("a figure");
        assert_eq!(figure.caption(text), "Größe »x« [1]");
        assert_eq!(figure.path(text), "a b.png");
        assert_eq!(figure.label_name(text), "groß");
        assert_eq!(figure.placement, Placement::Top);
        assert!(Figure::parse("![a](b.png) and text\n").is_none());
        assert!(Figure::parse("![a](b.png \"title\")\n").is_none());
        assert!(Figure::parse("text\n").is_none());
        assert!(Figure::parse("![a](b.png){.x}\n").is_none());
    }

    #[test]
    fn writing_a_figure_changes_only_what_changed() {
        let text = "![Alt](a.png){#fig:x}\n";
        let figure = Figure::parse(text).expect("a figure");
        assert_eq!(figure.edit(text, 0, "Alt", "x", Placement::Here), None);
        let edit = figure.edit(text, 10, "Neu", "x", Placement::Bottom).expect("an edit");
        assert_eq!(edit.range, 12..30);
        assert_eq!(edit.text, "Neu](a.png){#fig:x placement=bottom");
        assert_eq!(write("a b.png", "", "", Placement::Here), "![](<a b.png>)");
    }

    #[test]
    fn bringing_in_copies_with_a_safe_name_and_never_moves_the_original() {
        let dir = folder("bring");
        let outside = folder("outside");
        let source = outside.join("Bild ß.png");
        fs::write(&source, PNG).expect("source");
        assert_eq!(bring_in(&dir, true, &source).as_deref(), Ok("assets/Bild ß.png"));
        assert!(source.is_file());
        // The same bytes are the same image, and other bytes under the same name get a new name.
        assert_eq!(bring_in(&dir, true, &source).as_deref(), Ok("assets/Bild ß.png"));
        let mut other = PNG.to_vec();
        other.extend_from_slice(b"tail");
        fs::write(&source, &other).expect("source");
        assert_eq!(bring_in(&dir, true, &source).as_deref(), Ok("assets/Bild ß-2.png"));
        // A file inside the document is used where it stands.
        let inside = dir.join("figures/x.png");
        fs::create_dir_all(inside.parent().expect("a parent")).expect("folder");
        fs::write(&inside, PNG).expect("file");
        assert_eq!(bring_in(&dir, true, &inside).as_deref(), Ok("figures/x.png"));
        // A single file's document keeps it beside itself.
        let single = folder("single");
        assert_eq!(bring_in(&single, false, &source).as_deref(), Ok("Bild ß.png"));
        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&outside).ok();
        fs::remove_dir_all(&single).ok();
    }

    #[test]
    fn what_the_engine_refuses_is_refused_with_its_reason() {
        let dir = folder("refuse");
        let fake = dir.join("fake.png");
        fs::write(&fake, b"not an image").expect("file");
        assert!(bring_in(&dir, true, &fake).is_err());
        let gif = dir.join("x.gif");
        fs::write(&gif, b"GIF89a").expect("file");
        assert!(bring_in(&dir, true, &gif).is_err());
        assert!(!dir.join("assets").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inspecting_reads_png_jpeg_and_svg() {
        let dir = folder("inspect");
        let png = dir.join("a.png");
        fs::write(&png, PNG).expect("file");
        assert_eq!(inspect(&png), Ok((1., 1.)));
        assert_eq!(size_of(&png), Ok((1., 1.)));
        let svg = dir.join("a.svg");
        fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"/>"#,
        )
        .expect("file");
        assert_eq!(size_of(&svg), Ok((72., 36.)));
        let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/images");
        let jpeg = fs::read_dir(&samples)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jpg" || extension == "jpeg")
            });
        if let Some(jpeg) = jpeg {
            assert!(inspect(&jpeg).is_ok());
            assert!(size_of(&jpeg).is_ok());
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_list_counts_uses_and_names_missing_files() {
        let dir = folder("list");
        fs::create_dir_all(dir.join("assets/sub")).expect("folder");
        fs::write(dir.join("assets/used.png"), PNG).expect("file");
        fs::write(dir.join("assets/sub/idle.png"), PNG).expect("file");
        let used = ["assets/used.png", "assets/used.png", "gone.png"].map(str::to_owned);
        let listed = list(&dir, true, used.into_iter());
        let view: Vec<(&str, usize, bool)> = listed
            .iter()
            .map(|image| (image.path.as_str(), image.uses, image.missing))
            .collect();
        assert_eq!(
            view,
            [
                ("assets/sub/idle.png", 0, false),
                ("assets/used.png", 2, false),
                ("gone.png", 1, true)
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }
}
