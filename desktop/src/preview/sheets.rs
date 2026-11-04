//! Where page sheets sit in the scrolling column. Lengths are logical pixels; page sizes are points.

use std::ops::Range;

/// The sheets of a document stacked at one zoom.
pub struct Sheets {
    tops: Vec<f32>,
    sizes: Vec<(f32, f32)>,
    width: f32,
    height: f32,
    zoom: f32,
    gap: f32,
}

impl Sheets {
    pub fn new(pages: impl Iterator<Item = (f32, f32)>, zoom: f32, gap: f32) -> Self {
        let mut tops = Vec::new();
        let mut sizes = Vec::new();
        let (mut y, mut width) = (gap, 0.0_f32);
        for (page_width, page_height) in pages {
            tops.push(y);
            sizes.push((page_width * zoom, page_height * zoom));
            y += page_height * zoom + gap;
            width = width.max(page_width * zoom);
        }
        Self {
            tops,
            sizes,
            width: width + 2.0 * gap,
            height: y,
            zoom,
            gap,
        }
    }

    pub fn len(&self) -> usize {
        self.tops.len()
    }

    /// The width of the column with its margins.
    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn top(&self, page: usize) -> f32 {
        self.tops[page]
    }

    /// A sheet's size.
    pub fn size(&self, page: usize) -> (f32, f32) {
        self.sizes[page]
    }

    /// Where a sheet starts from the left of a view `view` pixels wide: centred, and never closer than the gap.
    pub fn left(&self, page: usize, view: f32, scroll: f32) -> f32 {
        ((view - self.sizes[page].0) / 2.0).max(self.gap) - scroll
    }

    /// The page under a point of a view `view` pixels wide scrolled to `scroll`, with the point on it in points.
    /// `None` between sheets and beside them.
    pub fn point_at(&self, (x, y): (f32, f32), view: f32, scroll: (f32, f32)) -> Option<(usize, f32, f32)> {
        let down = y + scroll.1;
        let page = self.tops.partition_point(|top| *top <= down).checked_sub(1)?;
        let (width, height) = self.sizes[page];
        let (across, below) = (x - self.left(page, view, scroll.0), down - self.tops[page]);
        ((0.0..width).contains(&across) && (0.0..height).contains(&below))
            .then(|| (page, across / self.zoom, below / self.zoom))
    }

    /// The scroll that shows `top..bottom` of `page`, in points, in a view `view` pixels tall: `scroll` if they are
    /// on screen already, else with them a third of the way down.
    pub fn scroll_to_show(&self, page: usize, (top, bottom): (f32, f32), view: f32, scroll: f32) -> f32 {
        let (top, bottom) = (self.tops[page] + top * self.zoom, self.tops[page] + bottom * self.zoom);
        if top >= scroll && bottom <= scroll + view {
            scroll
        } else {
            top - view / 3.0
        }
    }

    /// The pages that overlap the `view` pixels below `scroll`.
    pub fn visible(&self, scroll: f32, view: f32) -> Range<usize> {
        let start = self.tops.partition_point(|top| *top <= scroll).saturating_sub(1);
        let start = if self
            .tops
            .get(start)
            .is_some_and(|top| top + self.sizes[start].1 < scroll)
        {
            start + 1
        } else {
            start
        };
        let end = self.tops.partition_point(|top| *top < scroll + view);
        start..end.max(start).min(self.len())
    }
}

/// `scroll` limited to what a column of `content` pixels allows in a view of `view` pixels.
pub fn clamp_scroll(scroll: f32, content: f32, view: f32) -> f32 {
    scroll.clamp(0.0, (content - view).max(0.0))
}

/// The scroll that keeps the same place of the document at the middle of the view when its length changes.
pub fn rescale_scroll(scroll: f32, view: f32, old_content: f32, new_content: f32) -> f32 {
    (scroll + view / 2.0) / old_content.max(1.0) * new_content - view / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheets() -> Sheets {
        Sheets::new([(100.0, 200.0); 10].into_iter(), 1.0, 10.0)
    }

    #[test]
    fn a_point_in_the_view_finds_its_page_and_place() {
        // At zoom 2, sheets are 200 wide and 400 tall, and centred in a view 400 wide, so they start at x = 100.
        let sheets = Sheets::new([(100.0, 200.0); 3].into_iter(), 2.0, 10.0);
        assert_eq!(sheets.point_at((120.0, 30.0), 400.0, (0.0, 0.0)), Some((0, 10.0, 10.0)));
        assert_eq!(
            sheets.point_at((120.0, 30.0), 400.0, (0.0, 410.0)),
            Some((1, 10.0, 10.0))
        );
        assert_eq!(
            sheets.point_at((130.0, 30.0), 400.0, (10.0, 0.0)),
            Some((0, 20.0, 10.0)),
            "scrolled across"
        );
        assert_eq!(
            sheets.point_at((50.0, 30.0), 400.0, (0.0, 0.0)),
            None,
            "beside the sheet"
        );
        assert_eq!(
            sheets.point_at((120.0, 415.0), 400.0, (0.0, 0.0)),
            None,
            "between sheets"
        );
    }

    #[test]
    fn text_on_screen_stays_put_and_text_off_screen_comes_to_the_upper_third() {
        let sheets = sheets();
        assert_eq!(sheets.scroll_to_show(0, (10.0, 20.0), 300.0, 0.0), 0.0);
        assert_eq!(
            sheets.scroll_to_show(5, (10.0, 20.0), 300.0, 0.0),
            1050.0 + 10.0 + 10.0 - 100.0
        );
        assert_eq!(sheets.scroll_to_show(0, (10.0, 20.0), 300.0, 1000.0), 20.0 - 100.0);
    }

    #[test]
    fn stacks_pages_with_gaps() {
        let sheets = sheets();
        assert_eq!((sheets.top(0), sheets.top(1)), (10.0, 220.0));
        assert_eq!((sheets.width(), sheets.height()), (120.0, 2110.0));
    }

    #[test]
    fn only_overlapping_pages_are_visible() {
        let sheets = sheets();
        assert_eq!(sheets.visible(0.0, 100.0), 0..1);
        assert_eq!(sheets.visible(215.0, 2.0), 1..1, "the gap shows no page");
        assert_eq!(sheets.visible(215.0, 300.0), 1..3);
        assert_eq!(sheets.visible(2000.0, 500.0), 9..10);
    }

    #[test]
    fn zoom_scales_sizes_and_positions() {
        let sheets = Sheets::new([(100.0, 200.0); 2].into_iter(), 2.0, 10.0);
        assert_eq!(sheets.size(1), (200.0, 400.0));
        assert_eq!(sheets.top(1), 420.0);
    }

    #[test]
    fn scroll_is_limited_and_keeps_its_place_across_zoom() {
        assert_eq!(clamp_scroll(-5.0, 1000.0, 100.0), 0.0);
        assert_eq!(clamp_scroll(5000.0, 1000.0, 100.0), 900.0);
        assert_eq!(clamp_scroll(50.0, 80.0, 100.0), 0.0);
        assert_eq!(rescale_scroll(450.0, 100.0, 1000.0, 2000.0), 950.0);
    }
}
