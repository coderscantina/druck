::: columns
# Introduction {#sec:intro}

A paper opens with the question it answers and why the answer matters to its readers. The text of this starter is about the page it is set on: two columns, short lines, and paragraphs broken as a whole.

Short lines are quick to read but hard to set. A column of this width holds about fifty characters, the lower half of the 45 to 75 that @bringhurst2004 calls comfortable. With so few words on a line, every space counts, and a method that fills one line at a time leaves loose lines behind.

Citations take a number in this paper, and the works are listed under References at the end. Several works share one bracket [@knuthplass1981; @bringhurst2004], and a page can be named [@bringhurst2004, p. 26].

# Method {#sec:method}

Say what was done in enough detail that a reader could do it again. A section is a level 1 heading and takes a number, and a subsection takes two.

## Material

The whole paper is one file, and its text stands in a block named `columns`. In Write the block shows as a region, and everything in it is set in two columns.

## Procedure

Paragraphs are broken by weighing all the lines of a paragraph together, as @knuthplass1981 describe. A break that makes one line a little worse is accepted when it saves the next line from being much worse.[^cost]

[^cost]: Footnotes run across both columns at the foot of the page.

The steps of a procedure can be numbered:

1. Set every paragraph at the width of a column.
2. Balance the two columns where the section ends.
3. Put the notes of both columns in one area below them.

# Results {#sec:results}

Report what was found before saying what it means. @tbl:lines holds numbers that stand for your own, and the text refers to the table by its label, so the number stays right when tables move.

| Measure | One column | Two columns |
| --- | --- | --- |
| Characters per line | 95 | 50 |
| Lines per page | 52 | 104 |
| Hyphenated lines | 2 | 9 |

: Lines of a page in one and two columns {#tbl:lines}

Narrow columns hyphenate more often, as @tbl:lines shows. That is the price of the shorter line, and a reader pays it gladly: the eye finds the next line without effort.

::: full-width
A block named `full-width` runs across both columns. It suits a wide table, a figure, or a sentence that should stand apart, such as this one. The columns above it are balanced and resume below it.
:::

# Discussion {#sec:discussion}

Say what the results mean, where they stop, and what a reader should do with them. The method of @sec:method is no proof, and the numbers of @sec:results are placeholders.

A discussion also names what the paper leaves open. Here that is everything: the question, the material, and the answer are yours to write.

# Conclusion

End with what the reader should remember. One paragraph is enough.

::: bibliography
:::
