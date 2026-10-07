---
title: Setting a complete report
subtitle: Structure, numbering, and references
author: [Ada Example, Bo Sample]
date: 7 October 2026
abstract: |
  This report is set from one Markdown file. Its title page, table of contents, numbered headings, running headers, and page numbers come from the theme, and every reference to a section, figure, table, or page is generated.

  The report also combines columns, figures, tables, and footnotes, so the generated numbers have to hold up against a layout that keeps changing while they are written.
lang: en
title-page: true
toc: true
bibliography: references.bib
---

::: page-break

# Introduction {#sec:intro}

A report is more than a sequence of paragraphs. Its reader expects a title page, a table of contents that agrees with the pages, headings numbered in order, and references that lead to the right place. When any of these is set by hand, the next edit breaks it. Druck generates all of them from the source and from the theme, so the author writes `@sec:tables` and the text reads @sec:tables.

The craft behind these expectations is old and well documented. @bringhurst2004 treats them as questions of proportion and rhythm, @forssman2002 collects the details a typesetter has to decide, and @tschichold1928 argued for them at the start of modern typography.

The sections below follow the order in which a reader meets the parts of a report. @sec:columns sets text in two columns with notes from both. @sec:figures holds the figures, and @sec:tables starts with a short table and ends with a long one that runs over several pages. @sec:references explains how the references in this text are resolved, and why the page numbers in them can be trusted. @sec:sources shows how the works behind this text are cited.[^trust]

[^trust]: The page numbers are those of the final layout. @sec:references on [@sec:references, page] describes how they settle.

## What the reader sees first

The title page carries the title, the subtitle, the authors, the date, and the abstract, each in its own slot of the theme. A slot without a value is left out together with the space above it, so a report without a subtitle needs no different theme. How readers meet such a page on paper and on screen is the subject of the essays in @hartmann2015. The table of contents follows on its own page, and the body starts on the page after it.

Every body page has a header with the title of its section and a footer with its page number. Pages are counted from the title page, so the number in the footer is the number a PDF viewer shows for the page.

# Columns {#sec:columns}

A report often holds material that a reader scans rather than reads: lists of terms, short notes on sources, or a summary of results. Two columns suit it. The section below switches to two columns, runs across a page break, and returns to one column at its end.

::: columns
## Reading order

In a two-column section the text runs down the first column, then down the second, then on to the next page. A reader who reaches the foot of the second column turns the page and continues at the top of the first column there. The search for page breaks treats the two columns of a page as one region whose height is the taller column, so a section can begin in the middle of one page and end in the middle of another. Balancing the columns against footnotes and tables is a known hard case [@okafor2022; @schaefer2018].[^order]

[^order]: Notes are numbered in reading order, so a note referenced in the first column comes before a note referenced in the second. Notes that the author writes at the end of a source file but that print at the foot of a page go back to the literate programs of @knuth1984lit.

Each column is narrower than the text area, so its lines hold fewer words and hyphenation becomes more frequent. The paragraph breaker sets every line of a column paragraph at the column width and chooses the breaks for the whole paragraph at once, exactly as it does for full-width text. The method is that of @knuthplass1981, and the hyphenation patterns follow @liang1983. Where a line may break at all is decided by the rules of @unicode16.

## Notes in columns

Footnotes do not belong to a column. They share one area across the full width at the foot of the page, below a short rule, wherever their references stand.[^shared] A long note can still continue on the next page under its number and the continuation label.

[^shared]: A page with two column sections and a full-width block between them still has one note area, and its notes stay in the order of their references.

::: full-width
![A bar chart set across the full width, between two column regions.](images/chart.svg){#fig:wide}
:::

The figure above, @fig:wide, interrupts the columns. The columns before it are balanced, the figure runs across the text area, and the columns resume below it. A reference to it from anywhere in the report reads the same.

Balancing chooses the column break that makes the two columns most even. When the most even break would strand a line, a break one line less even is accepted instead. A column region is never stretched beyond the bounds that apply to all space between blocks.
:::

The section ends with a paragraph across the full width again. Its first line starts below the taller of the two columns above it.

# Figures {#sec:figures}

Figures are numbered in the order they appear, across the whole report. The chart in @sec:columns is @fig:wide. The two figures in this section continue the count, and their labels let the text refer to them before or after they appear. Both drawings reach the PDF as drawing operators of the format that @adobe2006 describes and that is standardised as [@iso32000].

![A landscape drawn as a small raster image.](images/landscape.png){#fig:landscape}

@fig:landscape is a PNG image of 240 by 150 pixels, set at its natural size. @fig:tower below is a tall drawing that shrinks to the height left beside its caption.

![A tall drawing, scaled to fit the page together with its caption.](images/tower.svg){#fig:tower}

An image keeps its place in the text, since there are no floats. When it does not fit in the room left on a page, it moves to the next page with its caption, and the page it leaves ends short.

# Tables {#sec:tables}

Tables are numbered apart from figures. @tbl:settings is short and keeps its natural width. @tbl:history is long: it starts on [@tbl:history, page] and repeats its header on every page it continues on.

| Setting | Default | Effect |
|:--|:-:|:--|
| `numbered-headings` | true | Numbers headings down to `numbering-depth`. |
| `toc` | false | Sets a table of contents before the body. |
| `title-page` | false | Sets the title on a page of its own. |

: Document settings for the structure of a report. {#tbl:settings}

| Year | Event | Notes |
|--:|:--|:--|
| 1439 | Movable type in Mainz | Gutenberg combines a type mould, oil-based ink, and a screw press. |
| 1455 | The 42-line Bible | About 180 copies, some on vellum. Each page is set in two columns of 42 lines. |
| 1470 | Jenson's roman | A roman type cut in Venice that still serves as a model for book faces. |
| 1495 | *De Aetna* | Aldus Manutius prints Bembo's account of climbing Etna in a new roman. |
| 1501 | Italic type | The first italic, cut by Francesco Griffo for small portable editions of the classics. |
| 1530 | Garamond | Claude Garamond's romans spread through the French and then the European book trade. |
| 1557 | Civilité | Robert Granjon cuts a type modelled on French handwriting. |
| 1692 | Romain du Roi | A commission of the French Academy of Sciences designs a roman on a fine grid. |
| 1734 | Caslon | William Caslon's specimen sheet. |
| 1757 | Baskerville's Virgil | Wove paper, black ink, and sharp contrast. |
| 1784 | Didot point | François-Ambroise Didot defines the point that French and German printers measure type in. |
| 1798 | Didone types | Bodoni and Didot push contrast to its limit: hairline serifs and vertical stress. |
| 1814 | Steam press | The Times is printed on a steam-driven cylinder press by Koenig and Bauer. |
| 1816 | Sans serif | Caslon IV shows the first sans serif type, in capitals only. |
| 1845 | Rotary press | Richard Hoe's rotary press puts the type on a cylinder. |
| 1886 | Linotype | Ottmar Mergenthaler's machine casts whole lines of type from brass matrices set at a keyboard.[^linotype] |
| 1887 | Monotype | Tolbert Lanston separates keyboard and caster: a punched paper ribbon drives the casting of single letters. |
| 1896 | Kelmscott Chaucer | William Morris prints Chaucer with woodcut borders. |
| 1916 | Johnston Sans | Edward Johnston's alphabet for the London Underground. |
| 1927 | Futura | Paul Renner's geometric sans serif. |
| 1928 | *Die neue Typographie* | Jan Tschichold argues for asymmetric layout and sans serif type; he later turns back to classical book typography. |
| 1932 | Times New Roman | Designed for the newspaper that gives it its name, with narrow proportions to save space in columns. |
| 1949 | Phototypesetting | The Lumitype projects letters from a disc onto film. |
| 1957 | Univers and Helvetica | Two sans serifs in one year; Univers plans a family of 21 weights and widths from the start. |
| 1965 | Digital type | The Digiset draws letters on a cathode ray tube from stored outlines. |
| 1978 | TeX | Donald Knuth starts TeX to typeset the second volume of *The Art of Computer Programming*. |
| 1984 | PostScript | A page description language that treats type as outlines. |
| 1985 | Desktop publishing | The LaserWriter and PageMaker put typesetting on a desk. |
| 1993 | PDF | Portable Document Format, built on the PostScript imaging model. |
| 1996 | OpenType | One font format for both outline flavours, with tables for ligatures, kerning, and alternates. |
| 2010 | WOFF | Fonts for the web. |

: Five centuries of type and printing. {#tbl:history}

[^linotype]: A footnote referenced in a table cell goes on the page of its row, like any other note.

Prose continues after the table. The caption of @tbl:history stays with its first rows, and its header row is set again at the top of each page it continues on. Its rows follow the history told in [@tschichold1928; @bringhurst2004], and the row for 1978 refers to the program described in @knuth1984. A small font that holds only the glyphs of a document keeps files short, as measured in @idocs2019.

# Cross-references {#sec:references}

A label names a heading, a figure, or a table. It starts with the kind of thing it names: `sec:` for a heading, `fig:` for a figure, and `tbl:` for a table. A reference writes the label after an at sign and shows the label of its kind and the number, as in @fig:landscape or @tbl:settings. In brackets with `page` it shows the page instead: the long table starts on [@tbl:history, page], and the tall drawing is on [@fig:tower, page]. Every reference is a link to what it names.

## Page numbers that settle

A page reference is text, and its width can move a line break, a page break, and with it the page it refers to. The table of contents has the same problem: its length decides where the body starts. Druck lays the report out, reads the pages of every heading, figure, and table, and lays it out again with those numbers until they no longer change. Most documents need two passes. A document whose numbers keep moving is reported rather than written with wrong numbers.

## Unnumbered headings

A heading below the numbering depth has no number, and a reference to it shows its text instead. The default theme numbers three levels, so the fourth-level heading below has none, and a reference to it reads @sec:quiet.

#### A quiet heading {#sec:quiet}

This heading is at the fourth level. It is not listed in the table of contents, which goes down to `toc-depth`.

# Sources {#sec:sources}

Citations work like cross-references. An at sign and a key from the file named by `bibliography` in the front matter cite a work. In brackets, as in `[@key]`, the citation stands in parentheses, and several works are separated by semicolons, as in `[@key1; @key2]`. Without brackets, as in `@key`, the work becomes part of the sentence.

A locator follows the key after a comma inside brackets: `[@key, p. 12]` or `[@key, pp. 3-5]`. A narrative citation takes it in brackets after one space: `@key [p. 12]`. Both forms read well in either citation style. Grouped citations suit the end of a claim that several works support [@knuthplass1981; @liang1983; @okafor2022], and a narrative one suits a claim about a single work. The collection @hartmann2015 [pp. 31-35] shows how a locator reads, and the benchmark in @okafor2022 [p. 4] shows it with one page.

A citation can also stand in a note.[^cite] The sections above cite inside the two columns of @sec:columns, in the prose after the long table of @sec:tables, and in the notes. Works on screen typography are @koenig2021 and, for tables, @schaefer2018.

[^cite]: A note may cite as well, for instance the standard that @unicode16 defines, or the German handbook by @forssman2002, which lists rules for numbers, dashes, and quotation marks.

The list of references at the end of this report is generated. It holds only the works cited in the text, in the order the citation style asks for, and every entry that the text cites appears in it once. Adding a citation adds an entry, and removing the last citation of a work removes it.

# Conclusion

A report set this way stays consistent while it changes. Moving @sec:figures before @sec:columns renumbers both sections, the figures in them, the table of contents, and every reference, and the page references follow the new layout. The same holds for the reference list, which follows the citations.
