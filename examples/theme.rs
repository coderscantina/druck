//! A custom JSON theme and caller overrides. Layers apply in order: bundled defaults, the theme, the document's
//! own settings, then the overrides, so the override wins. The resolved configuration is checked before any layout.
//!
//! Run it with `cargo run --example theme`.

mod common;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeSelection};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{Failure, PdfOptions, Renderer};

const THEME: &str = r##"{
  "version": 1,
  "tokens": { "colors": { "accent": "#7a2a2a" } },
  "styles": { "body": { "size": "10pt" }, "heading-1": { "color": "$colors.accent" } }
}"##;

const MARKDOWN: &str = "---\ntitle: Themed\nfont-size: 11pt\n---\n\n# A heading\n\nBody text.\n";

fn main() -> common::Outcome {
    common::on_big_stack(|| -> common::Outcome {
        let origin = Origin::Document("/memory".into());
        let input = Input {
            text: DocumentText::Single {
                text: MARKDOWN.to_owned(),
                origin: origin.clone(),
                name: "themed.md".to_owned(),
            },
            // A theme in memory: its assets, if it had any, would be relative to `origin`.
            theme: ThemeSelection::Json {
                text: THEME.to_owned(),
                origin,
            },
            // The same syntax as `druck render --set`.
            overrides: Overrides::parse(&["font-size=12pt".to_owned()], "/memory".as_ref()).map_err(Failure::from)?,
            today: clock_date()?,
        };
        let renderer = Renderer::new();
        let resolver = MemoryResolver::new();
        let cancel = Cancel::default();

        let configured = renderer.job(&resolver, &input, &cancel).configure()?;
        let config: serde_json::Value = serde_json::from_str(&configured.config_json())?;
        // The theme says 10pt and the document 11pt, the override 12pt.
        println!("body size: {}", config["styles"]["body"]["size"]);
        assert_eq!(config["styles"]["body"]["size"], 12.0);

        let pdf = configured
            .layout(&Default::default())?
            .pdf(&PdfOptions::default(), &cancel)?;
        assert!(pdf.bytes.starts_with(b"%PDF-"));
        Ok(())
    })
}
