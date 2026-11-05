//! `Ctrl+Shift+G`: the Git panel, in the rail panels' form. A branch picker, the changes, a message, then history. It is a
//! right panel in Write, through [`Slot`], and the view itself mounts as a rail tab in Publish.

use gpui::prelude::*;
use gpui::{Context, Entity, Focusable as _, ScrollHandle, Subscription, WeakEntity, Window, div};

use super::repo::{Branch, Overview};
use super::strings::Key;
use super::{RepoState, Versions, change_words, dates, repo, sync_words, ui};
use crate::a11y::Control as _;
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::tokens::{self, Tokens};
use crate::workspace::Mode;

pub struct GitPanel {
    view: WeakEntity<Versions>,
    open: bool,
    /// The branch list shows under the picker.
    branches_open: bool,
    /// The unmerged branch whose Delete asked once already.
    confirm_delete: Option<String>,
    message: Entity<TextInput>,
    new_branch: Entity<TextInput>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl GitPanel {
    pub fn new(view: WeakEntity<Versions>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_label(crate::t!(Key::Message))
                .with_placeholder(crate::t!(Key::Message))
        });
        let new_branch = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_label(crate::t!(Key::NewBranch))
                .with_placeholder(crate::t!(Key::NewBranch))
        });
        let subscriptions = vec![
            cx.subscribe_in(&message, window, |panel, _, _: &Submitted, window, cx| {
                panel.commit(false, window, cx)
            }),
            cx.subscribe_in(&new_branch, window, |panel, _, _: &Submitted, window, cx| {
                panel.create(window, cx)
            }),
            cx.subscribe_in(&message, window, |panel, _, _: &Cancelled, window, cx| {
                panel.leave(window, cx)
            }),
            cx.subscribe_in(&new_branch, window, |panel, _, _: &Cancelled, window, cx| {
                panel.leave(window, cx)
            }),
        ];
        Self {
            view,
            open: false,
            branches_open: false,
            confirm_delete: None,
            message,
            new_branch,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes it in Write, and gives the keys to the message.
    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.message.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Opens the branch list, for screenshots.
    pub fn show_branches(&mut self, cx: &mut Context<Self>) {
        self.branches_open = true;
        cx.notify();
    }

    pub fn committed(&mut self, cx: &mut Context<Self>) {
        self.message
            .update(cx, |input, cx| input.set_value(String::new(), true, cx));
    }

    /// Escape in a field gives the keys back to the text. The panel stays until its key closes it.
    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.view.upgrade() {
            let workspace = view.read(cx).workspace.clone();
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| workspace.focus_main(window, cx));
            }
        }
    }

    fn commit(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).text().to_owned();
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.commit(message, push, window, cx));
        }
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_branch.read(cx).text().trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.new_branch
            .update(cx, |input, cx| input.set_value(String::new(), true, cx));
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.create_branch(name, window, cx));
        }
    }

    fn field(input: &Entity<TextInput>) -> impl IntoElement + use<> {
        div().flex_none().px(tokens::SPACE_3).child(input.clone())
    }

    fn picker(&self, tokens: &Tokens, overview: &Overview, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = if overview.detached {
            crate::t!(Key::Detached, id = overview.branch)
        } else {
            overview.branch.clone()
        };
        div()
            .id("git-branch")
            .button(tokens, crate::t!(Key::BranchGroup))
            .flex_none()
            .mx(tokens::SPACE_3)
            .h(tokens::FIELD_HEIGHT)
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .justify_between()
            .gap(tokens::SPACE_2)
            .bg(tokens.field)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .on_click(cx.listener(|panel, _, _, cx| {
                panel.branches_open = !panel.branches_open;
                panel.confirm_delete = None;
                cx.notify();
            }))
            .child(div().flex_1().min_w_0().truncate().child(name))
            .child(
                div()
                    .flex_none()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(sync_words(overview.sync)),
            )
    }

    fn branch_row(
        &self,
        at: usize,
        branch: &Branch,
        tokens: &Tokens,
        can_move: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let name = branch.name.clone();
        let asked = self.confirm_delete.as_deref() == Some(branch.name.as_str());
        let switch = branch.name.clone();
        let merge = branch.name.clone();
        let delete = branch.name.clone();
        let merged = branch.merged;
        let current = branch.current;
        div()
            .id(("git-branch-row", at))
            .button(tokens, name.clone())
            .flex_none()
            .min_h(tokens::VERSIONS_ROW_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .text_size(tokens::TEXT_SMALL)
            .text_color(if current { tokens.text } else { tokens.text_quiet })
            .when(!current && can_move, |row| {
                row.cursor_pointer()
                    .hover(|row| row.bg(tokens.raised))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.branches_open = false;
                        if let Some(view) = panel.view.upgrade() {
                            let branch = switch.clone();
                            view.update(cx, |view, cx| view.switch(branch, window, cx));
                        }
                    }))
            })
            .child(div().flex_1().min_w_0().truncate().child(name))
            .when(current, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_color(tokens.text_faint)
                        .child(crate::t!(Key::Current)),
                )
            })
            .when(!current && can_move, |row| {
                row.child(
                    ui::button(tokens, ("git-merge", at), crate::t!(Key::Merge)).on_click(cx.listener(
                        move |panel, _, window, cx| {
                            panel.branches_open = false;
                            if let Some(view) = panel.view.upgrade() {
                                let branch = merge.clone();
                                view.update(cx, |view, cx| view.merge_branch(branch, window, cx));
                            }
                        },
                    )),
                )
            })
            .when(!current, |row| {
                let label = if asked {
                    crate::t!(Key::DeleteUnmerged)
                } else {
                    crate::t!(Key::Delete)
                };
                row.child(
                    ui::button(tokens, ("git-delete", at), label)
                        .when(asked, |button| button.text_color(tokens.accent))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            // Git's own rule: a branch whose work is nowhere else asks first.
                            if !merged && panel.confirm_delete.as_deref() != Some(delete.as_str()) {
                                panel.confirm_delete = Some(delete.clone());
                                cx.notify();
                                return;
                            }
                            panel.confirm_delete = None;
                            if let Some(view) = panel.view.upgrade() {
                                let branch = delete.clone();
                                view.update(cx, |view, cx| view.delete_branch(branch, window, cx));
                            }
                        })),
                )
            })
    }
}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let root = div()
            .id("git-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(tokens.panel)
            .text_size(tokens::TEXT_BODY)
            .child(ui::section_label(&tokens, crate::t!(Key::GitLabel)));
        let Some(view) = self.view.upgrade() else {
            return root.into_any_element();
        };
        let read = view.read(cx);
        let overview = match read.state() {
            RepoState::Single => {
                return root
                    .child(ui::quiet_line(&tokens, crate::t!(Key::SingleFile)).pt(tokens::SPACE_2))
                    .into_any_element();
            }
            RepoState::Reading => {
                return root
                    .child(ui::quiet_line(&tokens, crate::t!(Key::Reading)).pt(tokens::SPACE_2))
                    .into_any_element();
            }
            RepoState::Broken(reason) => {
                return root
                    .child(ui::accent_line(&tokens, reason.clone()).pt(tokens::SPACE_2))
                    .into_any_element();
            }
            RepoState::NoRepo => {
                let cloud = read.book_folder(cx).as_deref().and_then(repo::cloud_service);
                return root
                    .child(ui::quiet_line(&tokens, crate::t!(Key::NoRepoLine)).pt(tokens::SPACE_2))
                    .children(
                        cloud.map(|service| ui::accent_line(&tokens, crate::t!(Key::CloudWarning, service = service))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .justify_end()
                            .px(tokens::SPACE_3)
                            .pt(tokens::SPACE_2)
                            .child(
                                ui::button(&tokens, "panel-make-repo", crate::t!(Key::MakeRepo))
                                    .text_color(tokens.accent)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        if let Some(view) = panel.view.upgrade() {
                                            view.update(cx, |view, cx| view.make_repo(window, cx));
                                        }
                                    })),
                            ),
                    )
                    .into_any_element();
            }
            RepoState::Ready(overview) => overview.clone(),
        };
        let installed = read.git_installed();
        let error = read.error().map(str::to_owned);
        let cloud = read.book_folder(cx).as_deref().and_then(repo::cloud_service);
        let can_move = installed && overview.busy.is_none();
        let rows: Vec<_> = (overview.changes.iter())
            .map(|change| {
                let words = change_words(change, read.in_conflict(&change.path, cx));
                ui::change_row(&tokens, read.title_of(&change.path, &change.name, cx), words).into_any_element()
            })
            .collect();
        let now = super::history::now_ms() / 1000;
        let history: Vec<_> = (overview.log.iter())
            .map(|commit| {
                ui::change_row(&tokens, commit.summary.clone(), dates::short(commit.time, now)).into_any_element()
            })
            .collect();
        let branch_rows: Vec<_> = if self.branches_open {
            (overview.branches.iter().enumerate())
                .map(|(at, branch)| self.branch_row(at, branch, &tokens, can_move, cx).into_any_element())
                .collect()
        } else {
            Vec::new()
        };
        let can_push = installed && overview.remote.is_some();
        let can_pull = can_push && !matches!(overview.sync, repo::Sync::NotPushed);
        root.children(cloud.map(|service| {
            ui::accent_line(&tokens, crate::t!(Key::CloudWarning, service = service)).pt(tokens::SPACE_2)
        }))
        .children((!installed).then(|| ui::quiet_line(&tokens, crate::t!(Key::GitMissing)).pt(tokens::SPACE_2)))
        .children(
            overview
                .busy
                .as_ref()
                .map(|operation| ui::accent_line(&tokens, crate::t!(Key::BusyElsewhere, operation = operation))),
        )
        .child(ui::group_label(&tokens, crate::t!(Key::BranchGroup)))
        .child(self.picker(&tokens, &overview, cx))
        .when(self.branches_open, |root| {
            root.child(
                div()
                    .flex_none()
                    .pt(tokens::SPACE_1)
                    .flex()
                    .flex_col()
                    .children(branch_rows),
            )
            .child(
                div()
                    .flex_none()
                    .pt(tokens::SPACE_1)
                    .child(Self::field(&self.new_branch)),
            )
        })
        .children(overview.merging.as_ref().map(|message| {
            div()
                .flex_none()
                .pt(tokens::SPACE_2)
                .flex()
                .flex_col()
                .child(ui::accent_line(
                    &tokens,
                    crate::t!(Key::MergingBanner, message = message),
                ))
                .child(ui::quiet_line(&tokens, crate::t!(Key::ConflictsOnLine)))
                .when(installed, |banner| {
                    banner.child(div().flex_none().flex().justify_end().px(tokens::SPACE_3).child(
                        ui::button(&tokens, "git-abort", crate::t!(Key::AbortMerge)).on_click(cx.listener(
                            |panel, _, window, cx| {
                                if let Some(view) = panel.view.upgrade() {
                                    view.update(cx, |view, cx| view.abort_merge(window, cx));
                                }
                            },
                        )),
                    ))
                })
        }))
        .child(ui::group_label(&tokens, crate::t!(Key::ChangesGroup)))
        .when(rows.is_empty(), |root| {
            root.child(ui::quiet_line(&tokens, crate::t!(Key::NothingChanged)))
        })
        .children(rows)
        .child(div().flex_none().pt(tokens::SPACE_2).child(Self::field(&self.message)))
        .children(error.map(|error| ui::accent_line(&tokens, error).pt(tokens::SPACE_2)))
        .child(
            div()
                .flex_none()
                .flex()
                .justify_end()
                .flex_wrap()
                .gap(tokens::SPACE_1)
                .px(tokens::SPACE_3)
                .pt(tokens::SPACE_2)
                .when(can_pull, |row| {
                    row.child(
                        ui::button(&tokens, "git-pull", crate::t!(Key::Pull)).on_click(cx.listener(
                            |panel, _, window, cx| {
                                if let Some(view) = panel.view.upgrade() {
                                    view.update(cx, |view, cx| view.pull(window, cx));
                                }
                            },
                        )),
                    )
                })
                .when(can_push, |row| {
                    row.child(
                        ui::button(&tokens, "git-push", crate::t!(Key::Push)).on_click(cx.listener(
                            |panel, _, window, cx| {
                                if let Some(view) = panel.view.upgrade() {
                                    view.update(cx, |view, cx| view.push(window, cx));
                                }
                            },
                        )),
                    )
                })
                .child(
                    ui::button(&tokens, "git-commit", crate::t!(Key::Commit))
                        .text_color(tokens.accent)
                        .on_click(cx.listener(|panel, _, window, cx| panel.commit(false, window, cx))),
                ),
        )
        .child(ui::group_label(&tokens, crate::t!(Key::HistoryGroup)))
        .when(history.is_empty(), |root| {
            root.child(ui::quiet_line(&tokens, crate::t!(Key::NoCommits)))
        })
        .children(history)
        .child(div().flex_none().h(tokens::SPACE_3))
        .into_any_element()
    }
}

/// The panel at the right of the text. In Publish, once a rail tab holds the panel, [`Versions`] shows the tab instead.
pub struct Slot {
    panel: Entity<GitPanel>,
    view: WeakEntity<Versions>,
}

impl Slot {
    pub fn new(panel: Entity<GitPanel>, view: WeakEntity<Versions>) -> Self {
        Self { panel, view }
    }
}

impl Render for Slot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let in_rail = self.view.upgrade().is_some_and(|view| {
            let view = view.read(cx);
            view.mode(cx) == Mode::Publish && view.has_rail_tab(cx)
        });
        if !self.panel.read(cx).is_open() || in_rail {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx);
        div()
            .flex_none()
            .w(tokens::INSPECTOR_WIDTH)
            .h_full()
            .border_l_1()
            .border_color(tokens.divider)
            .child(self.panel.clone())
            .into_any_element()
    }
}
