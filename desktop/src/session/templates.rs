//! What a new section and a new project start from: plain Markdown and plain project folders. Nothing here marks a
//! section as a type: once inserted, it is text like any other.

use std::fs;
use std::path::Path;

use super::outline::{self, Kind as RowKind};
use super::parts::Place;
use super::strings::Key;
use super::yaml;
use crate::shell::strings::Language;

/// A template's text in English and in German, from the `de` folder for one with words to translate.
macro_rules! text {
    ($name:literal) => {
        [include_str!(concat!("../../templates/sections/", $name, ".md")); 2]
    };
    ($name:literal, de) => {
        [
            include_str!(concat!("../../templates/sections/", $name, ".md")),
            include_str!(concat!("../../templates/sections/de/", $name, ".md")),
        ]
    };
}

/// The list a template shows under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Front,
    Body,
    Back,
}

impl Group {
    pub const ALL: [Self; 3] = [Self::Front, Self::Body, Self::Back];

    pub fn label(self) -> &'static str {
        crate::t!(match self {
            Self::Front => Key::GroupFront,
            Self::Body => Key::GroupBody,
            Self::Back => Key::GroupBack,
        })
    }
}

pub struct Template {
    pub slug: &'static str,
    pub name: Key,
    pub description: Key,
    pub group: Group,
    pub icon: &'static str,
    /// In English and in German.
    texts: [&'static str; 2],
}

impl Template {
    /// The template's text in the book's language.
    pub fn text(&self, language: Language) -> &'static str {
        self.texts[usize::from(language == Language::German)]
    }

    /// The template's Markdown in the book's language, with `name` as its heading if it has one.
    pub fn markdown(&self, name: Option<&str>, language: Language) -> String {
        let text = self.text(language);
        match (name, outline::outline(text).heading) {
            (Some(name), Some(heading)) => format!("{}{name}{}", &text[..heading.start], &text[heading.end..]),
            _ => text.to_owned(),
        }
    }
}

pub const TEMPLATES: &[Template] = &[
    Template {
        slug: "half-title",
        name: Key::HalfTitleName,
        description: Key::HalfTitleDescription,
        group: Group::Front,
        icon: "icons/title.svg",
        texts: text!("half-title"),
    },
    Template {
        slug: "title-page",
        name: Key::TitlePageName,
        description: Key::TitlePageDescription,
        group: Group::Front,
        icon: "icons/title.svg",
        texts: text!("title-page"),
    },
    Template {
        slug: "imprint",
        name: Key::ImprintName,
        description: Key::ImprintDescription,
        group: Group::Front,
        icon: "icons/copyright.svg",
        texts: text!("imprint", de),
    },
    Template {
        slug: "dedication",
        name: Key::DedicationName,
        description: Key::DedicationDescription,
        group: Group::Front,
        icon: "icons/dedication.svg",
        texts: text!("dedication", de),
    },
    Template {
        slug: "epigraph",
        name: Key::EpigraphName,
        description: Key::EpigraphDescription,
        group: Group::Front,
        icon: "icons/file.svg",
        texts: text!("epigraph", de),
    },
    Template {
        slug: "contents",
        name: Key::ContentsName,
        description: Key::ContentsDescription,
        group: Group::Front,
        icon: "icons/contents.svg",
        texts: text!("contents"),
    },
    Template {
        slug: "abstract",
        name: Key::AbstractName,
        description: Key::AbstractDescription,
        group: Group::Front,
        icon: "icons/book.svg",
        texts: text!("abstract", de),
    },
    Template {
        slug: "chapter",
        name: Key::ChapterName,
        description: Key::ChapterDescription,
        group: Group::Body,
        icon: "icons/file.svg",
        texts: text!("chapter", de),
    },
    Template {
        slug: "part",
        name: Key::PartName,
        description: Key::PartDescription,
        group: Group::Body,
        icon: "icons/book.svg",
        texts: text!("part", de),
    },
    Template {
        slug: "section",
        name: Key::SectionName,
        description: Key::SectionDescription,
        group: Group::Body,
        icon: "icons/file.svg",
        texts: text!("section", de),
    },
    Template {
        slug: "columns",
        name: Key::ColumnsName,
        description: Key::ColumnsDescription,
        group: Group::Body,
        icon: "icons/directive-columns.svg",
        texts: text!("columns", de),
    },
    Template {
        slug: "acknowledgments",
        name: Key::AcknowledgmentsName,
        description: Key::AcknowledgmentsDescription,
        group: Group::Back,
        icon: "icons/book.svg",
        texts: text!("acknowledgments", de),
    },
    Template {
        slug: "about-the-author",
        name: Key::AboutAuthorName,
        description: Key::AboutAuthorDescription,
        group: Group::Back,
        icon: "icons/book.svg",
        texts: text!("about-the-author", de),
    },
    Template {
        slug: "bibliography",
        name: Key::BibliographyName,
        description: Key::BibliographyDescription,
        group: Group::Back,
        icon: "icons/bibliography.svg",
        texts: text!("bibliography"),
    },
    Template {
        slug: "index",
        name: Key::IndexName,
        description: Key::IndexDescription,
        group: Group::Back,
        icon: "icons/index.svg",
        texts: text!("index"),
    },
];

/// A page the Binder's gallery adds: its template, its name there, where it goes, and the role a book has once at
/// most, if it is one.
pub struct Page {
    pub slug: &'static str,
    pub name: Key,
    pub place: Place,
    pub once: Option<RowKind>,
}

impl Page {
    pub fn template(&self) -> &'static Template {
        TEMPLATES
            .iter()
            .find(|template| template.slug == self.slug)
            .expect("every page has its template")
    }
}

pub const PAGES: &[Page] = &[
    Page {
        slug: "title-page",
        name: Key::TitlePageName,
        place: Place::Opening(0),
        once: Some(RowKind::TitlePage),
    },
    Page {
        slug: "imprint",
        name: Key::CopyrightName,
        place: Place::Opening(1),
        once: Some(RowKind::Copyright),
    },
    Page {
        slug: "dedication",
        name: Key::DedicationName,
        place: Place::Opening(2),
        once: None,
    },
    Page {
        slug: "epigraph",
        name: Key::EpigraphName,
        place: Place::Opening(3),
        once: None,
    },
    Page {
        slug: "contents",
        name: Key::ContentsName,
        place: Place::Contents,
        once: Some(RowKind::Contents),
    },
    Page {
        slug: "acknowledgments",
        name: Key::AcknowledgmentsName,
        place: Place::Back,
        once: None,
    },
    Page {
        slug: "about-the-author",
        name: Key::AboutAuthorName,
        place: Place::Back,
        once: None,
    },
    Page {
        slug: "index",
        name: Key::IndexName,
        place: Place::End,
        once: Some(RowKind::Index),
    },
];

/// A whole project to start from: a theme, and sample text that shows the theme's elements.
pub struct Starter {
    pub slug: &'static str,
    pub name: Key,
    pub description: Key,
    /// The new folder's suggested name, which becomes the title.
    pub folder: Key,
    /// The name its manifest gives its theme. A project is known by it, never by a record of its starter.
    pub theme: &'static str,
    /// The page of the sample, counted from 0, that the gallery shows.
    pub shown: usize,
    /// The templates a new section is offered, by slug.
    templates: &'static [&'static str],
    /// Paths in the new folder, with their contents, in English and in German.
    files: [&'static [(&'static str, &'static [u8])]; 2],
}

macro_rules! file {
    ($starter:literal, $path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../templates/starters/", $starter, "/", $path)) as &[u8],
        )
    };
}

/// A file of the German starter, from its `de` folder.
macro_rules! german {
    ($starter:literal, $path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../templates/starters/", $starter, "/de/", $path)) as &[u8],
        )
    };
}

/// What a book's repo leaves out, since exports are made again at will.
pub const GITIGNORE: &str = ".gitignore";
pub const GITIGNORE_TEXT: &str = "build/\n";

pub const STARTERS: &[Starter] = &[
    Starter {
        slug: "novel",
        name: Key::NovelName,
        description: Key::NovelDescription,
        folder: Key::NovelFolder,
        theme: "novel",
        shown: 6,
        templates: &[
            "half-title",
            "title-page",
            "imprint",
            "dedication",
            "contents",
            "chapter",
            "part",
        ],
        files: [
            &[
                file!("novel", "druck.yaml"),
                file!("novel", "druck-words.txt"),
                file!("novel", "manuscript/00-front.md"),
                file!("novel", "manuscript/01-first-chapter.md"),
                file!("novel", "manuscript/02-second-chapter.md"),
                file!("novel", "themes/novel/theme.json"),
                file!("novel", "themes/novel/ornament.svg"),
            ],
            &[
                german!("novel", "druck.yaml"),
                german!("novel", "manuscript/00-vorspann.md"),
                german!("novel", "manuscript/01-erstes-kapitel.md"),
                german!("novel", "manuscript/02-zweites-kapitel.md"),
                file!("novel", "themes/novel/theme.json"),
                file!("novel", "themes/novel/ornament.svg"),
            ],
        ],
    },
    Starter {
        slug: "nonfiction",
        name: Key::NonfictionName,
        description: Key::NonfictionDescription,
        folder: Key::NonfictionFolder,
        theme: "nonfiction",
        shown: 6,
        templates: &[
            "half-title",
            "title-page",
            "imprint",
            "dedication",
            "contents",
            "chapter",
            "part",
            "bibliography",
            "index",
        ],
        files: [
            &[
                file!("nonfiction", "druck.yaml"),
                file!("nonfiction", "references.bib"),
                file!("nonfiction", "manuscript/00-front.md"),
                file!("nonfiction", "manuscript/01-first-chapter.md"),
                file!("nonfiction", "manuscript/02-second-chapter.md"),
                file!("nonfiction", "manuscript/03-back.md"),
                file!("nonfiction", "themes/nonfiction/theme.json"),
            ],
            &[
                german!("nonfiction", "druck.yaml"),
                file!("nonfiction", "references.bib"),
                german!("nonfiction", "manuscript/00-vorspann.md"),
                german!("nonfiction", "manuscript/01-erstes-kapitel.md"),
                german!("nonfiction", "manuscript/02-zweites-kapitel.md"),
                german!("nonfiction", "manuscript/03-anhang.md"),
                file!("nonfiction", "themes/nonfiction/theme.json"),
            ],
        ],
    },
    Starter {
        slug: "article",
        name: Key::ArticleName,
        description: Key::ArticleDescription,
        folder: Key::ArticleFolder,
        theme: "article",
        shown: 0,
        templates: &["section", "columns", "bibliography"],
        files: [
            &[
                file!("article", "druck.yaml"),
                file!("article", "manuscript/01-article.md"),
                file!("article", "themes/article/theme.json"),
            ],
            &[
                german!("article", "druck.yaml"),
                german!("article", "manuscript/01-artikel.md"),
                file!("article", "themes/article/theme.json"),
            ],
        ],
    },
    Starter {
        slug: "paper",
        name: Key::PaperName,
        description: Key::PaperDescription,
        folder: Key::PaperFolder,
        theme: "paper",
        shown: 0,
        templates: &["columns"],
        files: [
            &[
                file!("paper", "druck.yaml"),
                file!("paper", "references.bib"),
                file!("paper", "manuscript/01-paper.md"),
                file!("paper", "themes/paper/theme.json"),
            ],
            &[
                german!("paper", "druck.yaml"),
                file!("paper", "references.bib"),
                german!("paper", "manuscript/01-paper.md"),
                file!("paper", "themes/paper/theme.json"),
            ],
        ],
    },
    Starter {
        slug: "thesis",
        name: Key::ThesisName,
        description: Key::ThesisDescription,
        folder: Key::ThesisFolder,
        theme: "thesis",
        shown: 0,
        templates: &["dedication", "abstract", "contents", "chapter", "bibliography", "index"],
        files: [
            &[
                file!("thesis", "druck.yaml"),
                file!("thesis", "references.bib"),
                file!("thesis", "manuscript/00-front.md"),
                file!("thesis", "manuscript/01-introduction.md"),
                file!("thesis", "manuscript/02-findings.md"),
                file!("thesis", "manuscript/03-back.md"),
                file!("thesis", "themes/thesis/theme.json"),
            ],
            &[
                german!("thesis", "druck.yaml"),
                file!("thesis", "references.bib"),
                german!("thesis", "manuscript/00-vorspann.md"),
                german!("thesis", "manuscript/01-einleitung.md"),
                german!("thesis", "manuscript/02-ergebnisse.md"),
                german!("thesis", "manuscript/03-anhang.md"),
                file!("thesis", "themes/thesis/theme.json"),
            ],
        ],
    },
];

/// The templates a new section is offered in a project whose manifest names `theme`: a starter's own in a project that
/// keeps its starter's theme, and all of them in any other.
pub fn templates_for(theme: Option<&str>) -> Vec<&'static Template> {
    let starter = STARTERS.iter().find(|starter| Some(starter.theme) == theme);
    TEMPLATES
        .iter()
        .filter(|template| starter.is_none_or(|starter| starter.templates.contains(&template.slug)))
        .collect()
}

impl Starter {
    /// Creates the project in `dir`, a folder that does not exist or is empty, titled after the folder, in `language`.
    pub fn create(&self, dir: &Path, language: Language) -> Result<(), String> {
        let describe = |error: std::io::Error| crate::t!(Key::CannotCreate, path = dir.display(), error = error);
        if dir.read_dir().is_ok_and(|mut files| files.next().is_some()) {
            return Err(crate::t!(Key::AlreadyHoldsFiles, path = dir.display()));
        }
        fs::create_dir_all(dir).map_err(describe)?;
        fs::write(dir.join(GITIGNORE), GITIGNORE_TEXT).map_err(describe)?;
        for (path, bytes) in self.files[usize::from(language == Language::German)] {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(describe)?;
            }
            if *path == druck::input::MANIFEST {
                let manifest = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
                fs::write(&target, titled(manifest, dir)?).map_err(describe)?;
            } else {
                fs::write(&target, bytes).map_err(describe)?;
            }
        }
        Ok(())
    }
}

/// The manifest with the folder's name as its title.
pub(super) fn titled(manifest: &str, dir: &Path) -> Result<String, String> {
    let Some(name) = dir.file_name().map(|name| name.to_string_lossy()) else {
        return Ok(manifest.to_owned());
    };
    let edit = yaml::set(manifest, "title", &yaml::scalar(&name), &[])?;
    Ok(format!(
        "{}{}{}",
        &manifest[..edit.range.start],
        edit.text,
        &manifest[edit.range.end..]
    ))
}
