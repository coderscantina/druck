//! Layout scheduling: asking the engine for pages when the text or theme changes, one run at a time with the newest
//! waiting, taking in what a run gave, and keeping Publish ready while Write is on screen.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
#[cfg(any(debug_assertions, feature = "measure"))]
use std::time::Instant;

use druck::Failure;
use gpui::prelude::*;
use gpui::{AnyWindowHandle, Bounds, Context, Pixels};

use super::job::{Job, Layout, Scheduler, Started, Typesetter};
use super::{Laid, PreviewView, SPECIMEN, SPECIMEN_FILE, TypeRead, Typeset};
use crate::session::{Joined, Kind};

/// What is typeset when: one run at a time with the newest waiting, the document's or the specimen's, in which theme.
pub(super) struct Typesetting {
    pub scheduler: Scheduler<Job>,
    pub typesetter: Arc<Mutex<Typesetter>>,
    /// Typesets the bundled specimen in the same theme, for a view of every element.
    pub specimen_typesetter: Arc<Mutex<Typesetter>>,
    /// The theme to typeset with, if not the document's, which the next layout follows.
    pub theme: Option<PathBuf>,
    /// The document's text as last asked for, to typeset again when the theme changes or the specimen goes.
    pub latest: Option<Joined>,
    /// Whether the pages show the specimen instead of the document.
    pub specimen: bool,
    /// The request whose pages are published.
    pub published: u64,
}

impl Typesetting {
    /// Typesetting of the document `kind` in `theme`, or the theme it names.
    pub(super) fn new(kind: Kind, theme: Option<PathBuf>) -> Self {
        let specimen = Kind::Single {
            path: kind.base_dir().join(SPECIMEN_FILE),
        };
        Self {
            scheduler: Scheduler::new(),
            specimen_typesetter: Arc::new(Mutex::new(Typesetter::new(specimen, None))),
            typesetter: Arc::new(Mutex::new(Typesetter::new(kind, theme.clone()))),
            theme,
            latest: None,
            specimen: false,
            published: 0,
        }
    }

    /// The run for what the pages show, the specimen or the document as last asked for, if it can start now.
    fn next(&mut self) -> Option<Started<Job>> {
        let joined = self.latest.clone()?;
        let joined = if self.specimen {
            Joined::single(SPECIMEN.to_owned(), joined.revision)
        } else {
            joined
        };
        self.scheduler.request(Job {
            joined,
            theme: self.theme.clone(),
            specimen: self.specimen,
        })
    }

    fn typesetter(&self, specimen: bool) -> Arc<Mutex<Typesetter>> {
        if specimen {
            self.specimen_typesetter.clone()
        } else {
            self.typesetter.clone()
        }
    }
}

/// Where the pages would paint if they were on screen.
#[derive(Clone, Copy)]
pub struct Ahead {
    pub bounds: Bounds<Pixels>,
    pub scale: f32,
    /// The window whose atlas holds what they painted before, which a page evicted meanwhile is freed from.
    pub window: AnyWindowHandle,
}

impl PreviewView {
    /// Lays out a copy of the document on a background thread and publishes the pages when done. A newer request
    /// cancels this one.
    pub fn request(&mut self, joined: Joined, cx: &mut Context<Self>) {
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(bench) = &mut self.bench {
            bench.requested(&joined.text);
        }
        self.typesetting.latest = Some(joined);
        // The specimen stays as it is while the document changes. A theme edit asks for it again with `refresh`.
        if !self.typesetting.specimen || self.layout.is_none() {
            self.typeset_latest(cx);
        }
    }

    /// Typesets again what the pages show, since the theme file changed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.typeset_latest(cx);
    }

    /// Typesets what the pages show: the specimen, or the document as last asked for.
    pub(super) fn typeset_latest(&mut self, cx: &mut Context<Self>) {
        if let Some(started) = self.typesetting.next() {
            self.start_layout(started, cx);
        }
    }

    /// Typesets with another theme file, or with the document's own when `None`. The next layout takes it.
    pub fn set_theme(&mut self, theme: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.typesetting.theme != theme {
            self.typesetting.theme = theme;
            self.typeset_latest(cx);
        }
    }

    /// Shows the bundled specimen in pages, or the document again.
    pub fn set_specimen(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.typesetting.specimen != on {
            self.typesetting.specimen = on;
            self.typeset_latest(cx);
        }
    }

    pub fn is_specimen(&self) -> bool {
        self.typesetting.specimen
    }

    pub(super) fn start_layout(&mut self, (number, job, cancel): Started<Job>, cx: &mut Context<Self>) {
        let Job {
            joined,
            theme,
            specimen,
        } = job;
        let typesetter = self.typesetting.typesetter(specimen);
        let revision = joined.revision;
        let layout = cx.background_spawn(async move {
            // A panic is caught inside the run, so this lock is not poisoned, and if it were the typesetter is still whole.
            let mut typesetter = typesetter.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            typesetter.set_theme(theme);
            let result = typesetter.run(joined, &cancel);
            (result, typesetter.take_failed_type())
        });
        cx.spawn(async move |this, cx| {
            let (result, failed_type) = layout.await;
            this.update(cx, |view, cx| {
                let (publish, next) = view.typesetting.scheduler.finished(number);
                if publish {
                    view.publish(number, revision, specimen, result, cx);
                    // Write keeps the theme's type when the pages cannot be set.
                    if let Some(book_type) = failed_type.filter(|_| !specimen) {
                        cx.emit(TypeRead(book_type));
                    }
                }
                if let Some(next) = next {
                    view.start_layout(next, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn publish(
        &mut self,
        number: u64,
        revision: u64,
        specimen: bool,
        result: Result<Layout, Failure>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(layout) => {
                self.typesetting.published = number;
                // Offsets in the specimen are not offsets in any chapter.
                self.revision = (!specimen).then_some(revision);
                self.diagnostics.clear();
                #[cfg(any(debug_assertions, feature = "measure"))]
                if let Some(bench) = &mut self.bench {
                    bench.published(&layout);
                }
                let book_type = layout.book_type.clone();
                // Freeing a whole document's runs takes long enough to drop a frame.
                let old = self.layout.replace(Arc::new(layout));
                cx.background_spawn(async move { drop(old) }).detach();
                self.mark_problems();
                cx.emit(Typeset(book_type));
            }
            Err(Failure::Errors(diagnostics)) => self.diagnostics = diagnostics,
            Err(Failure::Cancelled) => {}
        }
        // The specimen's warnings and errors are about the specimen, not the document the diagnostics list shows.
        if !specimen {
            cx.emit(Laid);
        }
        cx.notify();
    }

    /// Keeps rastering what would show at `ahead` while the pages are not on screen, so that entering Publish finds them
    /// finished. `None` once they are on screen.
    pub fn ready_ahead(&mut self, ahead: Option<Ahead>, cx: &mut Context<Self>) {
        self.ahead = ahead;
        self.prepare_ahead(cx);
    }

    pub(super) fn prepare_ahead(&mut self, cx: &mut Context<Self>) {
        if let Some(ahead) = self.ahead {
            self.prepare(ahead.bounds, ahead.scale, cx);
        }
    }

    /// Whether a layout is running, which the next request would cancel.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn is_laying_out(&self) -> bool {
        self.typesetting.scheduler.is_running()
    }

    /// The moment the pages of the newest request were first ready to paint, if they have been.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn ready_at(&self) -> Option<Instant> {
        (self.ready.0 == self.typesetting.scheduler.latest()).then_some(self.ready.1)
    }

    /// The last frame that showed every visible page exact.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn exact_frame(&self) -> Option<Instant> {
        self.exact_frame
    }
}
