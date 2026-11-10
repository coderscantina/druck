//! The strings of the window with no document, in English and German.

crate::strings! {
    pub enum Key {
        Open => ("Open", "Öffnen"),
        OpenHint => ("A Markdown file or a project folder", "Eine Markdown-Datei oder ein Projektordner"),
        NewProject => ("New project", "Neues Projekt"),
        AgentOffer => ("Writing partner", "Schreibpartner"),
        AgentOfferWhat => (
            "Druck can work on your book with an agent: editing passes such as a copyedit, and changes or advice when you ask on Cmd+J. Nothing runs unless you ask, and you accept or reject every change.",
            "Druck kann mit einem Agenten an deinem Buch arbeiten: Durchgänge wie ein Korrektorat, und Änderungen oder Rat, wenn du auf Cmd+J danach fragst. Nichts läuft ungefragt, und jede Änderung nimmst du an oder lehnst sie ab."
        ),
        AgentOfferNeeds => (
            "It needs Claude Code or Codex installed on this computer and set up with an API key. Druck holds no keys. You can change this in Settings at any time.",
            "Dafür muss Claude Code oder Codex auf diesem Computer installiert und mit einem API-Schlüssel eingerichtet sein. Druck speichert keine Schlüssel. In den Einstellungen lässt sich das jederzeit ändern."
        ),
        AgentOfferYes => ("Turn on", "Einschalten"),
        AgentOfferNo => ("Not now", "Jetzt nicht"),
    }
}
