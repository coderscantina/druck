//! Cooperative cancellation. A token stops a job from any thread, between pieces of work. A cancelled job returns
//! `Failure::Cancelled`, never partial output, and the renderer is as good as it was.
//!
//! Run it with `cargo run --example cancel`.

mod common;

use std::time::Duration;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{Failure, PdfOptions, Renderer};

fn input(text: String) -> Result<Input, String> {
    Ok(Input {
        text: DocumentText::Single {
            text,
            origin: Origin::Document("/memory".into()),
            name: "long.md".to_owned(),
        },
        theme: ThemeSelection::Document {
            dirs: ThemeDirs::default(),
        },
        overrides: Overrides::default(),
        today: clock_date()?,
    })
}

fn main() -> common::Outcome {
    common::on_big_stack(|| -> common::Outcome {
        let renderer = Renderer::new();
        let resolver = MemoryResolver::new();

        // A book of a few hundred pages, which takes far longer to lay out than the wait below.
        let paragraph = "A paragraph long enough to break over several lines, with words to hyphenate. ".repeat(12);
        let long = input(format!("# Long\n\n{}", format!("{paragraph}\n\n").repeat(1500)))?;

        let cancel = Cancel::default();
        let result = std::thread::scope(|scope| {
            let token = cancel.clone();
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                token.cancel();
            });
            renderer.render(&resolver, &long, &PdfOptions::default(), &cancel)
        });
        assert!(matches!(result, Err(Failure::Cancelled)), "stopped while laying out");
        println!("cancelled: no bytes, no partial layout");

        // Each job needs a token of its own, since cancelling is permanent. The renderer is fine.
        let short = input("# Short\n\nThis one finishes.\n".to_owned())?;
        let rendered = renderer.render(&resolver, &short, &PdfOptions::default(), &Cancel::default())?;
        println!("the same renderer then wrote {} pages", rendered.parts);
        Ok(())
    })
}
