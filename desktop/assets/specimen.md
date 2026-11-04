---
title: A short essay on setting type
subtitle: A specimen of every element in a theme
author: Ada Example
date: 4 November 2026
lang: en
meta:
  offer: "2026-00"
abstract: |
  This document is a specimen. It uses every element that Druck can set, so a theme author sees all styles of a theme at once.

  The text is a short essay about typesetting, which is also what Druck does.
---

::: toc

# Why type matters

Good typesetting is hard to notice. A reader of a well set page does not think about the font, the margins, or the spacing between lines. They read. When something goes wrong, a loose line or a heading stranded at the foot of a page, the reader stops, and the spell breaks.

The first paragraph after a heading has no indent. This one is the second, so it starts with the first-line indent of the body style. It has *emphasis*, **strong text**, `inline code`, and a [link to the CommonMark spec](https://spec.commonmark.org/). Quotes are set typographically, as in "Peter's 'best' day", a range reads 1455--1457, and a pause---like this one---uses a long dash. An ellipsis comes out right too...

Text can be ~~struck out~~, set in ^superscript^ or ~subscript~, [underlined]{.underline}, and set in [small capitals]{.smallcaps}. The area is 4 m^2^, and water is H~2~O. A footnote sits at the foot of the page.[^foot] A note to the author stays visible until it is done: {todo: Check the date of the first edition}.

[^foot]: A footnote is numbered in the order of its references and set below a short rule at the foot of the page that holds its reference.

This essay has {words} words and takes about {reading-time} minutes to read. A hard line break\
starts a new line inside the same paragraph.

## A short history

Movable type came to Europe around 1450, and the craft changed little for four hundred years.{^history} Typesetters picked metal letters from a case, set them in a stick, and locked whole pages into a frame. Every line was justified by hand, with thin pieces of metal between the words.

### Hot metal

In 1886 the Linotype machine began to cast whole lines at once. Newspapers adopted it within a decade.

#### Phototypesetting

Letters were projected from a disc onto film in the 1950s.

##### Digital type

Outlines replaced metal and film. Fonts became files.

###### The present

Today a layout engine breaks the lines of a whole paragraph at once, as Knuth and Plass described in 1981.

# Quotations and callouts

A block quotation sets a longer passage apart from the text around it.

> Typography exists to honor content. The reader should find the text easy to enter and hard to leave, and should never be asked to admire the machinery.
>
> A second paragraph in the same quotation, with *emphasis* and a ~~struck~~ word.

Five callouts follow, one for each kind that GitHub alerts know.

> [!NOTE]
> Page references can move the page they name, so the layout is run again until all of them agree.

> [!TIP]
> Write the content first. Choose a theme afterwards, and change it with a single line.

> [!IMPORTANT]
> A callout may hold any block that a quotation holds, such as a list:
>
> - paragraphs,
> - lists,
> - code.

> [!WARNING]
> A word that is wider than its line, even after hyphenation, is an error and not a silent overflow.

> [!CAUTION]
> The next section is full of lists, code, and a table. Check them in every theme you write.

# Lists and code

A bullet list:

- Hot metal, cast line by line
- Phototypesetting, projected on film
- Digital type, drawn from outlines
- Layout engines, which break paragraphs as a whole

A numbered list that starts at three:

3. Choose a typeface for the body text.
4. Set the size and the line height.
5. Test the result with real text.

A nested list:

- Typefaces
  - Serif faces, for long texts
  - Sans serif faces, for screens
    - Geometric designs
    - Humanist designs
- Page layout
  1. Margins
  2. Columns
  3. Running headers

A fenced code block with a language, so the highlighting shows:

```rust
use std::collections::HashMap;

/// Counts how often each word occurs in a text.
fn count(text: &str) -> HashMap<&str, u32> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        *map.entry(word).or_insert(0) += 1;
    }
    map // 42 words
}
```

```json
{ "name": "specimen", "draft": false }
```

A block without a language is set plain:

```
No highlighting here.
```

An indented code block works too:

    fn main() {}

# Tables

A pipe table has a header row and an alignment for each column. The caption follows the table and is numbered.

| Typeface | Year | Designer | Class |
|:--|--:|:--|:-:|
| Garamond | 1530 | Claude Garamond | serif |
| Baskerville | 1757 | John Baskerville | serif |
| Futura | 1927 | Paul Renner | sans |
| Univers | 1957 | Adrian Frutiger | sans |

: Four typefaces and their designers. {#tbl:faces}

@tbl:faces lists four faces, and it starts on [@tbl:faces, page]. A list table holds lists in its cells and can span columns:

::: table {align="left right" widths="* auto"}
- - Item
  - Amount
- - {span=2} Design
- - Theme for the corporate design

    - Colors and typefaces
    - Cover and footers
  - 2 280.00
- - Final files
  - 420.00
:::

: A list table with a spanning row.

# Marks in the text

Some elements are not visible in the text, but the reader finds them in the index.[^index] A term is marked where the text treats it{^typesetting}. A mark can also wrap words that stay in the sentence, as in [line breaking]{^} and [the hyphenation patterns]{^hyphenation}, and it can add a sub-entry{^typesetting!history}.

[^index]: The index at the end of this document lists every term marked in the text.

A cross-reference names a heading. @sec:columns shows the same text in two columns.

***

A scene break separates two parts of a text. The paragraph after it has no indent.

# Columns {#sec:columns}

The next section is set in two columns. Text flows down the first column and then the second. When the section ends, both columns are balanced.

::: columns
Two columns suit short items that a reader scans rather than reads. The lines are shorter, so a paragraph breaker has less room, and hyphenation matters more.

## A heading in a column

A heading stays at the top of the text it introduces, also in a column. Columns run on to the next page when the section is long enough, with the footnotes of both in a shared area.[^cols]

[^cols]: A footnote referenced in a column goes to the foot of the page, across the full width.

::: keep
A kept group stays whole. It is never split across a column or a page, so a theme author sees the spacing of the group as one block.
:::

> A quotation inside a column takes the column width.

- A list in a column
- Another item

::: full-width
A full-width block interrupts the columns and runs across the whole text area.
:::

The columns resume below the block. Their text goes on and ends where both columns are about the same height.
:::

The paragraph after the section is set across the full width again.

# Closing {-}

A heading marked with a dash has no number. Typesetting is a craft of small decisions, each invisible and all of them together the difference between a page that is read and one that is merely looked at.{^typesetting!craft}

::: index
