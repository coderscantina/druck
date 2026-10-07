---
title: Breaking pages
author: Ada Example
lang: en
---

# Breaking pages

A page break is a decision about the whole document, not about one line. Moving a single line from one page to the next changes where every later page begins, so a good choice for one page can force a poor one three pages later. Kyber therefore weighs all candidate breaks together, much as it weighs the line breaks of a paragraph.[^weigh]

The rules it follows are old ones. A heading should never be the last thing on a page. The first line of a paragraph should not sit alone at the bottom of a page, and its last line should not sit alone at the top of the next. Pages should end at the same height, and when they cannot, the difference should be absorbed by the space between blocks rather than by stretching the lines of a paragraph apart.

[^weigh]: The search keeps, for every place a page may end, the cheapest way of reaching it. It looks at most one page ahead from each such place, so the work grows with the length of the document.

## Orphans and widows

Typesetters call a first line stranded at the foot of a page an orphan and a last line stranded at the head of a page a widow. Both break the shape of the paragraph and make the reader hunt for the continuation. The traditional remedy is to move a line, to run a page one line short, or to let the spacing between blocks absorb the difference.

None of these remedies is free. A short page leaves a visible gap at the foot of the text area, and a page with stretched spacing looks looser than its neighbours. A page-breaking method has to compare these costs and choose the least objectionable one, and it has to do so consistently, so that the same document always breaks the same way.[^stable]

[^stable]: Repeated renders of the same input produce identical files. Nothing in the choice of breaks depends on timing, hashing, or the order in which files happen to be read.

Kyber gives each break a cost. A break between two blocks costs nothing by itself. A break inside a paragraph costs a little, and a break that strands a single line costs a great deal. A page whose content ends short of the bottom costs more the further it falls short, and the space above headings, lists, quotations, and code may grow by half of its natural height to close the gap.

The final page of a document may be as short as it needs to be. So may a page that ends at an explicit page break, because the author asked for the break there.

## Headings at the foot of a page

A heading announces what follows, so it belongs with the text it introduces. If a heading would otherwise land at the foot of a page, it moves to the next page together with at least the first two lines of its text. The page it leaves behind runs short, which is the lesser evil.

This paragraph and the ones around it are long enough that, depending on the page size and margins, one of the headings in this sample will fall near a page boundary. Render the sample with different margins to watch the heading move with its text instead of being left behind: try `--set margins=20mm` or `--set margins=35mm`.

When a page runs short because a heading moved on, or because a long note took the room,[^long] the spaces between the blocks on that page may stretch a little to bring the last line closer to the bottom. They never stretch by more than half their natural height, so a short page stays visibly short rather than visibly distorted.

## Footnotes

Footnotes are numbered in the order of their references.[^order] Each note starts on the page that holds its reference, at the foot of that page, below a short rule. Because the notes take space from the text area, a note can change where a page ends: when a reference falls near the foot of a page and its note does not fit below, the line with the reference moves to the next page together with its note.[^space]

[^order]: A note may be defined anywhere in the document, before or after its reference. Only the order of the references matters.

[^space]: Notes are part of page composition from the start. They are not added after the breaks have been chosen, which would leave the choice of breaks blind to their height.

A long note may not fit on the page of its reference at all. Then it begins there and continues at the foot of the next page, where a marker with its number repeats so that the reader can tell which note is being continued. Note 3 in this sample is such a note.

[^long]: This note is deliberately long, so that it cannot fit at the foot of the page that holds its reference. Long notes are common in scholarly writing, where a note may carry a digression, a quotation from a source, or a discussion of the literature that would interrupt the argument of the main text. Typesetters have always had to break such notes across pages. The usual convention is to start the note on the page of its reference, so that the reader finds it where expected, and to continue it at the foot of the following page.

    A continued note needs a signal. Without one, the reader who turns the page sees a note without a number and may take it for part of the note that follows. Kyber repeats the note number at the start of the continuation and adds the word given by the theme's `continued` label, which is "(continued)" in English and "(Fortsetzung)" in German.

    Notes of this length raise the question of where a note should break. Kyber breaks a note between two of its lines, wherever the foot of the page is reached. It does not yet apply widow and orphan rules inside notes, so a continued note may begin or end with a single line on either page. In practice the choice of the main text's break usually leaves enough room that this does not happen.

    A note this long is also a test of the footnote area itself. The area grows upward from the foot of the page, the short rule sits at its top, and the main text above must stop at least the theme's footnote gap above the rule. When the area grows, the main text gives way line by line, and the search for page breaks sees every one of those lines.

    Scholarly editions sometimes carry notes longer than the text they annotate. A commentary on a single sentence can run to a page or more, and an apparatus of variant readings can fill the foot of every page in a volume. Such documents are the hardest case for a page-breaking method, because the notes, not the main text, decide how much of each page is left for the argument. They are also the case where careless breaking is most visible: a note that starts two pages after its reference, or a continuation that the reader cannot connect to its beginning, sends the reader searching back and forth through the book.

    The rest of the note continues here. Only the last note on a page may continue, because notes keep their order: a note that has not finished cannot be followed by the next one on the same page. Kyber also prefers not to split a note at all when moving a few lines of the main text to the next page would make room for the whole note.

The text after a long note continues normally. The space the note takes on the following page is simply unavailable to the main text there, and the breaks on that page are chosen with it in mind.

## Keeping blocks together

Some passages read badly when divided. A short definition followed by its example, a question and its answer, or a list and the sentence that introduces it can be wrapped in a keep group:

::: keep
**Definition.** A keep group is a sequence of blocks that Kyber places on one page. If the group does not fit in the space left on the current page, the whole group starts on the next page.

**Example.** This paragraph and the definition above it form one keep group. They always appear on the same page, even when that leaves the previous page short.

- A keep group may contain lists,
- quotations, and code,
- as long as the whole group fits on one page.
:::

A keep group taller than a page cannot be honoured. Kyber does not split it silently; it reports the group's line and the heights involved, and writes no PDF until the document is changed.

## Spacing between blocks

Paragraphs that follow one another are separated by an indent, not by space, so a page of plain prose has no room to stretch. Its last line ends wherever the line grid ends, at most one line short of the bottom. Pages with headings, lists, quotations, or code have space between blocks, and that space can grow a little to bring the last line to the bottom.

The growth is bounded. Each space may grow by half of its natural height and no more. A page that would need more than that runs short instead, and the cost of running short grows with the square of the missing height, so the search prefers several pages that are each slightly short over one page with a large gap.

> A quotation is set apart by space above and below it. That space is one of the places where a page can absorb a difference in height without anyone noticing.

Code blocks behave the same way:

```
page cost = break penalty
          + 100 * (stretch used)^3
          + 1000 * (lines short)^2
```

The constants are internal for now. They were chosen by rendering the samples and looking at the pages, and they may change when columns, images, and tables join the page composition in later milestones.

Lists are another source of flexible space. The space around a list and between its items is part of the flow, and a break between two items is as good as a break between two paragraphs:

1. A break between list items costs nothing.
2. A break inside an item's paragraph costs as much as one inside any other paragraph.
3. An item's first line is never left alone at the foot of a page if a better break exists.

The same rules apply in quotations and in the notes themselves, with one difference: notes are broken only at the foot of a page, where they would otherwise not fit, and only the last note on a page may be broken.

::: page-break

## An explicit page break

This section begins on a new page because the source has a page-break directive before its heading. The page before it may end short without penalty, because the author asked for the break.

Explicit breaks are useful before appendices, after a title block, or wherever the structure of a document calls for a fresh page. They are not needed to keep a heading with its text or to avoid a stranded line; Kyber does that on its own.[^own]

[^own]: An explicit break overrides the other rules at the place where it stands. A page break directly after a heading is honoured, although it separates the heading from its text, because the author asked for it.

A document usually needs very few of them.
