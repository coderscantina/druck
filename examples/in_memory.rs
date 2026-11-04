//! Fully in-memory rendering with an explicit resolver. Each resource is stored under the origin its reference is
//! relative to: the document's image and bibliography under the document's folder, the theme's assets under the
//! theme's. Nothing touches the disk, and a resource that is not stored fails with its name and origin.
//!
//! Run it with `cargo run --example in_memory`.

mod common;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{Failure, PdfOptions, Renderer};

const MARKDOWN: &str = "---\ntitle: In memory\nbibliography: refs.bib\n---\n\n# In memory\n\nCited once [@ada].\n\n![A square](square.svg)\n";
const BIB: &str = "@book{ada, author = {Lovelace, Ada}, title = {Notes}, year = {1843}, publisher = {Taylor}}\n";
const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="60"><rect width="60" height="60" fill="#7a2a2a"/></svg>"##;

fn main() -> common::Outcome {
    common::on_big_stack(|| -> common::Outcome {
        let folder = Origin::Document("/book".into());
        let mut resolver = MemoryResolver::new();
        resolver.insert(folder.clone(), "refs.bib", BIB.as_bytes());
        resolver.insert(folder.clone(), "square.svg", SQUARE.as_bytes());

        let input = Input {
            text: DocumentText::Single {
                text: MARKDOWN.to_owned(),
                origin: folder,
                name: "memory.md".to_owned(),
            },
            theme: ThemeSelection::Document {
                dirs: ThemeDirs::default(),
            },
            overrides: Overrides::default(),
            today: clock_date()?,
        };
        let renderer = Renderer::new();
        let rendered = renderer.render(&resolver, &input, &PdfOptions::default(), &Cancel::default())?;
        assert!(rendered.bytes.starts_with(b"%PDF-"));
        println!("{} pages from memory", rendered.parts);

        // A resource that is not stored fails with its name and origin, and nothing is laid out.
        let Failure::Errors(errors) = renderer
            .render(
                &MemoryResolver::new(),
                &input,
                &PdfOptions::default(),
                &Cancel::default(),
            )
            .expect_err("nothing is stored")
        else {
            unreachable!("nothing cancels this job")
        };
        for error in &errors {
            println!("expected: {error}");
        }
        assert!(errors.iter().any(|error| error.resource.is_some()));
        Ok(())
    })
}
