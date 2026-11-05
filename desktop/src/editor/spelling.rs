//! Spelling in the galley: what is checked, a cache by each block's text, the quiet underlines, and the menu of
//! suggestions. The check itself runs off the main thread behind [`crate::spell::Checker`], only when the writer pauses,
//! for the blocks on screen.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{Context, Task, Window};

use super::EditorView;
use super::float::Float;
use crate::source::inline::{SpanKind, inline_spans, line_markers};
use crate::source::{BlockKind, Selection};
use crate::spell::words::WordList;
use crate::spell::{self, Checker};

/// A block's text as a key, so an answer stays good for any block that did not change.
fn key(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// How long after the last key the blocks on screen are checked.
const PAUSE: Duration = Duration::from_millis(500);
/// How many suggestions the menu offers.
pub(super) const SUGGESTIONS: usize = 6;

/// A block's text as the checker sees it: without markers and what is not prose, with the way back to the block's bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct Prose {
    pub text: String,
    /// Where each kept piece starts, in the checked text and in the block.
    pieces: Vec<(usize, usize)>,
}

impl Prose {
    /// The block's bytes for a range of the checked text. A word that crosses a hidden marker takes the marker in.
    pub fn to_block(&self, range: Range<usize>) -> Range<usize> {
        self.place(range.start)..self.place(range.end - 1) + 1
    }

    fn place(&self, at: usize) -> usize {
        let index = self.pieces.partition_point(|piece| piece.0 <= at).saturating_sub(1);
        let (clean, source) = self.pieces[index];
        source + at - clean
    }
}

/// What to check of a block, or `None` for what is not prose: code, tables, directives, comments, front matter, and a
/// line that holds only an image. Left out are line markers, inline markers, code spans, citation keys, cross-references,
/// footnote labels, comments, attribute and placeholder groups, link targets, and URLs and addresses.
pub fn prose_of(kind: BlockKind, text: &str) -> Option<Prose> {
    let checked = matches!(
        kind,
        BlockKind::Paragraph
            | BlockKind::Heading { .. }
            | BlockKind::List
            | BlockKind::Quote
            | BlockKind::FootnoteDefinition
    );
    if !checked || text.trim_start().starts_with("![") {
        return None;
    }
    let mut left_out: Vec<Range<usize>> = line_markers(kind, text)
        .into_iter()
        .map(|marker| marker.range)
        .collect();
    for span in inline_spans(kind, text) {
        match span.kind {
            SpanKind::Code
            | SpanKind::Citation
            | SpanKind::CrossReference
            | SpanKind::FootnoteReference
            | SpanKind::Comment => left_out.push(span.range),
            _ => {
                left_out.push(span.open);
                left_out.push(span.close);
            }
        }
    }
    left_out.extend(groups(text));
    left_out.extend(addresses(text));
    if kind == BlockKind::FootnoteDefinition
        && let Some(close) = text.find("]:")
    {
        left_out.push(0..close + 2);
    }
    left_out.retain(|range| !range.is_empty());
    left_out.sort_by_key(|range| range.start);
    let mut prose = Prose {
        text: String::with_capacity(text.len()),
        pieces: Vec::with_capacity(left_out.len() + 1),
    };
    let mut at = 0;
    for range in left_out.iter().chain(std::iter::once(&(text.len()..text.len()))) {
        if range.start > at {
            prose.pieces.push((prose.text.len(), at));
            prose.text.push_str(&text[at..range.start]);
        }
        at = at.max(range.end);
    }
    Some(prose)
}

/// `{...}` groups on one line: placeholders, attributes, and styles.
fn groups(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find('{').map(|at| at + from) {
        let Some(close) = text[open..].find(['}', '\n']).map(|at| at + open) else {
            break;
        };
        if text.as_bytes()[close] == b'}' && !text[open..close].starts_with("{todo:") {
            found.push(open..close + 1);
        }
        from = close + 1;
    }
    found
}

/// Words that are URLs or addresses, and narrative citations the parser left in the text.
fn addresses(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut at = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end();
        if trimmed.contains("://") || trimmed.starts_with("www.") || trimmed.contains('@') {
            found.push(at..at + trimmed.len());
        }
        at += word.len();
    }
    found
}

/// What the galley knows of spelling.
pub struct Spelling {
    pub on: bool,
    checker: Rc<dyn Checker>,
    /// The misspelled words of each block's text, as bytes of the block.
    found: HashMap<u64, Rc<[Range<usize>]>>,
    asked: HashSet<u64>,
    words: WordList,
    pause: Option<Task<()>>,
    results: UnboundedSender<(u64, Vec<Range<usize>>, String)>,
    _inbox: Option<Task<()>>,
    /// The writer asked for the next word and the chapter's blocks were not all checked yet.
    next_wanted: bool,
}

impl Spelling {
    pub fn new(cx: &mut Context<EditorView>, folder: &std::path::Path) -> Self {
        let (results, mut inbox) = unbounded();
        let task = cx.spawn(async move |this, cx| {
            while let Some(result) = inbox.next().await {
                let mut batch = vec![result];
                while let Ok(more) = inbox.try_recv() {
                    batch.push(more);
                }
                if this
                    .update(cx, |this: &mut EditorView, cx| this.spelling_results(batch, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            on: true,
            checker: spell::system(),
            found: HashMap::new(),
            asked: HashSet::new(),
            words: WordList::open(folder),
            pause: None,
            results,
            _inbox: Some(task),
            next_wanted: false,
        }
    }
}

impl EditorView {
    /// The document's language for the checker.
    fn spelling_language(&self) -> String {
        let language = &self.session.details().language;
        if language.is_empty() {
            "en".to_owned()
        } else {
            language.clone()
        }
    }

    /// Waits out the pause again. The only thing that happens on a key.
    pub(super) fn restart_spelling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.spelling.on {
            return;
        }
        self.spelling.pause = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            this.update(cx, |this, cx| {
                this.spelling.pause = None;
                this.check_blocks(false, cx);
            })
            .ok();
        }));
    }

    /// Starts the pause if a block on screen has no answer, as after scrolling.
    pub(super) fn spelling_wanted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.spelling.on || self.spelling.pause.is_some() || self.visible.is_empty() {
            return;
        }
        let text = self.session.active_buffer().text();
        let unknown = self.visible.iter().any(|(index, _)| {
            let block = &self.blocks.as_slice()[*index];
            let key = key(&text[block.range.clone()]);
            !self.spelling.found.contains_key(&key) && !self.spelling.asked.contains(&key)
        });
        if unknown {
            self.restart_spelling(window, cx);
        }
    }

    /// Asks the checker about blocks with no answer: those on screen, or all of the chapter's.
    fn check_blocks(&mut self, whole_chapter: bool, cx: &mut Context<Self>) {
        if self.spelling.words.reload() {
            self.spelling.found.clear();
            cx.notify();
        }
        let language = self.spelling_language();
        let text = self.session.active_buffer().text();
        let blocks = self.blocks.as_slice();
        let indices: Vec<usize> = if whole_chapter {
            (0..blocks.len()).collect()
        } else {
            self.visible.iter().map(|(index, _)| *index).collect()
        };
        for index in indices {
            let block = &blocks[index];
            let block_text = &text[block.range.clone()];
            let key = key(block_text);
            if self.spelling.found.contains_key(&key) || !self.spelling.asked.insert(key) {
                continue;
            }
            let Some(prose) = prose_of(block.kind, block_text) else {
                self.spelling.found.insert(key, Rc::from(Vec::new()));
                self.spelling.asked.remove(&key);
                continue;
            };
            let results = self.spelling.results.clone();
            let checked = prose.text.clone();
            let written = block_text.to_owned();
            self.spelling.checker.check(
                checked,
                &language,
                Box::new(move |ranges| {
                    let ranges = ranges.into_iter().map(|range| prose.to_block(range)).collect();
                    results.unbounded_send((key, ranges, written)).ok();
                }),
            );
        }
    }

    /// Takes the checker's answers, on the main thread, leaving out the project's words.
    fn spelling_results(&mut self, batch: Vec<(u64, Vec<Range<usize>>, String)>, cx: &mut Context<Self>) {
        let mut shows = false;
        for (key, ranges, written) in batch {
            self.spelling.asked.remove(&key);
            let kept: Vec<_> = ranges
                .into_iter()
                .filter(|range| !self.spelling.words.contains(&written[range.clone()]))
                .collect();
            shows |= !kept.is_empty();
            self.spelling.found.insert(key, Rc::from(kept));
        }
        if shows {
            cx.notify();
        }
        if self.spelling.next_wanted && self.spelling.asked.is_empty() {
            self.spelling.next_wanted = false;
            cx.notify();
        }
    }

    /// The misspelled words of the visible blocks in the active chapter's offsets.
    pub(super) fn spelling_ranges(&self) -> Vec<Range<usize>> {
        if !self.spelling.on {
            return Vec::new();
        }
        let text = self.session.active_buffer().text();
        let mut ranges = Vec::new();
        for (index, _) in &self.visible {
            let block = &self.blocks.as_slice()[*index];
            if let Some(found) = self.spelling.found.get(&key(&text[block.range.clone()])) {
                ranges.extend(
                    found
                        .iter()
                        .map(|range| block.range.start + range.start..block.range.start + range.end),
                );
            }
        }
        ranges
    }

    /// The first known misspelled word at or after `from`, going round the chapter's end.
    fn next_misspelled(&self, from: usize) -> Option<Range<usize>> {
        let text = self.session.active_buffer().text();
        let mut all = Vec::new();
        for block in self.blocks.as_slice() {
            if let Some(found) = self.spelling.found.get(&key(&text[block.range.clone()])) {
                all.extend(
                    found
                        .iter()
                        .map(|range| block.range.start + range.start..block.range.start + range.end),
                );
            }
        }
        all.iter().find(|range| range.end >= from).or(all.first()).cloned()
    }

    /// Offers suggestions for the word at the caret, or goes to the next misspelled word. A first press in a chapter
    /// checks the blocks not yet on screen too, and the next press finds them.
    pub(super) fn show_spelling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.spelling.on {
            return;
        }
        self.check_blocks(true, cx);
        // A word already selected is the one the writer has dealt with.
        let selected = self.selection.range();
        let from = if selected.is_empty() {
            selected.start
        } else {
            selected.end + 1
        };
        let Some(word) = self.next_misspelled(from) else {
            self.spelling.next_wanted = true;
            return;
        };
        let text = self.session.active_buffer().text();
        let written = text[word.clone()].to_owned();
        let language = self.spelling_language();
        let mut suggestions = self.spelling.checker.suggestions(&written, &language);
        suggestions.truncate(SUGGESTIONS);
        self.set_selection(
            Selection {
                anchor: word.start,
                head: word.end,
            },
            false,
            window,
            cx,
        );
        self.toolbar_dismissed = true;
        self.open_spelling_menu(word, suggestions, cx);
    }

    /// Writes a suggestion over the word, as one undo step.
    pub(super) fn apply_suggestion(
        &mut self,
        word: Range<usize>,
        suggestion: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.float = Float::None;
        let after = Selection::caret(word.start + suggestion.len());
        let edit = crate::source::Edit {
            range: word,
            text: suggestion.to_owned(),
        };
        self.edit(crate::source::EditKind::Discrete, vec![edit], after, None, window, cx);
    }

    /// Adds the word to the project's list and takes its underlines away everywhere.
    pub(super) fn add_to_word_list(&mut self, word: Range<usize>, cx: &mut Context<Self>) {
        self.float = Float::None;
        let written = self.session.active_buffer().text()[word].to_owned();
        match self.spelling.words.add(&written) {
            Ok(()) => {
                self.spelling.found.clear();
                self.check_blocks(false, cx);
            }
            Err(problem) => self.status = problem.to_string().into(),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(kind: BlockKind, text: &str) -> String {
        prose_of(kind, text).expect("prose").text
    }

    #[test]
    fn code_and_other_blocks_that_are_not_prose_are_not_checked() {
        for kind in [
            BlockKind::Code,
            BlockKind::Table,
            BlockKind::Directive,
            BlockKind::FrontMatter,
            BlockKind::Comment,
        ] {
            assert_eq!(prose_of(kind, "teh words"), None);
        }
        assert_eq!(prose_of(BlockKind::Paragraph, "![teh picture](a.png)"), None);
    }

    #[test]
    fn markers_code_keys_labels_and_urls_are_left_out_and_words_stay_whole() {
        assert_eq!(
            checked(
                BlockKind::Paragraph,
                "Ein **fet**tes `kodee` Wort [@kaey] und [^notte] bei https://exampel.org.\n"
            ),
            "Ein fettes  Wort  und  bei \n"
        );
    }

    #[test]
    fn placeholders_and_a_style_at_the_end_are_left_out() {
        assert_eq!(
            checked(BlockKind::Paragraph, "Text {meta.publisher} mehr {.copyrigt}"),
            "Text  mehr "
        );
    }

    #[test]
    fn a_heading_and_a_list_lose_their_line_markers() {
        assert_eq!(
            checked(BlockKind::Heading { level: 2 }, "## Titel {#sec:labl}\n"),
            "Titel \n"
        );
        assert_eq!(checked(BlockKind::List, "- eins\n- zwei\n"), "eins\nzwei\n");
    }

    #[test]
    fn a_footnote_definition_loses_its_label() {
        assert_eq!(
            checked(BlockKind::FootnoteDefinition, "[^labl]: Der Text.\n"),
            " Der Text.\n"
        );
    }

    #[test]
    fn a_found_word_maps_back_to_the_blocks_bytes_past_what_was_left_out() {
        let text = "Ä `code` Wört **fet**t";
        let prose = prose_of(BlockKind::Paragraph, text).expect("prose");
        let word = prose.text.find("Wört").expect("kept");
        let range = prose.to_block(word..word + "Wört".len());
        assert_eq!(&text[range], "Wört");
        let word = prose.text.find("fett").expect("joined across the marker");
        let range = prose.to_block(word..word + "fett".len());
        assert_eq!(&text[range], "fet**t");
    }
}
