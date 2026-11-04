//! The menu bar and the keys the shell adds. Actions the shell owns are declared here; the Edit menu uses the
//! galley's own actions and the View menu the workspace's and the preview's, so a menu item does what its key does.

use std::path::{Path, PathBuf};

use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

use super::text::Key;
use crate::app::{NewBook, NewOffer, NewReport, Open, Quit};
use crate::editor::{
    Copy, Cut, InsertCitation, InsertFootnote, InsertImage, InsertReference, InsertTable, Paste, Redo, Save, SelectAll,
    ShowSpelling, TogglePrintLines, ToggleSpelling, Undo,
};
use crate::preview::{ZoomIn, ZoomOut, ZoomToFit};
use crate::workspace::{FocusNext, FocusPrevious, ShowPages, ShowSplit, ShowWrite};

actions!(
    shell,
    [
        Find,
        FindNext,
        FindPrevious,
        FindInProject,
        ShowSettings,
        ShowExport,
        ShowDiagnostics,
        ShowSelection,
        ShowStyles,
        ShowFonts,
        ShowImages,
        ShowDetails,
        ShowNotes,
        ShowAgent,
        FocusEditor,
        CloseDocument,
        ToggleSidebar,
        FocusSidebar,
        ToggleInspector,
        About,
        HideApp,
        HideOthers,
        MinimizeWindow,
        ZoomWindow,
        OpenHelp,
        ClearRecent,
    ]
);

actions!(
    recent,
    [
        Recent0, Recent1, Recent2, Recent3, Recent4, Recent5, Recent6, Recent7, Recent8, Recent9
    ]
);

/// What the menus show that changes while the app runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuState {
    pub recent: Vec<PathBuf>,
    pub sidebar_shown: bool,
    pub inspector_shown: bool,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseDocument, None),
        KeyBinding::new("cmd-,", ShowSettings, None),
        KeyBinding::new("cmd-f", Find, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-shift-f", FindInProject, None),
        KeyBinding::new("cmd-e", ShowExport, None),
        KeyBinding::new("cmd-shift-d", ShowDiagnostics, None),
        KeyBinding::new("cmd-alt-s", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-1", FocusSidebar, None),
        KeyBinding::new("cmd-alt-0", FocusEditor, None),
        KeyBinding::new("ctrl-cmd-1", ShowSelection, None),
        KeyBinding::new("ctrl-cmd-2", ShowStyles, None),
        KeyBinding::new("ctrl-cmd-3", ShowFonts, None),
        KeyBinding::new("ctrl-cmd-4", ShowImages, None),
        KeyBinding::new("ctrl-cmd-5", ShowDetails, None),
        KeyBinding::new("ctrl-cmd-6", ShowNotes, None),
        KeyBinding::new("ctrl-cmd-7", ShowAgent, None),
        KeyBinding::new("tab", FocusNext, Some("Workspace")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("Workspace")),
        KeyBinding::new("ctrl-tab", FocusNext, Some("Workspace")),
        KeyBinding::new("ctrl-shift-tab", FocusPrevious, Some("Workspace")),
        KeyBinding::new("cmd-alt-i", ToggleInspector, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
    ]);
}

/// The name a recent document shows under: its folder or file name.
pub fn recent_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn recent_action(index: usize) -> Box<dyn gpui::Action> {
    match index {
        0 => Box::new(Recent0),
        1 => Box::new(Recent1),
        2 => Box::new(Recent2),
        3 => Box::new(Recent3),
        4 => Box::new(Recent4),
        5 => Box::new(Recent5),
        6 => Box::new(Recent6),
        7 => Box::new(Recent7),
        8 => Box::new(Recent8),
        _ => Box::new(Recent9),
    }
}

fn item(key: Key, action: impl gpui::Action) -> MenuItem {
    MenuItem::action(crate::t!(key), action)
}

pub fn build(state: &MenuState) -> Vec<Menu> {
    let mut recent: Vec<MenuItem> = state
        .recent
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, path)| MenuItem::Action {
            name: recent_name(path).into(),
            action: recent_action(index),
            os_action: None,
            checked: false,
            disabled: false,
        })
        .collect();
    if !recent.is_empty() {
        recent.push(MenuItem::separator());
    }
    recent.push(item(Key::ClearRecent, ClearRecent).disabled(state.recent.is_empty()));
    vec![
        Menu::new("Druck").items([
            item(Key::About, About),
            MenuItem::separator(),
            item(Key::Settings, ShowSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(crate::t!(Key::Services), SystemMenuType::Services),
            MenuItem::separator(),
            item(Key::HideApp, HideApp),
            item(Key::HideOthers, HideOthers),
            MenuItem::separator(),
            item(Key::Quit, Quit),
        ]),
        Menu::new(crate::t!(Key::MenuFile)).items([
            MenuItem::submenu(Menu::new(crate::t!(Key::New)).items([
                item(Key::NewBook, NewBook),
                item(Key::NewReport, NewReport),
                item(Key::NewOffer, NewOffer),
            ])),
            item(Key::Open, Open),
            MenuItem::submenu(Menu::new(crate::t!(Key::OpenRecent)).items(recent)),
            MenuItem::separator(),
            item(Key::Close, CloseDocument),
            item(Key::SaveItem, Save),
            MenuItem::separator(),
            item(Key::ExportItem, ShowExport),
        ]),
        Menu::new(crate::t!(Key::MenuEdit)).items([
            item(Key::Undo, Undo),
            item(Key::Redo, Redo),
            MenuItem::separator(),
            item(Key::Cut, Cut),
            item(Key::Copy, Copy),
            item(Key::Paste, Paste),
            item(Key::SelectAll, SelectAll),
            MenuItem::separator(),
            item(Key::Find, Find),
            item(Key::FindNext, FindNext),
            item(Key::FindPrevious, FindPrevious),
            item(Key::FindInProject, FindInProject),
            MenuItem::separator(),
            item(Key::NextMisspelled, ShowSpelling),
            item(Key::CheckSpelling, ToggleSpelling),
        ]),
        Menu::new(crate::t!(Key::MenuView)).items([
            item(Key::ViewWrite, ShowWrite),
            item(Key::ViewPages, ShowPages),
            item(Key::ViewSplit, ShowSplit),
            item(Key::ViewPrintLines, TogglePrintLines),
            MenuItem::separator(),
            item(Key::ZoomIn, ZoomIn),
            item(Key::ZoomOut, ZoomOut),
            item(Key::ZoomFit, ZoomToFit),
            MenuItem::separator(),
            if state.sidebar_shown {
                item(Key::HideSidebar, ToggleSidebar)
            } else {
                item(Key::ShowSidebar, ToggleSidebar)
            },
            item(Key::FocusSidebar, FocusSidebar),
            item(Key::FocusEditor, FocusEditor),
            MenuItem::submenu(Menu::new(crate::t!(Key::Inspector)).items([
                item(Key::ShowSelection, ShowSelection),
                item(Key::ShowStyles, ShowStyles),
                item(Key::ShowFonts, ShowFonts),
                item(Key::ShowImages, ShowImages),
                item(Key::ShowDetails, ShowDetails),
                item(Key::ShowNotes, ShowNotes),
                item(Key::ShowAgent, ShowAgent),
                item(Key::ShowDiagnostics, ShowDiagnostics),
                item(Key::ExportItem, ShowExport),
            ])),
            if state.inspector_shown {
                item(Key::HideInspector, ToggleInspector)
            } else {
                item(Key::ShowInspector, ToggleInspector)
            },
        ]),
        Menu::new(crate::t!(Key::MenuInsert)).items([
            item(Key::InsertFootnote, InsertFootnote),
            item(Key::InsertCitation, InsertCitation),
            item(Key::InsertReference, InsertReference),
            item(Key::InsertImage, InsertImage),
            item(Key::InsertTable, InsertTable),
        ]),
        Menu::new(crate::t!(Key::MenuWindow))
            .items([item(Key::Minimize, MinimizeWindow), item(Key::WindowZoom, ZoomWindow)]),
        Menu::new(crate::t!(Key::MenuHelp)).items([item(Key::HelpWeb, OpenHelp)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_bar_has_its_menus_in_the_language_set() {
        let _guard = super::super::strings::LANGUAGE_TEST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::super::strings::set_language(super::super::strings::Language::German);
        let menus = build(&MenuState::default());
        let names: Vec<_> = menus.iter().map(|menu| menu.name.to_string()).collect();
        assert_eq!(
            names,
            [
                "Druck",
                "Ablage",
                "Bearbeiten",
                "Darstellung",
                "Einfügen",
                "Fenster",
                "Hilfe"
            ]
        );
        super::super::strings::set_language(super::super::strings::Language::English);
        assert_eq!(build(&MenuState::default())[1].name, "File");
    }

    #[test]
    fn recent_documents_list_under_open_recent_with_a_clear_item() {
        let _guard = super::super::strings::LANGUAGE_TEST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = MenuState {
            recent: vec!["/books/one".into(), "/books/two".into()],
            ..MenuState::default()
        };
        let file = &build(&state)[1];
        let MenuItem::Submenu(recent) = &file.items[2] else {
            panic!("open recent is a submenu");
        };
        let names: Vec<_> = recent
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["one", "two", "Clear Menu"]);
    }
}
