//! Acceptance checks over every sample, with its custom theme where it has one: the laid-out pages are
//! inspected for overflow, lost content, broken references, and unstable pagination, and the PDF for
//! embedded fonts, text maps, links, and bookmarks. See `docs/ACCEPTANCE.md` for what each requirement
//! rests on.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use crate::page::{Item, Output};
use crate::renderer::{LayoutOptions, PdfOptions, Renderer};
use crate::resource::FileResolver;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");

/// Layout in a debug build needs more stack than a test thread has by default.
const STACK: usize = 16 * 1024 * 1024;

struct Variant {
    name: String,
    document: PathBuf,
    theme: Option<PathBuf>,
}

/// Every Markdown sample, and again with the theme of the same name where there is one.
fn variants() -> Vec<Variant> {
    let mut documents: Vec<PathBuf> = fs::read_dir(SAMPLES)
        .expect("samples folder")
        .map(|entry| entry.expect("sample entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    documents.sort();
    let mut variants = Vec::new();
    for document in documents {
        let name = document.file_stem().expect("file name").to_string_lossy().into_owned();
        let theme = Path::new(SAMPLES).join("themes").join(format!("{name}.json"));
        if theme.is_file() {
            variants.push(Variant {
                name: format!("{name} with its theme"),
                document: document.clone(),
                theme: Some(theme),
            });
        }
        variants.push(Variant {
            name,
            document,
            theme: None,
        });
    }
    variants
}

struct Rendered {
    source: String,
    laid: Output,
    pdf: Vec<u8>,
}

fn render(variant: &Variant) -> Rendered {
    let source = fs::read_to_string(&variant.document).expect("sample");
    let resolver = FileResolver::new();
    let input = Input {
        text: DocumentText::read(&resolver, &variant.document).expect("sample"),
        theme: match &variant.theme {
            Some(path) => ThemeSelection::File { path: path.clone() },
            None => ThemeSelection::Document {
                dirs: ThemeDirs::default(),
            },
        },
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    };
    let renderer = Renderer::new();
    let configured = renderer
        .job(&resolver, &input)
        .configure()
        .unwrap_or_else(|failure| panic!("{}: {failure:?}", variant.name));
    let layout = configured
        .layout(&LayoutOptions::default())
        .unwrap_or_else(|failure| panic!("{}: {failure:?}", variant.name));
    let pdf = layout
        .pdf(&PdfOptions::default())
        .unwrap_or_else(|failure| panic!("{}: {failure:?}", variant.name))
        .bytes;
    Rendered {
        source,
        laid: layout.laid,
        pdf,
    }
}

type Check = fn(&Variant, &Rendered) -> Vec<String>;

/// The checks, by what they establish.
const CHECKS: [(&str, Check); 5] = [
    ("overflow", overflow),
    ("lost content", lost_content),
    ("references", references),
    ("stability", stability),
    ("pdf", pdf_features),
];

/// Nothing is drawn outside its page.
fn overflow(_: &Variant, rendered: &Rendered) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, page) in rendered.laid.pages.iter().enumerate() {
        let (width, height) = (page.width.0 + 0.01, page.height.0 + 0.01);
        for item in &page.items {
            let (left, top, right, bottom) = match item {
                Item::Text { x, y, run, .. } => (x.0, y.0 - run.size.0, x.0 + run.width.0, y.0 + run.size.0 * 0.3),
                Item::Rect { rect, .. } | Item::Link { rect, .. } | Item::Image { rect, .. } => {
                    (rect.x.0, rect.y.0, rect.x.0 + rect.width.0, rect.y.0 + rect.height.0)
                }
                _ => continue,
            };
            let is_link = matches!(item, Item::Link { .. });
            if !is_link && (left < -0.01 || top < -0.01 || right > width || bottom > height) {
                problems.push(format!(
                    "page {} draws outside the page: {left:.1},{top:.1} to {right:.1},{bottom:.1}",
                    index + 1
                ));
            }
        }
    }
    problems
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once("<!--") {
        out.push_str(before);
        rest = after.split_once("-->").map_or("", |(_, tail)| tail);
    }
    out + rest
}

/// The words of the plain paragraphs of Markdown source: letters only, long enough that markup, smart
/// punctuation, and no-break spaces cannot change them.
fn source_words(source: &str) -> BTreeSet<String> {
    let body = match source.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")) {
        Some((_, body)) => body,
        None => source,
    };
    let body = strip_comments(body);
    let mut in_code = false;
    let mut words = BTreeSet::new();
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        let plain = line.chars().next().is_some_and(char::is_alphabetic);
        if in_code || !plain || line.contains("](") || line.contains('{') || line.contains('<') {
            continue;
        }
        let line = line.replace(['`', '|'], " ");
        let line = line
            .split_whitespace()
            .filter(|token| !token.contains('@') && !token.contains("[^"))
            .collect::<Vec<_>>()
            .join(" ");
        for word in line.split(|c: char| !c.is_alphabetic()) {
            if word.chars().count() >= 6 {
                words.insert(word.to_lowercase());
            }
        }
    }
    words
}

/// Every long word of a plain paragraph is on some page. Hyphenation and spacing are ignored by comparing
/// the text of the pages without spaces and hyphens.
fn lost_content(_: &Variant, rendered: &Rendered) -> Vec<String> {
    let mut stream = String::new();
    for page in &rendered.laid.pages {
        for item in &page.items {
            if let Item::Text { run, .. } = item {
                stream.extend(
                    run.text
                        .chars()
                        .filter(|c| c.is_alphabetic())
                        .flat_map(char::to_lowercase),
                );
            }
        }
    }
    let words = source_words(&rendered.source);
    let mut problems = Vec::new();
    if words.len() < 5 {
        problems.push(format!("only {} words are checked", words.len()));
    }
    problems.extend(
        words
            .iter()
            .filter(|word| !stream.contains(word.as_str()))
            .map(|word| format!("the word \"{word}\" is in the source and on no page")),
    );
    problems
}

/// Every link, bookmark, and cross-reference leads to a destination on a page, and no reference is left
/// unresolved in the text.
fn references(_: &Variant, rendered: &Rendered) -> Vec<String> {
    let laid = &rendered.laid;
    let mut problems = Vec::new();
    let mut placed = BTreeSet::new();
    for (index, page) in laid.pages.iter().enumerate() {
        for item in &page.items {
            match item {
                Item::Anchor { id, .. } => {
                    placed.insert(*id);
                    if laid.anchors.get(*id).map(|position| position.page) != Some(index) {
                        problems.push(format!("anchor {id} is listed on another page than page {}", index + 1));
                    }
                }
                Item::Text { run, .. } => {
                    for marker in ["??", "{page"] {
                        if run.text.contains(marker) {
                            problems.push(format!("page {} shows \"{}\"", index + 1, run.text));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for page in &laid.pages {
        for item in &page.items {
            if let Item::Link {
                link: crate::document::Link::Anchor(id),
                ..
            } = item
                && !placed.contains(id)
            {
                problems.push(format!("a link goes to anchor {id}, which no page holds"));
            }
        }
    }
    for bookmark in &laid.outline {
        if !placed.contains(&bookmark.anchor) {
            problems.push(format!("the bookmark \"{}\" goes nowhere", bookmark.title));
        }
    }
    problems
}

/// Two layouts of the same input write the same PDF.
fn stability(variant: &Variant, rendered: &Rendered) -> Vec<String> {
    if render(variant).pdf == rendered.pdf {
        Vec::new()
    } else {
        vec!["a second render differs from the first".to_owned()]
    }
}

fn count(pdf: &[u8], needle: &str) -> usize {
    let text = String::from_utf8_lossy(pdf);
    text.matches(needle).count()
}

/// Fonts are embedded with a text map, links and bookmarks are in the file.
fn pdf_features(_: &Variant, rendered: &Rendered) -> Vec<String> {
    let pdf = &rendered.pdf;
    let mut problems = Vec::new();
    let fonts = count(pdf, "/Subtype/Type0");
    let descriptors = count(pdf, "/Type/FontDescriptor");
    let files = count(pdf, "/FontFile");
    if fonts == 0 || descriptors != fonts || files != fonts {
        problems.push(format!(
            "{fonts} fonts, {descriptors} descriptors, {files} embedded files"
        ));
    }
    if count(pdf, "/ToUnicode") != fonts {
        problems.push("a font has no text map, so its text could not be selected".to_owned());
    }
    let links = rendered
        .laid
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| matches!(item, Item::Link { .. }))
        .count();
    if count(pdf, "/Subtype/Link") != links {
        problems.push(format!(
            "{links} links on the pages, {} in the file",
            count(pdf, "/Subtype/Link")
        ));
    }
    if !rendered.laid.outline.is_empty() && count(pdf, "/Type/Outlines") != 1 {
        problems.push("the headings are not bookmarked".to_owned());
    }
    problems
}

/// Renders every variant on a thread of its own, once, and runs every check on the result.
#[test]
fn every_sample_passes_the_acceptance_checks() {
    let problems: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = variants()
            .into_iter()
            .map(|variant| {
                std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn_scoped(scope, move || {
                        let rendered = render(&variant);
                        let mut problems = Vec::new();
                        for (name, check) in CHECKS {
                            for problem in check(&variant, &rendered) {
                                problems.push(format!("{} ({name}): {problem}", variant.name));
                            }
                        }
                        problems
                    })
                    .expect("thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("rendering did not panic"))
            .collect()
    });
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
