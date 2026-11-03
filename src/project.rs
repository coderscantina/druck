//! Project folders: a `druck.yaml` manifest, chapter files joined into one document, and named exports.
//!
//! Loading only returns values and diagnostics. Presenting them is up to the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_saphyr::Spanned;

use crate::config::front_matter::{self, Export, FrontMatter};
use crate::config::resolved::Config;
use crate::config::source::{ChapterMap, Source, normalize};
use crate::diagnostic::Diagnostic;
use crate::input::{DocumentText, ExportSettings};
use crate::pipeline::{self, DocumentFile, Request};
use crate::renderer::PdfOptions;

pub const MANIFEST: &str = "druck.yaml";

/// The folder whose Markdown files must be listed as chapters or are reported as left out.
const MANUSCRIPT: &str = "manuscript";

/// A project with its manifest read and its chapters joined.
pub struct Project {
    dir: PathBuf,
    exports: BTreeMap<String, Spanned<Export>>,
    /// Markdown files in `manuscript/` the chapter list does not name.
    warnings: Vec<Diagnostic>,
    manifest: PathBuf,
    settings: FrontMatter,
    source: Source,
    text: String,
}

impl Project {
    /// Reads the manifest in `dir`, which is absolute, and joins the chapters it lists.
    pub fn open(dir: &Path) -> Result<Self, Vec<Diagnostic>> {
        let manifest = dir.join(MANIFEST);
        let source = Source::Document(manifest.clone());
        if !manifest.is_file() {
            return Err(vec![Diagnostic::new(
                Some(source),
                "not found: a project folder needs a druck.yaml",
            )]);
        }
        let text = pipeline::read(&manifest, &source)?;
        let mut settings = pipeline::parse_settings(&text, 1, &source)?;
        let chapters = settings.chapters.take().unwrap_or_default();
        let exports = settings.exports.take().unwrap_or_default();
        check_exports(&exports, &source)?;
        let paths = chapter_paths(&chapters, dir, &source)?;
        let (map, text) = join(&paths, manifest.clone())?;
        let listed: BTreeSet<&PathBuf> = paths.iter().collect();
        let warnings = markdown_files(&dir.join(MANUSCRIPT))
            .into_iter()
            .filter(|path| !listed.contains(path))
            .map(|path| {
                Diagnostic::warning(
                    Some(Source::Document(path)),
                    format!("excluded from the book: not listed under chapters in {MANIFEST}"),
                )
            })
            .collect();
        Ok(Self {
            dir: dir.to_owned(),
            exports,
            warnings,
            manifest,
            settings,
            source: Source::Chapters(Arc::new(map)),
            text,
        })
    }

    /// Markdown files in `manuscript/` the chapter list does not name.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// The named exports of the manifest.
    pub fn exports(&self) -> impl Iterator<Item = (&str, &Export)> {
        self.exports.iter().map(|(name, export)| (name.as_str(), &export.value))
    }

    /// The document with the export's theme and draft applied, with its print and final settings beside it.
    pub fn export(mut self, name: Option<&str>) -> Result<(DocumentText, ExportSettings), Vec<Diagnostic>> {
        let mut settings = ExportSettings {
            pdf: PdfOptions::default(),
            is_final: false,
            output: None,
            print_conflict: None,
        };
        if let Some(name) = name {
            let manifest = Source::Document(self.manifest.clone());
            let Some(export) = self.exports.get(name) else {
                let message = format!("no export named \"{name}\"");
                return Err(vec![Diagnostic::new(Some(manifest), message)]);
            };
            let has_print = export.value.bleed.is_some() || export.value.crop_marks;
            settings.print_conflict = has_print.then(|| {
                let message = format!("export \"{name}\": bleed and crop-marks apply to PDF output only, not to EPUB");
                at(Diagnostic::new(Some(manifest), message), &export.referenced)
            });
            let export = &export.value;
            settings.pdf.bleed = export.bleed.unwrap_or(settings.pdf.bleed);
            settings.pdf.crop_marks = export.crop_marks;
            settings.is_final = export.is_final;
            settings.output = export.output.as_ref().map(|path| normalize(&self.dir.join(path)));
            self.settings.theme = export.theme.clone().or(self.settings.theme);
            self.settings.draft = export.draft.or(self.settings.draft);
        }
        Ok((DocumentText::Project(Box::new(self)), settings))
    }

    /// Resolves the configuration: the manifest's settings with the export's, then `args`.
    pub(crate) fn configure(
        &self,
        args: &Request,
        working_dir: &Path,
    ) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
        let manifest_source = Source::Document(self.manifest.clone());
        let config = pipeline::resolve_settings(
            args,
            self.settings.clone(),
            &manifest_source,
            Some(&self.dir),
            working_dir,
        )?;
        let document = DocumentFile {
            path: self.manifest.clone(),
            source: self.source.clone(),
            text: self.text.clone(),
        };
        Ok((config, document))
    }
}

/// The errors for a final export of a document with open placeholders, at their real locations.
pub fn check_final(config: &Config, document: &DocumentFile, export: &str) -> Result<(), Vec<Diagnostic>> {
    let errors: Vec<_> = pipeline::parse(config, document)?
        .todos
        .into_iter()
        .map(|todo| {
            let message = format!(
                "export \"{export}\" is final, but this placeholder is open: {}",
                todo.text
            );
            Diagnostic::new(Some(document.source.clone()), message).at(todo.at.line, todo.at.column)
        })
        .collect();
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn at(diagnostic: Diagnostic, location: &serde_saphyr::Location) -> Diagnostic {
    diagnostic.at(location.line(), location.column())
}

fn check_exports(exports: &BTreeMap<String, Spanned<Export>>, source: &Source) -> Result<(), Vec<Diagnostic>> {
    let errors: Vec<_> = exports
        .iter()
        .filter(|(name, _)| !crate::theme_path::is_name(name))
        .map(|(name, export)| {
            let message = format!("export name \"{name}\" must consist of letters, digits, \"-\", and \"_\"");
            at(Diagnostic::new(Some(source.clone()), message), &export.referenced)
        })
        .collect();
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The absolute chapter paths, checked against the project folder. Errors name the manifest line.
fn chapter_paths(chapters: &[Spanned<String>], dir: &Path, source: &Source) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
    if chapters.is_empty() {
        return Err(vec![Diagnostic::new(
            Some(source.clone()),
            "chapters: list at least one chapter file",
        )]);
    }
    let mut paths = Vec::with_capacity(chapters.len());
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for chapter in chapters {
        let path = normalize(&dir.join(&chapter.value));
        let name = &chapter.value;
        let problem = if !path.starts_with(dir) {
            Some(format!("chapter \"{name}\" is outside the project folder"))
        } else if !path.is_file() {
            Some(format!("chapter not found: {name}"))
        } else if !seen.insert(path.clone()) {
            Some(format!("chapter \"{name}\" is listed twice"))
        } else {
            None
        };
        match problem {
            Some(message) => errors.push(at(Diagnostic::new(Some(source.clone()), message), &chapter.referenced)),
            None => paths.push(path),
        }
    }
    if errors.is_empty() { Ok(paths) } else { Err(errors) }
}

/// Joins chapter files in order with a blank line between them, recording where each one landed.
fn join(paths: &[PathBuf], manifest: PathBuf) -> Result<(ChapterMap, String), Vec<Diagnostic>> {
    let mut map = ChapterMap::new(manifest);
    let mut joined = String::new();
    let mut next_line = 1;
    let mut errors = Vec::new();
    for path in paths {
        let source = Source::Document(path.clone());
        let text = match pipeline::read(path, &source) {
            Ok(text) => text,
            Err(mut read) => {
                errors.append(&mut read);
                continue;
            }
        };
        match front_matter::split(&text) {
            Ok(None) => {}
            Ok(Some(_)) => errors.push(
                Diagnostic::new(
                    Some(source),
                    format!("a chapter has no front matter; move it to {MANIFEST}"),
                )
                .at(1, 1),
            ),
            Err(message) => errors.push(Diagnostic::new(Some(source), message).at(1, 1)),
        }
        let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
        map.push(path.clone(), next_line, body.lines().count() as u64);
        joined.push_str(body);
        if !body.is_empty() && !body.ends_with('\n') {
            joined.push('\n');
        }
        joined.push('\n');
        next_line = joined.matches('\n').count() as u64 + 1;
    }
    if errors.is_empty() {
        Ok((map, joined))
    } else {
        Err(errors)
    }
}

/// The Markdown files below `dir`, in every subfolder, sorted. A missing folder has none.
fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for path in std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
        {
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
