//! Everything the app can do by name, in one table. The command palette lists it, and the settings window's Keys list
//! reads it. A new command is a line here. The menus are hand-built, and a test keeps every item of theirs in this table.

use gpui::{Action, App};

use super::menus::*;
use super::text::Key;
use crate::app::{NewArticle, NewBook, NewPaper, Open, Quit};
use crate::editor::{
    Copy, Cut, EditRich, InsertCitation, InsertFootnote, InsertImage, InsertReference, InsertTable, OpenLink, Paste,
    Redo, Save, SelectAll, ShowSpelling, ToggleBold, ToggleBullets, ToggleItalic, ToggleNumbers, ToggleQuote,
    ToggleSmallCaps, ToggleSource, ToggleStrike, ToggleSubscript, ToggleSuperscript, ToggleUnderline, Undo,
};
use crate::persist::ShowKeptCopies;
use crate::versions::{ShowCommitCard, ShowGitPanel, ShowTimeline};
use crate::workspace::{NewSection, ShowPublish, ShowWrite, ZoomIn, ZoomOut, ZoomReset};

/// Where a command makes sense. The palette leaves out what the mode on screen has no use for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Anywhere,
    /// Needs the text on screen.
    Write,
    /// Only while the agent is enabled in Settings.
    Agent,
}

pub struct Command {
    pub label: Key,
    pub action: Box<dyn Action>,
    pub scope: Scope,
}

impl Command {
    /// How the palette and the Keys list name the command: as the menu does, without the ellipsis a menu item that asks
    /// for more shows by macOS's rule.
    pub fn name(&self) -> &'static str {
        crate::t!(self.label).trim_end_matches('\u{2026}')
    }

    /// Whether the settings let this command exist at all. With the agent off, its commands leave no trace.
    pub fn is_available(&self, cx: &App) -> bool {
        self.scope != Scope::Agent || crate::settings::get(cx).agent
    }
}

fn command(label: Key, scope: Scope, action: impl Action) -> Command {
    Command {
        label,
        action: Box::new(action),
        scope,
    }
}

/// Every command, roughly by how often it is wanted.
pub fn all() -> Vec<Command> {
    use Scope::{Agent, Anywhere, Write};
    vec![
        command(Key::ViewWrite, Anywhere, ShowWrite),
        command(Key::ViewPublish, Anywhere, ShowPublish),
        command(Key::ToggleBinder, Anywhere, ToggleBinder),
        command(Key::FocusBinder, Anywhere, FocusBinder),
        command(Key::FocusOutline, Write, FocusOutline),
        command(Key::FocusEditor, Anywhere, FocusEditor),
        command(Key::GoToPath, Anywhere, crate::navigator::ShowColumns),
        command(Key::BookOverview, Anywhere, crate::navigator::ShowOverview),
        command(Key::Find, Write, Find),
        command(Key::FindNext, Write, FindNext),
        command(Key::FindPrevious, Write, FindPrevious),
        command(Key::FindInProject, Anywhere, FindInProject),
        command(Key::SaveItem, Anywhere, Save),
        command(Key::ExportItem, Anywhere, ShowExport),
        command(Key::CommitCard, Anywhere, ShowCommitCard),
        command(Key::Timeline, Anywhere, ShowTimeline),
        command(Key::GitPanel, Anywhere, ShowGitPanel),
        command(Key::KeptCopies, Anywhere, ShowKeptCopies),
        command(Key::Settings, Anywhere, ShowSettings),
        command(Key::SourceView, Write, ToggleSource),
        command(Key::NewSection, Anywhere, NewSection),
        command(Key::Bold, Write, ToggleBold),
        command(Key::Italic, Write, ToggleItalic),
        command(Key::Underline, Write, ToggleUnderline),
        command(Key::Strike, Write, ToggleStrike),
        command(Key::Superscript, Write, ToggleSuperscript),
        command(Key::Subscript, Write, ToggleSubscript),
        command(Key::SmallCaps, Write, ToggleSmallCaps),
        command(Key::Quote, Write, ToggleQuote),
        command(Key::Bullets, Write, ToggleBullets),
        command(Key::Numbers, Write, ToggleNumbers),
        command(Key::Link, Write, OpenLink),
        command(Key::InsertFootnote, Write, InsertFootnote),
        command(Key::InsertCitation, Write, InsertCitation),
        command(Key::InsertReference, Write, InsertReference),
        command(Key::InsertImage, Write, InsertImage),
        command(Key::InsertTable, Write, InsertTable),
        command(Key::EditAtCaret, Write, EditRich),
        command(Key::NextMisspelled, Write, ShowSpelling),
        command(Key::CheckSpelling, Anywhere, ToggleSpelling),
        command(Key::Typewriter, Anywhere, ToggleTypewriter),
        command(Key::DimParagraphs, Anywhere, ToggleDim),
        command(Key::SelectionToolbar, Anywhere, ToggleSelectionToolbar),
        command(Key::ZoomIn, Anywhere, ZoomIn),
        command(Key::ZoomOut, Anywhere, ZoomOut),
        command(Key::ZoomReset, Anywhere, ZoomReset),
        command(Key::ShowStyles, Anywhere, ShowStyles),
        command(Key::ChooseTheme, Anywhere, ChooseTheme),
        command(Key::ShowFonts, Anywhere, ShowFonts),
        command(Key::ShowImages, Anywhere, ShowImages),
        command(Key::ShowDetails, Anywhere, ShowDetails),
        command(Key::AskAgent, Agent, AskAgent),
        command(Key::ShowAgent, Agent, ShowAgent),
        command(Key::ShowDiagnostics, Anywhere, ShowDiagnostics),
        command(Key::NextProblem, Anywhere, NextProblem),
        command(Key::PreviousProblem, Anywhere, PreviousProblem),
        command(Key::Undo, Write, Undo),
        command(Key::Redo, Write, Redo),
        command(Key::Cut, Write, Cut),
        command(Key::Copy, Write, Copy),
        command(Key::Paste, Write, Paste),
        command(Key::SelectAll, Write, SelectAll),
        command(Key::Open, Anywhere, Open),
        command(Key::NewBook, Anywhere, NewBook),
        command(Key::NewPaper, Anywhere, NewPaper),
        command(Key::NewArticle, Anywhere, NewArticle),
        command(Key::Close, Anywhere, CloseDocument),
        command(Key::ClearRecent, Anywhere, ClearRecent),
        command(Key::About, Anywhere, About),
        command(Key::HelpWeb, Anywhere, OpenHelp),
        command(Key::HideApp, Anywhere, HideApp),
        command(Key::HideOthers, Anywhere, HideOthers),
        command(Key::Minimize, Anywhere, MinimizeWindow),
        command(Key::WindowZoom, Anywhere, ZoomWindow),
        command(Key::Quit, Anywhere, Quit),
    ]
}

/// The key that runs an action, in the platform's symbols, whatever has the focus now.
pub fn keys_of(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).last()?;
    let os = cx.platform_info().os;
    Some(
        binding
            .keystrokes()
            .iter()
            .map(|keystroke| {
                let shown = keystroke.display(os).to_string();
                // gpui spells out the keys it has no symbol for, as in "⌘enter".
                match (os, mac_symbol(keystroke.key())) {
                    (gpui::OperatingSystem::Mac, Some(symbol)) => {
                        format!("{}{symbol}", shown.strip_suffix(keystroke.key()).unwrap_or(&shown))
                    }
                    _ => shown,
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The symbol macOS menus show for a key gpui writes as a word.
fn mac_symbol(key: &str) -> Option<&'static str> {
    Some(match key {
        "enter" => "\u{21a9}",
        "delete" => "\u{2326}",
        "home" => "\u{2196}",
        "end" => "\u{2198}",
        "pageup" => "\u{21de}",
        "pagedown" => "\u{21df}",
        "space" => "\u{2423}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gpui::{Menu, MenuItem};

    use super::*;
    use crate::shell::strings::{LANGUAGE_TEST, Language, set_language};

    fn collect(menu: &Menu, names: &mut Vec<String>) {
        for item in &menu.items {
            match item {
                MenuItem::Action { action, .. } => names.push(action.name().to_owned()),
                MenuItem::Submenu(submenu) => collect(submenu, names),
                _ => {}
            }
        }
    }

    #[test]
    fn every_menu_item_is_a_command() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_language(Language::English);
        let commands: HashSet<_> = all().iter().map(|command| command.action.name().to_owned()).collect();
        let mut names = Vec::new();
        for menu in build(&MenuState::default()) {
            collect(&menu, &mut names);
        }
        let missing: Vec<_> = names.iter().filter(|name| !commands.contains(*name)).collect();
        assert!(missing.is_empty(), "not in the palette: {missing:?}");
    }

    #[test]
    fn command_names_are_in_sentence_case() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_language(Language::English);
        let names = ["Druck", "Binder"];
        for command in all() {
            let capitals: Vec<_> = (command.name().split_whitespace().skip(1))
                .filter(|word| word.starts_with(char::is_uppercase) && !names.contains(word))
                .collect();
            assert!(capitals.is_empty(), "{}: {capitals:?}", command.name());
        }
    }

    #[test]
    fn a_command_appears_once() {
        let names: Vec<_> = all().iter().map(|command| command.action.name().to_owned()).collect();
        let unique: HashSet<_> = names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }
}
