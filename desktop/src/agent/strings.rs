//! Every string the agent panel shows, in English and German.

use super::approval::SessionLabel;

crate::strings! {
    pub enum Key {
        Title => ("AGENT", "AGENT"),
        NewConversation => ("New", "Neu"),
        Model => ("Model", "Modell"),
        ModelDefault => ("Default", "Standard"),
        Placeholder => ("Ask about this text\u{2026}", "Frag etwas zu diesem Text\u{2026}"),
        Send => ("Send", "Senden"),
        Stop => ("Stop", "Stopp"),
        Checking => ("Looking for Claude Code\u{2026}", "Suche nach Claude Code\u{2026}"),
        Stopped => ("Stopped.", "Angehalten."),
        Working => ("Working\u{2026}", "Arbeitet\u{2026}"),
        Stopping => ("Stopping\u{2026}", "Halte an\u{2026}"),
        Resumed => (
            "This project has an earlier conversation. Claude Code keeps it, so the next prompt continues it.",
            "Dieses Projekt hat eine frühere Unterhaltung. Claude Code bewahrt sie auf, der nächste Prompt setzt sie fort."
        ),
        SetupTitle => ("Set up Claude Code", "Claude Code einrichten"),
        SetupMissing => (
            "The agent panel runs the Claude Code program on this computer. It is not installed or not on the search path.",
            "Das Agent-Panel startet das Programm Claude Code auf diesem Computer. Es ist nicht installiert oder nicht im Suchpfad."
        ),
        SetupSteps => (
            "1. Install Claude Code.\n2. Set it up with an API key, as Anthropic's documentation describes.\n3. Reopen this tab.",
            "1. Claude Code installieren.\n2. Mit einem API-Schlüssel einrichten, wie in Anthropics Dokumentation beschrieben.\n3. Diesen Tab erneut öffnen."
        ),
        SetupOwner => (
            "Druck holds no keys and sends nothing itself. Claude Code does the login, and Druck only starts it. Everything else in Druck works without it.",
            "Druck speichert keine Schlüssel und sendet nichts selbst. Claude Code übernimmt die Anmeldung, Druck startet es nur. Alles andere in Druck funktioniert auch ohne."
        ),
        SubscriptionNote => (
            "Whether Anthropic allows a subscription login for an app like Druck is not confirmed. An API key is the documented way.",
            "Ob Anthropic eine Abo-Anmeldung für eine App wie Druck erlaubt, ist nicht bestätigt. Ein API-Schlüssel ist der dokumentierte Weg."
        ),
        LoggedOut => (
            "Claude Code reports that nobody is logged in. Set it up with an API key before the first prompt.",
            "Claude Code meldet, dass niemand angemeldet ist. Vor dem ersten Prompt mit einem API-Schlüssel einrichten."
        ),
        Intro => (
            "Ask Claude Code about this project. It works in the project folder and reads its CLAUDE.md, skills, and MCP servers as in a terminal. What it changes shows here, and you can take it back.",
            "Frag Claude Code zu diesem Projekt. Es arbeitet im Projektordner und liest CLAUDE.md, Skills und MCP-Server wie im Terminal. Was es ändert, steht hier, und du kannst es zurücknehmen."
        ),
        AllowOnce => ("Allow once", "Einmal erlauben"),
        Deny => ("Deny", "Ablehnen"),
        DenyWithReason => ("Deny with reason\u{2026}", "Mit Grund ablehnen\u{2026}"),
        ReasonLabel => ("Reason, which Claude sees", "Grund, den Claude sieht"),
        Back => ("Back", "Zurück"),
        SendDenial => ("Deny", "Ablehnen"),
        ScopeOnce => ("Allows this call only.", "Erlaubt nur diesen Aufruf."),
        ScopeSession => (
            "A session choice lasts until you close Druck or start a new conversation.",
            "Eine Sitzungswahl gilt, bis du Druck schließt oder eine neue Unterhaltung beginnst."
        ),
        NewFile => ("A new file", "Eine neue Datei"),
        UnsavedWarning => (
            "This chapter has text not saved yet. The agent works on the saved file, and its change merges with your text.",
            "Dieses Kapitel hat noch nicht gesicherten Text. Der Agent arbeitet an der gesicherten Datei, seine Änderung wird mit deinem Text zusammengeführt."
        ),
        ContextSent => ("Sent with this prompt", "Mit diesem Prompt gesendet"),
        ToolDenied => ("denied", "abgelehnt"),
        ToolFailed => ("failed", "fehlgeschlagen"),
        ToolWaiting => ("waiting", "wartet"),
        Revert => ("Revert", "Rückgängig"),
        Show => ("Show", "Zeigen"),
        HunkReverted => ("Reverted", "Zurückgesetzt"),
        HunkMoved => ("Text changed since", "Text seither geändert"),
        HunkNotOpen => ("Not open here", "Hier nicht geöffnet"),
        CliUnreadable => (
            "Claude Code did not report a version Druck can read.",
            "Claude Code meldet keine Version, die Druck lesen kann."
        ),

        ChipChapter => ("Chapter {path}", "Kapitel {path}"),
        ChipSelectionOne => ("Selection, {lines} line", "Auswahl, {lines} Zeile"),
        ChipSelectionMany => ("Selection, {lines} lines", "Auswahl, {lines} Zeilen"),
        ChipProblemOne => ("{count} problem", "{count} Problem"),
        ChipProblemMany => ("{count} problems", "{count} Probleme"),
        CliUntested => (
            "Claude Code {version} is not a version Druck was tested with. It may still work.",
            "Claude Code {version} gehört nicht zu den getesteten Versionen. Es kann trotzdem funktionieren."
        ),
        LoginSubscription => ("a Claude subscription login", "eine Claude-Abo-Anmeldung"),
        LoginApiKey => ("an API key", "einen API-Schlüssel"),
        LoginReports => ("Claude Code reports {what}{plan}.", "Claude Code meldet {what}{plan}."),
        LoginUnknown => (
            "Claude Code did not say which login it uses.",
            "Claude Code sagt nicht, welche Anmeldung es nutzt."
        ),
        AllowTool => ("{tool} {target}?", "{tool} {target} erlauben?"),
        MoreWaiting => ("{count} more waiting", "{count} weitere warten"),
        SessionAcceptEdits => ("Allow all edits this session", "Alle Änderungen in dieser Sitzung erlauben"),
        SessionMode => ("Use mode {mode} this session", "Modus {mode} in dieser Sitzung nutzen"),
        SessionRule => ("Always allow {rule} this session", "{rule} in dieser Sitzung immer erlauben"),
        SessionOther => ("Always allow this this session", "Das in dieser Sitzung immer erlauben"),
        ChangesOneOne => ("1 change in 1 file", "1 Änderung in 1 Datei"),
        ChangesManyOne => ("{hunks} changes in 1 file", "{hunks} Änderungen in 1 Datei"),
        ChangesManyMany => ("{hunks} changes in {files} files", "{hunks} Änderungen in {files} Dateien"),
        CreatedFile => ("Created {path}", "{path} angelegt"),
        DeletedFile => ("{path} was deleted", "{path} gelöscht"),
        HunkLine => ("line {line}", "Zeile {line}"),
        MergeConflict => (
            "The agent changed {path} where you are writing too. Your text stays, and the footer asks what to do.",
            "Der Agent hat {path} dort geändert, wo du auch schreibst. Dein Text bleibt, die Fußzeile fragt, was geschehen soll."
        ),
        HunkConflict => (
            "This change could not be taken back without touching text you edited since, so it stays.",
            "Diese Änderung lässt sich nicht zurücknehmen, ohne deinen späteren Text zu berühren, deshalb bleibt sie."
        ),
        CannotStart => ("Claude Code did not start: {error}", "Claude Code ließ sich nicht starten: {error}"),
        CannotSend => ("The prompt did not reach Claude Code: {error}", "Der Prompt hat Claude Code nicht erreicht: {error}"),
        Exited => ("Claude Code stopped unexpectedly.{detail}", "Claude Code wurde unerwartet beendet.{detail}"),
    }
}

/// What the CLI is told a denial means. For the model, so never translated.
pub const DENIED_DEFAULT: &str = "The author declined this change.";
pub const STOPPED_DENIAL: &str = "The author stopped the turn.";

pub fn context_sent_with(labels: &[String]) -> String {
    format!("{}: {}", crate::t!(Key::ContextSent), labels.join(", "))
}

pub fn chip_chapter(path: &str) -> String {
    crate::t!(Key::ChipChapter, path = path)
}

pub fn chip_selection(lines: usize) -> String {
    crate::t!(
        if lines == 1 {
            Key::ChipSelectionOne
        } else {
            Key::ChipSelectionMany
        },
        lines = lines
    )
}

pub fn chip_diagnostics(count: usize) -> String {
    crate::t!(
        if count == 1 {
            Key::ChipProblemOne
        } else {
            Key::ChipProblemMany
        },
        count = count
    )
}

pub fn cli_version(version: &str) -> String {
    format!("Claude Code {version}")
}

pub fn cli_untested(version: &str) -> String {
    crate::t!(Key::CliUntested, version = version)
}

/// The login Claude Code reports, in plain words.
pub fn login_line(method: &str, plan: Option<&str>) -> String {
    let what = match method {
        "claude.ai" => crate::t!(Key::LoginSubscription),
        method if method.to_lowercase().contains("api") => crate::t!(Key::LoginApiKey),
        method => method,
    };
    let plan = plan.map(|plan| format!(" ({plan})")).unwrap_or_default();
    crate::t!(Key::LoginReports, what = what, plan = plan)
}

pub fn allow_tool(tool: &str, target: &str) -> String {
    crate::t!(Key::AllowTool, tool = tool, target = target)
}

pub fn more_waiting(count: usize) -> String {
    crate::t!(Key::MoreWaiting, count = count)
}

pub fn session_button(label: &SessionLabel) -> String {
    match label {
        SessionLabel::AcceptEdits => crate::t!(Key::SessionAcceptEdits).to_owned(),
        SessionLabel::Mode(mode) => crate::t!(Key::SessionMode, mode = mode),
        SessionLabel::Rule(rule) => crate::t!(Key::SessionRule, rule = rule),
        SessionLabel::Other => crate::t!(Key::SessionOther).to_owned(),
    }
}

pub fn turn_cost(cost: f64) -> String {
    let digits = if cost < 0.01 {
        4
    } else if cost < 1. {
        3
    } else {
        2
    };
    format!("${cost:.digits$}")
}

pub fn changes_summary(hunks: usize, files: usize) -> String {
    match (hunks, files) {
        (1, _) => crate::t!(Key::ChangesOneOne).to_owned(),
        (hunks, 1) => crate::t!(Key::ChangesManyOne, hunks = hunks),
        (hunks, files) => crate::t!(Key::ChangesManyMany, hunks = hunks, files = files),
    }
}

pub fn created_file(path: &str) -> String {
    crate::t!(Key::CreatedFile, path = path)
}

pub fn deleted_file(path: &str) -> String {
    crate::t!(Key::DeletedFile, path = path)
}

pub fn hunk_line(line: usize) -> String {
    crate::t!(Key::HunkLine, line = line)
}

pub fn merge_conflict(path: &str) -> String {
    crate::t!(Key::MergeConflict, path = path)
}

pub fn cannot_start(error: &str) -> String {
    crate::t!(Key::CannotStart, error = error)
}

pub fn cannot_send(error: &str) -> String {
    crate::t!(Key::CannotSend, error = error)
}

pub fn exited(stderr: &str) -> String {
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(" {stderr}")
    };
    crate::t!(Key::Exited, detail = detail)
}
