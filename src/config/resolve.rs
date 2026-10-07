//! Configuration resolution: bundled default, one theme, document settings, CLI overrides.
//!
//! Each input is validated on its own first, so a later override never excuses an invalid
//! earlier value. The merged result is then checked for references and combinations.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;

use super::front_matter::FrontMatter;
use super::layer::{PropertyError, check_theme_layer, default_theme_value, deserialize_theme};
use super::merge::Merged;
use super::resolved::{self, Config, FontFiles, PageGeometry, SlotContent, Style};
use super::source::{Resource, Source};
use super::template::{Placeholder, Template};
use super::theme::{self, BlockStyle, FontStyle, PageSize, PaperSize, Theme, TitleSlot, Weight};
use super::values::{Color, FontName, Length, Pt, Size, Spacing, Spec, TokenKind, TokenName};
use crate::diagnostic::Diagnostic;

pub struct ThemeInput {
    pub source: Source,
    pub value: Value,
}

pub struct SettingsInput {
    pub source: Source,
    pub settings: FrontMatter,
}

/// Configuration inputs in precedence order, lowest first after the bundled default.
pub struct Inputs {
    pub theme: Option<ThemeInput>,
    pub document: SettingsInput,
    pub overrides: SettingsInput,
}

pub fn resolve(inputs: Inputs) -> Result<Config, Vec<Diagnostic>> {
    let Inputs {
        theme,
        document,
        overrides,
    } = inputs;
    let mut sources = vec![Source::Bundled];
    let mut merged = Merged::new(default_theme_value(), 0);

    if let Some(theme) = theme {
        if let Err(error) = check_theme_layer(&theme.value) {
            return Err(vec![property_diagnostic(Some(theme.source), error)]);
        }
        merged.apply(theme.value, sources.len());
        sources.push(theme.source);
    }
    for settings in [&document, &overrides] {
        merged.apply(settings.settings.theme_layer(), sources.len());
        sources.push(settings.source.clone());
    }

    let theme = deserialize_theme(merged.value.clone()).map_err(|error| {
        let source = error.property.as_deref().and_then(|p| merged.source_of(&pointer_of(p)));
        vec![property_diagnostic(source.map(|i| sources[i].clone()), error)]
    })?;

    let resolver = Resolver::new(&theme, &merged, &sources);
    let config = resolver.config(&document, &overrides);
    let errors = resolver.errors.into_inner();
    match config {
        Some(config) if errors.is_empty() => Ok(config),
        _ => Err(errors),
    }
}

fn property_diagnostic(source: Option<Source>, error: PropertyError) -> Diagnostic {
    Diagnostic::new(source, error.message).property(error.property)
}

fn pointer_of(property: &str) -> String {
    format!("/{}", property.replace('~', "~0").replace('/', "~1").replace('.', "/"))
}

/// Tokens resolved to literals. `None` marks a token whose error was already reported.
struct TokenTable<T>(BTreeMap<TokenName, Option<T>>);

struct Tokens {
    fonts: TokenTable<FontName>,
    sizes: TokenTable<Size>,
    spacing: TokenTable<Spacing>,
    colors: TokenTable<Color>,
}

struct Resolver<'a> {
    theme: &'a Theme,
    merged: &'a Merged,
    sources: &'a [Source],
    tokens: Tokens,
    errors: RefCell<Vec<Diagnostic>>,
}

impl<'a> Resolver<'a> {
    fn new(theme: &'a Theme, merged: &'a Merged, sources: &'a [Source]) -> Self {
        let mut resolver = Self {
            theme,
            merged,
            sources,
            tokens: Tokens {
                fonts: TokenTable(BTreeMap::new()),
                sizes: TokenTable(BTreeMap::new()),
                spacing: TokenTable(BTreeMap::new()),
                colors: TokenTable(BTreeMap::new()),
            },
            errors: RefCell::new(Vec::new()),
        };
        resolver.tokens = Tokens {
            fonts: resolver.token_table(&theme.tokens.fonts),
            sizes: resolver.token_table(&theme.tokens.sizes),
            spacing: resolver.token_table(&theme.tokens.spacing),
            colors: resolver.token_table(&theme.tokens.colors),
        };
        resolver
    }

    fn error(&self, property: &str, message: impl Into<String>) {
        self.error_among(property, &[], message);
    }

    /// Reports an error caused by a combination of values. The most recent layer among
    /// `property` and `related` is named, since it introduced the conflict.
    fn error_among(&self, property: &str, related: &[&str], message: impl Into<String>) {
        let source = [property]
            .iter()
            .chain(related)
            .filter_map(|p| self.merged.source_of(&pointer_of(p)))
            .max()
            .map(|i| self.sources[i].clone());
        self.errors
            .borrow_mut()
            .push(Diagnostic::new(source, message).property(Some(property.to_owned())));
    }

    fn resource(&self, pointer: &str, path: &theme::ResourcePath) -> Resource {
        let layer = self.merged.source_of(pointer).expect("every merged value has a layer");
        Resource {
            origin: self.sources[layer].origin(),
            path: path.0.clone(),
        }
    }

    /// Follows token-to-token references, reporting undefined targets and cycles once.
    fn token_table<T: TokenKind + Clone>(&self, group: &BTreeMap<TokenName, Spec<T>>) -> TokenTable<T> {
        let resolved = group
            .keys()
            .map(|start| {
                let mut chain = vec![start];
                let mut spec = &group[start];
                let value = loop {
                    match spec {
                        Spec::Literal(value) => break Some(value.clone()),
                        Spec::Token(next) if chain.contains(&next) => {
                            let cycle = &chain[chain.iter().position(|n| *n == next).expect("contained")..];
                            if next == start && cycle.iter().all(|n| start <= *n) {
                                let names: Vec<_> = cycle
                                    .iter()
                                    .chain([&start])
                                    .map(|n| format!("${}.{n}", T::GROUP))
                                    .collect();
                                self.error(
                                    &format!("tokens.{}.{start}", T::GROUP),
                                    format!("cyclic token reference {}", names.join(" -> ")),
                                );
                            }
                            break None;
                        }
                        Spec::Token(next) => match group.get_key_value(next) {
                            Some((key, target)) => {
                                chain.push(key);
                                spec = target;
                            }
                            None => {
                                if chain.len() == 1 {
                                    self.error(
                                        &format!("tokens.{}.{start}", T::GROUP),
                                        format!("undefined token ${}.{next}", T::GROUP),
                                    );
                                }
                                break None;
                            }
                        },
                    }
                };
                (start.clone(), value)
            })
            .collect();
        TokenTable(resolved)
    }

    fn value<T: TokenKind + Clone>(&self, table: &TokenTable<T>, spec: &Spec<T>, property: &str) -> Option<T> {
        match spec {
            Spec::Literal(value) => Some(value.clone()),
            Spec::Token(name) => match table.0.get(name) {
                Some(value) => value.clone(),
                None => {
                    self.error(property, format!("undefined token ${}.{name}", T::GROUP));
                    None
                }
            },
        }
    }

    fn spacing(&self, spec: &Spec<Spacing>, property: &str, em: Pt) -> Option<Pt> {
        self.value(&self.tokens.spacing, spec, property).map(|s| s.0.to_pt(em))
    }

    fn color(&self, spec: &Spec<Color>, property: &str) -> Option<Color> {
        self.value(&self.tokens.colors, spec, property)
    }

    /// A font family name defined in `fonts` with the requested face.
    fn font(&self, spec: &Spec<FontName>, property: &str, weight: Weight, style: FontStyle) -> Option<String> {
        let name = self.value(&self.tokens.fonts, spec, property)?.0;
        let token = match spec {
            Spec::Token(token) => format!("tokens.fonts.{token}"),
            Spec::Literal(_) => String::new(),
        };
        let via = match spec {
            Spec::Token(_) => format!(" (via {spec})"),
            Spec::Literal(_) => String::new(),
        };
        let Some(family) = self.theme.fonts.get(&name) else {
            self.error_among(
                property,
                &[&token],
                format!("font family \"{name}\"{via} is not defined in fonts"),
            );
            return None;
        };
        let has_face = match (weight, style) {
            (Weight::Regular, FontStyle::Normal) => true,
            (Weight::Regular, FontStyle::Italic) => family.italic.is_some(),
            (Weight::Bold, FontStyle::Normal) => family.bold.is_some(),
            (Weight::Bold, FontStyle::Italic) => family.bold_italic.is_some(),
        };
        if !has_face {
            self.error_among(
                property,
                &[&token, &format!("fonts.{name}")],
                format!("font family \"{name}\"{via} has no {} face", face_name(weight, style)),
            );
            return None;
        }
        Some(name)
    }

    fn body_size(&self) -> Option<Pt> {
        let property = "styles.body.size";
        match self
            .value(&self.tokens.sizes, &self.theme.styles.body.size, property)?
            .0
        {
            Length::Pt(pt) if pt > 0.0 => Some(Pt(pt)),
            Length::Pt(_) => {
                self.error(property, "body font size must be greater than zero");
                None
            }
            Length::Em(_) => {
                self.error(
                    property,
                    "body font size must be an absolute length because em sizes refer to it",
                );
                None
            }
        }
    }

    fn block(&self, name: &str, raw: &BlockStyle, body: Pt) -> Option<Style> {
        let property = |field: &str| format!("styles.{name}.{field}");
        let font = self.font(&raw.font, &property("font"), raw.weight, raw.style);
        let size = self
            .value(&self.tokens.sizes, &raw.size, &property("size"))
            .map(|s| s.0.to_pt(body));
        if size.is_some_and(|s| s.0 <= 0.0) {
            self.error(&property("size"), "font size must be greater than zero");
        }
        let em = size.unwrap_or(body);
        let color = self.color(&raw.color, &property("color"));
        let space_before = self.spacing(&raw.space_before, &property("space-before"), em);
        let space_after = self.spacing(&raw.space_after, &property("space-after"), em);
        let indent = self.spacing(&raw.indent, &property("indent"), em);
        let first_line_indent = self.spacing(&raw.first_line_indent, &property("first-line-indent"), em);
        Some(Style {
            font: font?,
            size: size?,
            color: color?,
            space_before: space_before?,
            space_after: space_after?,
            indent: indent?,
            first_line_indent: first_line_indent?,
            weight: raw.weight,
            style: raw.style,
            line_height: raw.line_height.get(),
            align: raw.align,
            hyphenate: raw.hyphenate,
        })
    }

    fn page(&self, body: Pt) -> Option<PageGeometry> {
        let page = &self.theme.page;
        let (width, height) = match &page.size {
            PageSize::Named(paper) => paper_size(*paper),
            PageSize::Custom(size) => (
                self.spacing(&size.width, "page.size.width", body)?,
                self.spacing(&size.height, "page.size.height", body)?,
            ),
        };
        let margin_inner = self.spacing(&page.margins.inner, "page.margins.inner", body)?;
        let margin_outer = self.spacing(&page.margins.outer, "page.margins.outer", body)?;
        let text_width = Pt(width.0 - margin_inner.0 - margin_outer.0);
        let prose_width = match &page.text_width {
            Some(spec) => self.spacing(spec, "page.text-width", body)?,
            None => text_width,
        };
        let geometry = PageGeometry {
            width,
            height,
            margin_top: self.spacing(&page.margins.top, "page.margins.top", body)?,
            margin_bottom: self.spacing(&page.margins.bottom, "page.margins.bottom", body)?,
            margin_inner,
            margin_outer,
            mirror: page.margins.mirror,
            prose_width,
            wide: page.wide.clone(),
            column_gap: self.spacing(&page.column_gap, "page.column-gap", body)?,
            header_offset: self.spacing(&page.header_offset, "page.header-offset", body)?,
            footer_offset: self.spacing(&page.footer_offset, "page.footer-offset", body)?,
        };
        let text_height = geometry.text_height();
        let width_parts = ["page.size", "page.margins.inner", "page.margins.outer"];
        let prose_parts = [
            "page.size",
            "page.margins.inner",
            "page.margins.outer",
            "page.text-width",
        ];
        let height_parts = ["page.size", "page.margins.top", "page.margins.bottom"];
        if text_width.0 <= 0.0 {
            self.error_among(
                "page.margins",
                &width_parts,
                format!(
                    "inner and outer margins leave no text width on a {:.1}pt wide page",
                    width.0
                ),
            );
        } else if prose_width.0 <= 0.0 || prose_width.0 > text_width.0 {
            self.error_among(
                "page.text-width",
                &prose_parts,
                format!(
                    "text width must be greater than zero and at most the {:.1}pt between the margins",
                    text_width.0
                ),
            );
        } else if geometry.column_gap.0 >= prose_width.0 {
            self.error_among(
                "page.column-gap",
                &prose_parts,
                format!(
                    "column gap {:.1}pt leaves no column width in a {:.1}pt text area",
                    geometry.column_gap.0, prose_width.0
                ),
            );
        }
        if text_height.0 <= 0.0 {
            self.error_among(
                "page.margins",
                &height_parts,
                format!(
                    "top and bottom margins leave no text height on a {:.1}pt tall page",
                    height.0
                ),
            );
        }
        if geometry.header_offset.0 > geometry.margin_top.0 {
            self.error_among(
                "page.header-offset",
                &["page.margins.top"],
                "header offset must fit within the top margin",
            );
        }
        if geometry.footer_offset.0 > geometry.margin_bottom.0 {
            self.error_among(
                "page.footer-offset",
                &["page.margins.bottom"],
                "footer offset must fit within the bottom margin",
            );
        }
        Some(geometry)
    }

    fn fonts(&self) -> BTreeMap<String, FontFiles> {
        let face = |family: &str, face: &str, path: &theme::ResourcePath| {
            self.resource(&format!("/fonts/{}/{face}", escape(family)), path)
        };
        self.theme
            .fonts
            .iter()
            .map(|(name, files)| {
                let files = FontFiles {
                    regular: face(name, "regular", &files.regular),
                    italic: files.italic.as_ref().map(|p| face(name, "italic", p)),
                    bold: files.bold.as_ref().map(|p| face(name, "bold", p)),
                    bold_italic: files.bold_italic.as_ref().map(|p| face(name, "bold-italic", p)),
                };
                (name.clone(), files)
            })
            .collect()
    }

    /// Title slots named `{prefix}.slots`, set across `width`.
    #[allow(clippy::too_many_arguments, reason = "the title block and each title page group")]
    fn title_slots(
        &self,
        prefix: &str,
        slots: &[TitleSlot],
        styles: Option<&theme::Styles<Style>>,
        images: &BTreeMap<String, Resource>,
        page: &PageGeometry,
        width: Pt,
        body: Pt,
    ) -> Option<Vec<resolved::TitleSlot>> {
        let resolved: Vec<_> = slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let property = |field: &str| format!("{prefix}.slots.{index}{field}");
                let em = styles.map_or(body, |s| s.get(slot.style).size);
                let content = match (&slot.text, &slot.image, &slot.width) {
                    (Some(text), None, None) => {
                        self.check_placeholders(
                            text,
                            &property(".text"),
                            "title slots",
                            Placeholder::is_page_dependent,
                        );
                        Some(SlotContent::Text(text.clone()))
                    }
                    (Some(_), None, Some(_)) => {
                        self.error(&property(".width"), "width applies only to image slots");
                        None
                    }
                    (None, Some(name), Some(image_width)) => {
                        let image = images.get(name).cloned();
                        if image.is_none() {
                            self.error(
                                &property(".image"),
                                format!("image \"{name}\" is not defined in images"),
                            );
                        }
                        let image_width = self.spacing(image_width, &property(".width"), em);
                        if image_width.is_some_and(|w| w.0 > width.0) {
                            self.error(&property(".width"), "image width exceeds the width of its slots");
                        }
                        Some(SlotContent::Image {
                            image: image?,
                            width: image_width?,
                        })
                    }
                    (None, Some(_), None) => {
                        self.error(&property(""), "an image slot needs a width");
                        None
                    }
                    _ => {
                        self.error(&property(""), "a title slot needs either text or image");
                        None
                    }
                };
                let space_before = self.spacing(&slot.space_before, &property(".space-before"), em);
                if space_before.is_some_and(|s| s.0 > page.text_height().0) {
                    self.error(&property(".space-before"), "spacing exceeds the text area height");
                }
                Some(resolved::TitleSlot {
                    content: content?,
                    style: slot.style,
                    required: slot.required,
                    space_before: space_before?,
                })
            })
            .collect();
        resolved.into_iter().collect()
    }

    fn check_placeholders(&self, text: &Template, property: &str, context: &str, rejected: fn(&Placeholder) -> bool) {
        for placeholder in text.placeholders().filter(|p| rejected(p)) {
            self.error(property, format!("{{{placeholder}}} is not available in {context}"));
        }
    }

    /// Resolves the placement of slot groups named `{property}.{index}` and their slots with `slots`,
    /// which gets each group's property and width.
    fn groups<S, T>(
        &self,
        property: &str,
        groups: &[theme::Group<S>],
        page: &PageGeometry,
        body: Pt,
        slots: impl Fn(&str, &[S], Pt) -> Option<Vec<T>>,
    ) -> Option<Vec<resolved::Group<T>>> {
        let frame = page.text_width();
        let resolved: Vec<_> = groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let property = format!("{property}.{index}");
                let x = self.spacing(&group.offset.x, &format!("{property}.offset.x"), body);
                let y = self.spacing(&group.offset.y, &format!("{property}.offset.y"), body);
                let width = match &group.width {
                    Some(width) => self.spacing(width, &format!("{property}.width"), body),
                    None => x.map(|x| Pt(frame.0 - x.0)),
                };
                if let (Some(x), Some(width)) = (x, width)
                    && (width.0 <= 0.0 || x.0 + width.0 > frame.0)
                {
                    let message = format!(
                        "the group needs a width greater than zero that fits with its offset in the {:.1}pt margin frame",
                        frame.0
                    );
                    self.error_among(&property, &["page.size", "page.margins"], message);
                }
                let slots = slots(&property, &group.slots, width.unwrap_or(frame));
                Some(resolved::Group {
                    anchor: group.anchor,
                    x: x?,
                    y: y?,
                    width: width?,
                    align: group.align,
                    slots: slots?,
                })
            })
            .collect();
        resolved.into_iter().collect()
    }

    /// A header or footer slot whose style has the size `em`.
    fn band_slot(&self, property: &str, slot: &theme::BandSlot, em: Pt) -> Option<resolved::BandSlot> {
        self.check_placeholders(&slot.text, &format!("{property}.text"), "headers and footers", |p| {
            *p == Placeholder::Abstract
        });
        let space_before = self.spacing(&slot.space_before, &format!("{property}.space-before"), em);
        Some(resolved::BandSlot {
            text: slot.text.clone(),
            style: slot.style,
            required: slot.required,
            space_before: space_before?,
        })
    }

    fn page_variants(
        &self,
        page: &PageGeometry,
        styles: Option<&theme::Styles<Style>>,
        body: Pt,
    ) -> Option<resolved::PageVariants> {
        let pages = &self.theme.pages;
        let variant = |name: &str, variant: &theme::PageVariant| {
            let band = |band: &str, groups: &Option<theme::Band>, own: Option<&Style>| {
                let Some(groups) = groups else { return Some(None) };
                let property = format!("pages.{name}.{band}");
                let slots = |property: &str, slots: &[theme::BandSlot], _| {
                    let resolved: Vec<_> = slots
                        .iter()
                        .enumerate()
                        .map(|(index, slot)| {
                            let style = slot.style.map_or(own, |style| styles.map(|s| s.get(style)));
                            let em = style.map_or(body, |s| s.size);
                            self.band_slot(&format!("{property}.slots.{index}"), slot, em)
                        })
                        .collect();
                    resolved.into_iter().collect()
                };
                self.groups(&property, groups, page, body, slots).map(Some)
            };
            Some(resolved::PageVariant {
                header: band("header", &variant.header, styles.map(|s| &s.header))?,
                footer: band("footer", &variant.footer, styles.map(|s| &s.footer))?,
            })
        };
        let optional = |name: &str, page: &Option<theme::PageVariant>| match page {
            Some(page) => variant(name, page).map(Some),
            None => Some(None),
        };
        let title = optional("title", &pages.title);
        let first = optional("first", &pages.first);
        let odd = optional("odd", &pages.odd);
        let even = optional("even", &pages.even);
        let body = variant("body", &pages.body);
        Some(resolved::PageVariants {
            title: title?,
            first: first?,
            odd: odd?,
            even: even?,
            body: body?,
        })
    }

    fn config(&self, document: &SettingsInput, overrides: &SettingsInput) -> Option<Config> {
        let theme = self.theme;
        let body = self.body_size()?;
        let page = self.page(body);
        let styles = (theme.styles.clone())
            .try_map(|name, raw| Ok::<_, ()>(self.block(name, &raw, body)))
            .and_then(|styles| styles.try_map(|_, style| style.ok_or(())))
            .ok();
        let images: BTreeMap<_, _> = theme
            .images
            .iter()
            .map(|(name, path)| (name.clone(), self.resource(&format!("/images/{}", escape(name)), path)))
            .collect();

        let em_of = |style: Option<&Style>| style.map_or(body, |s| s.size);
        let s = styles.as_ref();
        let list_em = em_of(s.map(|s| &s.list));
        let table_em = em_of(s.map(|s| &s.table_cell));
        let footnote_em = em_of(s.map(|s| &s.footnote));

        let inline = &theme.inline;
        let inline = (|| {
            Some(resolved::InlineStyles {
                code_font: self.font(
                    &inline.code.font,
                    "inline.code.font",
                    Weight::Regular,
                    FontStyle::Normal,
                )?,
                code_size: self.value(&self.tokens.sizes, &inline.code.size, "inline.code.size")?.0,
                code_color: self.color(&inline.code.color, "inline.code.color")?,
                link_color: self.color(&inline.link.color, "inline.link.color")?,
                link_underline: inline.link.underline,
                footnote_marker_size: self
                    .value(
                        &self.tokens.sizes,
                        &inline.footnote_marker.size,
                        "inline.footnote-marker.size",
                    )?
                    .0,
                footnote_marker_raise: self
                    .value(
                        &self.tokens.spacing,
                        &inline.footnote_marker.raise,
                        "inline.footnote-marker.raise",
                    )?
                    .0,
            })
        })();

        if theme.lists.bullets.is_empty() || theme.lists.bullets.iter().any(|b| b.trim().is_empty()) {
            self.error("lists.bullets", "bullets must list at least one non-empty marker");
        }
        let lists = (|| {
            Some(resolved::Lists {
                indent: self.spacing(&theme.lists.indent, "lists.indent", list_em)?,
                item_spacing: self.spacing(&theme.lists.item_spacing, "lists.item-spacing", list_em)?,
                bullets: theme.lists.bullets.clone(),
            })
        })();
        let tables = (|| {
            Some(resolved::Tables {
                cell_padding: self.spacing(&theme.tables.cell_padding, "tables.cell-padding", table_em)?,
                rule_thickness: self.spacing(&theme.tables.rule_thickness, "tables.rule-thickness", table_em)?,
                rule_color: self.color(&theme.tables.rule_color, "tables.rule-color")?,
            })
        })();
        let notes = &theme.footnotes;
        let footnotes = (|| {
            Some(resolved::Footnotes {
                gap: self.spacing(&notes.gap, "footnotes.gap", footnote_em)?,
                separator_width: self.spacing(&notes.separator_width, "footnotes.separator-width", footnote_em)?,
                separator_thickness: self.spacing(
                    &notes.separator_thickness,
                    "footnotes.separator-thickness",
                    footnote_em,
                )?,
                separator_color: self.color(&notes.separator_color, "footnotes.separator-color")?,
                spacing: self.spacing(&notes.spacing, "footnotes.spacing", footnote_em)?,
            })
        })();
        let bibliography_em = em_of(s.map(|s| &s.bibliography));
        let bibliography = (|| {
            Some(resolved::Bibliography {
                hanging_indent: self.spacing(
                    &theme.bibliography.hanging_indent,
                    "bibliography.hanging-indent",
                    bibliography_em,
                )?,
                entry_spacing: self.spacing(
                    &theme.bibliography.entry_spacing,
                    "bibliography.entry-spacing",
                    bibliography_em,
                )?,
            })
        })();
        let toc = self
            .spacing(
                &theme.toc.level_indent,
                "toc.level-indent",
                em_of(s.map(|s| &s.toc_entry)),
            )
            .map(|level_indent| resolved::Toc {
                level_indent,
                leader: theme.toc.leader,
            });

        let page = page?;
        let title_block = self.title_slots(
            "title-block",
            &theme.title_block.slots,
            s,
            &images,
            &page,
            page.prose_width,
            body,
        );
        let title_block_space = self.spacing(&theme.title_block.space_after, "title-block.space-after", body);
        let title_page = self.groups(
            "title-page.groups",
            &theme.title_page.groups,
            &page,
            body,
            |property, slots, width| self.title_slots(property, slots, s, &images, &page, width, body),
        );
        let pages = self.page_variants(&page, s, body);

        let bibliography_file = [overrides, document].into_iter().find_map(|input| {
            let path = input.settings.bibliography.as_ref()?;
            Some(Resource {
                origin: input.source.origin(),
                path: path.clone(),
            })
        });

        Some(Config {
            metadata: document.settings.metadata_overridden_by(&overrides.settings),
            document: theme.document.clone(),
            bibliography_file,
            page,
            fonts: self.fonts(),
            images,
            styles: styles?,
            inline: inline?,
            lists: lists?,
            tables: tables?,
            footnotes: footnotes?,
            caption_separator: theme.captions.separator.clone(),
            bibliography: bibliography?,
            toc: toc?,
            title_block: resolved::TitleBlock {
                slots: title_block?,
                space_after: title_block_space?,
            },
            title_page: title_page?,
            pages: pages?,
            labels: theme.labels.get(theme.document.lang).clone(),
        })
    }
}

fn face_name(weight: Weight, style: FontStyle) -> &'static str {
    match (weight, style) {
        (Weight::Regular, FontStyle::Normal) => "regular",
        (Weight::Regular, FontStyle::Italic) => "italic",
        (Weight::Bold, FontStyle::Normal) => "bold",
        (Weight::Bold, FontStyle::Italic) => "bold-italic",
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Paper dimensions in points, portrait.
fn paper_size(paper: PaperSize) -> (Pt, Pt) {
    let mm = |v: f64| Pt(v * 72.0 / 25.4);
    match paper {
        PaperSize::A4 => (mm(210.0), mm(297.0)),
        PaperSize::A5 => (mm(148.0), mm(210.0)),
        PaperSize::B5 => (mm(176.0), mm(250.0)),
        PaperSize::Letter => (Pt(612.0), Pt(792.0)),
        PaperSize::Legal => (Pt(612.0), Pt(1008.0)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn inputs(theme: Value) -> Inputs {
        Inputs {
            theme: Some(ThemeInput {
                source: Source::Theme("/fake/themes/t.json".into()),
                value: theme,
            }),
            document: SettingsInput {
                source: Source::Document("/fake/doc.md".into()),
                settings: FrontMatter::default(),
            },
            overrides: SettingsInput {
                source: Source::Cli {
                    working_dir: "/fake".into(),
                },
                settings: FrontMatter::default(),
            },
        }
    }

    /// The single diagnostic a theme is expected to produce, as property and message.
    fn rejection(theme: Value) -> (String, String) {
        let errors = resolve(inputs(theme)).expect_err("theme is rejected");
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = &errors[0];
        assert_eq!(error.source, Some(Source::Theme("/fake/themes/t.json".into())));
        (error.property.clone().expect("property named"), error.message.clone())
    }

    #[test]
    fn rejects_a_body_size_in_em() {
        let (property, message) = rejection(json!({"version": 1, "styles": {"body": {"size": "1.2em"}}}));
        assert_eq!(property, "styles.body.size");
        assert!(message.contains("absolute length"), "{message}");
    }

    #[test]
    fn resolves_block_spacing_in_em_against_the_elements_own_size() {
        let theme = json!({"version": 1, "styles": {"quote": {"size": "20pt", "space-before": "0.5em"}}});
        let config = resolve(inputs(theme)).expect("theme resolves");
        assert_eq!(config.styles.quote.size, Pt(20.0));
        assert_eq!(config.styles.quote.space_before, Pt(10.0));
    }

    #[test]
    fn rejects_title_slots_with_both_or_neither_of_text_and_image() {
        for slot in [json!({"text": "{title}", "image": "logo"}), json!({"style": "title"})] {
            let (property, message) = rejection(json!({"version": 1, "title-block": {"slots": [slot]}}));
            assert_eq!(property, "title-block.slots.0");
            assert_eq!(message, "a title slot needs either text or image");
        }
    }

    #[test]
    fn rejects_page_placeholders_in_title_slots() {
        let groups = json!([{"anchor": "top-left", "slots": [{"text": "Page {page} of {pages}"}]}]);
        let errors = resolve(inputs(json!({"version": 1, "title-page": {"groups": groups}}))).unwrap_err();
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "{page} is not available in title slots",
                "{pages} is not available in title slots"
            ]
        );
        assert_eq!(errors[0].property.as_deref(), Some("title-page.groups.0.slots.0.text"));
    }

    #[test]
    fn rejects_a_text_width_or_slot_group_wider_than_the_margin_frame() {
        let (property, message) = rejection(json!({"version": 1, "page": {"text-width": "50cm"}}));
        assert_eq!(property, "page.text-width");
        assert!(message.starts_with("text width must be greater than zero"), "{message}");

        let group = json!({"anchor": "top-right", "offset": {"x": "2cm"}, "width": "15cm", "slots": []});
        let (property, message) = rejection(json!({"version": 1, "title-page": {"groups": [group]}}));
        assert_eq!(property, "title-page.groups.0");
        assert!(message.starts_with("the group needs a width"), "{message}");
    }

    #[test]
    fn rejects_a_bold_style_on_a_family_without_a_bold_face() {
        let theme = json!({
            "version": 1,
            "fonts": {"Plain": {"regular": "plain.otf"}},
            "styles": {"body": {"font": "Plain", "weight": "bold"}},
        });
        let (property, message) = rejection(theme);
        assert_eq!(property, "styles.body.font");
        assert_eq!(message, "font family \"Plain\" has no bold face");
    }
}
