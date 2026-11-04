//! Every string the agent panel shows, in English and German. The language follows `DRUCK_LANG`, else the system's
//! `LC_ALL`, `LC_MESSAGES`, or `LANG`. The Shell slice's table replaces `language` with its own choice.

use std::sync::OnceLock;

use super::approval::SessionLabel;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

pub fn language() -> Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    *LANGUAGE.get_or_init(|| {
        let setting = ["DRUCK_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
        match setting {
            Some(setting) if setting.to_lowercase().starts_with("de") => Language::German,
            _ => Language::English,
        }
    })
}

/// A string in both languages.
pub struct Text(&'static str, &'static str);

impl Text {
    pub fn get(&self) -> &'static str {
        match language() {
            Language::English => self.0,
            Language::German => self.1,
        }
    }
}

/// Picks one of two strings for the current language.
fn pick(english: String, german: String) -> String {
    match language() {
        Language::English => english,
        Language::German => german,
    }
}

pub const TITLE: Text = Text("AGENT", "AGENT");
pub const NEW_CONVERSATION: Text = Text("New", "Neu");
pub const MODEL: Text = Text("Model", "Modell");
pub const MODEL_DEFAULT: Text = Text("Default", "Standard");
pub const PLACEHOLDER: Text = Text("Ask about this text\u{2026}", "Frag etwas zu diesem Text\u{2026}");
pub const SEND: Text = Text("Send", "Senden");
pub const STOP: Text = Text("Stop", "Stopp");
pub const CHECKING: Text = Text("Looking for Claude Code\u{2026}", "Suche nach Claude Code\u{2026}");
pub const STOPPED: Text = Text("Stopped.", "Angehalten.");
pub const WORKING: Text = Text("Working\u{2026}", "Arbeitet\u{2026}");
pub const STOPPING: Text = Text("Stopping\u{2026}", "Halte an\u{2026}");
pub const RESUMED: Text = Text(
    "This project has an earlier conversation. Claude Code keeps it, so the next prompt continues it.",
    "Dieses Projekt hat eine frühere Unterhaltung. Claude Code bewahrt sie auf, der nächste Prompt setzt sie fort.",
);

pub const SETUP_TITLE: Text = Text("Set up Claude Code", "Claude Code einrichten");
pub const SETUP_MISSING: Text = Text(
    "The agent panel runs the Claude Code program on this computer. It is not installed or not on the search path.",
    "Das Agent-Panel startet das Programm Claude Code auf diesem Computer. Es ist nicht installiert oder nicht im Suchpfad.",
);
pub const SETUP_STEPS: Text = Text(
    "1. Install Claude Code.\n2. Set it up with an API key, as Anthropic's documentation describes.\n3. Reopen this tab.",
    "1. Claude Code installieren.\n2. Mit einem API-Schlüssel einrichten, wie in Anthropics Dokumentation beschrieben.\n3. Diesen Tab erneut öffnen.",
);
pub const SETUP_OWNER: Text = Text(
    "Druck holds no keys and sends nothing itself. Claude Code does the login, and Druck only starts it. Everything else in Druck works without it.",
    "Druck speichert keine Schlüssel und sendet nichts selbst. Claude Code übernimmt die Anmeldung, Druck startet es nur. Alles andere in Druck funktioniert auch ohne.",
);
pub const SUBSCRIPTION_NOTE: Text = Text(
    "Whether Anthropic allows a subscription login for an app like Druck is not confirmed. An API key is the documented way.",
    "Ob Anthropic eine Abo-Anmeldung für eine App wie Druck erlaubt, ist nicht bestätigt. Ein API-Schlüssel ist der dokumentierte Weg.",
);
pub const LOGGED_OUT: Text = Text(
    "Claude Code reports that nobody is logged in. Set it up with an API key before the first prompt.",
    "Claude Code meldet, dass niemand angemeldet ist. Vor dem ersten Prompt mit einem API-Schlüssel einrichten.",
);
pub const INTRO: Text = Text(
    "Ask Claude Code about this project. It works in the project folder and reads its CLAUDE.md, skills, and MCP servers as in a terminal. Every change is asked about first, and shown here.",
    "Frag Claude Code zu diesem Projekt. Es arbeitet im Projektordner und liest CLAUDE.md, Skills und MCP-Server wie im Terminal. Jede Änderung wird erst erfragt und hier gezeigt.",
);

pub const ALLOW_ONCE: Text = Text("Allow once", "Einmal erlauben");
pub const DENY: Text = Text("Deny", "Ablehnen");
pub const DENY_WITH_REASON: Text = Text("Deny with reason\u{2026}", "Mit Grund ablehnen\u{2026}");
pub const REASON_LABEL: Text = Text("Reason, which Claude sees", "Grund, den Claude sieht");
pub const BACK: Text = Text("Back", "Zurück");
pub const SEND_DENIAL: Text = Text("Deny", "Ablehnen");
pub const DENIED_DEFAULT: &str = "The author declined this change.";
pub const STOPPED_DENIAL: &str = "The author stopped the turn.";
pub const SCOPE_ONCE: Text = Text("Allows this call only.", "Erlaubt nur diesen Aufruf.");
pub const SCOPE_SESSION: Text = Text(
    "A session choice lasts until you close Druck or start a new conversation.",
    "Eine Sitzungswahl gilt, bis du Druck schließt oder eine neue Unterhaltung beginnst.",
);
pub const NEW_FILE: Text = Text("A new file", "Eine neue Datei");
pub const UNSAVED_WARNING: Text = Text(
    "This chapter has unsaved changes in the editor. The agent works on the saved file.",
    "Dieses Kapitel hat ungesicherte Änderungen im Editor. Der Agent arbeitet an der gesicherten Datei.",
);

pub const CONTEXT_SENT: Text = Text("Sent with this prompt", "Mit diesem Prompt gesendet");

pub const TOOL_DENIED: Text = Text("denied", "abgelehnt");
pub const TOOL_FAILED: Text = Text("failed", "fehlgeschlagen");
pub const TOOL_WAITING: Text = Text("waiting", "wartet");

pub const REVERT: Text = Text("Revert", "Rückgängig");
pub const SHOW: Text = Text("Show", "Zeigen");
pub const HUNK_REVERTED: Text = Text("Reverted", "Zurückgesetzt");
pub const HUNK_MOVED: Text = Text("Text changed since", "Text seither geändert");
pub const HUNK_NOT_OPEN: Text = Text("Not open here", "Hier nicht geöffnet");

pub const NO_PATCH: Text = Text(
    "The agent changed a file, but Claude Code sent no patch for it. The editor does not show the change until the chapter is reloaded.",
    "Der Agent hat eine Datei geändert, aber Claude Code hat keinen Patch dazu gesendet. Der Editor zeigt die Änderung erst nach dem Neuladen des Kapitels.",
);
pub const CLI_UNREADABLE: Text = Text(
    "Claude Code did not report a version Druck can read.",
    "Claude Code meldet keine Version, die Druck lesen kann.",
);

pub fn context_sent_with(labels: &[String]) -> String {
    format!("{}: {}", CONTEXT_SENT.get(), labels.join(", "))
}

pub fn chip_chapter(path: &str) -> String {
    pick(format!("Chapter {path}"), format!("Kapitel {path}"))
}

pub fn chip_selection(lines: usize) -> String {
    pick(
        format!("Selection, {lines} {}", if lines == 1 { "line" } else { "lines" }),
        format!("Auswahl, {lines} {}", if lines == 1 { "Zeile" } else { "Zeilen" }),
    )
}

pub fn chip_diagnostics(count: usize) -> String {
    pick(
        format!("{count} {}", if count == 1 { "problem" } else { "problems" }),
        format!("{count} {}", if count == 1 { "Problem" } else { "Probleme" }),
    )
}

pub fn cli_version(version: &str) -> String {
    format!("Claude Code {version}")
}

pub fn cli_untested(version: &str) -> String {
    pick(
        format!("Claude Code {version} is not a version Druck was tested with. It may still work."),
        format!("Claude Code {version} gehört nicht zu den getesteten Versionen. Es kann trotzdem funktionieren."),
    )
}

/// The login Claude Code reports, in plain words.
pub fn login_line(method: &str, plan: Option<&str>) -> String {
    let what = match method {
        "claude.ai" => pick(
            "a Claude subscription login".to_owned(),
            "eine Claude-Abo-Anmeldung".to_owned(),
        ),
        method if method.to_lowercase().contains("api") => {
            pick("an API key".to_owned(), "ein API-Schlüssel".to_owned())
        }
        method => method.to_owned(),
    };
    let plan = plan.map(|plan| format!(" ({plan})")).unwrap_or_default();
    pick(
        format!("Claude Code reports {what}{plan}."),
        format!("Claude Code meldet {what}{plan}."),
    )
}

pub fn login_unknown() -> String {
    pick(
        "Claude Code did not say which login it uses.".to_owned(),
        "Claude Code sagt nicht, welche Anmeldung es nutzt.".to_owned(),
    )
}

pub fn allow_tool(tool: &str, target: &str) -> String {
    pick(format!("{tool} {target}?"), format!("{tool} {target} erlauben?"))
}

pub fn more_waiting(count: usize) -> String {
    pick(format!("{count} more waiting"), format!("{count} weitere warten"))
}

pub fn session_button(label: &SessionLabel) -> String {
    match label {
        SessionLabel::AcceptEdits => pick(
            "Allow all edits this session".to_owned(),
            "Alle Änderungen in dieser Sitzung erlauben".to_owned(),
        ),
        SessionLabel::Mode(mode) => pick(
            format!("Use mode {mode} this session"),
            format!("Modus {mode} in dieser Sitzung nutzen"),
        ),
        SessionLabel::Rule(rule) => pick(
            format!("Always allow {rule} this session"),
            format!("{rule} in dieser Sitzung immer erlauben"),
        ),
        SessionLabel::Other => pick(
            "Always allow this this session".to_owned(),
            "Das in dieser Sitzung immer erlauben".to_owned(),
        ),
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
    pick(
        format!(
            "{hunks} {} in {files} {}",
            if hunks == 1 { "change" } else { "changes" },
            if files == 1 { "file" } else { "files" }
        ),
        format!(
            "{hunks} {} in {files} {}",
            if hunks == 1 { "Änderung" } else { "Änderungen" },
            if files == 1 { "Datei" } else { "Dateien" }
        ),
    )
}

pub fn created_file(path: &str) -> String {
    pick(format!("Created {path}"), format!("{path} angelegt"))
}

pub fn hunk_line(line: usize) -> String {
    pick(format!("line {line}"), format!("Zeile {line}"))
}

pub fn merge_conflict(count: usize, path: &str) -> String {
    pick(
        format!(
            "{count} of the agent's changes to {path} overlap text you edited. The editor keeps your text, and the file on disk has the agent's version until you save."
        ),
        format!(
            "{count} Änderungen des Agenten an {path} überschneiden sich mit deinem Text. Der Editor behält deinen Text, die Datei auf der Platte hat die Version des Agenten, bis du sicherst."
        ),
    )
}

pub fn cannot_start(error: &str) -> String {
    pick(
        format!("Claude Code did not start: {error}"),
        format!("Claude Code ließ sich nicht starten: {error}"),
    )
}

pub fn cannot_send(error: &str) -> String {
    pick(
        format!("The prompt did not reach Claude Code: {error}"),
        format!("Der Prompt hat Claude Code nicht erreicht: {error}"),
    )
}

pub fn exited(stderr: &str) -> String {
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(" {stderr}")
    };
    pick(
        format!("Claude Code stopped unexpectedly.{detail}"),
        format!("Claude Code wurde unerwartet beendet.{detail}"),
    )
}
