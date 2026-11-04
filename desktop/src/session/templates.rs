//! What a new section and a new project start from: plain Markdown and plain project folders. Nothing here marks a
//! section as a type: once inserted, it is text like any other.

use std::fs;
use std::path::Path;

use super::outline;
use super::strings::Key;
use super::yaml;

macro_rules! text {
    ($name:literal) => {
        include_str!(concat!("../../templates/sections/", $name, ".md"))
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
    pub text: &'static str,
}

impl Template {
    /// The template's Markdown, with `name` as its heading if it has one.
    pub fn markdown(&self, name: Option<&str>) -> String {
        match (name, outline::outline(self.text).heading) {
            (Some(name), Some(heading)) => {
                format!("{}{name}{}", &self.text[..heading.start], &self.text[heading.end..])
            }
            _ => self.text.to_owned(),
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
        text: text!("half-title"),
    },
    Template {
        slug: "title-page",
        name: Key::TitlePageName,
        description: Key::TitlePageDescription,
        group: Group::Front,
        icon: "icons/title.svg",
        text: text!("title-page"),
    },
    Template {
        slug: "imprint",
        name: Key::ImprintName,
        description: Key::ImprintDescription,
        group: Group::Front,
        icon: "icons/file.svg",
        text: text!("imprint"),
    },
    Template {
        slug: "dedication",
        name: Key::DedicationName,
        description: Key::DedicationDescription,
        group: Group::Front,
        icon: "icons/file.svg",
        text: text!("dedication"),
    },
    Template {
        slug: "contents",
        name: Key::ContentsName,
        description: Key::ContentsDescription,
        group: Group::Front,
        icon: "icons/contents.svg",
        text: text!("contents"),
    },
    Template {
        slug: "abstract",
        name: Key::AbstractName,
        description: Key::AbstractDescription,
        group: Group::Front,
        icon: "icons/book.svg",
        text: text!("abstract"),
    },
    Template {
        slug: "chapter",
        name: Key::ChapterName,
        description: Key::ChapterDescription,
        group: Group::Body,
        icon: "icons/file.svg",
        text: text!("chapter"),
    },
    Template {
        slug: "part",
        name: Key::PartName,
        description: Key::PartDescription,
        group: Group::Body,
        icon: "icons/book.svg",
        text: text!("part"),
    },
    Template {
        slug: "bibliography",
        name: Key::BibliographyName,
        description: Key::BibliographyDescription,
        group: Group::Back,
        icon: "icons/contents.svg",
        text: text!("bibliography"),
    },
    Template {
        slug: "index",
        name: Key::IndexName,
        description: Key::IndexDescription,
        group: Group::Back,
        icon: "icons/contents.svg",
        text: text!("index"),
    },
];

/// A whole project to start from.
pub struct Starter {
    pub slug: &'static str,
    pub name: Key,
    pub description: Key,
    /// Paths in the new folder, with their contents.
    pub files: &'static [(&'static str, &'static [u8])],
}

macro_rules! file {
    ($starter:literal, $path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../templates/starters/", $starter, "/", $path)) as &[u8],
        )
    };
}

pub const STARTERS: &[Starter] = &[
    Starter {
        slug: "book",
        name: Key::BookName,
        description: Key::BookDescription,
        files: &[
            file!("book", "druck.yaml"),
            file!("book", "manuscript/00-front.md"),
            file!("book", "manuscript/01-first-chapter.md"),
            file!("book", "manuscript/02-second-chapter.md"),
            file!("book", "themes/novel/theme.json"),
            file!("book", "themes/novel/ornament.svg"),
        ],
    },
    Starter {
        slug: "report",
        name: Key::ReportName,
        description: Key::ReportDescription,
        files: &[
            file!("report", "druck.yaml"),
            file!("report", "references.bib"),
            file!("report", "manuscript/01-introduction.md"),
            file!("report", "manuscript/02-findings.md"),
            file!("report", "themes/report/theme.json"),
            file!("report", "themes/report/logo.svg"),
        ],
    },
    Starter {
        slug: "offer",
        name: Key::OfferName,
        description: Key::OfferDescription,
        files: &[
            file!("offer", "druck.yaml"),
            file!("offer", "manuscript/01-offer.md"),
            file!("offer", "themes/business/theme.json"),
        ],
    },
];

impl Starter {
    /// Creates the project in `dir`, a folder that does not exist or is empty, titled after the folder.
    pub fn create(&self, dir: &Path) -> Result<(), String> {
        let describe = |error: std::io::Error| format!("cannot create {}: {error}", dir.display());
        if dir.read_dir().is_ok_and(|mut files| files.next().is_some()) {
            return Err(format!("{} already holds files", dir.display()));
        }
        for (path, bytes) in self.files {
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
fn titled(manifest: &str, dir: &Path) -> Result<String, String> {
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
