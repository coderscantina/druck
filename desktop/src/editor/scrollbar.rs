//! The scrollbar's arithmetic, without gpui. The galley scrolls by a block and an offset into it, and block heights are
//! estimates until they are shaped, so the thumb is a good reading of the place rather than an exact one.

/// Where the thumb is in a track of `track` pixels, as `(top, height)`, or `None` if everything fits.
pub fn thumb(scrolled: f32, content: f32, viewport: f32, track: f32, min_thumb: f32) -> Option<(f32, f32)> {
    if content <= viewport || track <= 0. {
        return None;
    }
    let height = (track * viewport / content).max(min_thumb).min(track);
    let room = track - height;
    let range = content - viewport;
    Some(((scrolled / range).clamp(0., 1.) * room, height))
}

/// How far down the content a thumb at `top` means.
pub fn scrolled_at(top: f32, height: f32, content: f32, viewport: f32, track: f32) -> f32 {
    let room = (track - height).max(1.);
    (top / room).clamp(0., 1.) * (content - viewport).max(0.)
}

/// The block at a distance into the content, and how far into that block it is.
pub fn block_at(heights: impl IntoIterator<Item = f32>, distance: f32) -> (usize, f32) {
    let mut covered = 0.;
    let mut last = (0, 0.);
    for (index, height) in heights.into_iter().enumerate() {
        if distance < covered + height {
            return (index, distance - covered);
        }
        covered += height;
        last = (index, height);
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_has_no_thumb() {
        assert_eq!(thumb(0., 500., 600., 600., 28.), None);
    }

    #[test]
    fn the_thumb_shows_how_much_shows_and_where() {
        assert_eq!(thumb(0., 2000., 500., 500., 28.), Some((0., 125.)));
        assert_eq!(thumb(1500., 2000., 500., 500., 28.), Some((375., 125.)));
        assert_eq!(thumb(750., 2000., 500., 500., 28.), Some((187.5, 125.)));
    }

    #[test]
    fn a_very_long_text_keeps_a_thumb_that_can_be_grabbed() {
        assert_eq!(thumb(0., 1_000_000., 500., 500., 28.), Some((0., 28.)));
    }

    #[test]
    fn a_thumb_position_maps_back_to_the_same_place() {
        let (top, height) = thumb(750., 2000., 500., 500., 28.).expect("a thumb");
        assert_eq!(scrolled_at(top, height, 2000., 500., 500.), 750.);
    }

    #[test]
    fn a_distance_finds_its_block() {
        let heights = [100., 0., 50., 200.];
        assert_eq!(block_at(heights, 0.), (0, 0.));
        assert_eq!(block_at(heights, 120.), (2, 20.));
        assert_eq!(block_at(heights, 150.), (3, 0.));
        assert_eq!(block_at(heights, 349.), (3, 199.));
        assert_eq!(block_at(heights, 500.), (3, 200.));
    }
}
