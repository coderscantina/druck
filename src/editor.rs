//! Unstable internals for `druck-desktop`, behind the `editor` feature. Outside `API_VERSION`: anything here
//! may change in any release, and the desktop, which pins the path dependency, follows.
//!
//! Each item says what the desktop needs it for. Everything else it uses is the public API.

/// The version the CLI prints, as in `26.10.7-1a2b3c4`: the desktop's About box and `--version` say the same.
pub const VERSION: &str = env!("DRUCK_VERSION");

/// Citation syntax: the source layer must tint exactly what the parser reads as a citation.
pub use crate::bibliography::syntax;
/// Comment finding: the source layer must cut a comment out of a block exactly where the parser does.
pub use crate::markdown::comments;
/// Inline mark pairing: the source layer must pair `^x^`, `~x~`, and `{todo:` as the parser does.
pub use crate::markdown::marks;

/// The theme as a JSON value: the theme file code edits and saves partial themes over the bundled default.
pub mod theme_value {
    pub use crate::config::layer::default_theme_value;
    pub use crate::config::merge::merge;
}

/// Where named themes are kept: the theme file code lists, finds, and creates them.
pub mod theme_path {
    pub use crate::theme_path::*;
}

/// The resolved configuration: the theme validator runs `resolve` as a render does, and the preview reads the
/// body style and the document's title and authors from `Config`.
pub mod resolved_config {
    pub use crate::config::front_matter::{ChapterEntry, DEFAULT_STATUSES, FrontMatter, split as split_front_matter};
    pub use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
    pub use crate::config::resolved::{Config, Style};
    pub use crate::config::source::InputSource;
    pub use crate::config::theme::{FontStyle, Weight};
    /// The session reads `druck.yaml` and front matter as the engine does, so the app has no second YAML reader.
    pub use crate::pipeline::parse_settings;

    pub fn of<'a>(configured: &'a crate::Configured<'_>) -> &'a Config {
        &configured.config
    }
}

/// Installed fonts: the theme validator asks for the families a theme takes from the machine as a layout does, so a
/// font family that is neither in `fonts` nor installed is refused when it is typed and not when the pages fail.
pub mod installed_fonts {
    pub use crate::installed::lookup;
}

/// Which theme file a selection or a document means: the Styles tab edits that file.
pub mod engine {
    pub use crate::pipeline::{document_theme, theme_file};
}

/// Word counts: the footer counts a chapter and the book the way `{words}` counts a document, from the text alone.
pub mod statistics {
    use std::path::PathBuf;

    use crate::config::source::InputSource;
    use crate::config::theme::Lang;

    pub use crate::statistics::Statistics;

    /// The statistics of Markdown text with no front matter, read as `lang` (`"en"` or `"de"`, anything else is
    /// English). `None` when the text alone does not parse, such as a chapter that closes a directive another opened.
    pub fn count(markdown: &str, lang: &str) -> Option<Statistics> {
        let lang = if lang == "de" { Lang::De } else { Lang::En };
        let source = InputSource::Overrides {
            working_dir: PathBuf::new(),
        };
        let document = crate::markdown::parse_with(markdown, 1, &source, lang, false).ok()?;
        Some(Statistics::count(&document, lang))
    }
}

/// Placeholders in the text: Write shows the value a render writes, from the same names, counts, and dates.
pub mod placeholders {
    use crate::config::theme::Lang;

    pub use crate::config::template::Placeholder;
    pub use crate::date::Date;
    /// The length of a placeholder name after `{`, by the parser's rule.
    pub use crate::markdown::marks::field_name;
    pub use crate::statistics::Statistics;

    fn lang(code: &str) -> Lang {
        if code == "de" { Lang::De } else { Lang::En }
    }

    /// A count as the text shows it, grouped by thousands for `lang`: "12,480" or "12.480".
    pub fn count(n: usize, lang_code: &str) -> String {
        crate::layout::fields::grouped(n, lang(lang_code))
    }

    /// `{reading-time}` in whole minutes, at the speed for `lang`.
    pub fn reading_time(statistics: &Statistics, lang_code: &str) -> usize {
        statistics.reading_time(lang(lang_code))
    }

    /// The words a minute `{reading-time}` reads at in `lang`.
    pub fn words_per_minute(lang_code: &str) -> usize {
        crate::statistics::words_per_minute(lang(lang_code))
    }

    /// `{build-date}`: the day written out for `lang`.
    pub fn long_date(date: Date, lang_code: &str) -> String {
        date.long(lang(lang_code))
    }

    /// The default theme's `draft` label for `lang`, the value of `{draft}` in a draft.
    pub fn draft_label(lang_code: &str) -> String {
        let theme = crate::config::layer::default_theme_value();
        let code = if lang_code == "de" { "de" } else { "en" };
        theme["labels"][code]["draft"].as_str().unwrap_or_default().to_owned()
    }

    /// The day a render run now would use: `SOURCE_DATE_EPOCH`'s, or the clock's in UTC.
    pub fn today() -> Option<Date> {
        match crate::resource::source_date() {
            Ok(Some(date)) => Some(date),
            Ok(None) => crate::resource::clock_date().ok(),
            Err(_) => None,
        }
    }
}

/// What the galley's rich blocks ask of the engine, so a chip, a picker, or a refused edit says what the pages say.
pub mod rich {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::bibliography::Citations;
    use crate::bibliography::syntax::Citation;
    use crate::config::source::InputSource;
    use crate::config::theme::Lang;
    use crate::{Configured, Failure, citations, layout, markdown, pipeline};

    /// The engine's text for what a document cites and refers to.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct Labels {
        /// By [`citation_key`]. A citation with a key the bibliography lacks is not here.
        pub citations: HashMap<String, String>,
        /// By label with its prefix, such as `sec:intro`: "Section 2", or an unnumbered heading's text.
        pub references: HashMap<String, String>,
    }

    /// What two citations must share to read the same, whatever their place in the text.
    pub fn citation_key(citation: &Citation) -> String {
        use std::fmt::Write;
        let mut key = format!("{:?}", citation.form);
        for item in &citation.items {
            write!(
                key,
                ";{:?}|{}|{:?}|{:?}",
                item.prefix, item.key, item.locator, item.suffix
            )
            .expect("writing to a string works");
        }
        key
    }

    /// Reads the labels of a configured document. A citation of an unknown key is left out here and still fails the
    /// layout, so the other chips keep their text while that one is wrong.
    pub fn labels(configured: &Configured<'_>) -> Result<Labels, Failure> {
        let document = pipeline::parse(&configured.config, &configured.document)?;
        let bibliography = pipeline::load_bibliography(&configured.job, &configured.config)?;
        let mut labels = Labels::default();
        let mut references = Vec::new();
        if let Some(bibliography) = &bibliography {
            let mut order = Vec::with_capacity(document.citations.len());
            citations::reading_order(&document.blocks, &document, &mut order);
            let mut cited = Citations::new(bibliography);
            let mut added = Vec::with_capacity(order.len());
            for index in order {
                let syntax = &document.citations[index].syntax;
                if let Ok(id) = cited.add(syntax) {
                    added.push((citation_key(syntax), id));
                }
            }
            let rendered = cited.finish(
                configured.config.document.citation_style,
                configured.config.document.lang,
            );
            for (key, id) in added {
                let text = rendered.citation(id).iter().map(|part| part.text.as_str()).collect();
                labels.citations.insert(key, text);
            }
            references = rendered.into_references();
        }
        let structure = layout::structure::Structure::new(&document, &configured.config, &references);
        labels.references = structure
            .label_texts()
            .map(|(label, text)| (label.to_owned(), text.to_owned()))
            .collect();
        Ok(labels)
    }

    /// What the parser says is wrong with Markdown text with no front matter, read as `lang`, as messages without a
    /// place. References to labels and notes outside the text count, so a caller checks whole documents.
    pub fn parse_problems(markdown: &str, lang: &str) -> Vec<String> {
        let lang = if lang == "de" { Lang::De } else { Lang::En };
        let source = InputSource::Overrides {
            working_dir: PathBuf::new(),
        };
        match markdown::parse_with(markdown, 1, &source, lang, false) {
            Ok(_) => Vec::new(),
            Err(problems) => problems.into_iter().map(|problem| problem.message).collect(),
        }
    }

    /// The natural size in points of an image's bytes, or why the engine would refuse it. `extension` is the file's.
    pub fn image_size(bytes: Vec<u8>, extension: &str) -> Result<(f32, f32), String> {
        let image = crate::image::Image::decode(bytes, extension)?;
        let (width, height) = image.size();
        Ok((width.0 as f32, height.0 as f32))
    }

    /// One work of a bibliography, for a picker to search.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct Work {
        pub key: String,
        /// Family names, as a citation names them.
        pub authors: String,
        pub title: String,
        pub year: String,
    }

    /// The works of a BibTeX text in file order, as far as it can be read. A file with problems still lists the rest:
    /// the diagnostics report them, and the picker should not go empty over one bad entry.
    pub fn works(bibtex: &str) -> Vec<Work> {
        crate::bibliography::read_works(bibtex)
    }
}
