//! The logical structure of laid out content, for tagged PDF.
//!
//! Layout adds an element for each block and inline that carries meaning, in document order, and marks
//! the page items that are its content with [`Item::Begin`] and [`Item::End`]. Content outside every
//! element is decoration: running heads, page numbers, rules, ornaments, and the watermark. Paragraph
//! layout marks links and footnote references with [`Mark::Link`] and [`Mark::Note`]; [`Tree::tag`] turns
//! them into elements of their own when the line is placed. [`crate::pdf`] maps each [`Kind`] to a
//! standard structure type in one function, `pdf::structure_type`.

use crate::document::{Link, Location};
use crate::page::Item;

/// An element of a [`Tree`]. Elements are numbered in document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(usize);

impl Id {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

/// What an element is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// The title slot of a title block or title page.
    Title,
    Paragraph,
    /// A heading of `level` 1 to 6, with its number and text.
    Heading {
        level: u8,
        text: String,
    },
    List {
        ordered: bool,
    },
    ListItem,
    /// The marker of a list item.
    Label,
    ListBody,
    Quote,
    Code,
    /// An image with the text of its Markdown description, empty for none, and where it is written.
    Figure {
        alt: String,
        at: Location,
    },
    Caption,
    Table,
    TableRow,
    /// A table cell spanning `span` columns, in the header row or not.
    TableCell {
        header: bool,
        span: usize,
    },
    /// A footnote, which follows its reference in the reading order.
    Note,
    /// A footnote reference.
    Reference,
    Link,
    Contents,
    ContentsEntry,
    BibEntry,
    /// A callout with its label and content.
    Callout,
    /// The generated index, after its heading.
    Index,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Kind,
    /// `None` for an element of the document itself.
    pub parent: Option<Id>,
}

/// What an [`Item::Begin`] starts.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// The content of an element.
    Element(Id),
    /// Link text, from paragraph layout. Becomes a [`Kind::Link`] when the line is tagged.
    Link(Link),
    /// The reference to footnote `index`, from paragraph layout. Becomes a [`Kind::Reference`] followed by
    /// the note's [`Kind::Note`] when the line is tagged.
    Note(usize),
    /// Decoration inside an element's content, such as leader dots.
    Artifact,
    /// Content of the current element that the next content continues without a word break, as a drop
    /// capital continues into the rest of its word.
    Joined,
}

/// The elements of a document, with the note element of each footnote that has been referenced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
    notes: Vec<(usize, Id)>,
    /// The note element of each margin note that a tagged line has referenced.
    margin_notes: Vec<(usize, Id)>,
    /// The link the last tagged line ended in: its line's element, target, and link element. A link that
    /// wraps continues it on the next line.
    open_link: Option<(Id, Link, Id)>,
}

impl Tree {
    /// Adds an element of `kind` after the elements added so far.
    pub fn add(&mut self, kind: Kind, parent: Option<Id>) -> Id {
        self.nodes.push(Node { kind, parent });
        Id(self.nodes.len() - 1)
    }

    /// The note element of footnote `index`, once a tagged line has referenced it.
    pub fn note(&self, index: usize) -> Option<Id> {
        self.notes.iter().find(|(note, _)| *note == index).map(|(_, id)| *id)
    }

    /// Adds the note element of margin note `index` to `items`, a line tagged as content of `element`. A
    /// margin note has no mark in the line, so an empty mark at the line's end places it in the reading order.
    pub fn add_margin_note(&mut self, index: usize, element: Id, items: &mut Vec<Item>) {
        let note = self.add(Kind::Note, Some(element));
        self.margin_notes.push((index, note));
        let end = items.len() - usize::from(matches!(items.last(), Some(Item::End)));
        items.splice(end..end, [Item::Begin(Mark::Element(note)), Item::End]);
    }

    /// The note element of margin note `index`, once a tagged line has referenced it.
    pub fn margin_note(&self, index: usize) -> Option<Id> {
        self.margin_notes
            .iter()
            .find(|(note, _)| *note == index)
            .map(|(_, id)| *id)
    }

    /// Marks `items` as content of `element`, turning link and note marks into elements inside it. With
    /// `notes` false, note marks are dropped and their numbers stay plain content, as in a footnote's own
    /// number. Without an element, link and note marks are dropped and the rest is kept, so unmarked items
    /// stay decoration.
    pub fn tag(&mut self, element: Option<Id>, items: Vec<Item>, notes: bool) -> Vec<Item> {
        let mut tagged = Vec::with_capacity(items.len() + 2);
        let continued = self.open_link.take();
        if let Some(element) = element {
            tagged.push(Item::Begin(Mark::Element(element)));
        }
        // Whether each open mark is kept, and the link it opened.
        let mut open: Vec<(bool, Option<(Link, Id)>)> = Vec::new();
        let mut last_text: Option<(Link, Id)> = None;
        let mut started = false;
        for item in items {
            match item {
                Item::Begin(Mark::Link(target)) => {
                    let Some(element) = element else {
                        open.push((false, None));
                        continue;
                    };
                    let id = match &continued {
                        Some((line, link, id)) if !started && *line == element && *link == target => *id,
                        _ => self.add(Kind::Link, Some(element)),
                    };
                    tagged.push(Item::Begin(Mark::Element(id)));
                    open.push((true, Some((target, id))));
                }
                Item::Begin(Mark::Note(index)) => match element.filter(|_| notes) {
                    Some(element) => {
                        let reference = self.add(Kind::Reference, Some(element));
                        let note = self.add(Kind::Note, Some(element));
                        self.notes.push((index, note));
                        tagged.push(Item::Begin(Mark::Element(reference)));
                        open.push((true, None));
                    }
                    None => open.push((false, None)),
                },
                Item::Begin(mark) => {
                    tagged.push(Item::Begin(mark));
                    open.push((true, None));
                }
                Item::End => {
                    if open.pop().is_none_or(|(kept, _)| kept) {
                        tagged.push(Item::End);
                    }
                }
                item => {
                    if !matches!(item, Item::Anchor { .. } | Item::Link { .. }) {
                        started = true;
                        last_text = open.iter().rev().find_map(|(_, link)| link.clone());
                    }
                    tagged.push(item);
                }
            }
        }
        if let Some(element) = element {
            tagged.push(Item::End);
            self.open_link = last_text.map(|(link, id)| (element, link, id));
        }
        tagged
    }
}

/// `items` without marks, so all of them are decoration, as in a table header repeated on a later page.
pub fn untag(items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .filter(|item| !matches!(item, Item::Begin(_) | Item::End))
        .collect()
}

/// Inserts `item` at the start of the content of the element that `items` begin with, or at the start.
pub fn prepend(items: &mut Vec<Item>, item: Item) {
    let at = usize::from(matches!(items.first(), Some(Item::Begin(_))));
    items.insert(at, item);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::values::{Color, Pt};
    use crate::page::Rect;

    /// A drawn item, standing in for text.
    fn content() -> Item {
        let rect = Rect {
            x: Pt(0.0),
            y: Pt(0.0),
            width: Pt(1.0),
            height: Pt(1.0),
        };
        Item::Rect {
            rect,
            color: Color([0, 0, 0]),
        }
    }

    fn marks(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Begin(Mark::Element(id)) => format!("<{}", id.0),
                Item::Begin(mark) => format!("<{mark:?}"),
                Item::End => ">".to_owned(),
                _ => "x".to_owned(),
            })
            .collect()
    }

    #[test]
    fn turns_inline_marks_into_elements_and_continues_a_wrapped_link() {
        let mut tree = Tree::default();
        let paragraph = tree.add(Kind::Paragraph, None);
        let url = Link::Url("https://example.com".to_owned());
        let link = |items: Vec<Item>| {
            let mut marked = vec![Item::Begin(Mark::Link(url.clone()))];
            marked.extend(items);
            marked.push(Item::End);
            marked
        };
        let mut first = vec![content(), Item::Begin(Mark::Note(3)), content(), Item::End];
        first.extend(link(vec![content()]));
        let tagged = tree.tag(Some(paragraph), first, true);
        assert_eq!(marks(&tagged), ["<0", "x", "<1", "x", ">", "<3", "x", ">", ">"]);
        assert_eq!(tree.nodes[1].kind, Kind::Reference);
        assert_eq!(tree.note(3), Some(Id(2)));
        assert_eq!(tree.nodes[2].parent, Some(paragraph));
        assert_eq!(tree.nodes[3].kind, Kind::Link);

        // The first line ended inside the link, so the next line continues it, but only from its start.
        let mut second = link(vec![content()]);
        second.push(content());
        second.extend(link(vec![content()]));
        let tagged = tree.tag(Some(paragraph), second, true);
        assert_eq!(marks(&tagged), ["<0", "<3", "x", ">", "x", "<4", "x", ">", ">"]);

        // Without an element, inline marks go and the content stays decoration.
        let decoration = vec![Item::Begin(Mark::Note(4)), content(), Item::End];
        assert_eq!(marks(&tree.tag(None, decoration, true)), ["x"]);
        assert_eq!(tree.note(4), None);
    }
}
