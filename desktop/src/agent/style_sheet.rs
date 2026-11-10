//! The book's style sheet: the decisions an editing pass has made, kept in the project so the next run follows them.
//!
//! It is a Markdown file the author can read and change. A decision is a list line, `- **entry**: rule`. The agent
//! records one through the `record_style` tool and the app writes it, so every other line stays as the author left it.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use super::strings::Key;

/// Where it is, from the project folder. Beside the passes that keep it.
pub const FILE: &str = "passes/style-sheet.md";

/// The file's text, if the project has one.
pub fn read(project: &Path) -> Option<String> {
    fs::read_to_string(project.join(FILE)).ok()
}

/// The decisions a style sheet holds, in its order.
#[cfg(test)]
pub fn entries(text: &str) -> Vec<(&str, &str)> {
    text.lines().filter_map(entry).collect()
}

fn entry(line: &str) -> Option<(&str, &str)> {
    let (entry, rule) = line.trim_start().strip_prefix("- **")?.split_once("**:")?;
    Some((entry, rule.trim()))
}

/// The text with a decision in it: in place of the entry's line if it has one, or as a new last line.
pub fn record(text: &str, entry_name: &str, rule: &str) -> String {
    let one_line = |words: &str| words.split_whitespace().collect::<Vec<_>>().join(" ");
    let (entry_name, rule) = (one_line(entry_name).replace("**", ""), one_line(rule));
    let line = format!("- **{entry_name}**: {rule}");
    let mut replaced = false;
    let mut lines: Vec<&str> = text
        .lines()
        .map(|old| match entry(old) {
            Some((name, _)) if name == entry_name && !replaced => {
                replaced = true;
                line.as_str()
            }
            _ => old,
        })
        .collect();
    if !replaced {
        lines.push(&line);
    }
    lines.join("\n") + "\n"
}

/// Writes what the tool was passed into the project's style sheet, starting one if there is none.
pub fn write(project: &Path, input: &Value) -> io::Result<()> {
    let (Some(entry_name), Some(rule)) = (input["entry"].as_str(), input["rule"].as_str()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a style decision needs an entry and a rule",
        ));
    };
    let file = project.join(FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            format!(
                "# {}\n\n{}\n\n",
                crate::t!(Key::StyleSheetTitle),
                crate::t!(Key::StyleSheetIntro)
            )
        }
        Err(error) => return Err(error),
    };
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    crate::persist::atomic::write(&file, record(&text, entry_name, rule).as_bytes())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_decision_is_replaced_in_place_and_the_authors_lines_stay() {
        let sheet = "# Style sheet\n\nMy own remark.\n\n- **e-mail**: with a hyphen\n- **Anna Berg**: never Anne\n";
        let changed = record(sheet, "e-mail", "one word,\nno hyphen");
        assert_eq!(
            changed,
            "# Style sheet\n\nMy own remark.\n\n- **e-mail**: one word, no hyphen\n- **Anna Berg**: never Anne\n"
        );
        let added = record(&changed, "Numbers", "words up to twelve");
        assert!(added.ends_with("- **Anna Berg**: never Anne\n- **Numbers**: words up to twelve\n"));
        assert_eq!(
            entries(&added),
            [
                ("e-mail", "one word, no hyphen"),
                ("Anna Berg", "never Anne"),
                ("Numbers", "words up to twelve"),
            ]
        );
    }

    #[test]
    fn the_sheet_a_run_writes_is_what_the_next_run_reads() {
        let project = std::env::temp_dir().join(format!("druck-style-sheet-{}", std::process::id()));
        let _ = fs::remove_dir_all(&project);
        assert_eq!(read(&project), None);
        write(&project, &json!({"entry": "E-Mail", "rule": "mit Bindestrich"})).unwrap();
        write(&project, &json!({"entry": "Zahlen", "rule": "bis zwölf in Worten"})).unwrap();
        write(&project, &json!({"entry": "E-Mail", "rule": "immer mit Bindestrich"})).unwrap();
        let text = read(&project).expect("the sheet is in the project");
        assert!(text.starts_with("# "));
        assert_eq!(
            entries(&text),
            [("E-Mail", "immer mit Bindestrich"), ("Zahlen", "bis zwölf in Worten")]
        );
        assert!(write(&project, &json!({"entry": "x"})).is_err());
        fs::remove_dir_all(project).unwrap();
    }
}
