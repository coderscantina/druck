//! Paragraph composition: shapes inline content, chooses line breaks for the whole paragraph, and
//! positions the lines.
//!
//! Words are shaped once. A word that breaks across lines is reshaped only at the break, with a
//! visible hyphen when hyphenated. Justified lines stretch or shrink their interword spaces, never
//! the glyphs, and let punctuation at their edges hang into the margins.

mod breaking;
mod hyphenation;
pub(super) mod protrusion;

use std::collections::HashMap;

use self::breaking::{Hang, Item as Element, Measure};
use super::{Line, inline_face, text_item};
use crate::config::resolved::{InlineStyles, Style};
use crate::config::theme::{Align, FontStyle, Lang, Weight};
use crate::config::values::{Color, Pt};
use crate::document::{Inline, Link};
use crate::page::{Item, Rect};
use crate::text::{FaceId, Fonts, Metrics, ShapedRun};

/// Interword spaces may stretch by half and shrink by a third of their natural width.
const STRETCH: f64 = 1.0 / 2.0;
const SHRINK: f64 = 1.0 / 3.0;
/// Ragged lines may fall short of the measure by about this many ems before they count as loose.
const RAGGED: f64 = 3.0;
/// Cost of a break at a hyphenation point or after an explicit hyphen.
const HYPHEN_COST: f64 = 50.0;

/// A piece of a word in one face and size.
struct Fragment {
    run: ShapedRun,
    color: Color,
    link: Option<Link>,
    metrics: Metrics,
    breaks: Breaks,
    /// How far the baseline is raised, for footnote markers.
    rise: f64,
    /// The footnote this fragment marks.
    note: Option<usize>,
    /// Letter spacing in em, which reshaping keeps.
    tracking: f64,
}

/// Where a fragment may break inside a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Breaks {
    /// At hyphenation points and after joining hyphens.
    Prose,
    /// After a slash between two other characters, without a hyphen: link text that spells its URL.
    Url,
    /// Nowhere: code, footnote markers, and cross-reference text.
    Never,
}

/// A place where a line may end inside a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// A hyphenation point, which adds a hyphen.
    Hyphen,
    /// After a hyphen or dash in the text.
    Explicit,
    /// After a slash in a URL.
    Url,
}

/// Text between two spaces. Style changes inside a word split it into fragments.
#[derive(Default)]
struct Word {
    fragments: Vec<Fragment>,
}

enum Token {
    Word(Word),
    /// An interword space with its natural width.
    Space(f64),
    LineBreak,
}

/// A place in the paragraph text: a byte offset in a fragment of a word.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cursor {
    word: usize,
    fragment: usize,
    offset: usize,
}

impl Cursor {
    fn word(word: usize) -> Self {
        Self {
            word,
            fragment: 0,
            offset: 0,
        }
    }
}

/// Where a line ends: before `at`, with an added hyphen or at a hard break or the paragraph end.
#[derive(Debug, Clone, Copy)]
struct Mark {
    at: Cursor,
    hyphen: bool,
    forced: bool,
}

/// Breaks inline content into positioned lines of `width`. The first line is indented by `first_indent`.
/// Fails for a word wider than its line even after hyphenation, or a character the font cannot show.
pub fn lines(
    content: &[Inline],
    style: &Style,
    inline: &InlineStyles,
    fonts: &Fonts,
    lang: Lang,
    width: f64,
    first_indent: f64,
) -> Result<Vec<Line>, String> {
    prepare(content, style, inline, fonts, lang)?.lines(width, first_indent)
}

/// Shapes inline content once, so it can be measured and then broken at a chosen width. Fails for a
/// character the font cannot show.
pub fn prepare<'a>(
    content: &[Inline],
    style: &'a Style,
    inline: &'a InlineStyles,
    fonts: &'a Fonts,
    lang: Lang,
) -> Result<Prepared<'a>, String> {
    let tokens = tokens(content, style, inline, fonts, lang)?;
    let mut builder = Builder {
        fonts,
        lang,
        hyphenate: style.hyphenate,
        justify: style.align == Align::Justify,
        hyphens: HashMap::new(),
        words: Vec::new(),
        spaces: Vec::new(),
        pieces: Vec::new(),
        items: Vec::new(),
    };
    builder.build(tokens);
    let Builder {
        words,
        spaces,
        pieces,
        items,
        ..
    } = builder;
    Ok(Prepared {
        style,
        inline,
        fonts,
        lang,
        words,
        spaces,
        pieces,
        items,
    })
}

/// Shaped inline content with its breaking items, ready to be set at any width.
pub struct Prepared<'a> {
    style: &'a Style,
    inline: &'a InlineStyles,
    fonts: &'a Fonts,
    lang: Lang,
    words: Vec<Word>,
    /// Natural space before each word. Zero at the start of the paragraph and after hard breaks.
    spaces: Vec<f64>,
    /// The width of each word's first unbreakable piece and of its widest later piece, with the
    /// hyphen added at a hyphenation point.
    pieces: Vec<(f64, f64)>,
    items: Vec<Element<Mark>>,
}

impl Prepared<'_> {
    /// The width of the widest line when only hard line breaks end lines, without what may pass the
    /// edge at its end.
    pub fn natural(&self) -> f64 {
        let (mut widest, mut line) = (0.0f64, 0.0);
        for item in &self.items {
            match item {
                Element::Box(width) | Element::Glue { width, .. } => line += width,
                Element::Break { hang, .. } => widest = widest.max(std::mem::replace(&mut line, 0.0) - hang.end),
                Element::Penalty { .. } | Element::Fill => {}
            }
        }
        widest
    }

    /// The least width that holds every unbreakable piece, with the index of the word that needs it.
    pub fn minimum(&self) -> (f64, Option<usize>) {
        let widest = self
            .pieces
            .iter()
            .map(|&(first, rest)| first.max(rest))
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match widest {
            Some((index, width)) => (width, Some(index)),
            None => (0.0, None),
        }
    }

    /// The text of word `index`.
    pub fn word(&self, index: usize) -> String {
        self.words[index]
            .fragments
            .iter()
            .map(|f| f.run.text.as_str())
            .collect()
    }

    /// Breaks the content into positioned lines of `width`, the first indented by `first_indent`.
    /// Fails for a word wider than its line even after hyphenation.
    pub fn lines(&self, width: f64, first_indent: f64) -> Result<Vec<Line>, String> {
        let (style, fonts, lang) = (self.style, self.fonts, self.lang);
        let justify = style.align == Align::Justify;
        let measure = Measure {
            first: width - first_indent,
            rest: width,
            stretch: if justify { 0.0 } else { RAGGED * style.size.0 },
            hang: match self.words.first() {
                Some(word) if justify => protrusion::leading(&word.fragments[0].run, 0),
                _ => 0.0,
            },
        };
        let Some(marks) = breaking::breaks(&self.items, &measure) else {
            let (index, excess) = self
                .pieces
                .iter()
                .enumerate()
                .map(|(index, &(first, rest))| {
                    let available = if index == 0 { width - first_indent } else { width };
                    (index, (first - available).max(rest - width))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .expect("only words can make a paragraph unbreakable");
            return Err(format!(
                "\"{}\" is {excess:.1}pt wider than the line and cannot be broken",
                self.word(index)
            ));
        };

        let block_face = fonts.face(&style.font, style.weight, style.style);
        let height = style.size.0 * style.line_height;
        let baseline = baseline(height, &fonts.metrics(block_face, style.size));
        let mut start = Cursor::word(0);
        Ok(marks
            .into_iter()
            .enumerate()
            .map(|(index, mark)| {
                let runs = line_runs(&self.words, start, mark, fonts, lang);
                let notes = runs.iter().filter_map(|(_, fragment, _)| fragment.note).collect();
                start = mark.at;
                let indent = if index == 0 { first_indent } else { 0.0 };
                let mut items = position(
                    runs,
                    &self.spaces,
                    style.align,
                    width - indent,
                    indent,
                    mark.forced,
                    baseline,
                    self.inline.link_underline,
                );
                if !mark.hyphen
                    && let Some(Item::Text { run, .. }) =
                        items.iter_mut().rev().find(|item| matches!(item, Item::Text { .. }))
                {
                    keep_final_hyphen(run);
                }
                Line {
                    height,
                    baseline,
                    items,
                    notes,
                    // Only breaks inside a word have an offset into its fragment.
                    hyphenated: mark.at.offset > 0,
                }
            })
            .collect())
    }
}

/// Turns words and spaces into breaking items, with hyphenation points and margin hangs.
struct Builder<'a> {
    fonts: &'a Fonts,
    lang: Lang,
    hyphenate: bool,
    justify: bool,
    /// Width and right hang of a hyphen per face, size, and tracking; `None` if the face has no hyphen.
    hyphens: HashMap<(FaceId, u64, u64), Option<(f64, f64)>>,
    words: Vec<Word>,
    /// Natural space before each word. Zero at the start of the paragraph and after hard breaks.
    spaces: Vec<f64>,
    /// Each word's first and widest later unbreakable piece, see [`Prepared`].
    pieces: Vec<(f64, f64)>,
    items: Vec<Element<Mark>>,
}

impl Builder<'_> {
    fn build(&mut self, tokens: Vec<Token>) {
        let mut tokens = tokens.into_iter().peekable();
        let mut space = None;
        let mut line_has_word = false;
        while let Some(token) = tokens.next() {
            match token {
                Token::Space(width) => space = Some(width),
                Token::LineBreak => {
                    let next = self.words.len();
                    let start = match tokens.peek() {
                        Some(Token::Word(word)) => self.leading(word),
                        _ => 0.0,
                    };
                    let end = self.words.last().map_or(0.0, |previous| self.spacing_after(previous));
                    self.items.push(Element::Fill);
                    self.items.push(Element::Break {
                        at: Mark {
                            at: Cursor::word(next),
                            hyphen: false,
                            forced: true,
                        },
                        hang: self.hang(end, start),
                    });
                    space = None;
                    line_has_word = false;
                }
                Token::Word(word) => {
                    let index = self.words.len();
                    match space.take().filter(|_| line_has_word) {
                        Some(space) => {
                            let end = self.words.last().map_or(0.0, |previous| self.word_end(previous));
                            let (stretch, shrink) = if self.justify {
                                (space * STRETCH, space * SHRINK)
                            } else {
                                (0.0, 0.0)
                            };
                            self.items.push(Element::Glue {
                                width: space,
                                stretch,
                                shrink,
                                at: Mark {
                                    at: Cursor::word(index),
                                    hyphen: false,
                                    forced: false,
                                },
                                hang: self.hang(end, self.leading(&word)),
                            });
                            self.spaces.push(space);
                        }
                        None => self.spaces.push(0.0),
                    }
                    self.word(index, &word);
                    self.words.push(word);
                    line_has_word = true;
                }
            }
        }
        self.items.push(Element::Fill);
        self.items.push(Element::Break {
            at: Mark {
                at: Cursor::word(self.words.len()),
                hyphen: false,
                forced: true,
            },
            hang: Hang {
                end: self.words.last().map_or(0.0, |last| self.spacing_after(last)),
                start: 0.0,
            },
        });
    }

    /// Adds the boxes and inner breaks of word `index` and records its piece widths.
    fn word(&mut self, index: usize, word: &Word) {
        let mut piece_start = (0, 0);
        let (mut first, mut rest) = (None, 0.0f64);
        for (fragment, offset, cut) in self.cuts(word) {
            let piece = &word.fragments[fragment];
            let (hyphen_width, end) = if cut == Cut::Hyphen {
                self.hyphen(piece.run.face, piece.run.size, piece.tracking)
                    .expect("cuts only hyphenate faces with a hyphen")
            } else {
                (0.0, self.end(&piece.run, offset))
            };
            let piece = span_width(word, piece_start, (fragment, offset));
            match first {
                None => first = Some(piece + hyphen_width),
                Some(_) => rest = rest.max(piece + hyphen_width),
            }
            self.items.push(Element::Box(piece));
            self.items.push(Element::Penalty {
                width: hyphen_width,
                cost: HYPHEN_COST,
                flagged: cut != Cut::Url,
                at: Mark {
                    at: Cursor {
                        word: index,
                        fragment,
                        offset,
                    },
                    hyphen: cut == Cut::Hyphen,
                    forced: false,
                },
                hang: self.hang(end, leading_at(word, fragment, offset)),
            });
            piece_start = (fragment, offset);
        }
        let last = word.fragments.len() - 1;
        let piece = span_width(word, piece_start, (last, word.fragments[last].run.text.len()));
        self.pieces.push(match first {
            None => (piece, 0.0),
            Some(first) => (first, rest.max(piece)),
        });
        self.items.push(Element::Box(piece));
    }

    /// Places inside `word` where a line may end, as fragment, byte offset, and kind. A cut at a
    /// fragment boundary belongs to the end of the earlier fragment.
    fn cuts(&mut self, word: &Word) -> Vec<(usize, usize, Cut)> {
        if word.fragments.iter().all(|f| f.breaks == Breaks::Never) {
            return Vec::new();
        }
        let text: String = word.fragments.iter().map(|f| f.run.text.as_str()).collect();
        let mut points: Vec<(usize, Cut)> = hyphenation::explicit(&text)
            .into_iter()
            .map(|p| (p, Cut::Explicit))
            .collect();
        if self.hyphenate {
            points.extend(
                hyphenation::points(&text, self.lang)
                    .into_iter()
                    .map(|p| (p, Cut::Hyphen)),
            );
        }
        if word.fragments.iter().any(|f| f.breaks == Breaks::Url) {
            points.extend(hyphenation::url(&text).into_iter().map(|p| (p, Cut::Url)));
        }
        points.sort_by_key(|&(offset, _)| offset);
        let mut cuts = Vec::with_capacity(points.len());
        for (point, cut) in points {
            let mut start = 0;
            let Some((fragment, offset)) = word.fragments.iter().enumerate().find_map(|(index, f)| {
                let end = start + f.run.text.len();
                let found = (point <= end).then_some((index, point - start));
                start = end;
                found
            }) else {
                continue;
            };
            let current = &word.fragments[fragment];
            let next = match word.fragments.get(fragment + 1) {
                Some(next) if offset == current.run.text.len() => next.breaks,
                _ => current.breaks,
            };
            let allowed = match cut {
                Cut::Url => Breaks::Url,
                Cut::Hyphen | Cut::Explicit => Breaks::Prose,
            };
            let has_hyphen = cut != Cut::Hyphen
                || self
                    .hyphen(current.run.face, current.run.size, current.tracking)
                    .is_some();
            if current.breaks == allowed && next == allowed && has_hyphen {
                cuts.push((fragment, offset, cut));
            }
        }
        cuts
    }

    /// Width and end of a hyphen in a face, size, and tracking, see [`Builder::end`], or `None` if the
    /// face has no hyphen.
    fn hyphen(&mut self, face: FaceId, size: Pt, tracking: f64) -> Option<(f64, f64)> {
        let key = (face, size.0.to_bits(), tracking.to_bits());
        if let Some(&known) = self.hyphens.get(&key) {
            return known;
        }
        let run = self.fonts.shape_tracked("-", face, size, self.lang, tracking);
        let hyphen = missing_glyph(&run).is_none().then(|| (run.width.0, self.end(&run, 1)));
        self.hyphens.insert(key, hyphen);
        hyphen
    }

    /// How far a line ending before byte `to` of `run` may pass the measure: the tracking after
    /// its last character, and in justified text the hang of trailing punctuation.
    fn end(&self, run: &ShapedRun, to: usize) -> f64 {
        let hang = if self.justify {
            protrusion::trailing(run, to)
        } else {
            0.0
        };
        run.spacing.0 + hang
    }

    /// [`Builder::end`] after all of `word`.
    fn word_end(&self, word: &Word) -> f64 {
        let last = &word.fragments[word.fragments.len() - 1].run;
        self.end(last, last.text.len())
    }

    /// The tracking after `word`, which may pass the measure where a hard break or the paragraph
    /// end follows. Punctuation does not hang there, since those lines are not justified.
    fn spacing_after(&self, word: &Word) -> f64 {
        word.fragments[word.fragments.len() - 1].run.spacing.0
    }

    fn leading(&self, word: &Word) -> f64 {
        protrusion::leading(&word.fragments[0].run, 0)
    }

    /// Left margin hangs apply to justified text only; the end comes from [`Builder::end`].
    fn hang(&self, end: f64, start: f64) -> Hang {
        Hang {
            end,
            start: if self.justify { start } else { 0.0 },
        }
    }
}

/// The natural width of `word` between two (fragment, offset) positions. A glyph counts where its
/// text starts, so a ligature across a cut counts before it; the exact width comes from reshaping.
fn span_width(word: &Word, from: (usize, usize), to: (usize, usize)) -> f64 {
    let mut width = 0.0;
    for index in from.0..=to.0 {
        let run = &word.fragments[index].run;
        let low = if index == from.0 { from.1 } else { 0 };
        let high = if index == to.0 { to.1 } else { run.text.len() };
        width += run
            .glyphs
            .iter()
            .filter(|glyph| (low..high).contains(&glyph.text.start))
            .map(|glyph| glyph.x_advance.0)
            .sum::<f64>();
    }
    width
}

/// The left hang of a line starting at a cut.
fn leading_at(word: &Word, fragment: usize, offset: usize) -> f64 {
    let run = &word.fragments[fragment].run;
    match word.fragments.get(fragment + 1) {
        Some(next) if offset == run.text.len() => protrusion::leading(&next.run, 0),
        _ => protrusion::leading(run, offset),
    }
}

/// The shaped runs of one line, each with its fragment and whether a space comes before it.
/// Fragments cut by a break are reshaped; the hyphen of a hyphenated line joins its fragment.
fn line_runs<'a>(
    words: &'a [Word],
    start: Cursor,
    end: Mark,
    fonts: &Fonts,
    lang: Lang,
) -> Vec<(ShapedRun, &'a Fragment, Option<usize>)> {
    let last_word = if end.at.fragment == 0 && end.at.offset == 0 {
        end.at.word
    } else {
        end.at.word + 1
    };
    let mut runs = Vec::new();
    for (index, word) in words.iter().enumerate().take(last_word).skip(start.word) {
        let mut first = true;
        for (number, fragment) in word.fragments.iter().enumerate() {
            if index == start.word && number < start.fragment {
                continue;
            }
            if index == end.at.word && number > end.at.fragment {
                break;
            }
            let text = &fragment.run.text;
            let low = if index == start.word && number == start.fragment {
                start.offset
            } else {
                0
            };
            let at_end = index == end.at.word && number == end.at.fragment;
            let high = if at_end { end.at.offset } else { text.len() };
            if low >= high {
                continue;
            }
            let hyphen = at_end && end.hyphen;
            let run = if low == 0 && high == text.len() && !hyphen {
                fragment.run.clone()
            } else {
                let mut piece = text[low..high].to_owned();
                if hyphen {
                    piece.push('-');
                }
                fonts.shape_tracked(&piece, fragment.run.face, fragment.run.size, lang, fragment.tracking)
            };
            let space = (first && index > start.word).then_some(index);
            runs.push((run, fragment, space));
            first = false;
        }
    }
    runs
}

/// Positions the runs of one line of `available` width, starting at `indent`.
#[allow(clippy::too_many_arguments)]
fn position(
    runs: Vec<(ShapedRun, &Fragment, Option<usize>)>,
    spaces: &[f64],
    align: Align,
    available: f64,
    indent: f64,
    last: bool,
    baseline: f64,
    underline: bool,
) -> Vec<Item> {
    let space_total: f64 = runs
        .iter()
        .filter_map(|(.., space)| space.map(|word| spaces[word]))
        .sum();
    let natural = space_total + runs.iter().map(|(run, ..)| run.width.0).sum::<f64>();
    // Justified lines hang punctuation into the margins; every line ends at its last glyph, not
    // at the tracking after it.
    let (left, right) = match (runs.first(), runs.last()) {
        (Some((first, ..)), Some((last, ..))) if align == Align::Justify => (
            protrusion::leading(first, 0),
            last.spacing.0 + protrusion::trailing(last, last.text.len()),
        ),
        (_, Some((last, ..))) => (0.0, last.spacing.0),
        _ => (0.0, 0.0),
    };
    let slack = available + left + right - natural;
    let (start, adjust) = match align {
        Align::Justify if space_total > 0.0 && (!last || slack < 0.0) => (-left, slack / space_total),
        Align::Justify => (-left, 0.0),
        Align::Left => (0.0, 0.0),
        Align::Center => (slack / 2.0, 0.0),
        Align::Right => (slack, 0.0),
    };

    let mut items = Vec::new();
    let mut open_link: Option<usize> = None;
    let mut x = indent + start;
    for (run, fragment, space) in runs {
        if let Some(word) = space {
            x += spaces[word] * (1.0 + adjust);
        }
        let width = run.width.0;
        if let Some(link) = &fragment.link {
            let metrics = fragment.metrics;
            let extends = open_link.and_then(|i| match &mut items[i] {
                Item::Link { rect, link: open } if open == link => Some(rect),
                _ => None,
            });
            match extends {
                Some(rect) => rect.width = Pt(x + width - rect.x.0),
                None => {
                    open_link = Some(items.len());
                    items.push(Item::Link {
                        rect: Rect {
                            x: Pt(x),
                            y: Pt(baseline - metrics.ascender.0),
                            width: Pt(width),
                            height: Pt(metrics.ascender.0 + metrics.descender.0),
                        },
                        link: link.clone(),
                    });
                }
            }
            if underline {
                let rect = Rect {
                    x: Pt(x),
                    y: Pt(baseline + metrics.underline_position.0),
                    width: Pt(width),
                    height: metrics.underline_thickness,
                };
                items.push(Item::Rect {
                    rect,
                    color: fragment.color,
                });
            }
        } else {
            open_link = None;
        }
        items.push(text_item(x, baseline - fragment.rise, run, fragment.color));
        x += width;
    }
    items
}

/// Makes a hyphen of the text that ends a line extract as a non-breaking hyphen (U+2011). PDF
/// readers take a hyphen, soft hyphen, or U+2010 at a line end for an added one and drop it when
/// they join the lines, which would turn "state-of-the-|art" into "state-of-theart". Added
/// hyphens keep their `-`, so readers join those words as intended.
fn keep_final_hyphen(run: &mut ShapedRun) {
    let Some(hyphen) = run.text.strip_suffix(['-', '\u{2010}']).map(str::len) else {
        return;
    };
    run.text.truncate(hyphen);
    run.text.push('\u{2011}');
    if let Some(glyph) = run.glyphs.iter_mut().rev().find(|glyph| glyph.text.start == hyphen) {
        glyph.text.end = run.text.len();
    }
}

/// The baseline offset from the top of a line of `height`, centering the font's ascender and descender.
pub fn baseline(height: f64, metrics: &Metrics) -> f64 {
    (height - metrics.ascender.0 - metrics.descender.0) / 2.0 + metrics.ascender.0
}

/// Describes the first character of `run` that the font has no glyph for.
pub fn missing_glyph(run: &ShapedRun) -> Option<String> {
    let glyph = run.glyphs.iter().find(|glyph| glyph.id == 0)?;
    let character = run.text[glyph.text.clone()].chars().next()?;
    Some(format!(
        "character '{character}' (U+{:04X}) has no glyph in the selected font",
        u32::from(character)
    ))
}

fn tokens(
    content: &[Inline],
    style: &Style,
    inline: &InlineStyles,
    fonts: &Fonts,
    lang: Lang,
) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut word = Word::default();
    let mut space_widths: HashMap<(FaceId, u64, u64), f64> = HashMap::new();
    let finish = |word: &mut Word, tokens: &mut Vec<Token>| {
        if !word.fragments.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for piece in content {
        let (text, text_style) = match piece {
            Inline::LineBreak => {
                finish(&mut word, &mut tokens);
                tokens.push(Token::LineBreak);
                continue;
            }
            Inline::Text { text, style } => (text, style),
            Inline::Ref(_) | Inline::Citation { .. } => {
                unreachable!("layout resolves cross-references and citations before setting text")
            }
            Inline::FootnoteRef(index) => {
                let (weight, font_style) = inline_face(style, false, false);
                let face = fonts.face(&style.font, weight, font_style);
                let size = inline.footnote_marker_size.to_pt(style.size);
                let run = fonts.shape(&(index + 1).to_string(), face, size, lang);
                if let Some(problem) = missing_glyph(&run) {
                    return Err(problem);
                }
                word.fragments.push(Fragment {
                    run,
                    color: style.color,
                    link: None,
                    metrics: fonts.metrics(face, size),
                    breaks: Breaks::Never,
                    rise: inline.footnote_marker_raise.to_pt(style.size).0,
                    note: Some(*index),
                    tracking: 0.0,
                });
                continue;
            }
        };
        // Code keeps its case and spacing.
        let (face, size, tracking) = if text_style.code {
            let face = fonts.face(&inline.code_font, Weight::REGULAR, FontStyle::Normal);
            (face, inline.code_size.to_pt(style.size), 0.0)
        } else {
            let (weight, font_style) = inline_face(style, text_style.emphasis, text_style.strong);
            (fonts.face(&style.font, weight, font_style), style.size, style.tracking)
        };
        let upper;
        let text = if style.uppercase && !text_style.code {
            upper = text.to_uppercase();
            &upper
        } else {
            text
        };
        let color = match (&text_style.link, text_style.code) {
            (Some(_), _) => inline.link_color,
            (None, true) => inline.code_color,
            (None, false) => style.color,
        };
        let metrics = fonts.metrics(face, size);
        for (index, part) in text.split(' ').enumerate() {
            if index > 0 {
                finish(&mut word, &mut tokens);
                let space = *space_widths
                    .entry((face, size.0.to_bits(), tracking.to_bits()))
                    .or_insert_with(|| fonts.shape_tracked(" ", face, size, lang, tracking).width.0);
                match tokens.last_mut() {
                    Some(Token::Space(width)) => *width = width.max(space),
                    _ => tokens.push(Token::Space(space)),
                }
            }
            if part.is_empty() {
                continue;
            }
            let run = fonts.shape_tracked(part, face, size, lang, tracking);
            if let Some(problem) = missing_glyph(&run) {
                return Err(problem);
            }
            let spells_url = matches!(&text_style.link,
                Some(Link::Url(url)) if url == part || url.strip_prefix("mailto:") == Some(part));
            let breaks = if text_style.code || text_style.unbreakable {
                Breaks::Never
            } else if spells_url {
                Breaks::Url
            } else {
                Breaks::Prose
            };
            word.fragments.push(Fragment {
                run,
                color,
                link: text_style.link.clone(),
                metrics,
                breaks,
                rise: 0.0,
                note: None,
                tracking,
            });
        }
    }
    finish(&mut word, &mut tokens);
    Ok(tokens)
}
