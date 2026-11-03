//! Rasterized pages by content and resolution, within a memory budget.

/// What a page image shows. Equal keys mean equal pixels, so an image survives a refresh that did not
/// change its page, even when the page moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    /// The painter, which stands for the loaded fonts.
    pub painter: u64,
    pub fingerprint: u64,
    /// The bits of the pixels per point.
    pub resolution: u32,
}

struct Entry<I> {
    key: PageKey,
    /// The page index this image was last wanted for.
    page: usize,
    image: I,
    bytes: usize,
    used: u64,
}

pub struct PageCache<I> {
    entries: Vec<Entry<I>>,
    budget: usize,
    clock: u64,
}

impl<I: Clone> PageCache<I> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: Vec::new(),
            budget,
            clock: 0,
        }
    }

    /// Sets the budget that the next insert keeps to.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// The exact image for `key`, now counted as used for `page`.
    pub fn get(&mut self, key: PageKey, page: usize) -> Option<I> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.iter_mut().find(|entry| entry.key == key)?;
        entry.used = clock;
        entry.page = page;
        Some(entry.image.clone())
    }

    /// The image most recently used for `page`, of any content or resolution, to show until the exact one
    /// is ready.
    pub fn stale(&self, page: usize) -> Option<I> {
        self.entries
            .iter()
            .filter(|entry| entry.page == page)
            .max_by_key(|entry| entry.used)
            .map(|entry| entry.image.clone())
    }

    /// Stores an image and returns those evicted to stay within budget, least recently used first. The new
    /// image always stays.
    pub fn insert(&mut self, key: PageKey, page: usize, image: I, bytes: usize) -> Vec<I> {
        self.clock += 1;
        self.entries.push(Entry {
            key,
            page,
            image,
            bytes,
            used: self.clock,
        });
        let mut evicted = Vec::new();
        let mut total: usize = self.entries.iter().map(|entry| entry.bytes).sum();
        while total > self.budget && self.entries.len() > 1 {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(index, _)| index)
                .expect("more than one entry");
            let entry = self.entries.swap_remove(oldest);
            total -= entry.bytes;
            evicted.push(entry.image);
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fingerprint: u64) -> PageKey {
        PageKey {
            painter: 1,
            fingerprint,
            resolution: 2,
        }
    }

    #[test]
    fn evicts_the_least_recently_used_beyond_the_budget() {
        let mut cache = PageCache::new(20);
        cache.insert(key(1), 0, "a", 10);
        cache.insert(key(2), 1, "b", 10);
        assert_eq!(cache.get(key(1), 0), Some("a"));
        assert_eq!(cache.insert(key(3), 2, "c", 10), vec!["b"]);
        assert_eq!(cache.get(key(2), 1), None);
    }

    #[test]
    fn an_image_larger_than_the_budget_still_stays() {
        let mut cache = PageCache::new(5);
        assert!(cache.insert(key(1), 0, "a", 10).is_empty());
        assert_eq!(cache.insert(key(2), 0, "b", 10), vec!["a"]);
        assert_eq!(cache.get(key(2), 0), Some("b"));
    }

    #[test]
    fn stale_finds_the_latest_image_of_the_page_index() {
        let mut cache = PageCache::new(100);
        cache.insert(key(1), 3, "old", 1);
        cache.insert(key(2), 3, "new", 1);
        cache.insert(key(3), 4, "other", 1);
        assert_eq!(cache.stale(3), Some("new"));
        assert_eq!(cache.stale(9), None);
    }

    #[test]
    fn a_moved_page_keeps_its_image_under_the_new_index() {
        let mut cache = PageCache::new(100);
        cache.insert(key(1), 3, "moved", 1);
        assert_eq!(cache.get(key(1), 5), Some("moved"));
        assert_eq!(cache.stale(3), None);
        assert_eq!(cache.stale(5), Some("moved"));
    }
}
