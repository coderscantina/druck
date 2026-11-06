//! Where the material panel is, as a breadcrumb: a kind's list, the item opened from it, and each source followed from
//! that item. Going up drops the deepest place; going down again returns to it until another place is opened.

use super::model::Kind;

/// What the panel shows: a kind's list, or one item by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    List(Kind),
    Item(String),
}

/// How many places the breadcrumb shows before it folds the middle ones into a gap: the list and the last two.
pub const SHOWN: usize = 3;
/// How many items the jump field lists as recent.
const RECENT: usize = 8;

/// One step of the breadcrumb as it shows: a place with its depth, or the gap standing for the places folded away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Crumb<'a> {
    Place(usize, &'a Place),
    Gap,
}

#[derive(Debug, Clone)]
pub struct Trail {
    /// From the kind's list down to the current place. Never empty, and its first place is always a list.
    path: Vec<Place>,
    /// The places gone up from, the nearest last, for going down again.
    below: Vec<Place>,
    /// Items opened, most recent first.
    recent: Vec<String>,
}

impl Trail {
    pub fn new(kind: Kind) -> Self {
        Self {
            path: vec![Place::List(kind)],
            below: Vec::new(),
            recent: Vec::new(),
        }
    }

    pub fn current(&self) -> &Place {
        self.path.last().expect("the path starts at a list")
    }

    /// Shows a kind's list, as its rail tab does.
    pub fn list(&mut self, kind: Kind) {
        self.below.clear();
        self.path = vec![Place::List(kind)];
    }

    /// Opens an item from its kind's list, the jump field, or the library.
    pub fn open(&mut self, id: &str, kind: Kind) {
        self.remember(id);
        if self.path.len() == 2 && self.path[0] == Place::List(kind) && *self.current() == Place::Item(id.to_owned()) {
            return;
        }
        self.below.clear();
        self.path = vec![Place::List(kind), Place::Item(id.to_owned())];
    }

    /// Follows a source the current item names: one level deeper, or back up to it when it is on the way already.
    pub fn follow(&mut self, id: &str) {
        self.remember(id);
        let place = Place::Item(id.to_owned());
        self.below.clear();
        match self.path.iter().position(|step| *step == place) {
            Some(at) => self.path.truncate(at + 1),
            None => self.path.push(place),
        }
    }

    fn remember(&mut self, id: &str) {
        self.recent.retain(|recent| recent != id);
        self.recent.insert(0, id.to_owned());
        self.recent.truncate(RECENT);
    }

    /// Goes up a level, returning whether there was one.
    pub fn up(&mut self) -> bool {
        if self.path.len() < 2 {
            return false;
        }
        self.below.extend(self.path.pop());
        true
    }

    /// Goes down again to where the panel went up from, returning whether there was such a place.
    pub fn down(&mut self) -> bool {
        let Some(place) = self.below.pop() else {
            return false;
        };
        self.path.push(place);
        true
    }

    pub fn can_go_up(&self) -> bool {
        self.path.len() > 1
    }

    /// Goes up to the place at `depth`, for a click on the breadcrumb.
    pub fn up_to(&mut self, depth: usize) {
        while self.path.len() > depth + 1 && self.up() {}
    }

    /// The breadcrumb: the list, then the deeper places, with the middle ones folded into a gap past [`SHOWN`].
    pub fn crumbs(&self) -> Vec<Crumb<'_>> {
        let crumb = |depth: usize| Crumb::Place(depth, &self.path[depth]);
        let last = self.path.len() - 1;
        if self.path.len() <= SHOWN {
            return (0..=last).map(crumb).collect();
        }
        let mut crumbs = vec![crumb(0), Crumb::Gap];
        crumbs.extend((last + 2 - SHOWN..=last).map(crumb));
        crumbs
    }

    /// The items opened, most recent first.
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Forgets items that are gone, and goes up past any on the way.
    pub fn retain(&mut self, exists: impl Fn(&str) -> bool) {
        self.recent.retain(|id| exists(id));
        let gone = |place: &Place| matches!(place, Place::Item(id) if !exists(id));
        if let Some(at) = self.path.iter().position(gone) {
            self.path.truncate(at);
        }
        self.below.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Place {
        Place::Item(id.to_owned())
    }

    #[test]
    fn the_breadcrumb_goes_down_by_following_sources_and_up_again() {
        let mut trail = Trail::new(Kind::Research);
        trail.open("interview", Kind::Research);
        trail.follow("scan");
        assert_eq!(trail.current(), &item("scan"));
        assert!(trail.up());
        assert_eq!(trail.current(), &item("interview"));
        assert!(trail.down());
        assert_eq!(trail.current(), &item("scan"));
        assert!(!trail.down());
        trail.up_to(0);
        assert_eq!(trail.current(), &Place::List(Kind::Research));
        assert!(!trail.up());
    }

    #[test]
    fn a_tab_or_a_fresh_open_starts_the_breadcrumb_again_instead_of_adding_to_it() {
        let mut trail = Trail::new(Kind::Research);
        trail.open("interview", Kind::Research);
        trail.list(Kind::Notes);
        assert_eq!(trail.crumbs(), [Crumb::Place(0, &Place::List(Kind::Notes))]);
        trail.open("idea", Kind::Notes);
        assert_eq!(
            trail.crumbs(),
            [
                Crumb::Place(0, &Place::List(Kind::Notes)),
                Crumb::Place(1, &item("idea"))
            ]
        );
        trail.up();
        trail.open("other", Kind::Notes);
        assert!(!trail.down());
    }

    #[test]
    fn following_a_source_already_on_the_way_goes_back_up_to_it() {
        let mut trail = Trail::new(Kind::Research);
        trail.open("a", Kind::Research);
        trail.follow("b");
        trail.follow("a");
        assert_eq!(
            trail.crumbs(),
            [
                Crumb::Place(0, &Place::List(Kind::Research)),
                Crumb::Place(1, &item("a"))
            ]
        );
    }

    #[test]
    fn a_deep_breadcrumb_shows_the_list_a_gap_and_the_last_two() {
        let mut trail = Trail::new(Kind::Research);
        trail.open("a", Kind::Research);
        for id in ["b", "c"] {
            trail.follow(id);
        }
        assert_eq!(
            trail.crumbs(),
            [
                Crumb::Place(0, &Place::List(Kind::Research)),
                Crumb::Gap,
                Crumb::Place(2, &item("b")),
                Crumb::Place(3, &item("c")),
            ]
        );
    }

    #[test]
    fn recent_items_come_most_recent_first_without_repeats() {
        let mut trail = Trail::new(Kind::Notes);
        trail.open("a", Kind::Notes);
        trail.open("b", Kind::Notes);
        trail.follow("a");
        assert_eq!(trail.recent(), ["a", "b"]);
    }
}
