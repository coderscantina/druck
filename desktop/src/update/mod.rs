//! Updates through Sparkle 2, in the bundle `scripts/package-desktop.py` builds. The Info.plist it writes holds the
//! feed, the public key, and the choices: a check once a day, and Sparkle's own prompt with the release notes before
//! anything is installed. A build run from cargo has no framework beside it, so no updater and no menu item.

#[cfg(target_os = "macos")]
mod macos;

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, MenuItem, actions};

use crate::shell::text::Key;

actions!(update, [CheckForUpdates]);

/// Whether an updater runs, so the menus know to show its item.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Starts the updater when the bundle carries Sparkle. Call before the menus are first built.
pub fn install(cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some(updater) = macos::Updater::start() {
        cx.set_global(updater);
        cx.on_action(|_: &CheckForUpdates, cx| cx.global::<macos::Updater>().check());
        RUNNING.store(true, Ordering::Relaxed);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

/// Whether the bundle's updater started, so the menu and the palette offer it.
pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// The app menu's "Check for updates…", while an updater runs.
pub fn menu_item() -> Option<MenuItem> {
    running().then(item)
}

fn item() -> MenuItem {
    MenuItem::action(crate::t!(Key::CheckForUpdates), CheckForUpdates)
}

#[cfg(test)]
mod tests {
    use crate::shell::strings::{LANGUAGE_TEST, Language, set_language};

    #[test]
    fn the_item_names_the_check_in_the_language_set() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let name = |language| {
            set_language(language);
            match super::item() {
                gpui::MenuItem::Action { name, .. } => name.to_string(),
                _ => unreachable!("the item is an action"),
            }
        };
        assert_eq!(name(Language::German), "Nach Updates suchen\u{2026}");
        assert_eq!(name(Language::English), "Check for updates\u{2026}");
    }
}
