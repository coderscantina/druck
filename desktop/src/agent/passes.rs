//! Editing passes: named instructions the agent runs over the book, a chapter, or a selection, only when asked.
//!
//! A pass is a Markdown file. Between two `---` lines it says its `name` and what it `returns`, `changes` or `notes`,
//! and the rest is its instructions. Four are built in. A file in the project's `passes/` folder is a pass too, and one
//! with a built-in's file name takes its place. No gpui here.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::context::Selected;
use super::style_sheet;
use super::tools;

/// The project's folder of passes.
pub const FOLDER: &str = "passes";

/// The passes that ship with the app, by file name without `.md`.
const BUILT_IN: [(&str, &str); 4] = [
    ("copyedit", include_str!("../../passes/copyedit.md")),
    ("consistency", include_str!("../../passes/consistency.md")),
    ("references", include_str!("../../passes/references.md")),
    ("line-edit", include_str!("../../passes/line-edit.md")),
];

/// What a pass hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returns {
    /// Edits in the files, which show in the text as tinted changes.
    Changes,
    /// Margin notes only. No file is written.
    Notes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    pub name: String,
    pub returns: Returns,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    BuiltIn,
    Project,
}

/// A pass as the palette lists it. A project file that is no valid pass is listed with what is wrong with it, so
/// asking for it can say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The file's name without `.md`, which is what a project's copy shares with a built-in.
    pub slug: String,
    pub origin: Origin,
    pub pass: Result<Pass, String>,
}

/// Reads a pass file. `slug` names it where the file gives no name.
pub fn parse(slug: &str, text: &str) -> Result<Pass, String> {
    let header = text
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---"))
        .map(|(header, body)| (header, body.split_once('\n').map_or("", |(_, body)| body)));
    let Some((header, body)) = header else {
        return Err("it should start with a `---` block that says what it `returns`".to_owned());
    };
    let (mut name, mut returns) = (None, None);
    for line in header.lines() {
        match line.split_once(':').map(|(key, value)| (key.trim(), value.trim())) {
            Some(("name", value)) if !value.is_empty() => name = Some(value.trim_matches(['"', '\'']).to_owned()),
            Some(("returns", "changes")) => returns = Some(Returns::Changes),
            Some(("returns", "notes")) => returns = Some(Returns::Notes),
            Some(("returns", other)) => return Err(format!("`returns` should be `changes` or `notes`, not `{other}`")),
            _ => {}
        }
    }
    let instructions = body.trim();
    if instructions.is_empty() {
        return Err("it has no instructions".to_owned());
    }
    Ok(Pass {
        name: name.unwrap_or_else(|| slug.to_owned()),
        returns: returns.ok_or("it should say what it `returns`: `changes` or `notes`")?,
        instructions: instructions.to_owned(),
    })
}

/// Every pass of a project: the built-in ones in their order, each replaced by the project's file of the same name,
/// then the author's own by file name.
pub fn all(project: &Path) -> Vec<Listed> {
    let mut own = project_files(project);
    let mut listed: Vec<Listed> = BUILT_IN
        .iter()
        .map(|(slug, text)| match own.iter().position(|(other, _)| other == slug) {
            Some(at) => project_pass(own.remove(at)),
            None => Listed {
                slug: (*slug).to_owned(),
                origin: Origin::BuiltIn,
                pass: parse(slug, text),
            },
        })
        .collect();
    listed.extend(own.into_iter().map(project_pass));
    listed
}

pub fn find(project: &Path, slug: &str) -> Option<Listed> {
    all(project).into_iter().find(|listed| listed.slug == slug)
}

fn project_pass((slug, text): (String, String)) -> Listed {
    Listed {
        pass: parse(&slug, &text),
        slug,
        origin: Origin::Project,
    }
}

/// The Markdown files in the project's `passes/`, by name, without the style sheet the passes keep there.
fn project_files(project: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(project.join(FOLDER)) else {
        return Vec::new();
    };
    let style_sheet = project.join(style_sheet::FILE);
    let mut files: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter(|path| *path != style_sheet)
        .filter_map(|path| Some((path.file_stem()?.to_str()?.to_owned(), fs::read_to_string(&path).ok()?)))
        .collect();
    files.sort();
    files
}

/// Copies a built-in pass into the project, where the author changes it. Returns the file. One that is there stays.
pub fn customize(project: &Path, slug: &str) -> io::Result<PathBuf> {
    let (_, text) = (BUILT_IN.iter())
        .find(|(built_in, _)| *built_in == slug)
        .ok_or(io::ErrorKind::NotFound)?;
    let file = project.join(FOLDER).join(format!("{slug}.md"));
    if !file.exists() {
        fs::create_dir_all(project.join(FOLDER))?;
        crate::persist::atomic::write(&file, text.as_bytes())?;
    }
    Ok(file)
}

/// What one step of a run works on: a file of the book, or lines of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The file, relative to the project folder.
    pub file: String,
    pub selection: Option<Selected>,
}

/// What the CLI is sent for one step. For the model, so never translated. The file is named by its full path too:
/// a model that has to build one from the project's folder gets it wrong now and then, and asks to read elsewhere.
pub fn prompt(pass: &Pass, project: &Path, target: &Target, style_sheet: Option<&str>) -> String {
    let file = format!(
        "`{}`, whose full path is `{}`",
        target.file,
        project.join(&target.file).display()
    );
    let scope = match &target.selection {
        None => format!("Work on the file {file}, and on nothing else."),
        Some(selected) => format!(
            "Work only on lines {} to {} of the file {file}. The author selected them. Change nothing around them:\n\n{}",
            selected.lines.0,
            selected.lines.1,
            quoted(&selected.text),
        ),
    };
    let returns = match pass.returns {
        Returns::Changes => {
            "This pass returns changes. Edit the file in place with your normal tools. The author reviews every change \
             in the text, so make only the changes these instructions call for and leave every other byte alone. Leave \
             no margin notes."
                .to_owned()
        }
        Returns::Notes => format!(
            "This pass returns margin notes only. Do not edit, create, or delete any file. Call the druck tool `{}` \
             once per point, quoting the exact text it is about, with `{}` as its chapter.",
            tools::LEAVE_NOTE,
            target.file
        ),
    };
    let sheet = sheet_section(style_sheet);
    format!(
        "Run the editing pass \"{}\".\n\n<druck-pass>\n{}\n</druck-pass>\n\n{scope}\n\n{returns}\n\n{sheet}\n\nWhen you \
         are done, reply with one short sentence in the book's language saying what you found.",
        pass.name, pass.instructions
    )
}

/// The pass that reads the whole book first, before its run over the book edits chapter by chapter: one turn sees
/// every chapter, which no chapter's own turn does. A project's copy of it, of the same file name, does too.
pub const SURVEYS_BOOK: &str = "consistency";

/// What the CLI is sent for the read-only turn that starts a run of [`SURVEYS_BOOK`] over the book: read every file and
/// record the book's rules in the style sheet, which the chapters' turns are then handed. For the model, so never
/// translated.
pub fn survey_prompt(pass: &Pass, project: &Path, files: &[String], style_sheet: Option<&str>) -> String {
    let files: Vec<String> = (files.iter())
        .map(|file| format!("- `{file}`, whose full path is `{}`", project.join(file).display()))
        .collect();
    format!(
        "Run the first step of the editing pass \"{}\": collect the rules of the whole book, before its chapters are \
         edited one by one.\n\nRead every file of the book, in this order:\n\n{}\n\nFind what the book writes in more \
         than one way, and what a chapter could write differently from the others:\n\n\
         - Names of people, places, organisations, and products, and their short forms.\n\
         - Spelling variants of one word: with or without a hyphen, as one word or two, capitalised or not, British or \
         American forms, old or reformed German spelling.\n\
         - Hyphenation of compounds, and dashes, quotation marks, and ellipses.\n\
         - The number style: digits or words, dates, times, units, and currencies.\n\
         - Abbreviations, and terms the book uses with a fixed meaning.\n\n\
         For each, take the form the book uses most. If two forms are equally common, take the one a good dictionary \
         of the text's language prefers. Record each decision with the druck tool `{}`: the entry is the thing as it \
         will be written, the rule says in a few words what was decided, in the book's language. Record only what a \
         chapter could get wrong: a word written one way everywhere needs no entry. The decisions in the style sheet \
         are settled: do not record them again.\n\nDo not edit, create, or delete any file. Each chapter is edited in \
         a later step, which follows what you record.\n\n{}\n\nWhen you are done, reply with one short sentence in \
         the book's language saying what you recorded.",
        pass.name,
        files.join("\n"),
        tools::RECORD_STYLE,
        sheet_section(style_sheet),
    )
}

/// The style sheet as a prompt hands it on.
fn sheet_section(style_sheet: Option<&str>) -> String {
    match style_sheet.map(str::trim).filter(|sheet| !sheet.is_empty()) {
        Some(sheet) => format!(
            "The book's style sheet, `{}`, holds the decisions made so far:\n\n<druck-style-sheet>\n{sheet}\n\
             </druck-style-sheet>",
            style_sheet::FILE
        ),
        None => "The book has no style sheet yet.".to_owned(),
    }
}

fn quoted(text: &str) -> String {
    format!("> {}", text.trim_end().replace('\n', "\n> "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("druck-passes-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(FOLDER)).expect("dir");
        dir
    }

    #[test]
    fn a_pass_file_says_its_name_what_it_returns_and_its_instructions() {
        let pass = parse(
            "tighten",
            "---\nname: Tighten\nreturns: notes\n---\n\nCut filler.\n\nKeep the voice.\n",
        )
        .unwrap();
        assert_eq!(pass.name, "Tighten");
        assert_eq!(pass.returns, Returns::Notes);
        assert_eq!(pass.instructions, "Cut filler.\n\nKeep the voice.");
        assert_eq!(
            parse("tighten", "---\nreturns: changes\n---\nCut.").unwrap().name,
            "tighten"
        );
        assert!(parse("x", "Cut filler.").unwrap_err().contains("`---`"));
        assert!(parse("x", "---\nname: X\n---\nCut.").unwrap_err().contains("`returns`"));
        assert!(
            parse("x", "---\nreturns: both\n---\nCut.")
                .unwrap_err()
                .contains("`both`")
        );
        assert!(
            parse("x", "---\nreturns: notes\n---\n\n")
                .unwrap_err()
                .contains("instructions")
        );
    }

    #[test]
    fn the_four_built_in_passes_return_what_the_brief_says() {
        let passes = all(Path::new("/nonexistent"));
        let brief: Vec<_> = (passes.iter())
            .map(|listed| (listed.slug.as_str(), listed.pass.as_ref().unwrap().returns))
            .collect();
        assert_eq!(
            brief,
            [
                ("copyedit", Returns::Changes),
                ("consistency", Returns::Changes),
                ("references", Returns::Notes),
                ("line-edit", Returns::Notes),
            ]
        );
        assert!(passes.iter().all(|listed| listed.origin == Origin::BuiltIn));
    }

    #[test]
    fn a_project_pass_wins_over_the_built_in_of_its_name_and_the_authors_own_follow() {
        let project = scratch("lookup");
        let folder = project.join(FOLDER);
        fs::write(
            folder.join("copyedit.md"),
            "---\nname: House copyedit\nreturns: changes\n---\nOurs.",
        )
        .unwrap();
        fs::write(folder.join("tighten.md"), "---\nreturns: notes\n---\nCut filler.").unwrap();
        fs::write(folder.join("broken.md"), "No header.").unwrap();
        fs::write(folder.join("style-sheet.md"), "# Style sheet\n").unwrap();
        fs::write(folder.join("notes.txt"), "not a pass").unwrap();
        let passes = all(&project);
        let slugs: Vec<_> = passes.iter().map(|listed| listed.slug.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "copyedit",
                "consistency",
                "references",
                "line-edit",
                "broken",
                "tighten"
            ]
        );
        assert_eq!(passes[0].origin, Origin::Project);
        assert_eq!(passes[0].pass.as_ref().unwrap().name, "House copyedit");
        assert_eq!(passes[1].origin, Origin::BuiltIn);
        assert!(passes[4].pass.is_err());
        assert_eq!(find(&project, "tighten").unwrap().origin, Origin::Project);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn customizing_copies_the_built_in_once_and_never_over_the_authors_file() {
        let project = scratch("customize");
        fs::remove_dir_all(project.join(FOLDER)).unwrap();
        let file = customize(&project, "line-edit").unwrap();
        assert_eq!(file, project.join("passes/line-edit.md"));
        assert_eq!(find(&project, "line-edit").unwrap().origin, Origin::Project);
        fs::write(&file, "---\nreturns: notes\n---\nMine.").unwrap();
        customize(&project, "line-edit").unwrap();
        assert!(fs::read_to_string(&file).unwrap().ends_with("Mine."));
        assert!(customize(&project, "tighten").is_err());
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn a_step_names_its_file_what_to_return_and_the_style_sheet() {
        let pass = parse("x", "---\nname: Tighten\nreturns: notes\n---\nCut filler.").unwrap();
        let chapter = Target {
            file: "manuscript/01.md".into(),
            selection: None,
        };
        let sent = prompt(&pass, Path::new("/book"), &chapter, Some("- **e-mail**: hyphenated\n"));
        assert!(sent.contains("<druck-pass>\nCut filler.\n</druck-pass>"));
        assert!(sent.contains("`manuscript/01.md`, whose full path is `/book/manuscript/01.md`, and on nothing else"));
        assert!(sent.contains("Do not edit") && sent.contains("leave_note"));
        assert!(sent.contains("<druck-style-sheet>\n- **e-mail**: hyphenated\n</druck-style-sheet>"));

        let changes = Pass {
            returns: Returns::Changes,
            ..pass
        };
        let selection = Target {
            file: "manuscript/01.md".into(),
            selection: Some(Selected {
                lines: (3, 4),
                text: "One.\nTwo.\n".into(),
            }),
        };
        let sent = prompt(&changes, Path::new("/book"), &selection, None);
        assert!(sent.contains("lines 3 to 4 of the file `manuscript/01.md`"));
        assert!(sent.contains("> One.\n> Two."));
        assert!(sent.contains("Edit the file in place") && sent.contains("no style sheet yet"));
    }

    #[test]
    fn the_first_turn_of_consistency_reads_every_file_and_only_records() {
        let consistency = find(Path::new("/nonexistent"), SURVEYS_BOOK).unwrap().pass.unwrap();
        let files = ["manuscript/01.md".to_owned(), "manuscript/02.md".to_owned()];
        let sent = survey_prompt(
            &consistency,
            Path::new("/book"),
            &files,
            Some("- **Anna**: never Anne\n"),
        );
        assert!(sent.contains(
            "- `manuscript/01.md`, whose full path is `/book/manuscript/01.md`\n- `manuscript/02.md`, whose full path is \
             `/book/manuscript/02.md`"
        ));
        assert!(sent.contains("`record_style`") && sent.contains("Do not edit, create, or delete any file"));
        assert!(sent.contains("<druck-style-sheet>\n- **Anna**: never Anne\n</druck-style-sheet>"));
    }
}
