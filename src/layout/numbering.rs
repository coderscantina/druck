//! Page numbers. Without front, main, or back matter every page shows its physical number, counted from 1
//! with the title page. With them, each part numbers its pages in the format its `matter` entry sets: from 1
//! where it restarts, otherwise on from the page before it. Pages before the first part have no number.
//!
//! `{pages}` is the number of the last page that continues the same count, so in a book whose back matter
//! continues the main matter it is the last page of the back matter.

use crate::config::resolved::Config;
use crate::config::theme::NumberFormat;
use crate::document::Matter;
use crate::page::PageNumber;

pub(super) struct Numbering {
    /// The number of each physical page, `None` for a page without one.
    numbers: Vec<Option<PageNumber>>,
    /// The value of `{pages}` on each page.
    last: Vec<Option<PageNumber>>,
    /// Whether the document has parts, so the numbers differ from the physical ones.
    parts: bool,
}

impl Numbering {
    /// The numbers of `pages` pages, where `starts` holds the physical page each part starts on, in order.
    pub fn new(pages: usize, starts: &[(usize, Matter)], config: &Config) -> Self {
        if starts.is_empty() {
            let number = |value| {
                Some(PageNumber {
                    format: NumberFormat::Decimal,
                    value,
                })
            };
            return Self {
                numbers: (1..=pages).map(number).collect(),
                last: vec![number(pages); pages],
                parts: false,
            };
        }
        let mut numbers = vec![None; pages];
        // The page index where each count ends, exclusive, by the index where it starts.
        let mut counts: Vec<(usize, usize)> = Vec::new();
        let mut value = 0;
        for (index, &(start, matter)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(pages, |&(next, _)| next);
            let part = match matter {
                Matter::Front => config.matter.front,
                Matter::Main => config.matter.main,
                Matter::Back => config.matter.back,
            };
            if part.restart || counts.is_empty() {
                value = 0;
                counts.push((start, end));
            } else if let Some(count) = counts.last_mut() {
                count.1 = end;
            }
            for number in &mut numbers[start..end] {
                value += 1;
                *number = Some(PageNumber {
                    format: part.page_numbers,
                    value,
                });
            }
        }
        let mut last = vec![None; pages];
        for (start, end) in counts {
            if end == start {
                continue;
            }
            let total = numbers[end - 1].map(|number| number.value);
            for page in start..end {
                last[page] = numbers[page]
                    .zip(total)
                    .map(|(number, value)| PageNumber { value, ..number });
            }
        }
        Self {
            numbers,
            last,
            parts: true,
        }
    }

    /// The number page `index` shows, if it has one.
    pub fn page(&self, index: usize) -> Option<String> {
        self.numbers[index].map(text)
    }

    /// The value of `{pages}` on page `index`.
    pub fn pages(&self, index: usize) -> Option<String> {
        self.last[index].map(text)
    }

    /// The text of every page number, empty for a page without one.
    pub fn texts(&self) -> Vec<String> {
        (0..self.numbers.len())
            .map(|index| self.page(index).unwrap_or_default())
            .collect()
    }

    /// The page numbers for PDF page labels: none without parts, since viewers then count physical pages.
    pub fn labels(&self) -> Vec<Option<PageNumber>> {
        if self.parts { self.numbers.clone() } else { Vec::new() }
    }
}

fn text(number: PageNumber) -> String {
    number.format.format(number.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::tests::config;

    fn texts(numbering: &Numbering, pages: impl Fn(&Numbering, usize) -> Option<String>) -> Vec<String> {
        (0..numbering.numbers.len())
            .map(|index| pages(numbering, index).unwrap_or_else(|| "-".to_owned()))
            .collect()
    }

    #[test]
    fn numbers_physical_pages_without_parts() {
        let numbering = Numbering::new(3, &[], &config());

        assert_eq!(texts(&numbering, Numbering::page), ["1", "2", "3"]);
        assert_eq!(texts(&numbering, Numbering::pages), ["3", "3", "3"]);
        assert!(numbering.labels().is_empty());
    }

    #[test]
    fn restarts_and_continues_counts_by_part() {
        // Two title pages, three front matter pages, three main, two back.
        let starts = [(2, Matter::Front), (5, Matter::Main), (8, Matter::Back)];
        let numbering = Numbering::new(10, &starts, &config());

        let pages = ["-", "-", "i", "ii", "iii", "1", "2", "3", "4", "5"];
        assert_eq!(texts(&numbering, Numbering::page), pages);
        let totals = ["-", "-", "iii", "iii", "iii", "5", "5", "5", "5", "5"];
        assert_eq!(texts(&numbering, Numbering::pages), totals);
        let labels = numbering.labels();
        assert_eq!(labels[1], None);
        assert_eq!(
            labels[2],
            Some(PageNumber {
                format: NumberFormat::LowerRoman,
                value: 1
            })
        );
    }
}
