//! Where the material panel has been, as a browser's history: back and forward, and a trail of the last places.

use super::model::Kind;

/// What the panel shows: a kind's list, or one item by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    List(Kind),
    Item(String),
}

/// How many places the trail shows, ending at the current one.
pub const SHOWN: usize = 3;
/// How many items the jump field lists as recent.
const RECENT: usize = 8;

#[derive(Debug, Clone)]
pub struct Trail {
    places: Vec<Place>,
    at: usize,
    /// Items opened, most recent first.
    recent: Vec<String>,
}

impl Trail {
    pub fn new(start: Place) -> Self {
        Self {
            places: vec![start],
            at: 0,
            recent: Vec::new(),
        }
    }

    pub fn current(&self) -> &Place {
        &self.places[self.at]
    }

    /// Goes to `place`. What lay ahead of the current place is dropped, as a browser does, and going where the panel is
    /// already adds nothing.
    pub fn go(&mut self, place: Place) {
        if let Place::Item(id) = &place {
            self.recent.retain(|recent| recent != id);
            self.recent.insert(0, id.clone());
            self.recent.truncate(RECENT);
        }
        if *self.current() == place {
            return;
        }
        self.places.truncate(self.at + 1);
        self.places.push(place);
        self.at += 1;
    }

    /// Steps back, returning whether there was a place behind.
    pub fn back(&mut self) -> bool {
        let moved = self.at > 0;
        self.at = self.at.saturating_sub(1);
        moved
    }

    /// Steps forward, returning whether there was a place ahead.
    pub fn forward(&mut self) -> bool {
        let moved = self.at + 1 < self.places.len();
        if moved {
            self.at += 1;
        }
        moved
    }

    pub fn can_go_back(&self) -> bool {
        self.at > 0
    }

    /// The last places up to the current one, oldest first, with how many steps back each is.
    pub fn shown(&self) -> Vec<(usize, &Place)> {
        let from = (self.at + 1).saturating_sub(SHOWN);
        (from..=self.at).map(|index| (self.at - index, &self.places[index])).collect()
    }

    /// Steps back `steps` places, for a click on the trail.
    pub fn back_by(&mut self, steps: usize) {
        self.at = self.at.saturating_sub(steps);
    }

    /// The items opened, most recent first.
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Forgets items that are gone.
    pub fn retain(&mut self, exists: impl Fn(&str) -> bool) {
        self.recent.retain(|id| exists(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Place {
        Place::Item(id.to_owned())
    }

    #[test]
    fn the_trail_walks_back_and_forward_and_a_new_place_drops_what_lay_ahead() {
        let mut trail = Trail::new(Place::List(Kind::Research));
        trail.go(item("a"));
        trail.go(item("b"));
        assert!(trail.back());
        assert_eq!(trail.current(), &item("a"));
        assert!(trail.forward());
        assert!(!trail.forward());
        trail.back();
        trail.go(item("c"));
        assert!(!trail.forward());
        assert_eq!(trail.current(), &item("c"));
        trail.back();
        trail.back();
        assert!(!trail.back());
        assert_eq!(trail.current(), &Place::List(Kind::Research));
    }

    #[test]
    fn the_trail_shows_the_last_three_places_and_how_far_back_each_is() {
        let mut trail = Trail::new(Place::List(Kind::Research));
        for id in ["a", "b", "c"] {
            trail.go(item(id));
        }
        let shown: Vec<_> = trail.shown().into_iter().map(|(steps, place)| (steps, place.clone())).collect();
        assert_eq!(shown, [(2, item("a")), (1, item("b")), (0, item("c"))]);
        trail.back_by(2);
        assert_eq!(trail.current(), &item("a"));
    }

    #[test]
    fn recent_items_come_most_recent_first_without_repeats() {
        let mut trail = Trail::new(Place::List(Kind::Notes));
        trail.go(item("a"));
        trail.go(item("b"));
        trail.go(item("a"));
        assert_eq!(trail.recent(), ["a", "b"]);
        trail.go(item("a"));
        assert_eq!(trail.shown().len(), 3);
    }
}
