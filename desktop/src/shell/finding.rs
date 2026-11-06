//! Finding for the hub: project search and replace through the search panel, and the find bar in the open chapter.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext as _, Context, Entity, Focusable, Task, Window};

use super::Hub;
use super::find_bar::{FindBar, FindEvent};
use super::search::{self, Matcher, Options, Source};
use super::search_panel::{SearchEvent, SearchPanel, Status};
use crate::session::ChapterId;
use crate::source::EditKind;

/// How long typing in the search panel pauses before it searches.
const SEARCH_PAUSE: Duration = Duration::from_millis(150);

/// The search panel and the find bar, and what the hub keeps of their work.
pub(super) struct Finding {
    pub search: Entity<SearchPanel>,
    pub find: Entity<FindBar>,
    search_task: Task<()>,
    /// The count of a replacement, shown with the search that follows it.
    replaced: Option<usize>,
    /// Where the caret is, and where the find bar started.
    pub caret: usize,
    find_origin: usize,
}

impl Finding {
    pub fn new(search: Entity<SearchPanel>, find: Entity<FindBar>) -> Self {
        Self {
            search,
            find,
            search_task: Task::ready(()),
            replaced: None,
            caret: 0,
            find_origin: 0,
        }
    }
}

impl Hub {
    // Search across the project

    pub(super) fn search_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding.search.update(cx, |panel, cx| panel.open(window, cx));
    }

    pub(super) fn search_event(
        &mut self,
        _: &Entity<SearchPanel>,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::Changed => self.start_search(cx),
            SearchEvent::Open(id, range) => self.open_place(*id, range.clone(), window, cx),
            SearchEvent::ReplaceAll => self.replace(None, window, cx),
            SearchEvent::ReplaceFile(id) => self.replace(Some(*id), window, cx),
            SearchEvent::Closed => self.with_workspace(cx, |workspace, cx| workspace.focus_main(window, cx)),
        }
    }

    /// Every chapter the search covers: the book, files not in it, notes, and research.
    fn searchable(&self, cx: &Context<Self>) -> Vec<(ChapterId, String, Option<String>, PathBuf)> {
        let session = self.parts.editor.read(cx).session();
        let contents = session.contents();
        let rows = contents
            .book
            .iter()
            .chain(&contents.not_in_book)
            .chain(&contents.notes)
            .chain(&contents.research);
        rows.filter_map(|row| {
            let chapter = session.chapter(row.id)?;
            let text = chapter.is_read().then(|| chapter.text().to_owned());
            Some((row.id, row.title.clone(), text, chapter.path().to_owned()))
        })
        .collect()
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let (query, replacement, options) = self.finding.search.read(cx).request(cx);
        let matcher = match Matcher::new(&query, options) {
            Ok(Some(matcher)) => matcher,
            Ok(None) => {
                self.finding.search_task = Task::ready(());
                self.finding
                    .search
                    .update(cx, |panel, cx| panel.set_results(Vec::new(), Status::Idle, cx));
                return;
            }
            Err(message) => {
                self.finding.search_task = Task::ready(());
                self.finding.search.update(cx, |panel, cx| {
                    panel.set_results(Vec::new(), Status::Invalid(message), cx)
                });
                return;
            }
        };
        let sources = self.searchable(cx);
        let after_replace = self.finding.replaced.take();
        self.finding
            .search
            .update(cx, |panel, cx| panel.set_status(Status::Searching, cx));
        self.finding.search_task = cx.spawn(async move |hub, cx| {
            cx.background_executor().timer(SEARCH_PAUSE).await;
            let found = cx
                .background_spawn(async move {
                    let sources: Vec<Source> = sources
                        .into_iter()
                        .filter_map(|(id, title, text, path)| {
                            let text = text.or_else(|| std::fs::read_to_string(path).ok())?;
                            Some(Source { id, title, text })
                        })
                        .collect();
                    search::search(&sources, &matcher, &replacement)
                })
                .await;
            hub.update(cx, |hub, cx| {
                let status = after_replace.map_or(Status::Idle, Status::Replaced);
                hub.finding
                    .search
                    .update(cx, |panel, cx| panel.set_results(found, status, cx));
            })
            .ok();
        });
    }

    /// Replaces in one chapter or in all, as one undo step per chapter, then searches again.
    fn replace(&mut self, only: Option<ChapterId>, window: &mut Window, cx: &mut Context<Self>) {
        let (query, replacement, options) = self.finding.search.read(cx).request(cx);
        if replacement.is_empty() {
            return;
        }
        let Ok(Some(matcher)) = Matcher::new(&query, options) else {
            return;
        };
        let ids: Vec<ChapterId> = self
            .searchable(cx)
            .into_iter()
            .map(|(id, ..)| id)
            .filter(|id| only.is_none_or(|only| only == *id))
            .collect();
        let count = self.parts.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    let mut count = 0;
                    for id in ids {
                        session.load(id);
                        let edits = matcher.edits(session.buffer(id).text(), &replacement);
                        let made = edits.len();
                        if made > 0 && session.edit_chapter(id, edits, EditKind::Discrete).is_ok() {
                            count += made;
                        }
                    }
                    count
                },
                window,
                cx,
            )
        });
        self.finding.replaced = Some(count);
        self.start_search(cx);
    }

    // Find in the open chapter

    pub(super) fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding.find_origin = self.finding.caret;
        self.finding.find.update(cx, |bar, cx| bar.open(window, cx));
    }

    pub(super) fn find_event(
        &mut self,
        _: &Entity<FindBar>,
        event: &FindEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FindEvent::Query => self.go_to_match(self.finding.find_origin, false, window, cx),
            FindEvent::Next => self.step_find(false, window, cx),
            FindEvent::Previous => self.step_find(true, window, cx),
            FindEvent::Closed => {
                let focus = self.parts.editor.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            }
        }
    }

    /// Selects the next match from the caret, or the one before it. The bar's own text decides what a match is.
    pub(super) fn step_find(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.finding.find.read(cx).is_open() {
            self.find(window, cx);
            return;
        }
        self.go_to_match(self.finding.caret, backwards, window, cx);
    }

    fn go_to_match(&mut self, from: usize, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.finding.find.read(cx).query(cx);
        let matches = match Matcher::new(&query, Options::default()) {
            Ok(Some(matcher)) => matcher.find(self.parts.editor.read(cx).text()),
            _ => Vec::new(),
        };
        let position = search::next_match(&matches, from, backwards).map(|at| (at, matches[at].clone()));
        if let Some((_, range)) = &position {
            self.parts
                .editor
                .update(cx, |editor, cx| editor.select(range.clone(), window, cx));
        }
        let shown = position.map(|(at, _)| (at + 1, matches.len()));
        self.finding.find.update(cx, |bar, cx| bar.set_position(shown, cx));
    }
}
