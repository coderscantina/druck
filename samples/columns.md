---
title: Columns on the page
author: Ada Example
lang: en
---

A page may change between one and two columns as often as the text asks for it. This opening paragraph runs across the whole text area. The short section below switches to two columns, and the paragraph after it returns to one, all on the same page. Nothing forces a new page when the layout changes; a page ends only where the page-breaking search finds the best place.

::: columns
Two columns suit short items that a reader scans rather than reads: notes on sources, lists of terms, or a summary of results. The text flows down the first column and continues at the top of the second, so the reading order stays the same as in the source.

When a column section ends, its columns are balanced. Both end at about the same height, and the text that follows starts below the taller one. A column section never stretches its lines apart to fill a page.
:::

The paragraph after a column section returns to the full width of the text area. It starts below the taller of the two columns above it, with the usual space between blocks. A longer column section follows. It holds a heading, notes referenced from both columns, a group kept together, and a block across the full width in its middle.

::: columns
# Reading order

In a two-column section the text runs down the first column, then down the second, then on to the next page. A reader who reaches the foot of the second column turns the page and continues at the top of the first column there. The search for page breaks treats the two columns of a page as one region whose height is the taller column, so a section can begin in the middle of one page and end in the middle of another.[^order]

The columns of a page that the section fills completely end at the foot of the text area, above any notes. Their height is the same as the height of a page of single-column text, so the bottoms of facing pages line up whether they hold one column or two.

[^order]: Notes are numbered in reading order, so a note referenced in the first column comes before a note referenced in the second.

Each column is narrower than the text area, so its lines hold fewer words and hyphenation becomes more frequent. The paragraph breaker sets every line of a column paragraph at the column width and chooses the breaks for the whole paragraph at once, exactly as it does for full-width text. Narrow columns make the quality of those choices visible: a greedy method leaves loose lines and rivers of white space that a reader notices at once.

# Notes in columns

Footnotes do not belong to a column. They share one area across the full width at the foot of the page, below a short rule, wherever their references stand.[^shared] The area takes its height from the page, so a note referenced in either column moves the column bottoms up on that page. A long note can still continue on the next page under its number and the continuation label.

[^shared]: A page with two column sections and a full-width block between them still has one note area, and its notes stay in the order of their references.

The second column holds the reference to this note.[^second] Its number follows the note referenced in the first column, because the first column comes first in reading order.

[^second]: This note is referenced from the second column of its page and set in the same full-width area as the notes from the first column.

::: keep
## A kept group

A keep group inside a column stays in one column. The column break falls before or after it, never inside it. If the group is taller than a column, that is reported at its directive instead of being split or clipped.

The group holds two paragraphs and a heading, so it is a few lines taller than an ordinary paragraph.
:::

The balancing of the last columns of a section has to respect the same rules as a page break. A heading stays at the top of the text it introduces, a keep group stays whole, and a single line of a paragraph is not left alone at the top or the foot of a column if a slightly less even split avoids it.

::: full-width
A full-width block interrupts the columns. The columns before it are balanced, the block runs across the whole text area in its place in the document, and the columns resume below it. Later milestones place wide images and tables this way.
:::

# After the full-width block

The columns resume below the full-width block. The text again runs down the first column and then the second. This part of the section is long enough to need a further column break, and its final columns are balanced again where the section ends.

Balancing chooses the column break that makes the two columns most even. When the most even break would strand a line, a break one line less even is accepted instead. A break two lines less even is not, because columns of visibly different heights are a worse fault than a single stranded line.

A column region is never stretched to fill its page beyond the bounds that apply to all space between blocks. If the columns of a page cannot reach the foot of the text area within those bounds, the page runs short, and the search for page breaks weighs that shortfall like any other.
:::

The document ends with a paragraph across the full width again. Its first line starts below the taller of the two columns above it.
