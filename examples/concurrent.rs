//! One renderer for concurrent, independent jobs. The renderer and the resolver are shared by reference, and each
//! job has its own document, theme, and token. A job keeps its configuration to itself, so what each thread
//! writes is what it would write alone.
//!
//! Run it with `cargo run --example concurrent`.

mod common;

use druck::cancel::Cancel;
use druck::input::{DocumentText, Input, Overrides, ThemeSelection};
use druck::resource::{MemoryResolver, Origin, clock_date};
use druck::{PdfOptions, Renderer};

fn input(title: &str, body_size: u32) -> Result<Input, String> {
    let origin = Origin::Document("/memory".into());
    Ok(Input {
        text: DocumentText::Single {
            text: format!("# {title}\n\nThe text of {title}.\n"),
            origin: origin.clone(),
            name: format!("{title}.md"),
        },
        theme: ThemeSelection::Json {
            text: format!(r#"{{"version": 1, "styles": {{"body": {{"size": "{body_size}pt"}}}}}}"#),
            origin,
        },
        overrides: Overrides::default(),
        today: clock_date()?,
    })
}

fn main() -> common::Outcome {
    let renderer = Renderer::new();
    let resolver = MemoryResolver::new();
    let inputs = [("One", 9), ("Two", 10), ("Three", 11), ("Four", 12)]
        .into_iter()
        .map(|(title, size)| input(title, size))
        .collect::<Result<Vec<_>, _>>()?;

    let render = |input: &Input| {
        common::on_big_stack(|| {
            renderer
                .render(&resolver, input, &PdfOptions::default(), &Cancel::default())
                .map(|rendered| rendered.bytes)
        })
    };
    let together = std::thread::scope(|scope| {
        let jobs: Vec<_> = inputs.iter().map(|input| scope.spawn(|| render(input))).collect();
        jobs.into_iter()
            .map(|job| job.join().expect("a job does not panic"))
            .collect::<Result<Vec<_>, _>>()
    })?;

    for (input, bytes) in inputs.iter().zip(&together) {
        assert_eq!(bytes, &render(input)?, "a job gives what it gives alone");
    }
    println!("{} jobs on one renderer, each equal to its single run", together.len());
    Ok(())
}
