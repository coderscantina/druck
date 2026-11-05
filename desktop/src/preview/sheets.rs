//! Where page sheets sit: in spreads, or wrapped into a contact sheet. Lengths are logical pixels; page sizes are points.
//!
//! The first page is a recto and stands alone at the right of the gutter. After it, pages pair up as a printed book's do.

use std::ops::Range;

/// How sheets are arranged at a zoom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Arrangement {
    /// Facing pages about a centre gutter, `gutter` pixels wide.
    Spreads { gutter: f32 },
    /// Every page, wrapped to fill a view this wide.
    Contact { view: f32 },
}

/// One row of sheets.
struct Row {
    top: f32,
    height: f32,
    pages: Range<usize>,
}

/// The sheets of a document at one zoom.
pub struct Sheets {
    /// Each sheet's left edge within the content, and its size.
    lefts: Vec<f32>,
    sizes: Vec<(f32, f32)>,
    rows: Vec<Row>,
    width: f32,
    height: f32,
    zoom: f32,
}

impl Sheets {
    pub fn new(pages: impl Iterator<Item = (f32, f32)>, zoom: f32, gap: f32, arrangement: Arrangement) -> Self {
        let sizes: Vec<(f32, f32)> = pages.map(|(width, height)| (width * zoom, height * zoom)).collect();
        match arrangement {
            Arrangement::Spreads { gutter } => Self::spreads(sizes, zoom, gap, gutter),
            Arrangement::Contact { view } => Self::contact(sizes, zoom, gap, view),
        }
    }

    fn spreads(sizes: Vec<(f32, f32)>, zoom: f32, gap: f32, gutter: f32) -> Self {
        let widest = sizes.iter().map(|size| size.0).fold(0.0, f32::max);
        let centre = gap + widest + gutter / 2.0;
        let mut sheets = Self::empty(sizes, zoom);
        let mut y = gap;
        let mut start = 0;
        while start < sheets.sizes.len() {
            // Page 0 is a recto. After it, an odd index opens a spread, and an even one closes it.
            let end = if start == 0 {
                1
            } else {
                (start + 2).min(sheets.sizes.len())
            };
            for page in start..end {
                sheets.lefts[page] = if page % 2 == 0 {
                    centre + gutter / 2.0
                } else {
                    centre - gutter / 2.0 - sheets.sizes[page].0
                };
            }
            y = sheets.push_row(start..end, y) + gap;
            start = end;
        }
        sheets.width = 2.0 * (gap + widest) + gutter;
        sheets.height = y;
        sheets
    }

    fn contact(sizes: Vec<(f32, f32)>, zoom: f32, gap: f32, view: f32) -> Self {
        let widest = sizes.iter().map(|size| size.0).fold(0.0, f32::max);
        let columns = (((view - gap) / (widest + gap)).floor() as usize).max(1);
        let mut sheets = Self::empty(sizes, zoom);
        let mut y = gap;
        let mut start = 0;
        while start < sheets.sizes.len() {
            let end = (start + columns).min(sheets.sizes.len());
            for page in start..end {
                sheets.lefts[page] = gap + (page - start) as f32 * (widest + gap);
            }
            y = sheets.push_row(start..end, y) + gap;
            start = end;
        }
        sheets.width = gap + columns as f32 * (widest + gap);
        sheets.height = y;
        sheets
    }

    fn empty(sizes: Vec<(f32, f32)>, zoom: f32) -> Self {
        Self {
            lefts: vec![0.0; sizes.len()],
            sizes,
            rows: Vec::new(),
            width: 0.0,
            height: 0.0,
            zoom,
        }
    }

    /// Adds a row at `top` and returns where it ends.
    fn push_row(&mut self, pages: Range<usize>, top: f32) -> f32 {
        let height = self.sizes[pages.clone()].iter().map(|size| size.1).fold(0.0, f32::max);
        self.rows.push(Row { top, height, pages });
        top + height
    }

    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// The width of the content with its margins.
    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    fn row_of(&self, page: usize) -> &Row {
        let at = self.rows.partition_point(|row| row.pages.end <= page);
        &self.rows[at]
    }

    /// The top of the row a page stands in.
    pub fn top(&self, page: usize) -> f32 {
        self.row_of(page).top
    }

    /// A sheet's size.
    pub fn size(&self, page: usize) -> (f32, f32) {
        self.sizes[page]
    }

    /// Where a sheet starts from the left of a view `view` pixels wide: the content is centred, unless it is wider.
    pub fn left(&self, page: usize, view: f32, scroll: f32) -> f32 {
        ((view - self.width) / 2.0).max(0.0) + self.lefts[page] - scroll
    }

    /// The page under a point of a view `view` pixels wide scrolled to `scroll`, with the point on it in points.
    /// `None` between sheets and beside them.
    pub fn point_at(&self, (x, y): (f32, f32), view: f32, scroll: (f32, f32)) -> Option<(usize, f32, f32)> {
        let down = y + scroll.1;
        let row = &self.rows[self.rows.partition_point(|row| row.top <= down).checked_sub(1)?];
        row.pages.clone().find_map(|page| {
            let (width, height) = self.sizes[page];
            let (across, below) = (x - self.left(page, view, scroll.0), down - row.top);
            ((0.0..width).contains(&across) && (0.0..height).contains(&below))
                .then(|| (page, across / self.zoom, below / self.zoom))
        })
    }

    /// The scroll that shows `top..bottom` of `page`, in points, in a view `view` pixels tall: `scroll` if they are
    /// on screen already, else its whole row if the row fits, else with them a third of the way down.
    pub fn scroll_to_show(&self, page: usize, (top, bottom): (f32, f32), view: f32, scroll: f32) -> f32 {
        let row = self.row_of(page);
        let (top, bottom) = (row.top + top * self.zoom, row.top + bottom * self.zoom);
        if top >= scroll && bottom <= scroll + view {
            scroll
        } else if row.height <= view {
            self.scroll_to_centre(page, view)
        } else {
            top - view / 3.0
        }
    }

    /// The pages in rows that overlap the `view` pixels below `scroll`.
    pub fn visible(&self, scroll: f32, view: f32) -> Range<usize> {
        let first = self.rows.partition_point(|row| row.top + row.height < scroll);
        let last = self.rows.partition_point(|row| row.top < scroll + view);
        if first >= last {
            let page = self.rows.get(first).map_or(self.len(), |row| row.pages.start);
            return page..page;
        }
        self.rows[first].pages.start..self.rows[last - 1].pages.end
    }

    /// The page at the middle of the view.
    pub fn page_at_middle(&self, scroll: f32, view: f32) -> Option<usize> {
        let middle = scroll + view / 2.0;
        let at = self.rows.partition_point(|row| row.top <= middle).checked_sub(1);
        at.or((!self.rows.is_empty()).then_some(0))
            .map(|at| self.rows[at].pages.start)
    }

    /// The scroll with `page`'s row at the middle of the view.
    pub fn scroll_to_centre(&self, page: usize, view: f32) -> f32 {
        let row = self.row_of(page);
        row.top + row.height / 2.0 - view / 2.0
    }

    /// The pages of the row `page` stands in.
    pub fn row_pages(&self, page: usize) -> Range<usize> {
        self.row_of(page).pages.clone()
    }
}

/// `scroll` limited to what a column of `content` pixels allows in a view of `view` pixels.
pub fn clamp_scroll(scroll: f32, content: f32, view: f32) -> f32 {
    scroll.clamp(0.0, (content - view).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPREADS: Arrangement = Arrangement::Spreads { gutter: 0.0 };

    fn spreads(pages: usize, zoom: f32) -> Sheets {
        Sheets::new(std::iter::repeat_n((100.0, 200.0), pages), zoom, 10.0, SPREADS)
    }

    #[test]
    fn the_first_page_stands_alone_as_a_recto_and_the_rest_pair_up() {
        let sheets = spreads(5, 1.0);
        assert_eq!(sheets.row_pages(0), 0..1);
        assert_eq!(sheets.row_pages(1), 1..3);
        assert_eq!(sheets.row_pages(2), 1..3);
        assert_eq!(sheets.row_pages(4), 3..5);
        assert_eq!(sheets.width(), 220.0);
        // The gutter is at the middle: a recto starts there, and a verso ends there.
        assert_eq!(sheets.left(0, 220.0, 0.0), 110.0);
        assert_eq!(sheets.left(1, 220.0, 0.0), 10.0);
        assert_eq!(sheets.left(2, 220.0, 0.0), 110.0);
    }

    #[test]
    fn a_last_page_alone_is_a_verso() {
        let sheets = spreads(4, 1.0);
        assert_eq!(sheets.row_pages(3), 3..4);
        assert_eq!(sheets.left(3, 220.0, 0.0), 10.0);
    }

    #[test]
    fn a_contact_sheet_wraps_every_page_to_the_view() {
        let sheets = Sheets::new(
            std::iter::repeat_n((100.0, 200.0), 7),
            0.5,
            10.0,
            Arrangement::Contact { view: 200.0 },
        );
        // 50 wide with gaps of 10: three fit in 200.
        assert_eq!(sheets.row_pages(0), 0..3);
        assert_eq!(sheets.row_pages(6), 6..7);
        assert_eq!(sheets.top(3), 120.0);
        // The three columns with their gaps take 190 of the 200, so 5 is left on each side.
        assert_eq!(sheets.left(1, 200.0, 0.0), 5.0 + 10.0 + 60.0);
    }

    #[test]
    fn a_point_in_the_view_finds_its_page_and_place() {
        let sheets = spreads(3, 2.0);
        // At zoom 2, sheets are 200 wide and 400 tall. The content is 420 wide, centred in a view of 620.
        assert_eq!(sheets.point_at((320.0, 30.0), 620.0, (0.0, 0.0)), Some((0, 5.0, 10.0)));
        assert_eq!(
            sheets.point_at((120.0, 30.0), 620.0, (0.0, 410.0)),
            Some((1, 5.0, 10.0)),
            "a verso on the second row"
        );
        assert_eq!(
            sheets.point_at((120.0, 30.0), 620.0, (0.0, 0.0)),
            None,
            "left of the lone recto"
        );
        assert_eq!(sheets.point_at((320.0, 415.0), 620.0, (0.0, 0.0)), None, "between rows");
    }

    #[test]
    fn text_on_screen_stays_put_and_text_off_screen_brings_its_spread() {
        let sheets = spreads(11, 1.0);
        assert_eq!(sheets.scroll_to_show(0, (10.0, 20.0), 300.0, 0.0), 0.0);
        let row = sheets.top(5);
        assert_eq!(sheets.scroll_to_show(5, (10.0, 20.0), 300.0, 0.0), row + 100.0 - 150.0);
        // A spread taller than the view: the text comes to the upper third.
        assert_eq!(sheets.scroll_to_show(5, (10.0, 20.0), 150.0, 0.0), row + 10.0 - 50.0);
    }

    #[test]
    fn only_overlapping_rows_are_visible() {
        let sheets = spreads(5, 1.0);
        assert_eq!(sheets.visible(0.0, 100.0), 0..1);
        assert_eq!(sheets.visible(215.0, 2.0), 1..1, "the gap shows no page");
        assert_eq!(sheets.visible(215.0, 300.0), 1..5);
        assert_eq!(sheets.visible(2000.0, 500.0), 5..5);
    }

    #[test]
    fn the_middle_page_keeps_its_place_across_a_new_arrangement() {
        let wide = spreads(21, 1.0);
        let page = wide.page_at_middle(wide.top(9), 200.0).expect("a page");
        assert_eq!(page, 9);
        let contact = Sheets::new(
            std::iter::repeat_n((100.0, 200.0), 21),
            0.5,
            10.0,
            Arrangement::Contact { view: 200.0 },
        );
        let scroll = contact.scroll_to_centre(page, 200.0);
        assert!(contact.visible(scroll, 200.0).contains(&page));
    }

    #[test]
    fn scroll_is_limited() {
        assert_eq!(clamp_scroll(-5.0, 1000.0, 100.0), 0.0);
        assert_eq!(clamp_scroll(5000.0, 1000.0, 100.0), 900.0);
        assert_eq!(clamp_scroll(50.0, 80.0, 100.0), 0.0);
    }
}
