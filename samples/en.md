---
title: On quiet typography
author: Ada Example
lang: en
---

A well-set page asks for nothing. The reader should find the first line of a paragraph without effort, follow the text from line to line without losing the place, and reach the end of a chapter without noticing that a typesetter was ever involved. Typography of this kind is not decoration; it is the careful removal of friction.

This sample exercises the first rendering path of *Druck*: headings, paragraphs, **strong** and *emphasized* text, ***both at once***, inline `code`, and [a link to the CommonMark specification](https://spec.commonmark.org/). Ligatures such as fi, fl, ff, ffi, and ffl should appear in words like *office*, *affluent*, *difficult*, and *find*.

# Paragraphs and their rhythm

The traditional book paragraph starts without an indent after a heading and with an indent when it follows another paragraph. The indent is the only signal of a new paragraph, so no extra space separates them. Readers have relied on this convention for centuries, and it keeps the text block calm and even.

Line breaks are chosen for the whole paragraph at once. A greedy method fills each line as far as it can and cannot foresee that this leaves the next line loose; weighing all breaks together avoids that. Hyphenation of long words such as *characteristically*, *incomprehensibility*, or *extraordinarily* gives the method more room in narrow measures. Punctuation at the edge of a justified line, like a hyphen, a comma, or a closing quotation mark, hangs slightly into the margin, so the edge looks straight.

A line can also be broken by hand.\
This line follows a hard break and starts at the left edge.

## Lists

Lists are set with hanging markers:

- Bulleted items use the first bullet of the theme.
- An item may contain more than one sentence. When it does, the text wraps beneath the start of the item text rather than beneath the marker, so the marker stays visible at the edge.
  - Nested lists use the next bullet.
  - They are indented by another step.
- And back to the outer level.

Ordered lists keep their numbering:

3. Third, because the list starts at three.
4. Fourth.
5. Fifth, with `inline code` inside the item.

## Quotations

> The whole duty of typography, as of calligraphy, is to communicate to the imagination, without loss by the way, the thought or image intended to be communicated by the Author.
>
> A quotation may contain *emphasis*, which turns upright inside italic text.

## Code

Code is set in the monospaced face and never wrapped:

```rust
fn main() {
    let greeting = "Hello, typography";
    println!("{greeting}");
}
```

# Pages

A document of a few pages shows how text flows from one page to the next. The margins of odd and even pages mirror each other, so the inner margin sits next to the binding. Page breaks are chosen for the whole document: a heading moves to the next page with its text rather than being left alone at the bottom of a page.

Short lines at the end of a paragraph are allowed. Single lines stranded at the top or bottom of a page are avoided where a better break exists. The text below repeats a few paragraphs so that the document spans several pages.

The design of a page begins with its proportions. A text block that is too wide tires the eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the text into fragments and forces frequent hyphenation. Most book typographers settle on a measure of about sixty to seventy characters, which this default theme approaches at its body size.

Margins frame the text block and give the hands somewhere to hold the page. Classical proportions give the outer and lower margins more space than the inner and upper ones. The default theme keeps them simple and symmetric, leaving the choice to the theme author.

Leading, the distance between baselines, determines how dark and dense a page appears. Too little leading makes lines run into each other; too much leading makes the text fall apart into separate strips. The body text of this theme uses a moderate leading that suits the open forms of the serif face.

The design of a page begins with its proportions. A text block that is too wide tires the eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the text into fragments and forces frequent hyphenation. Most book typographers settle on a measure of about sixty to seventy characters, which this default theme approaches at its body size.

Margins frame the text block and give the hands somewhere to hold the page. Classical proportions give the outer and lower margins more space than the inner and upper ones. The default theme keeps them simple and symmetric, leaving the choice to the theme author.

Leading, the distance between baselines, determines how dark and dense a page appears. Too little leading makes lines run into each other; too much leading makes the text fall apart into separate strips. The body text of this theme uses a moderate leading that suits the open forms of the serif face.

The design of a page begins with its proportions. A text block that is too wide tires the eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the text into fragments and forces frequent hyphenation. Most book typographers settle on a measure of about sixty to seventy characters, which this default theme approaches at its body size.

Margins frame the text block and give the hands somewhere to hold the page. Classical proportions give the outer and lower margins more space than the inner and upper ones. The default theme keeps them simple and symmetric, leaving the choice to the theme author.
