//! Cooperative cancellation.
//!
//! The engine looks at the token between pieces of work: before reading a resource, parsing a face, decoding an
//! image, or formatting a citation, at each layout pass and block, every 64 lines of the page break search, and at
//! each page of a PDF and chapter of an EPUB. A cancelled job returns [`Failure::Cancelled`], never partial output,
//! and leaves its renderer as good as it was.

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::renderer::Failure;

/// A token shared between a job and whoever may stop it. Cancelling is permanent: each job needs its own token.
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    #[cfg(test)]
    probe: Option<Arc<Probe>>,
}

impl Cancel {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Whether to stop, at a named check point.
    #[cfg_attr(not(test), allow(unused_variables))]
    #[inline]
    pub(crate) fn stopped(&self, at: CheckPoint) -> bool {
        #[cfg(test)]
        if let Some(probe) = &self.probe {
            probe.visit(at, self);
        }
        self.is_cancelled()
    }

    /// `Err(Failure::Cancelled)` once cancelled, at a named check point.
    #[inline]
    pub(crate) fn check(&self, at: CheckPoint) -> Result<(), Failure> {
        if self.stopped(at) {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The places a job looks at its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckPoint {
    Resource,
    Face,
    Bibliography,
    Citation,
    Image,
    Pass,
    Block,
    Pagination,
    Page,
    Chapter,
}

/// Cancels at the `nth` visit of one check point and counts the visits of each, so a test can tell where a job
/// stopped and what it did after.
#[cfg(test)]
pub(crate) struct Probe {
    at: CheckPoint,
    nth: usize,
    visits: [AtomicUsize; 10],
}

#[cfg(test)]
impl Probe {
    fn visit(&self, at: CheckPoint, cancel: &Cancel) {
        let visits = self.visits[at as usize].fetch_add(1, Ordering::Relaxed) + 1;
        if at == self.at && visits == self.nth {
            cancel.cancel();
        }
    }
}

#[cfg(test)]
impl Cancel {
    /// A token that cancels itself at the `nth` visit of `at`, counting from 1.
    pub(crate) fn at(at: CheckPoint, nth: usize) -> Self {
        Self {
            flag: Arc::default(),
            probe: Some(Arc::new(Probe {
                at,
                nth,
                visits: Default::default(),
            })),
        }
    }

    /// How often the job looked at this token at `at`.
    pub(crate) fn visits(&self, at: CheckPoint) -> usize {
        let probe = self.probe.as_ref().expect("a token made with `at`");
        probe.visits[at as usize].load(Ordering::Relaxed)
    }
}
