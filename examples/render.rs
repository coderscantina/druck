//! Simple rendering: Markdown in, PDF bytes out, with the bundled theme and fonts. No file is read, so the
//! resolver has nothing to serve.
//!
//! Run it with `cargo run --example render`.

mod common;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{PdfOptions, Renderer};

const MARKDOWN: &str =
    "---\ntitle: Hello\nauthor: Ada Example\n---\n\n# Hello\n\nA paragraph with a note.[^1]\n\n[^1]: Set by Druck.\n";

fn main() -> common::Outcome {
    common::on_big_stack(|| -> common::Outcome {
        let input = Input {
            text: DocumentText::Single {
                text: MARKDOWN.to_owned(),
                origin: Origin::Document("/memory".into()),
                name: "hello.md".to_owned(),
            },
            theme: ThemeSelection::Document {
                dirs: ThemeDirs::default(),
            },
            overrides: Overrides::default(),
            today: clock_date()?,
        };
        let rendered = Renderer::new().render(
            &MemoryResolver::new(),
            &input,
            &PdfOptions::default(),
            &Cancel::default(),
        )?;
        assert!(rendered.bytes.starts_with(b"%PDF-"));
        println!("{} pages, {} bytes", rendered.parts, rendered.bytes.len());
        for warning in &rendered.warnings {
            println!("{warning}");
        }
        Ok(())
    })
}
