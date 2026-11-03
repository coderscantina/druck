//! HTML comments, `<!-- ... -->`, which a manuscript uses for notes to its author.
//!
//! They are cut out of the text before parsing, as if they had not been written, so a comment on a line of
//! its own leaves a blank line. Comments inside code stay code. [`Cuts`] maps offsets in the remaining text
//! back to the written text for diagnostics.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

/// The comments of a Markdown text.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Comments {
    /// The byte ranges of the comments in order. An unterminated one runs to the end of the text.
    removed: Vec<Range<usize>>,
    /// Where a comment starts that is never closed.
    pub unterminated: Option<usize>,
}

/// Finds the comments in `body`. Text without `<!--` is not parsed.
pub fn find(body: &str, options: Options) -> Comments {
    let mut comments = Comments::default();
    if !body.contains("<!--") {
        return comments;
    }
    let code: Vec<Range<usize>> = Parser::new_ext(body, options)
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Start(Tag::CodeBlock(_)) | Event::Code(_)).then_some(range))
        .collect();
    let mut from = 0;
    while let Some(found) = body[from..].find("<!--") {
        let start = from + found;
        let before = code.partition_point(|block| block.start <= start);
        if let Some(block) = before
            .checked_sub(1)
            .map(|last| &code[last])
            .filter(|block| block.end > start)
        {
            from = block.end;
            continue;
        }
        let backslashes = body[..start].len() - body[..start].trim_end_matches('\\').len();
        if backslashes % 2 == 1 {
            from = start + 1;
            continue;
        }
        let Some(length) = body[start + "<!--".len()..].find("-->") else {
            comments.removed.push(start..body.len());
            comments.unterminated = Some(start);
            break;
        };
        let end = start + "<!--".len() + length + "-->".len();
        comments.removed.push(start..end);
        from = end;
    }
    comments
}

impl Comments {
    /// `body` without the comments, and the offset map for it.
    pub fn strip<'a>(&self, body: &'a str) -> (Cow<'a, str>, Cuts) {
        if self.removed.is_empty() {
            return (Cow::Borrowed(body), Cuts::default());
        }
        let mut text = String::with_capacity(body.len());
        let (mut end, mut cuts, mut removed) = (0, Vec::with_capacity(self.removed.len()), 0);
        for comment in &self.removed {
            text.push_str(&body[end..comment.start]);
            removed += comment.len();
            cuts.push((text.len(), removed));
            end = comment.end;
        }
        text.push_str(&body[end..]);
        (Cow::Owned(text), Cuts(cuts))
    }
}

/// Where comments were cut from a text: the offset in the remaining text and the bytes cut up to there.
#[derive(Debug, Default)]
pub struct Cuts(Vec<(usize, usize)>);

impl Cuts {
    /// The offset in the written text of `offset` in the remaining text. An offset where a comment was cut
    /// is the text after it.
    pub fn original(&self, offset: usize) -> usize {
        let cut = self.0.partition_point(|&(at, _)| at <= offset);
        offset + cut.checked_sub(1).map_or(0, |last| self.0[last].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONS: Options = Options::ENABLE_TABLES;

    fn stripped(body: &str) -> String {
        find(body, OPTIONS).strip(body).0.into_owned()
    }

    #[test]
    fn cuts_block_inline_and_multi_line_comments_but_not_code() {
        assert_eq!(stripped("One.\n\n<!-- note -->\n\nTwo.\n"), "One.\n\n\n\nTwo.\n");
        assert_eq!(stripped("a <!-- x --> b<!-- y -->c"), "a  bc");
        assert_eq!(stripped("a <!-- one\n\ntwo --> b\n"), "a  b\n");
        assert_eq!(
            stripped("`<!-- x -->` and \\<!-- y -->"),
            "`<!-- x -->` and \\<!-- y -->"
        );
        assert_eq!(
            stripped("```\n<!-- x -->\n```\n\n    <!-- y -->\n"),
            "```\n<!-- x -->\n```\n\n    <!-- y -->\n"
        );
    }

    #[test]
    fn reports_where_an_unterminated_comment_starts_and_cuts_the_rest() {
        let comments = find("Text <!-- never\nclosed\n", OPTIONS);
        assert_eq!(comments.unterminated, Some(5));
        assert_eq!(comments.strip("Text <!-- never\nclosed\n").0, "Text ");
    }

    #[test]
    fn maps_offsets_back_to_the_written_text() {
        let body = "ab<!--xy-->cd<!--z-->ef";
        let (text, cuts) = find(body, OPTIONS).strip(body);
        assert_eq!(text, "abcdef");
        let written: Vec<usize> = (0..=text.len()).map(|offset| cuts.original(offset)).collect();
        assert_eq!(written, [0, 1, 11, 12, 21, 22, 23]);
    }
}
