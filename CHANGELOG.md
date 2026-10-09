# Changelog

## 26.10.9, 9 October 2026

### Features

- Write books as EPUB 3 with `render -o book.epub` (b26fbbc)
- Add a bleed and crop marks to print PDFs with `--bleed` and `--crop-marks` (d4cdcb0)
- Set copyright pages and imprints at the bottom of the page with `::: bottom` (55e4a7a)

### Fixes

- Put a duplex book's copyright page on the back of the title page (c4f92fc)

### Dependencies

- Add flate2 to deflate EPUB containers (b120556)


## 26.10.9, 9 October 2026

### Features

- Add a novel sample with front matter, chapters, and scene breaks (9ac018f)
- Number book parts and open chapters with breaks, sinks, ornaments, and drop capitals (aed2d55)


## 26.10.7, 7 October 2026

### Features

- Write subject, ISO creation date, heading title, and Coder's Cantina as producer into PDF metadata (3add2c9)
- Support variable font weights and italics from files and installed families (eb25cf8)
- Load named themes from the user config directory (d53559e)
- Add placeholders for text statistics, the build date, and section pages, read them in the body, and set a draft watermark (2679732)
- Read text after a citation locator as a suffix (397f361)
- Lay documents out in check, and scale custom lists and quotations in footnotes (8ade459)
- Let list styles name a marker style for bullets and numbers (c7dd6f5)
- Add document.duplex to start the body on an odd page after blank pages (2f01a2f)
- Add page.column-change-spacing for the space at a change between one and two columns (d9b316f)
- Link each cited work, set numeric labels in a column, add citation prefixes, heading number gaps, contents level styles, and scaled footnote lists (a5d617b)
- Size PNG and JPEG images by their stored density (cd783df)
- Check reads body and bibliography, text settings take numbers, unused font settings warn, paths print relative (dcdec08)
- Set list tables with block cells, spans, and row styles, draw theme rules, and let slots name custom styles (0c38e6b)
- Look up installed fonts, match weights, and apply custom styles with {.name} (5b220d3)
- Set prose narrower than wide blocks, unmirrored margins, and anchored slot groups with meta values and page totals (87d06cc)
- Cite works from a BibTeX file and set a linked bibliography (6991c35)
- Parse BibTeX and format citations in two built-in styles (8c92136)
- Number headings and set title pages, contents, running headers, and linked cross-references (152be8b)
- Lay out pipe tables with wrapped cells and repeated headers across pages and columns (108bdf3)
- Place images with numbered captions in pages and columns (0de1fee)
- Decode PNG, JPEG, and SVG images and draw them in the PDF (64b18d1)
- Set column sections with balanced columns and full-width blocks (ed890a8)
- Compose pages with scored breaks, keep groups, and footnotes (b1e877e)
- Break paragraphs with total-fit optimization (2f21bef)
- Render Markdown to PDF from the CLI (edaa3a5)
- Shape text and write PDFs with embedded subset fonts and links (785b6ab)
- Parse CommonMark into the document model (6988d85)
- Add CLI foundation with validated themes and document settings (153469d)

### Fixes

- Keep list block spacing around the list, not before every item (80cbfc6)
- Strand no line at page ends, penalize hyphens and introducing colons, apply the rules to notes and table rows (dd646ae)
- End tracked lines at their last glyph, keep references whole, hang punctuation runs, break URLs after slashes, and keep text hyphens in extraction (d3deb55)
- Score very loose lines in the final breaking pass instead of capping their badness (d0223ed)
- Adapt bibliography links to the milestone 08 link type (51431e0)

### Changes

- Remove trailing whitespace from the font fixture license (1c36769)
- Set captions of narrow tables to the frame's edge, and use smaller muted bullets in the offer (8dce682)
- Write line-end text hyphens as plain hyphens again (7c18011)
- Set table captions at the table's own width and left edge (03e760f)
- Give lists block spacing in the default theme (8f6d88a)
- Extend the offer sample with cover labels, eyebrows, and a cost table over three pages (ca305ca)
- Add an offer sample with eyebrows, check lists, weights, and step headings (a728e38)
- Add an offer sample with a cover, a three-column footer, and narrow prose beside wide tables (4aaf0b9)
- Cite every entry type in the English report and add a German report (3d06c6a)
- Add a report sample with a custom theme and put sample headings under the title block (5f2cfe3)
- Add a tables sample and render it in the sample test (5a4234f)
- Shape paragraphs once and break them at any width (a0c4f98)
- Add an images sample and render it in the sample test (21d63d5)
- Reuse the bundled fonts for SVG text and reject SVG file references (87df1ef)
- Add a column layout sample and render it in the sample test (499d07b)
- Add a pagination sample and describe scored breaks in the samples (181e045)
- Indent later paragraphs of a footnote in the default theme (318cba8)
- Show paragraph-wide breaking and German compounds in the samples (5b05e01)

### Dependencies

- Add unicode-segmentation for UAX #29 character, word, and sentence counts (0021d8a)
- Bundle English and German hyphenation patterns with hypher (f0e6356)
