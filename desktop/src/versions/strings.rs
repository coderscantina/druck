//! Every string of the commit card, the timeline, and the Git panel, in English and German. They use Git's own words:
//! commit, branch, push, pull.

crate::strings! {
    pub enum Key {
        Commit => ("Commit", "Commit"),
        CommitLabel => ("COMMIT", "COMMIT"),
        CommitAndPush => ("Commit and push", "Commit und Push"),
        Message => ("Message", "Nachricht"),
        NothingChanged => ("Nothing changed since the last commit.", "Seit dem letzten Commit hat sich nichts geändert."),
        Working => ("Working…", "Läuft…"),
        Reading => ("Reading the repo…", "Repo wird gelesen…"),
        Unreviewed => (
            "{count} agent changes not reviewed will be part of this commit.",
            "{count} nicht geprüfte Agent-Änderungen kommen in diesen Commit."
        ),
        Unsigned => (
            "Your settings ask for signed commits, and signing needs Git installed. This one stays unsigned.",
            "Deine Einstellungen verlangen signierte Commits, dazu braucht es Git. Dieser bleibt unsigniert."
        ),
        MergingNote => ("Finishes the merge.", "Schließt den Merge ab."),
        BothStand => (
            "Both versions stand in {chapter}.",
            "In {chapter} stehen beide Fassungen."
        ),
        CannotRunGit => ("cannot run git: {error}", "git lässt sich nicht starten: {error}"),
        CannotTalkToGit => ("cannot talk to git: {error}", "keine Verbindung zu git: {error}"),
        CannotWaitForGit => ("cannot wait for git: {error}", "git antwortet nicht: {error}"),
        GitTooLong => ("git {command} took too long and was stopped", "git {command} dauerte zu lange und wurde angehalten"),
        NotARepo => ("this book is not a Git repo", "dieses Buch ist kein Git-Repo"),
        OneWordAdded => ("+1 word", "+1 Wort"),
        OneWordRemoved => ("−1 word", "−1 Wort"),
        WordsAdded => ("+{added} words", "+{added} Wörter"),
        WordsRemoved => ("−{removed} words", "−{removed} Wörter"),
        WordsBoth => ("+{added}, −{removed} words", "+{added}, −{removed} Wörter"),
        WordsSame => ("changed", "geändert"),
        Binary => ("not text", "kein Text"),
        Deleted => ("deleted", "gelöscht"),
        InConflict => ("in conflict", "im Konflikt"),

        FirstCommitQuestion => (
            "This book is not a Git repo yet. Make it one for commits and branches? It adds a .git folder here. Local history works either way.",
            "Dieses Buch ist noch kein Git-Repo. Soll es eines werden, für Commits und Branches? Das legt hier einen .git-Ordner an. Der lokale Verlauf läuft so oder so."
        ),
        NoRepoLine => (
            "This book has local history only, on ⌘Y. A repo adds commits and branches.",
            "Dieses Buch hat nur den lokalen Verlauf, auf ⌘Y. Ein Repo bringt Commits und Branches."
        ),
        MakeRepo => ("Make a repo", "Repo anlegen"),
        NotNow => ("Not now", "Nicht jetzt"),
        SingleFile => (
            "A single Markdown file has local history only, on ⌘Y.",
            "Eine einzelne Markdown-Datei hat nur den lokalen Verlauf, auf ⌘Y."
        ),
        CloudWarning => (
            "This folder is in {service}. Sync services can damage a repo's .git folder. A folder outside it is safer.",
            "Dieser Ordner liegt in {service}. Sync-Dienste können den .git-Ordner eines Repos beschädigen. Ein Ordner außerhalb ist sicherer."
        ),
        IdentityQuestion => (
            "Git needs a name and an email for commits. They stay in this repo.",
            "Git braucht für Commits einen Namen und eine E-Mail. Sie bleiben in diesem Repo."
        ),
        Continue => ("Continue", "Weiter"),
        YourName => ("Name", "Name"),
        YourEmail => ("Email", "E-Mail"),

        GitLabel => ("GIT", "GIT"),
        BranchGroup => ("Branch", "Branch"),
        ChangesGroup => ("Changes", "Änderungen"),
        HistoryGroup => ("History", "Verlauf"),
        NoCommits => ("No commits yet.", "Noch keine Commits."),
        NewBranch => ("New branch", "Neuer Branch"),
        Merge => ("Merge", "Merge"),
        Delete => ("Delete", "Löschen"),
        DeleteUnmerged => ("Not merged. Delete?", "Nicht gemerged. Löschen?"),
        Current => ("current", "aktuell"),
        Push => ("Push", "Push"),
        Pull => ("Pull", "Pull"),
        AbortMerge => ("Abort merge", "Merge abbrechen"),
        MergingBanner => ("Merging: {message}", "Merge läuft: {message}"),
        ConflictsOnLine => (
            "Settle each file on the line at the window's foot, then commit.",
            "Entscheide jede Datei in der Zeile am Fensterrand, dann committe."
        ),
        Detached => ("{id} (no branch)", "{id} (kein Branch)"),
        GitMissing => (
            "Git is not installed. Commits, branches, and history work here. Switching, merging, pushing, and pulling need it.",
            "Git ist nicht installiert. Commits, Branches und Verlauf gehen hier. Wechseln, Merge, Push und Pull brauchen es."
        ),
        BusyElsewhere => (
            "A {operation} is running in this repo. Finish it in the tool that began it.",
            "In diesem Repo läuft ein {operation}. Schließe es in dem Programm ab, das es begonnen hat."
        ),

        Synced => ("Synced", "Synchron"),
        Ahead => ("{count} ahead", "{count} voraus"),
        Behind => ("{count} behind", "{count} zurück"),
        Diverged => ("{ahead} ahead, {behind} behind", "{ahead} voraus, {behind} zurück"),
        NoRemote => ("No remote", "Kein Remote"),
        NotPushed => ("Not pushed", "Nicht gepusht"),

        Now => ("Now", "Jetzt"),
        TimelineStrip => ("Versions of this chapter", "Versionen dieses Kapitels"),
        Restored => (
            "Restored. ⌘Z in the text takes it back.",
            "Wiederhergestellt. ⌘Z im Text nimmt es zurück."
        ),
        RestorePassage => ("Restore passage", "Passage wiederherstellen"),
        RestoreChapter => ("Restore chapter", "Kapitel wiederherstellen"),
        NoHistory => (
            "No versions of this chapter yet. Local history starts with your next pause.",
            "Noch keine Versionen dieses Kapitels. Der lokale Verlauf beginnt mit der nächsten Pause."
        ),
        NoPlaceInFile => ("This version has no such file.", "Diese Version hat diese Datei nicht."),
        SameAsNow => ("The same as now.", "Wie jetzt."),
        ChangeCount => ("{count} passages differ from now", "{count} Passagen anders als jetzt"),
        OneChange => ("1 passage differs from now", "1 Passage anders als jetzt"),
        LocalHistory => ("Local history", "Lokaler Verlauf"),
        UnplacedHistory => (
            "A file with this name had history at {path}. Is it this one?",
            "Eine Datei mit diesem Namen hatte einen Verlauf unter {path}. Ist es diese?"
        ),
        UseHistory => ("Use that history", "Diesen Verlauf nehmen"),
        HistoryFollowed => (
            "Local history followed this book from {from}.",
            "Der lokale Verlauf ist diesem Buch aus {from} gefolgt."
        ),
        HistoryLeft => (
            "{count} files here have no local history, but files with their names do elsewhere. ⌘Y shows them.",
            "{count} Dateien hier haben keinen lokalen Verlauf, gleichnamige anderswo schon. ⌘Y zeigt sie."
        ),

        BeforeOutside => ("Before a change from outside", "Vor einer Änderung von außen"),
        BeforeRestore => ("Before a restore", "Vor dem Wiederherstellen"),
        BeforeSwitch => ("Before switching to {branch}", "Vor dem Wechsel zu {branch}"),
        BeforeMerge => ("Before merging {branch}", "Vor dem Merge von {branch}"),
        BeforePull => ("Before a pull", "Vor einem Pull"),

        HeldConflict => (
            "A conflict is waiting at the window's foot. Settle it first.",
            "Am Fensterrand wartet ein Konflikt. Entscheide ihn zuerst."
        ),
        CannotSave => ("{file} cannot be saved: {reason}", "{file} lässt sich nicht sichern: {reason}"),
        LocalChanges => (
            "Your uncommitted changes touch the same files. Commit first.",
            "Deine Änderungen ohne Commit betreffen dieselben Dateien. Erst committen."
        ),
        MergeOutside => (
            "{file} needs settling in another Git tool, so the merge was not started.",
            "{file} muss in einem anderen Git-Programm entschieden werden, der Merge wurde nicht begonnen."
        ),
        NothingToCommit => ("Nothing to commit.", "Nichts zu committen."),
        NoIdentity => ("Git has no name and email for commits.", "Git hat keinen Namen und keine E-Mail für Commits."),
        BranchExists => ("There is a branch {name} already.", "Es gibt schon einen Branch {name}."),
        BadName => ("{name} cannot be a branch's name.", "{name} geht nicht als Name eines Branches."),
        NoUpstream => ("This branch has no upstream to pull from.", "Dieser Branch hat keinen Upstream für einen Pull."),
        Committed => ("Committed {hash}", "Commit {hash} angelegt"),
        TimelineKeys => (
            "\u{2190}\u{2192} walk  \u{2325}\u{2190}\u{2192} commits  \u{2191}\u{2193} passage  \u{21a9} restore  esc",
            "\u{2190}\u{2192} gehen  \u{2325}\u{2190}\u{2192} Commits  \u{2191}\u{2193} Passage  \u{21a9} zurückholen  esc"
        ),
        PushFailed => ("Committed, but the push failed: {reason}", "Commit ist da, aber der Push schlug fehl: {reason}"),
    }
}
