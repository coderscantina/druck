---
title: Notes, marks, and comments
author: Ada Example
lang: en
---

<!-- A comment on its own line leaves nothing behind, even when it runs over
several lines.

It may hold a blank line, `code`, and {todo: nothing that shows}. -->

This sample shows the inline marks and the notes that the author keeps in the source. Text can be ~~struck out~~, set in ^superscript^ or ~subscript~, [underlined]{.underline}, and set in [small capitals]{.smallcaps}. A mark may sit inside a word: water is H~2~O, the area is 4 m^2^, and the ordinal is 3^rd^. They combine with emphasis, **strong ~~text~~**, and [links](https://spec.commonmark.org/), so that [*[marked]{.underline} links*](https://spec.commonmark.org/) work as well.

A mark that crosses a line break keeps its line on both lines, and hyphenation still applies inside it: ~~the quick brown fox jumped over the lazy dogs and kept running through the incomprehensibly extraordinary night~~, then [underlined text of about the same length, so that its line runs on across the spaces between words and over the break]{.underline}.

<!-- A comment between paragraphs is the same as a blank line. -->

The next paragraph starts with the usual first-line indent, because the comment above changes nothing. A comment <!-- inline, even
across lines --> inside a sentence is cut out, so that wor<!-- mid -->ds join and `<!-- code -->` stays code.

{todo: Replace this figure with the final one}

Notes to the author show in the output until they are done. {todo: Check the date of the first edition} The sentence goes on after the note, and statistics ignore its words. Some characters stay text: ~/dir, 2^10 and a ~ b ~ c, and \~escaped\~.

# Small capitals and lists

- [Small capitals]{.smallcaps} in a list item, H~2~SO~4~, and E = mc^2^.
- A struck item, ~~no longer true~~, with a {todo: confirm with Ines} note.

> A quotation with ^raised^ and ~lowered~ text, and [underlined]{.underline} words.
