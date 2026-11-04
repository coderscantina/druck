//! Layout inspection, then PDF export. The layout is a display list that owns its fonts and images: it is read
//! here after the renderer and the input are gone, and exporting it shapes and paginates nothing.
//!
//! Run it with `cargo run --example inspect`.

mod common;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::output::{Item, Pt};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{LayoutOptions, PdfOptions, Renderer};

const MARKDOWN: &str = "# Inspect\n\nThe second paragraph is the one a click lands in.\n";

fn main() -> common::Outcome {
    common::on_big_stack(|| -> common::Outcome {
        let layout = {
            let renderer = Renderer::new();
            let resolver = MemoryResolver::new();
            let input = Input {
                text: DocumentText::Single {
                    text: MARKDOWN.to_owned(),
                    origin: Origin::Document("/memory".into()),
                    name: "inspect.md".to_owned(),
                },
                theme: ThemeSelection::Document {
                    dirs: ThemeDirs::default(),
                },
                overrides: Overrides::default(),
                today: clock_date()?,
            };
            let cancel = Cancel::default();
            // The renderer, resolver, and input end here. The layout does not need them.
            renderer
                .job(&resolver, &input, &cancel)
                .configure()?
                .layout(&LayoutOptions { source_map: true })?
        };

        // Points, origin at the top left of the page, y growing down. Items draw in order.
        let page = &layout.pages()[0];
        println!("page 1 is {} x {} pt", page.width.0, page.height.0);
        let texts = page.items.iter().filter_map(|item| match item {
            Item::Text { x, y, run, .. } => Some((x, y, run)),
            _ => None,
        });
        for (x, y, run) in texts.take(3) {
            let face = layout.face(run.face);
            println!(
                "{:?} at ({:.1}, {:.1}): {} glyphs of a {} byte font",
                run.text,
                x.0,
                y.0,
                run.glyphs.len(),
                face.data().len()
            );
        }

        // A click on the page finds the written text, and the written text finds its place on the page.
        let position = layout.position_at(0, Pt(100.0), Pt(150.0));
        println!("a click at 100, 150 is at {position:?}");
        if let Some(position) = position {
            let located = layout.locate(position).expect("a written place is on a page");
            println!("which is at {:?} on page {}", located.rect, located.page + 1);
        }

        let pdf = layout.pdf(&PdfOptions::default(), &Cancel::default())?;
        assert!(pdf.bytes.starts_with(b"%PDF-"));
        println!("exported {} pages", pdf.parts);
        Ok(())
    })
}
