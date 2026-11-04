//! The print-composer lines against the pages: for the same document, the galley's composer at the page's measure
//! breaks running text where the pages do. The samples run everywhere. A book runs where `DRUCK_BOOK` names a project
//! folder and says so when it does not.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use druck::cancel::Cancel;
use druck::editor::composer;
use druck::input::{Date, DocumentText, Input, Overrides, Project, ThemeDirs, ThemeSelection};
use druck::output::{BlockKind, Item, Layout, SourcePos};
use druck::resource::FileResolver;
use druck::{LayoutOptions, Renderer};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
const STACK: usize = 16 * 1024 * 1024;

/// Line starts on the pages, block by block: a new line wherever the baseline changes. A drop capital stands on a
/// lower baseline and counts with the line it starts.
fn page_lines(laid: &Layout) -> BTreeMap<u32, BTreeSet<SourcePos>> {
    let map = laid.source_map().expect("the layout asked for a source map");
    let prose = |block: u32| {
        matches!(
            map.blocks()[block as usize].kind,
            BlockKind::Paragraph | BlockKind::ListItem
        )
    };
    let mut lines: BTreeMap<u32, BTreeSet<SourcePos>> = BTreeMap::new();
    let mut current: Option<(usize, u32, f64)> = None;
    let mut initial = None;
    let mut sized = 0.0;
    for (page_index, page) in laid.pages().iter().enumerate() {
        let texts: Vec<_> = page
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text {
                    y,
                    run,
                    source: Some(id),
                    ..
                } => Some((y.0, run, *id)),
                _ => None,
            })
            .collect();
        for (at, (y, run, id)) in texts.iter().enumerate() {
            let source = map.text(*id, run);
            let block = source.block.0;
            if source.spans.is_empty() || !prose(block) {
                continue;
            }
            // A drop capital is far larger than the text after it, which starts higher up, beside it.
            if texts
                .get(at + 1)
                .is_some_and(|next| next.1.size.0 * 1.5 < run.size.0 && next.0 < *y)
            {
                initial = Some(map.nearest(*id, 0));
                continue;
            }
            // A raised footnote mark stays on its line, by the size of the line's text.
            let same = current.is_some_and(|(page, in_block, line)| {
                page == page_index && in_block == block && (line - y).abs() < sized * 0.6
            });
            if !same {
                sized = run.size.0;
                current = Some((page_index, block, *y));
                let start = initial.take().unwrap_or_else(|| map.nearest(*id, 0));
                lines.entry(block).or_default().insert(start);
            }
        }
    }
    lines
}

/// Compares every paragraph, returning how many lines matched.
fn compare(name: &str, input: &Input, resolver: &FileResolver) -> usize {
    let renderer = Renderer::new();
    let cancel = Cancel::default();
    let job = renderer.job(resolver, input, &cancel);
    let configured = job.configure().expect("the document configures");
    let laid = configured
        .layout(&LayoutOptions { source_map: true })
        .expect("the document lays out");
    let composed = composer::lines(&configured, composer::page_measure(&configured)).expect("the lines compose");
    let map = laid.source_map().expect("a source map");
    let on_pages: BTreeSet<SourcePos> = page_lines(&laid).into_values().flatten().collect();
    // The text of footnotes and table cells stands apart from the running text, which is what is compared.
    let running = |place: &SourcePos| {
        let block = map.blocks().iter().rev().find(|block| {
            block.range.0.chapter == place.chapter
                && block.range.0.offset <= place.offset
                && place.offset < block.range.0.offset + block.range.1
                && matches!(block.kind, BlockKind::Paragraph | BlockKind::ListItem)
        });
        block.is_some_and(|block| {
            !std::iter::successors(block.parent, |parent| map.blocks()[parent.0 as usize].parent).any(|parent| {
                matches!(
                    map.blocks()[parent.0 as usize].kind,
                    BlockKind::Footnote | BlockKind::Table
                )
            })
        })
    };
    let set: BTreeSet<SourcePos> = composed.iter().map(|line| line.start).filter(running).collect();
    let body: BTreeSet<SourcePos> = on_pages.iter().copied().filter(running).collect();
    let missing: Vec<_> = body.difference(&set).take(5).collect();
    let extra: Vec<_> = set.difference(&body).take(5).collect();
    println!(
        "{name}: {} composed lines, {} lines of running text on {} pages, {} only on pages, {} only composed",
        set.len(),
        body.len(),
        laid.pages().len(),
        body.difference(&set).count(),
        set.difference(&body).count()
    );
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: only on pages {missing:?}, only composed {extra:?}"
    );
    set.len()
}

fn input(path: &Path, theme: Option<PathBuf>, shared: bool, resolver: &FileResolver) -> Input {
    let dirs = ThemeDirs {
        project: path.is_dir().then(|| path.to_owned()),
        shared: shared.then(FileResolver::shared_themes).flatten(),
    };
    let theme = theme.map_or(ThemeSelection::Document { dirs }, |path| ThemeSelection::File { path });
    let text = if path.is_dir() {
        let project = Project::open(resolver, path).expect("a project");
        project.export(None).expect("the default export").0
    } else {
        DocumentText::read(resolver, path).expect("a sample")
    };
    Input {
        text,
        theme,
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    }
}

fn on_a_large_stack(work: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(work)
        .expect("a thread starts")
        .join()
        .expect("the comparison holds");
}

#[test]
fn the_composer_breaks_every_sample_where_its_pages_do() {
    on_a_large_stack(|| {
        let mut samples: Vec<PathBuf> = std::fs::read_dir(SAMPLES)
            .expect("samples folder")
            .map(|entry| entry.expect("sample entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .collect();
        samples.sort();
        let mut lines = 0;
        for path in samples {
            let resolver = FileResolver::new();
            let stem = path.file_stem().expect("a file name").to_string_lossy().into_owned();
            let theme = Path::new(SAMPLES).join("themes").join(format!("{stem}.json"));
            let input = input(&path, theme.is_file().then_some(theme), false, &resolver);
            lines += compare(&stem, &input, &resolver);
        }
        assert!(lines > 100);
    });
}

#[test]
fn the_composer_breaks_the_book_where_its_pages_do() {
    let Some(path) = std::env::var_os("DRUCK_BOOK").map(PathBuf::from) else {
        println!("skipping the book: DRUCK_BOOK does not name a project folder");
        return;
    };
    on_a_large_stack(move || {
        let resolver = FileResolver::new();
        let input = input(&path, None, true, &resolver);
        assert!(compare("the book", &input, &resolver) > 1000);
    });
}
