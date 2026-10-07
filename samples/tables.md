---
title: Tables
author: Ada Example
lang: en
---

# Tables

A table is written as a pipe table. The delimiter row below the header sets the alignment of each column: a colon on the left aligns left, on the right aligns right, and on both sides centers. A paragraph that starts with a colon and a space, directly after the table, is its caption. Tables are numbered apart from figures.

| Setting | Default | Effect |
|:--|:-:|:--|
| `cell-padding` | 0.4em | Space between a cell's text and the rules around it. |
| `rule-thickness` | 0.4pt | Thickness of the rules above and below the header and below each row. |
| `rule-color` | `#b5b5b5` | Color of all rules. |

: The theme settings for tables.

Column widths come from the text of the cells. A table whose cells fit on one line keeps its natural width and is centered, as the one above. A wider table fills the text width: narrow columns stay on one line, and the wide ones share the rest.[^widths]

[^widths]: Each wide column gets the same width, but never less than its longest word needs. A word that cannot fit even then is reported at its cell.

## A long table

The table below continues on the next page. Pages break only between rows, never inside one, and the header row is repeated at the top of every page the table continues on. The caption stays with the start of the table.

| Year | Event | Notes | Pages |
|--:|:--|:--|--:|
| 1439 | Movable type in Mainz | Gutenberg combines a type mould, oil-based ink, and a screw press. | 0 |
| 1455 | The 42-line Bible | About 180 copies, some on vellum. Each page is set in two columns of 42 lines. | 1 286 |
| 1470 | Jenson's roman | A roman type cut in Venice that still serves as a model for book faces. | 12 |
| 1495 | *De Aetna* | Aldus Manutius prints Bembo's account of climbing Etna in a new roman. | 60 |
| 1501 | Italic type | The first italic, cut by Francesco Griffo for small portable editions of the classics. | 228 |
| 1530 | Garamond | Claude Garamond's romans spread through the French and then the European book trade, and the name later covers faces that have little to do with his. | 340 |
| 1557 | Civilité | Robert Granjon cuts a type modelled on French handwriting. | 48 |
| 1692 | Romain du Roi | A commission of the French Academy of Sciences designs a roman on a fine grid, for the exclusive use of the royal printing office. | 96 |
| 1734 | Caslon | William Caslon's specimen sheet; his types are used for the first printings of the American Declaration of Independence. | 1 |
| 1757 | Baskerville's Virgil | Wove paper, black ink, and sharp contrast. | 432 |
| 1784 | Didot point | François-Ambroise Didot defines the point that French and German printers measure type in. | 2 |
| 1798 | Didone types | Bodoni and Didot push contrast to its limit: hairline serifs and vertical stress. | 504 |
| 1814 | Steam press | The Times is printed on a steam-driven cylinder press by Koenig and Bauer, about 1 100 sheets an hour. | 4 |
| 1816 | Sans serif | Caslon IV shows the first sans serif type, in capitals only. | 1 |
| 1845 | Rotary press | Richard Hoe's rotary press puts the type on a cylinder. | 8 |
| 1886 | Linotype | Ottmar Mergenthaler's machine casts whole lines of type from brass matrices set at a keyboard. Newspapers adopt it within a decade, and hand composition of text ends there.[^linotype] | 16 |
| 1887 | Monotype | Tolbert Lanston separates keyboard and caster: a punched paper ribbon drives the casting of single letters. | 24 |
| 1896 | Kelmscott Chaucer | William Morris prints Chaucer with woodcut borders. | 556 |
| 1916 | Johnston Sans | Edward Johnston's alphabet for the London Underground. | 1 |
| 1927 | Futura | Paul Renner's geometric sans serif. | 32 |
| 1928 | *Die neue Typographie* | Jan Tschichold argues for asymmetric layout and sans serif type; he later turns back to classical book typography. | 240 |
| 1932 | Times New Roman | Designed for the newspaper that gives it its name, with narrow proportions to save space in columns. | 4 |
| 1949 | Phototypesetting | The Lumitype projects letters from a disc onto film. | 12 |
| 1957 | Univers and Helvetica | Two sans serifs in one year; Univers plans a family of 21 weights and widths from the start. | 64 |
| 1965 | Digital type | The Digiset draws letters on a cathode ray tube from stored outlines. | 8 |
| 1978 | TeX | Donald Knuth starts TeX to typeset the second volume of *The Art of Computer Programming* after he sees the results of phototypesetting. Paragraphs are broken as a whole, which is the method Kyber uses. | 700 |
| 1984 | PostScript | A page description language that treats type as outlines. | 2 |
| 1985 | Desktop publishing | The LaserWriter and PageMaker put typesetting on a desk. | 1 |
| 1993 | PDF | Portable Document Format, built on the PostScript imaging model. | 1 |
| 1996 | OpenType | One font format for both outline flavours, with tables for ligatures, kerning, and alternates. | 6 |
| 2010 | WOFF | Fonts for the web. | 1 |

: Five centuries of type and printing, with **more rows** than fit on a page.

[^linotype]: A footnote referenced in a table cell goes on the page of its row, like any other note.

Prose continues after the table. A table is spaced like a figure: the caption style's space after it sets the distance to the text above and below.

## Tables in columns

::: columns
Inside a column section a table takes the width of its column. The short table below fits in the column at its natural width.

| Unit | Points |
|:--|--:|
| inch | 72 |
| pica | 12 |
| cm | 28.35 |
| mm | 2.835 |

: Lengths in points.

A long table in a column continues in the next column, and on the next page, with its header repeated in each column. Balancing places the column break between rows, as it places it between lines of text.

| Size | Use |
|--:|:--|
| 6pt | Footnotes in dense reference works. |
| 7pt | Captions and marginal notes. |
| 8pt | Footnotes. |
| 9pt | Captions, tables, and notes. |
| 10pt | Body text in books with small pages. |
| 10.5pt | Body text of this sample. |
| 11pt | Body text of reports. |
| 12pt | Body text of letters and manuscripts. |
| 14pt | Subheadings. |
| 18pt | Headings. |
| 24pt | Titles. |
| 36pt | Display type and covers. |

: Common type sizes and where they are used.

::: full-width
A full-width table balances the columns above it, runs across the whole text area, and the columns resume below it.

| Style | Font | Size | Weight | Alignment | Hyphenation |
|:--|:--|--:|:-:|:--|:-:|
| `table-cell` | Libertinus Serif | 0.86em | regular | left | no |
| `table-header` | Libertinus Serif | 0.86em | bold | left | no |
| `caption` | Libertinus Serif | 0.86em | regular | left | no |

: The styles a table uses in the default theme.
:::

The columns resume below the full-width table. Cell text uses the paragraph breaker of the body text at the width of its column, so wrapped cells get the same even lines as paragraphs.[^cells] Rows that wrap are as tall as their tallest cell.

[^cells]: Cells are left aligned and not hyphenated in the default theme. A theme can change both in `styles.table-cell`.

The section ends here, and its last columns are balanced.
:::

The paragraph after the column section runs across the full text area again.
