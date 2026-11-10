//! The trial as the agent meets it: the tool that tries fixes, what it answers, and the prompt that hands the agent the
//! list of warnings. All of it is for the model, so none of it is translated. Holds no gpui types.

use std::path::{Path, PathBuf};

use druck::cancel::Cancel;
use serde_json::{Value, json};

use super::fix::{self, Fix};
use super::trial::{Bench, Book, Warning};
use crate::agent::tools::TRY_FIXES;
use crate::source::{BlockKind, Blocks};

/// How many words of a block the list of warnings shows.
const EXCERPT_WORDS: usize = 6;
/// How many warnings an answer lists of those a fix cleared, caused, or left.
const EXAMPLES: usize = 12;

pub fn definition() -> Value {
    json!({
        "name": TRY_FIXES,
        "description": "Try typesetting fixes on a copy of the book. Each fix is laid out on top of the fixes kept so \
            far, and is kept for the author's review only if it clears at least one warning and causes no new one. \
            Nothing is written to the book. The answer lists, per fix, whether it was kept and which warnings it \
            cleared and caused. Send several fixes in one call.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "fixes": {
                    "type": "array",
                    "description": "The fixes to try, in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["page-break", "keep", "placement", "theme"],
                                "description": "page-break: a page break before the block. keep: the block is not \
                                    split across pages. placement: where a figure goes. theme: one book-wide value.",
                            },
                            "file": {"type": "string", "description": "The chapter's file, relative to the project folder. Not for theme."},
                            "line": {"type": "integer", "description": "The line the block starts on in that file as it is on disk. Not for theme."},
                            "property": {
                                "type": "string",
                                "description": "For theme: typesetting.hyphen-fragments, styles.<style>.tracking, or styles.<style>.hyphenate.",
                            },
                            "value": {"description": "For placement: top, bottom, or here. For theme: the value, such as [2, 2], 0.01, or true."},
                        },
                        "required": ["kind"],
                    },
                },
                "list_remaining": {"type": "boolean", "description": "Also list the first warnings that remain."},
            },
            "required": ["fixes"],
        },
    })
}

/// The copy of the book the tool tries fixes on, opened from disk at the first call.
pub struct Trials {
    /// The project folder or the single file.
    location: PathBuf,
    bench: Option<Bench>,
}

impl Trials {
    pub fn new(location: &Path) -> Self {
        Self {
            location: location.to_owned(),
            bench: None,
        }
    }

    /// Tries the fixes of one call. An error is a call that could not run at all.
    pub fn call(&mut self, arguments: &Value) -> Result<String, String> {
        let fixes = arguments["fixes"].as_array().ok_or("fixes is required, as a list.")?;
        let bench = match &mut self.bench {
            Some(bench) => bench,
            None => self.bench.insert(Bench::new(Book::open(&self.location)?)),
        };
        let cancel = Cancel::default();
        let results: Vec<Value> = fixes.iter().map(|input| try_one(bench, input, &cancel)).collect();
        let remaining = bench.warnings(&cancel)?;
        let mut answer = json!({"results": results, "remaining": remaining.len()});
        if arguments["list_remaining"] == true {
            answer["warnings"] = some(remaining);
        }
        Ok(answer.to_string())
    }
}

/// Up to [`EXAMPLES`] of some warnings as lines, with how many there are when that is more. A theme value can clear
/// and cause a hundred at once, and the agent pays for every line it is sent.
fn some(warnings: &[Warning]) -> Value {
    let lines: Vec<String> = warnings.iter().take(EXAMPLES).map(Warning::line).collect();
    if warnings.len() > EXAMPLES {
        json!({"count": warnings.len(), "first": lines})
    } else {
        json!(lines)
    }
}

fn try_one(bench: &mut Bench, input: &Value, cancel: &Cancel) -> Value {
    let tried = Fix::from_json(input).and_then(|fix| bench.try_fix(fix, cancel));
    match tried {
        Ok(tried) => {
            let outcome = &tried.outcome;
            let mut result = json!({
                "kept": outcome.keeps(),
                "cleared": some(&outcome.cleared),
                "caused": some(&outcome.caused),
            });
            if outcome.unchanged && !outcome.cleared.is_empty() {
                result["note"] = json!("no text moved to another page, so this would only hide a warning.");
            }
            if !outcome.excused.is_empty() {
                result["excused"] = some(&outcome.excused);
                result["note"] = json!("a page break only excuses the page it ends, which is no fix for that page.");
            }
            result
        }
        Err(problem) => json!({"kept": false, "error": problem}),
    }
}

/// Which fixes of a call the tool said it kept, in the call's order. `None` for an answer that is not the tool's.
pub fn kept_in(answer: &str) -> Option<Vec<bool>> {
    let answer: Value = serde_json::from_str(answer).ok()?;
    (answer["results"].as_array()?.iter())
        .map(|result| result["kept"].as_bool())
        .collect()
}

/// The first words of the block at a line, as they are written, for the agent to know the block by and to quote.
fn excerpt(text: &str, line: usize) -> Option<String> {
    let written = text.lines().nth(line.checked_sub(1)?)?;
    let end = written
        .split_whitespace()
        .nth(EXCERPT_WORDS - 1)
        .map_or(written.len(), |word| {
            word.as_ptr() as usize - written.as_ptr() as usize + word.len()
        });
    Some(written[..end].to_owned())
}

/// The line of the nearest heading above a line, which is the block to break before when its text falls short.
fn heading_above(text: &str, line: usize) -> Option<usize> {
    let blocks = Blocks::parse(text);
    (blocks.as_slice().iter())
        .filter(|block| matches!(block.kind, BlockKind::Heading { .. }))
        .map(|block| fix::line_of(text, block.range.start))
        .take_while(|heading| *heading < line)
        .last()
}

/// The line of the block that ends before a line, which is the one to move over when a page holds too little.
fn block_before(text: &str, line: usize) -> Option<usize> {
    let blocks = Blocks::parse(text);
    (blocks.as_slice().iter())
        .filter(|block| block.kind != BlockKind::Blank)
        .map(|block| fix::line_of(text, block.range.start))
        .take_while(|start| *start < line)
        .last()
}

fn listed(book: &Book, warning: &Warning) -> String {
    let page = warning.page.map(|page| format!("page {page}")).unwrap_or_default();
    let words = |file: &str, line: usize| {
        (book.file(file))
            .and_then(|(_, text)| excerpt(text, line))
            .map(|words| format!(": \"{words}\""))
            .unwrap_or_default()
    };
    match (&warning.at, &warning.first_block) {
        (Some((file, line)), _) => {
            let heading = (warning.kind == "heading-at-foot")
                .then(|| heading_above(book.file(file)?.1, *line))
                .flatten()
                .map(|heading| format!(", its heading is at line {heading}"))
                .unwrap_or_default();
            format!("- {file}:{line} {page}{heading}{}", words(file, *line))
        }
        (None, Some((file, line))) => {
            let before = (book.file(file))
                .and_then(|(_, text)| block_before(text, *line))
                .map(|before| format!(", the block before it is at line {before}"))
                .unwrap_or_default();
            format!(
                "- {page}, its first block is {file}:{line}{before}{}",
                words(file, *line)
            )
        }
        (None, None) => format!("- {page}"),
    }
}

/// The kinds of warning in the order the list has them: first those a directive at a block can mend, then those only
/// a theme value moves, then those of whole pages, which mostly follow from the others.
const ORDER: [&str; 11] = [
    "heading-at-foot",
    "one-liner-at-top",
    "one-liner-at-bottom",
    "list-item-at-top",
    "list-item-at-bottom",
    "chapter-end-short",
    "too-many-hyphens",
    "short-last-line",
    "loose-line",
    "page-short",
    "page-uneven",
];

/// What the agent is sent to fix the proof: the rules, and the warnings by kind. `limit` cuts the list, for a run that
/// should cost little.
pub fn prompt(book: &Book, warnings: &[Warning], limit: Option<usize>) -> String {
    let mut sorted: Vec<&Warning> = warnings.iter().collect();
    sorted.sort_by_key(|warning| ORDER.iter().position(|kind| *kind == warning.kind));
    let shown = &sorted[..limit.map_or(sorted.len(), |limit| limit.min(sorted.len()))];
    let mut list = String::new();
    for kind in ORDER {
        let of_kind: Vec<&&Warning> = shown.iter().filter(|warning| warning.kind == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        list.push_str(&format!("\n{kind} ({}):\n", of_kind.len()));
        for warning in of_kind {
            list.push_str(&listed(book, warning));
            list.push('\n');
        }
    }
    let cut = if shown.len() < warnings.len() {
        format!(" These are {} of {}. Work only on these.", shown.len(), warnings.len())
    } else {
        String::new()
    };
    format!(
        "Fix the proof of this book. Its pages break typesetting rules, listed below by kind, each with its file and \
         line, its page, and the first words of its block. A warning about a whole page names the first block that \
         starts on that page instead.{cut}\n\
         <druck-warnings>{list}</druck-warnings>\n\n\
         Try fixes with the druck tool `{TRY_FIXES}`. It lays the book out on a copy with each fix and answers with the \
         warnings the fix cleared and caused. A fix is kept only if it clears at least one warning and causes none, and \
         a kept fix stays in the copy for the trials after it. The author reviews every kept fix before it is written, \
         so do not edit, create, or delete any file. You need not read any file either: lines are those of the files \
         as they are on disk, also after fixes are kept.\n\n\
         A fix is a directive at a block or one book-wide theme value. There is no setting for a single paragraph.\n\
         - heading-at-foot: a page-break before the heading, at the heading's line.\n\
         - one-liner-at-top, one-liner-at-bottom, list-item-at-top, list-item-at-bottom: a page-break before the block \
         or the block before it, or a keep on the block before it.\n\
         - chapter-end-short: a page-break before the block ahead of that page's first block, both named in the list, \
         so the last page holds more. A break before the first block itself changes no page and is not kept.\n\
         - page-short, page-uneven: a page break cannot mend these, since it only excuses the page before it, and that \
         does not count as clearing. Leave them unless a fix for another warning clears them.\n\
         - loose-line, too-many-hyphens, short-last-line: only a theme value moves these without changing words. Try \
         typesetting.hyphen-fragments as [2, 2] and [2, 3], styles.body.tracking between -0.02 and 0.02, and \
         styles.body.hyphenate as true in case the theme turned hyphenation off. A theme value changes every page, so \
         it is kept only if it causes no new warning anywhere.\n\n\
         Send many fixes in one call and make few calls: one for the directives, one for theme values, and at most one \
         more for second attempts. Do not try a fix again that was not kept. Where only other words would help, leave \
         at most three margin notes with the druck tool `leave_note`, for the worst cases, quoting the words shown in \
         the list. When you are done, reply with one short sentence in the book's language saying what you found."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_says_which_fixes_were_kept() {
        let answer = json!({"results": [{"kept": true}, {"kept": false, "error": "no"}], "remaining": 3});
        assert_eq!(kept_in(&answer.to_string()), Some(vec![true, false]));
        assert_eq!(kept_in("The note stands beside the passage."), None);
    }

    #[test]
    fn the_list_names_each_block_by_its_words_and_a_heading_by_its_line() {
        let dir = std::env::temp_dir().join(format!("druck-fix-prompt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("book.md");
        std::fs::write(
            &file,
            "# One\n\n## Two\n\nThe lamps were lit along the quay that night.\n",
        )
        .expect("file");
        let book = Book::open(&file).expect("the book opens");
        let warning = |kind, at: Option<usize>| Warning {
            kind,
            at: at.map(|line| ("book.md".to_owned(), line)),
            page: Some(2),
            first_block: at.is_none().then(|| ("book.md".to_owned(), 5)),
            message: String::new(),
        };
        let warnings = [
            warning("heading-at-foot", Some(5)),
            warning("page-short", None),
            warning("loose-line", Some(5)),
        ];
        let all = prompt(&book, &warnings, None);
        assert!(all.contains(
            "heading-at-foot (1):\n- book.md:5 page 2, its heading is at line 3: \"The lamps were lit along the\"\n"
        ));
        assert!(all.contains(
            "page-short (1):\n- page 2, its first block is book.md:5, the block before it is at line 3: \"The lamps were lit \
             along the\"\n"
        ));
        let some = prompt(&book, &warnings, Some(2));
        assert!(
            some.contains("These are 2 of 3") && !some.contains("page-short ("),
            "whole pages come last"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
