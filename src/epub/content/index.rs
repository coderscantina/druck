//! The index in a reflowable book, marked up with the EPUB Indexes vocabulary. A book has no pages, so an
//! entry links to its marks by the heading each mark follows, as in "Hyphenation, 2 Line breaking", with each
//! heading listed once and linked to the entry's first mark under it.

use super::Writer;
use crate::epub::xml;
use crate::index::Entry;

impl Writer<'_> {
    pub(super) fn index(&mut self) {
        let cx = self.cx;
        let Some(index) = &cx.structure.index else {
            return;
        };
        let heading = &cx.structure.headings[self.headings];
        self.headings += 1;
        let id = self.define(heading.anchor);
        self.push(&format!("<h1 id=\"{id}\">{}</h1>", xml::text(&heading.text)));
        self.title.get_or_insert_with(|| heading.text.clone());
        for group in &index.groups {
            let letter = group.letter.as_deref().unwrap_or(&cx.config.labels.symbols);
            self.push(&format!(
                "<div epub:type=\"index-group\"><h2 class=\"index-letter\">{}</h2>",
                xml::text(letter)
            ));
            self.index_entries(&group.entries, "index-entry");
            self.push("</div>");
        }
    }

    fn index_entries(&mut self, entries: &[Entry], class: &str) {
        self.push("<ul class=\"index\" epub:type=\"index-entry-list\">");
        for entry in entries {
            self.index_entry(entry, class);
        }
        self.push("</ul>");
    }

    fn index_entry(&mut self, entry: &Entry, class: &str) {
        let (cx, structure) = (self.cx, self.cx.structure);
        self.push(&format!(
            "<li class=\"{class}\" epub:type=\"index-entry\"><span epub:type=\"index-term\">{}</span>",
            xml::text(&entry.term)
        ));
        let mut headings: Vec<Option<usize>> = Vec::new();
        for &mark in &entry.marks {
            let heading = structure.mark_headings[mark];
            if headings.contains(&heading) {
                continue;
            }
            let separator = if headings.is_empty() {
                &cx.config.index.separator
            } else {
                ", "
            };
            headings.push(heading);
            // A mark before the first heading is under the book's title, or else under its own term.
            let text = match heading {
                Some(heading) => structure.headings[heading].title(),
                None => (cx.config.metadata.title.clone()).unwrap_or_else(|| entry.term.clone()),
            };
            let href = self.href(structure.marks[mark]);
            self.push(&format!(
                "{}<a epub:type=\"index-locator\" href=\"{}\">{}</a>",
                xml::text(separator),
                xml::attribute(&href),
                xml::text(&text)
            ));
        }
        let labels = &cx.config.labels;
        let mut referred = !headings.is_empty();
        for (label, targets, kind) in [
            (&labels.see, &entry.see, "index-xref-preferred"),
            (&labels.see_also, &entry.see_also, "index-xref-related"),
        ] {
            if targets.is_empty() {
                continue;
            }
            let separator = if referred { "; " } else { &cx.config.index.separator };
            referred = true;
            let targets: Vec<String> = (targets.iter())
                .map(|target| format!("<span epub:type=\"{kind}\">{}</span>", xml::text(target)))
                .collect();
            self.push(&format!(
                "{}<em>{}</em> {}",
                xml::text(separator),
                xml::text(label),
                targets.join("; ")
            ));
        }
        if !entry.sub_entries.is_empty() {
            self.index_entries(&entry.sub_entries, "index-sub-entry");
        }
        self.push("</li>");
    }
}
