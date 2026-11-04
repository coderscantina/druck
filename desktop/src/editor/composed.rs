//! Print-composer lines: the galley's toggle between the toolkit's line breaks and the engine's. The engine sets the
//! whole document at the column's measure on another thread when the writer pauses. What comes back is kept by each
//! block's text, so it stays good for any block that did not change, and a block with no entry uses toolkit lines.
//!
//! The glyphs stay the toolkit's: only the breaks, the spaces of lines that reach the measure, a paragraph's first-line
//! indent, and added hyphens come from the engine. Painting the engine's own glyphs would be a second font path.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use druck::cancel::Cancel;
use druck::editor::composer::SetLines;
use gpui::{AppContext as _, Context, Window};

use super::EditorView;
use super::display::DisplayLine;
use super::layout::POINT;
use crate::preview::job::Typesetter;
use crate::session::Joined;
use crate::source::{BlockKind, Blocks};

/// How long after the last key the engine sets the lines. Longer than the pages' own pause, so they go first.
const PAUSE: Duration = Duration::from_millis(600);
/// A left edge below this many points is a line's hanging punctuation, and above it an indent.
const INDENT_FROM: f64 = 1.0;

/// A line of a block, as the engine set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Where it starts, relative to the block.
    pub start: usize,
    /// It ends in a hyphen the engine added, which is not in the text.
    pub hyphen: bool,
    /// It reaches the end of the measure.
    pub fills: bool,
}

/// A block's lines as the engine set them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComposedBlock {
    pub rows: Vec<Row>,
    /// How far in the first line starts, in points of the page.
    pub indent: f32,
}

/// Where a line of the galley breaks: at a displayed offset, with what is true of the row it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forced {
    pub at: usize,
    pub hyphen: bool,
    pub fills: bool,
}

impl ComposedBlock {
    /// The breaks inside one displayed line. A row that starts where the line does is no break. Rows the display
    /// cannot place, such as inside hidden text, end up at the place that text would appear.
    pub fn breaks(&self, line: &DisplayLine) -> Vec<Forced> {
        let mut forced: Vec<Forced> = Vec::new();
        for pair in self.rows.windows(2) {
            let (upper, row) = (pair[0], pair[1]);
            if row.start <= line.source.start || row.start >= line.source.end {
                continue;
            }
            let at = line.to_display(row.start);
            let after_the_last = forced.last().map_or(0, |last| last.at);
            if at > after_the_last && at < line.text.len() {
                forced.push(Forced {
                    at,
                    hyphen: upper.hyphen,
                    fills: upper.fills,
                });
            }
        }
        forced
    }
}

/// The key of a block's composed lines.
pub fn key(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Whether the engine's lines apply to a block of this kind. Headings, tables, code, and notes have other measures or
/// their own views.
pub fn composes(kind: BlockKind) -> bool {
    matches!(kind, BlockKind::Paragraph | BlockKind::Quote | BlockKind::List)
}

/// The engine's lines by the text of the block that holds them. Blocks with the same text set the same way almost
/// always, and the last one wins where they do not, such as one that opens a chapter with no indent.
pub fn by_text(joined: &Joined, set: &SetLines) -> HashMap<u64, Arc<ComposedBlock>> {
    let starts: Vec<usize> = set
        .chapters
        .iter()
        .map(|path| joined.map.range_of(path).map_or(0, |range| range.start))
        .collect();
    let mut rows: Vec<(usize, Row, f64)> = set
        .lines
        .iter()
        .map(|line| {
            let start = starts[line.start.chapter.0 as usize] + line.start.offset as usize;
            let row = Row {
                start,
                hyphen: line.hyphen,
                fills: line.fills,
            };
            (start, row, line.left)
        })
        .collect();
    rows.sort_by_key(|(start, ..)| *start);
    let blocks = Blocks::parse(&joined.text);
    let mut found = HashMap::new();
    for block in blocks.as_slice().iter().filter(|block| composes(block.kind)) {
        let inside = in_range(&rows, &block.range);
        if inside.is_empty() {
            continue;
        }
        let first_left = inside[0].2;
        let composed = ComposedBlock {
            rows: inside
                .iter()
                .map(|(_, row, _)| Row {
                    start: row.start - block.range.start,
                    ..*row
                })
                .collect(),
            indent: if block.kind == BlockKind::Paragraph && first_left > INDENT_FROM {
                first_left as f32
            } else {
                0.
            },
        };
        found.insert(key(&joined.text[block.range.clone()]), Arc::new(composed));
    }
    found
}

fn in_range<'a>(rows: &'a [(usize, Row, f64)], range: &Range<usize>) -> &'a [(usize, Row, f64)] {
    let from = rows.partition_point(|(start, ..)| *start < range.start);
    let to = rows.partition_point(|(start, ..)| *start < range.end);
    &rows[from..to]
}

/// The toggle's state.
#[derive(Default)]
pub struct PrintLines {
    pub on: bool,
    /// The page's measure, once the engine has said, in points.
    pub page_measure: Option<f32>,
    /// The measure `found` was set at.
    measure: f32,
    found: HashMap<u64, Arc<ComposedBlock>>,
    /// The writer typed in the block the caret is in, and has not left it.
    pub typing: bool,
    /// Something other than the text changed the lines, such as the theme.
    stale: bool,
    pause: Option<gpui::Task<()>>,
    running: Option<gpui::Task<()>>,
    cancel: Option<Cancel>,
}

impl PrintLines {
    /// A block's lines, when the engine has set this text at the measure in use.
    pub fn lines_of(&self, text: &str, measure: f32) -> Option<Arc<ComposedBlock>> {
        if !self.on || (self.measure - measure).abs() > f32::EPSILON {
            return None;
        }
        self.found.get(&key(text)).cloned()
    }

    /// Forgets lines that no longer hold anywhere, as after a change of theme.
    pub fn forget(&mut self) {
        self.found.clear();
    }

    /// Stops what is running or waiting, which a newer edit made out of date.
    fn stop(&mut self) {
        self.pause = None;
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.running = None;
    }
}

impl EditorView {
    /// Whether the galley shows the engine's line breaks.
    pub fn shows_print_lines(&self) -> bool {
        self.print.on
    }

    pub fn toggle_print_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.print.on = !self.print.on;
        self.print.typing = false;
        if self.print.on {
            self.compose_now(window, cx);
        } else {
            self.print.stop();
            self.print.forget();
        }
        self.forget_shapes();
        self.autoscroll = true;
        cx.emit(super::SourceViewChanged);
        cx.notify();
    }

    /// The theme or another file changed, so the lines are set again soon.
    pub fn composed_lines_are_stale(&mut self, cx: &mut Context<Self>) {
        if self.print.on {
            self.print.stale = true;
            cx.notify();
        }
    }

    /// Sets the lines once the writer pauses. Does nothing else on a key.
    pub(super) fn schedule_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.print.on {
            return;
        }
        self.print.stop();
        self.print.pause = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            this.update_in(cx, |this, window, cx| this.compose_now(window, cx)).ok();
        }));
    }

    /// Asks again if the column's room asks for another measure than the lines were set at.
    pub(super) fn compose_if_resized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.print.on || self.print.running.is_some() || self.print.pause.is_some() || self.source {
            return;
        }
        let wanted = self.composer_measure();
        if self.print.stale || (self.print.page_measure.is_some() && (self.print.measure - wanted).abs() > f32::EPSILON)
        {
            self.print.stale = false;
            self.schedule_composer(window, cx);
        }
    }

    /// The measure the lines should be set at, in points.
    pub(super) fn composer_measure(&self) -> f32 {
        let room = f32::from(self.column.room) / POINT;
        self.print.page_measure.map_or(room, |page| page.min(room))
    }

    fn compose_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.print.stop();
        let joined = self.session.joined();
        let kind = self.session.kind().clone();
        let theme = self.session.theme_file();
        let room = f64::from(f32::from(self.column.room) / POINT);
        let cancel = Cancel::default();
        self.print.cancel = Some(cancel.clone());
        let work = cx.background_spawn(async move {
            let (set, measure, page) = Typesetter::new(kind, theme).composed(joined.clone(), room, &cancel)?;
            Some((by_text(&joined, &set), measure as f32, page as f32))
        });
        self.print.running = Some(cx.spawn_in(window, async move |this, cx| {
            let found = work.await;
            this.update(cx, |this, cx| {
                this.print.running = None;
                this.print.cancel = None;
                let Some((found, measure, page)) = found else {
                    return;
                };
                if !this.print.on {
                    return;
                }
                this.print.found = found;
                this.print.measure = measure;
                this.print.page_measure = Some(page);
                cx.notify();
            })
            .ok();
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, source: Range<usize>) -> DisplayLine {
        super::super::display::display_lines(
            BlockKind::Paragraph,
            text,
            &super::super::display::Syntax::default(),
            &[],
            None,
            false,
            &[],
        )
        .into_iter()
        .find(|line| line.source == source)
        .expect("the line exists")
    }

    #[test]
    fn a_break_falls_where_the_engine_set_the_next_line_and_keeps_the_upper_rows_hyphen() {
        let composed = ComposedBlock {
            rows: vec![
                Row {
                    start: 0,
                    hyphen: true,
                    fills: true,
                },
                Row {
                    start: 8,
                    hyphen: false,
                    fills: false,
                },
            ],
            indent: 0.,
        };
        let line = line("Wortbruchstück", 0.."Wortbruchstück".len());
        assert_eq!(
            composed.breaks(&line),
            [Forced {
                at: 8,
                hyphen: true,
                fills: true
            }]
        );
    }

    #[test]
    fn a_row_at_the_start_of_a_line_is_no_break() {
        let composed = ComposedBlock {
            rows: vec![
                Row {
                    start: 0,
                    hyphen: false,
                    fills: true,
                },
                Row {
                    start: 6,
                    hyphen: false,
                    fills: false,
                },
            ],
            indent: 0.,
        };
        assert!(composed.breaks(&line("hello\nworld", 6..11)).is_empty());
        assert!(composed.breaks(&line("hello\nworld", 0..5)).is_empty());
    }

    #[test]
    fn lines_are_kept_by_the_text_of_the_block_that_holds_them() {
        let joined = Joined::single("First para.\n\nSecond one here.\n".to_owned(), 0);
        let place = |offset| druck::output::SourcePos {
            chapter: druck::output::ChapterId(0),
            offset,
        };
        let set = SetLines {
            chapters: vec!["/book.md".into()],
            lines: vec![
                druck::editor::composer::SetLine {
                    start: place(0),
                    hyphen: false,
                    fills: false,
                    left: 0.,
                },
                druck::editor::composer::SetLine {
                    start: place(13),
                    hyphen: false,
                    fills: true,
                    left: 12.,
                },
                druck::editor::composer::SetLine {
                    start: place(20),
                    hyphen: false,
                    fills: false,
                    left: 0.,
                },
            ],
        };
        let found = by_text(&joined, &set);
        let second = &found[&key("Second one here.\n")];
        assert_eq!(second.rows.iter().map(|row| row.start).collect::<Vec<_>>(), [0, 7]);
        assert_eq!(second.indent, 12.);
        assert_eq!(found[&key("First para.\n")].indent, 0.);
    }
}
