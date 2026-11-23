//! Every string the agent panel shows, in English and German.

use super::approval::SessionLabel;

crate::strings! {
    pub enum Key {
        RemoveChip => ("Remove {chip}", "{chip} entfernen"),
        Title => ("Agent", "Agent"),
        Close => ("Close", "Schließen"),
        SelectionPlaceholder => ("Say what to change\u{2026}", "Sag, was sich ändern soll\u{2026}"),
        ReviewHereOne => ("1 change to review in this chapter", "1 Änderung in diesem Kapitel zu prüfen"),
        ReviewHereMany => ("{count} changes to review in this chapter", "{count} Änderungen in diesem Kapitel zu prüfen"),
        ReviewElsewhereOne => ("1 change to review elsewhere", "1 Änderung anderswo zu prüfen"),
        ReviewElsewhereMany => ("{count} changes to review elsewhere", "{count} Änderungen anderswo zu prüfen"),
        ReviewAlsoElsewhere => ("{count} elsewhere", "{count} anderswo"),
        AcceptAll => ("Accept all", "Alle annehmen"),
        RejectAll => ("Reject all", "Alle ablehnen"),
        NextChange => ("Next", "Weiter"),
        ReplyingTo => ("Replying to: {title}", "Antwort auf: {title}"),
        ToolNote => ("Note", "Notiz"),
        TryAgainShown => ("Try again: {place}", "Neu versuchen: {place}"),
        TurnBusy => (
            "{cli} is still working. Try again when it has finished.",
            "{cli} arbeitet noch. Versuche es danach erneut."
        ),
        NewConversation => ("New", "Neu"),
        Model => ("Model", "Modell"),
        ModelDefault => ("Default", "Standard"),
        Placeholder => ("Ask about the book\u{2026}", "Frag etwas zum Buch\u{2026}"),
        Send => ("Send", "Senden"),
        Stop => ("Stop", "Anhalten"),
        Checking => ("Looking for {cli}\u{2026}", "Suche nach {cli}\u{2026}"),
        Stopped => ("Stopped.", "Angehalten."),
        Working => ("Working\u{2026}", "Arbeitet\u{2026}"),
        Stopping => ("Stopping\u{2026}", "Wird angehalten\u{2026}"),
        Resumed => (
            "This project has an earlier conversation. {cli} keeps it, so the next prompt continues it.",
            "Dieses Projekt hat eine frühere Unterhaltung. {cli} bewahrt sie auf, der nächste Prompt setzt sie fort."
        ),
        SetupTitle => ("Set up {cli}", "{cli} einrichten"),
        SetupFolded => ("{cli} setup", "Einrichtung von {cli}"),
        SetupMissing => (
            "The agent runs the {cli} program on this computer. It is not installed or not on the search path.",
            "Der Agent startet das Programm {cli} auf diesem Computer. Es ist nicht installiert oder nicht im Suchpfad."
        ),
        SetupSteps => (
            "1. Install {cli}.\n2. Set it up with an API key, as the documentation of {vendor} describes.\n3. Reopen Druck.",
            "1. {cli} installieren.\n2. Mit einem API-Schlüssel einrichten, wie in der Dokumentation von {vendor} beschrieben.\n3. Druck erneut öffnen."
        ),
        SetupOwner => (
            "Druck holds no keys and sends nothing itself. {cli} does the login, and Druck only starts it. Everything else in Druck works without it.",
            "Druck speichert keine Schlüssel und sendet nichts selbst. {cli} übernimmt die Anmeldung, Druck startet es nur. Alles andere in Druck funktioniert auch ohne."
        ),
        SubscriptionNote => (
            "Whether {vendor} allows a subscription login for an app like Druck is not confirmed. An API key is the documented way.",
            "Ob {vendor} eine Abo-Anmeldung für eine App wie Druck erlaubt, ist nicht bestätigt. Ein API-Schlüssel ist der dokumentierte Weg."
        ),
        LoggedOut => (
            "{cli} reports that nobody is logged in. Set it up with an API key before the first prompt.",
            "{cli} meldet, dass niemand angemeldet ist. Vor dem ersten Prompt mit einem API-Schlüssel einrichten."
        ),
        Intro => (
            "Ask {cli} about the book. It works in the project folder and reads its {memory}, skills, and MCP servers as in a terminal. What it changes shows in the text for you to accept or reject.",
            "Frag {cli} zum Buch. Es arbeitet im Projektordner und liest {memory}, Skills und MCP-Server wie im Terminal. Was es ändert, steht im Text und du nimmst es an oder lehnst es ab."
        ),
        AllowOnce => ("Allow once", "Einmal erlauben"),
        Deny => ("Deny", "Ablehnen"),
        DenyWithReason => ("Deny with reason\u{2026}", "Mit Grund ablehnen\u{2026}"),
        ReasonLabel => ("Reason, which the agent sees", "Grund, den der Agent sieht"),
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
        Revert => ("Reject", "Ablehnen"),
        Show => ("Show", "Zeigen"),
        HunkReverted => ("Rejected", "Abgelehnt"),
        HunkMoved => ("Text changed since", "Text seither geändert"),
        HunkNotOpen => ("Not open here", "Hier nicht geöffnet"),
        CliUnreadable => (
            "{cli} did not report a version Druck can read.",
            "{cli} meldet keine Version, die Druck lesen kann."
        ),

        ChipChapter => ("Chapter: {title}", "Kapitel: {title}"),
        ChipSelectionOne => ("Selection, {lines} line", "Auswahl, {lines} Zeile"),
        ChipSelectionMany => ("Selection, {lines} lines", "Auswahl, {lines} Zeilen"),
        ChipProblemOne => ("{count} problem", "{count} Problem"),
        ChipProblemMany => ("{count} problems", "{count} Probleme"),
        CliUntested => (
            "{cli} {version} is not a version Druck was tested with. It may still work.",
            "{cli} {version} gehört nicht zu den getesteten Versionen. Es kann trotzdem funktionieren."
        ),
        LoginSubscription => ("a {product} subscription login", "eine {product}-Abo-Anmeldung"),
        LoginApiKey => ("an API key", "einen API-Schlüssel"),
        LoginReports => ("{cli} reports {what}{plan}.", "{cli} meldet {what}{plan}."),
        LoginUnknown => (
            "{cli} did not say which login it uses.",
            "{cli} sagt nicht, welche Anmeldung es nutzt."
        ),
        AllowTool => ("{tool} {target}?", "{tool} {target} erlauben?"),
        MoreWaiting => ("{count} more waiting", "{count} weitere warten"),
        SessionAcceptEdits => ("Allow all edits this session", "Alle Änderungen in dieser Sitzung erlauben"),
        SessionMode => ("Use mode {mode} this session", "Modus {mode} in dieser Sitzung nutzen"),
        SessionRule => ("Always allow {rule} this session", "{rule} in dieser Sitzung immer erlauben"),
        SessionOther => ("Always allow this this session", "Das in dieser Sitzung immer erlauben"),
        ChangesOneOne => ("This turn: 1 change in 1 file", "In diesem Schritt: 1 Änderung in 1 Datei"),
        ChangesManyOne => ("This turn: {hunks} changes in 1 file", "In diesem Schritt: {hunks} Änderungen in 1 Datei"),
        ChangesManyMany => (
            "This turn: {hunks} changes in {files} files",
            "In diesem Schritt: {hunks} Änderungen in {files} Dateien"
        ),
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
        CannotStart => ("{cli} did not start: {error}", "{cli} ließ sich nicht starten: {error}"),
        CliNotFound => ("it is not installed or not on the path", "es ist nicht installiert oder nicht im Pfad"),
        CannotSend => ("The prompt did not reach {cli}: {error}", "Der Prompt hat {cli} nicht erreicht: {error}"),
        PassCopyedit => ("Copyedit", "Korrektorat"),
        PassConsistency => ("Consistency", "Konsistenz"),
        PassReferences => ("References", "Verweise"),
        PassLineEdit => ("Line edit", "Stillektorat"),
        PassOnSelection => ("{pass}: selection", "{pass}: Auswahl"),
        PassOnChapter => ("{pass}: this chapter", "{pass}: dieses Kapitel"),
        PassOnBook => ("{pass}: whole book", "{pass}: ganzes Buch"),
        PassCustomize => ("Customize pass: {pass}", "Durchgang anpassen: {pass}"),
        PassRow => ("{pass}: {chapter}", "{pass}: {chapter}"),
        PassProgress => ("{pass} \u{b7} {chapter}", "{pass} \u{b7} {chapter}"),
        PassProgressOf => (
            "{pass} \u{b7} chapter {at} of {total} \u{b7} {chapter}",
            "{pass} \u{b7} Kapitel {at} von {total} \u{b7} {chapter}"
        ),
        PassDone => ("{pass} is done.", "{pass} ist fertig."),
        PassDoneOf => (
            "{pass} is done: {done} of {total} chapters.",
            "{pass} ist fertig: {done} von {total} Kapiteln."
        ),
        PassStopped => (
            "{pass} was stopped after {done} of {total} chapters.",
            "{pass} wurde nach {done} von {total} Kapiteln angehalten."
        ),
        PassFailed => ("Not done: {chapters}.", "Nicht erledigt: {chapters}."),
        PassSurveying => ("{pass} \u{b7} reading the whole book", "{pass} \u{b7} das ganze Buch wird gelesen"),
        PassSurveyRow => ("{pass}: reading the whole book", "{pass}: das ganze Buch lesen"),
        PassSurveyFailed => ("reading the whole book", "das ganze Buch lesen"),
        PassUnusable => ("{file} cannot run: {problem}.", "{file} lässt sich nicht starten: {problem}."),
        PassNotCopied => ("The pass could not be copied: {error}", "Der Durchgang ließ sich nicht kopieren: {error}"),
        BeforePass => ("Before {pass}", "Vor {pass}"),
        FixProof => ("Fix the proof", "Satzwarnungen beheben"),
        FixReading => ("Fix the proof \u{b7} reading the pages", "Satzwarnungen beheben \u{b7} Seiten werden gelesen"),
        FixTrying => ("Fix the proof \u{b7} trying fixes", "Satzwarnungen beheben \u{b7} Korrekturen werden probiert"),
        FixTryingKept => (
            "Fix the proof \u{b7} trying fixes \u{b7} {kept} kept",
            "Satzwarnungen beheben \u{b7} Korrekturen werden probiert \u{b7} {kept} behalten"
        ),
        FixDoneNone => ("Fix the proof is done: no fix helped.", "Satzwarnungen beheben ist fertig: Keine Satzkorrektur half."),
        FixDoneOne => (
            "Fix the proof is done: 1 fix to review in Proof.",
            "Satzwarnungen beheben ist fertig: 1 Satzkorrektur im Reiter Korrektur zu prüfen."
        ),
        FixDoneMany => (
            "Fix the proof is done: {count} fixes to review in Proof.",
            "Satzwarnungen beheben ist fertig: {count} Satzkorrekturen im Reiter Korrektur zu prüfen."
        ),
        FixStopped => (
            "Fix the proof was stopped. Fixes to review: {count}.",
            "Satzwarnungen beheben wurde angehalten. Satzkorrekturen zu prüfen: {count}."
        ),
        FixFailed => (
            "Fix the proof did not finish. Fixes to review: {count}.",
            "Satzwarnungen beheben wurde nicht fertig. Satzkorrekturen zu prüfen: {count}."
        ),
        FixNothing => ("The pages break no typesetting rule.", "Die Seiten verletzen keine Satzregel."),
        FixNoPages => ("Fix the proof needs pages: {problem}", "Satzwarnungen beheben braucht Seiten: {problem}"),
        FixGone => (
            "A fix was left out, since its text has changed.",
            "Eine Satzkorrektur wurde ausgelassen, weil sich ihr Text geändert hat."
        ),
        BeforeFixes => ("Before proof fixes", "Vor den Satzkorrekturen"),
        FixesOne => ("1 fix to review", "1 Satzkorrektur zu prüfen"),
        FixesMany => ("{count} fixes to review", "{count} Satzkorrekturen zu prüfen"),
        FixAccept => ("Accept", "Annehmen"),
        FixReject => ("Reject", "Ablehnen"),
        FixBefore => ("Before \u{b7} page {page}", "Vorher \u{b7} Seite {page}"),
        FixAfter => ("After \u{b7} page {page}", "Nachher \u{b7} Seite {page}"),
        FixPageBreak => ("Page break before", "Seitenumbruch davor"),
        FixKeep => ("Keep together", "Zusammenhalten"),
        FixPlacementTop => ("Figure at the top of its page", "Abbildung oben auf der Seite"),
        FixPlacementBottom => ("Figure at the foot of its page", "Abbildung unten auf der Seite"),
        FixPlacementHere => ("Figure where it is written", "Abbildung an ihrer Stelle im Text"),
        FixTableTop => ("Table at the top of its page", "Tabelle oben auf der Seite"),
        FixTableBottom => ("Table at the foot of its page", "Tabelle unten auf der Seite"),
        FixTableHere => ("Table where it is written", "Tabelle an ihrer Stelle im Text"),
        FixTheme => ("Theme: {property} = {value}", "Theme: {property} = {value}"),
        FixWholeBook => ("Holds for the whole book", "Gilt für das ganze Buch"),
        FixClearsOne => ("Clears 1 warning", "Behebt 1 Warnung"),
        FixClearsMany => ("Clears {count} warnings", "Behebt {count} Warnungen"),
        FixClearsExcusedOne => (
            "Clears 1 warning, on the page the break ends",
            "Behebt 1 Warnung auf der Seite, die der Umbruch beendet"
        ),
        FixClearsExcused => (
            "Clears {count} warnings, {excused} of them on the page the break ends",
            "Behebt {count} Warnungen, davon {excused} auf der Seite, die der Umbruch beendet"
        ),
        FixCausesOnPage => ("New warnings: {count}, on page {pages}", "Neue Warnungen: {count}, auf Seite {pages}"),
        FixCausesOnPages => (
            "New warnings: {count}, on pages {pages}",
            "Neue Warnungen: {count}, auf den Seiten {pages}"
        ),
        FixPagesMore => ("{pages} and {more} more", "{pages} und {more} weiteren"),
        FixNotStarted => (
            "Fix the proof could not start: {problem}",
            "Satzwarnungen beheben ließ sich nicht starten: {problem}"
        ),
        NotStartedHeld => (
            "Nothing was started: a conflict is waiting at the window's foot. Settle it first.",
            "Nichts gestartet: Am Fensterrand wartet ein Konflikt. Entscheide ihn zuerst."
        ),
        NotStartedUnsaved => (
            "Nothing was started, since {file} cannot be saved: {reason}",
            "Nichts gestartet, weil sich {file} nicht sichern lässt: {reason}"
        ),
        FixShowPage => ("Show this page", "Diese Seite zeigen"),
        FixOffer => (
            "The agent tries fixes on a copy of the book and keeps those that clear warnings without causing any. A theme value may cause some, and says where.",
            "Der Agent probiert Korrekturen an einer Kopie des Buchs und behält, was Warnungen behebt, ohne neue zu verursachen. Ein Theme-Wert darf einige verursachen und sagt, wo."
        ),
        StyleSheetTitle => ("Style sheet", "Stilblatt"),
        StyleSheetIntro => (
            "The decisions the Consistency pass has made. Change them as you like: the next pass follows this file.",
            "Die Entscheidungen des Durchgangs Konsistenz. Ändere sie nach Belieben: Der nächste Durchgang folgt dieser Datei."
        ),
        StyleSheetUnwritten => (
            "The style sheet could not be written: {error}",
            "Das Stilblatt ließ sich nicht schreiben: {error}"
        ),
        Exited => ("The agent program stopped unexpectedly.{detail}", "Das Agent-Programm wurde unerwartet beendet.{detail}"),
    }
}

/// What the CLI is told a denial means. For the model, so never translated.
pub const DENIED_DEFAULT: &str = "The author declined this change.";
pub const STOPPED_DENIAL: &str = "The author stopped the turn.";
/// What a prompt at a selection adds. For the model, so never translated.
pub const SELECTION_ONLY: &str = "Work on the selected text only, and edit it in its file. Change nothing around it.";

/// What a reply to a note tells the model. Never translated.
pub fn note_reply(chapter: &str, quote: &str, title: &str, text: &str) -> String {
    format!(
        "<druck-note chapter=\"{chapter}\">\nYour margin note \"{title}\" on \"{quote}\" said: {text}\n</druck-note>\nThe author replies:"
    )
}

/// What Try again tells the model. Never translated.
pub fn try_again(path: &str, old: &str, new: &str) -> String {
    format!(
        "The author rejected your change to {path}. The text is back as it was:\n> {}\nYou had written:\n> {}\nTry again, differently, by editing the file.",
        old.trim_end().replace('\n', "\n> "),
        new.trim_end().replace('\n', "\n> ")
    )
}

/// What the next turn hears about a change the author took back, so it does not offer it again. Never translated.
pub fn rejected(path: &str, old: &str, new: &str) -> String {
    format!(
        "The author rejected your change to {path}. The text is back as it was:\n> {}\nYou had written:\n> {}\nDo not offer this change again unless asked.",
        old.trim_end().replace('\n', "\n> "),
        new.trim_end().replace('\n', "\n> ")
    )
}

pub fn review_foot(here: usize, elsewhere: usize) -> String {
    match (here, elsewhere) {
        (0, 1) => crate::t!(Key::ReviewElsewhereOne).to_owned(),
        (0, count) => crate::t!(Key::ReviewElsewhereMany, count = count),
        (1, 0) => crate::t!(Key::ReviewHereOne).to_owned(),
        (count, 0) => crate::t!(Key::ReviewHereMany, count = count),
        (1, others) => format!(
            "{} \u{b7} {}",
            crate::t!(Key::ReviewHereOne),
            crate::t!(Key::ReviewAlsoElsewhere, count = others)
        ),
        (count, others) => format!(
            "{} \u{b7} {}",
            crate::t!(Key::ReviewHereMany, count = count),
            crate::t!(Key::ReviewAlsoElsewhere, count = others)
        ),
    }
}

pub fn context_sent_with(labels: &[String]) -> String {
    format!("{}: {}", crate::t!(Key::ContextSent), labels.join(", "))
}

pub fn chip_chapter(title: &str) -> String {
    crate::t!(Key::ChipChapter, title = title)
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

pub fn cli_version(cli: &str, version: &str) -> String {
    format!("{cli} {version}")
}

pub fn cli_untested(cli: &str, version: &str) -> String {
    crate::t!(Key::CliUntested, cli = cli, version = version)
}

/// The login the CLI reports, in plain words. `product` is what the vendor's subscription is called.
pub fn login_line(cli: &str, product: &str, method: &str, plan: Option<&str>, subscription: bool) -> String {
    let what = match method {
        _ if subscription => crate::t!(Key::LoginSubscription, product = product),
        method if method.to_lowercase().contains("api") => crate::t!(Key::LoginApiKey).to_owned(),
        method => method.to_owned(),
    };
    let plan = plan.map(|plan| format!(" ({plan})")).unwrap_or_default();
    crate::t!(Key::LoginReports, cli = cli, what = what, plan = plan)
}

pub fn turn_busy(cli: &str) -> String {
    crate::t!(Key::TurnBusy, cli = cli)
}

/// A pass's row in the conversation.
pub fn pass_row(pass: &str, chapter: &str) -> String {
    crate::t!(Key::PassRow, pass = pass, chapter = chapter)
}

/// The line at the window's foot while the turn that reads the whole book runs.
pub fn pass_surveying(pass: &str) -> String {
    crate::t!(Key::PassSurveying, pass = pass)
}

pub fn pass_survey_row(pass: &str) -> String {
    crate::t!(Key::PassSurveyRow, pass = pass)
}

/// The line at the window's foot while a pass runs. No cost shows, as on the chat card.
pub fn pass_progress(pass: &str, at: usize, total: usize, chapter: &str) -> String {
    if total == 1 {
        crate::t!(Key::PassProgress, pass = pass, chapter = chapter)
    } else {
        crate::t!(
            Key::PassProgressOf,
            pass = pass,
            at = at,
            total = total,
            chapter = chapter
        )
    }
}

/// How a pass ended, with the chapters it could not do.
pub fn pass_ended(pass: &str, done: usize, total: usize, failed: &[String], stopped: bool) -> String {
    let summary = match (stopped, total) {
        (true, _) => crate::t!(Key::PassStopped, pass = pass, done = done, total = total),
        (false, 1) if failed.is_empty() => crate::t!(Key::PassDone, pass = pass),
        (false, _) => crate::t!(Key::PassDoneOf, pass = pass, done = done, total = total),
    };
    if failed.is_empty() {
        summary
    } else {
        format!("{summary} {}", crate::t!(Key::PassFailed, chapters = failed.join(", ")))
    }
}

pub fn pass_unusable(file: &str, problem: &str) -> String {
    crate::t!(Key::PassUnusable, file = file, problem = problem)
}

pub fn pass_not_copied(error: &str) -> String {
    crate::t!(Key::PassNotCopied, error = error)
}

pub fn before_pass(pass: &str) -> String {
    crate::t!(Key::BeforePass, pass = pass)
}

/// The line at the window's foot while the typesetter's turn runs.
pub fn fix_progress(kept: usize) -> String {
    match kept {
        0 => crate::t!(Key::FixTrying).to_owned(),
        kept => crate::t!(Key::FixTryingKept, kept = kept),
    }
}

/// How a run of the typesetter ended, with how many fixes wait for review.
pub fn fix_ended(kept: usize, outcome: super::conversation::Outcome) -> String {
    use super::conversation::Outcome;
    match (outcome, kept) {
        (Outcome::Stopped, count) => crate::t!(Key::FixStopped, count = count),
        (Outcome::Failed, count) => crate::t!(Key::FixFailed, count = count),
        (Outcome::Done, 0) => crate::t!(Key::FixDoneNone).to_owned(),
        (Outcome::Done, 1) => crate::t!(Key::FixDoneOne).to_owned(),
        (Outcome::Done, count) => crate::t!(Key::FixDoneMany, count = count),
    }
}

pub fn fix_no_pages(problem: &str) -> String {
    crate::t!(Key::FixNoPages, problem = problem)
}

pub fn fixes_to_review(count: usize) -> String {
    match count {
        1 => crate::t!(Key::FixesOne).to_owned(),
        count => crate::t!(Key::FixesMany, count = count),
    }
}

/// How many pages an entry names of those it causes warnings on, before it says how many more.
const PAGES_NAMED: usize = 12;

/// What an entry does to the warnings: what it clears, with those a page break excuses among them, and what it causes
/// and on which pages. One line each.
pub fn fix_counts(counts: &super::fixing::Counts) -> Vec<String> {
    let cleared = match (counts.cleared, counts.excused) {
        (1, 0) => crate::t!(Key::FixClearsOne).to_owned(),
        (1, _) => crate::t!(Key::FixClearsExcusedOne).to_owned(),
        (count, 0) => crate::t!(Key::FixClearsMany, count = count),
        (count, excused) => crate::t!(Key::FixClearsExcused, count = count, excused = excused),
    };
    if counts.caused == 0 {
        return vec![cleared];
    }
    let named: Vec<String> = (counts.caused_pages.iter().take(PAGES_NAMED))
        .map(usize::to_string)
        .collect();
    let mut pages = named.join(", ");
    if counts.caused_pages.len() > PAGES_NAMED {
        pages = crate::t!(
            Key::FixPagesMore,
            pages = pages,
            more = counts.caused_pages.len() - PAGES_NAMED
        );
    }
    let caused = match counts.caused_pages.len() {
        1 => crate::t!(Key::FixCausesOnPage, count = counts.caused, pages = pages),
        _ => crate::t!(Key::FixCausesOnPages, count = counts.caused, pages = pages),
    };
    vec![cleared, caused]
}

pub fn fix_not_started(problem: &str) -> String {
    crate::t!(Key::FixNotStarted, problem = problem)
}

pub fn style_sheet_unwritten(error: &str) -> String {
    crate::t!(Key::StyleSheetUnwritten, error = error)
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

pub fn cannot_start(cli: &str, error: &str) -> String {
    crate::t!(Key::CannotStart, cli = cli, error = error)
}

pub fn cannot_send(cli: &str, error: &str) -> String {
    crate::t!(Key::CannotSend, cli = cli, error = error)
}

pub fn exited(stderr: &str) -> String {
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(" {stderr}")
    };
    crate::t!(Key::Exited, detail = detail)
}
