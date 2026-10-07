//! End-to-end checks of `kyber check` and `kyber render`.
//!
//! Every run starts in a working directory that is neither the fixture nor the document
//! directory, so relative paths and origins are resolved for real.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// A scratch directory holding generated documents and themes, with an empty `cwd` inside.
struct Sandbox {
    root: PathBuf,
    cwd: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("kyber-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cwd")).expect("create sandbox");
        let root = root.canonicalize().expect("canonical sandbox path");
        Self {
            cwd: root.join("cwd"),
            root,
        }
    }

    /// Writes a file below the sandbox root and returns its absolute path.
    fn write(&self, name: &str, content: &str) -> String {
        let path = self.root.join(name);
        fs::write(&path, content).expect("write sandbox file");
        path.to_str().expect("utf-8 path").to_owned()
    }

    fn theme(&self, name: &str, theme: Value) -> String {
        self.write(name, &theme.to_string())
    }

    fn run(&self, args: &[&str]) -> Run {
        run_in(&self.cwd, args)
    }

    /// The resolved configuration of a document that must be valid.
    fn config(&self, args: &[&str]) -> Value {
        let run = self.run(&[&["check"], args, &["--print-config"]].concat());
        assert_eq!(run.code, 0, "{}", run.stderr);
        serde_json::from_str(&run.stdout).expect("print-config emits JSON")
    }

    /// The diagnostics of a document that must be rejected.
    fn rejection(&self, args: &[&str]) -> String {
        let run = self.run(&[&["check"], args].concat());
        assert_eq!(run.code, 1, "expected a diagnostic, got stdout: {}", run.stdout);
        run.stderr
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_in(cwd: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_kyber"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run kyber");
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
        stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
    }
}

fn assert_pt(config: &Value, pointer: &str, expected: f64) {
    let actual = config.pointer(pointer).and_then(Value::as_f64).expect(pointer);
    assert!((actual - expected).abs() < 1e-6, "{pointer}: {actual} != {expected}");
}

fn mm(value: f64) -> f64 {
    value * 72.0 / 25.4
}

#[test]
fn loads_a_document_with_a_partial_theme_and_tracks_resource_origins() {
    let sandbox = Sandbox::new("valid");
    let document = format!("{FIXTURES}/doc.md");
    let config = sandbox.config(&[&document]);

    let theme_dir = format!("{FIXTURES}/themes");
    assert_eq!(
        config["fonts"]["Fixture Serif"]["regular"],
        json!({"origin": "theme", "dir": theme_dir, "path": "fonts/dummy.otf"})
    );
    assert_eq!(
        config["bibliography-file"],
        json!({"origin": "document", "dir": FIXTURES, "path": "refs.bib"})
    );
    assert_eq!(config["fonts"]["Libertinus Mono"]["regular"]["origin"], "bundled");
    assert_eq!(config["styles"]["body"]["font"], "Fixture Serif");
}

#[test]
fn later_layers_override_earlier_ones_and_the_theme_flag_beats_front_matter() {
    let sandbox = Sandbox::new("precedence");
    let theme = sandbox.theme(
        "theme.json",
        json!({"version": 1, "page": {"margins": {"top": "24mm"}}}),
    );
    let other = sandbox.theme(
        "other.json",
        json!({"version": 1, "page": {"margins": {"top": "13mm"}}}),
    );
    let top = "/page/margin-top";

    let plain = sandbox.write("plain.md", "# Plain\n");
    assert_pt(&sandbox.config(&[&plain]), top, mm(27.0));

    let themed = sandbox.write("themed.md", &format!("---\ntheme: {theme}\n---\n"));
    assert_pt(&sandbox.config(&[&themed]), top, mm(24.0));

    let front = sandbox.write(
        "front.md",
        &format!("---\ntheme: {theme}\nmargins:\n  top: 18mm\n---\n"),
    );
    assert_pt(&sandbox.config(&[&front]), top, mm(18.0));
    assert_pt(&sandbox.config(&[&front, "--set", "margins.top=14mm"]), top, mm(14.0));

    assert_pt(&sandbox.config(&[&themed, "--theme", &other]), top, mm(13.0));
}

#[test]
fn objects_merge_by_field_arrays_replace_and_permitted_nulls_are_kept() {
    let sandbox = Sandbox::new("merge");
    let theme = sandbox.theme(
        "theme.json",
        json!({
            "version": 1,
            "styles": {"heading-1": {"size": "2em"}},
            "lists": {"bullets": ["*"]},
            "pages": {"first": null},
        }),
    );
    let document = sandbox.write("doc.md", "# Doc\n");
    let config = sandbox.config(&[&document, "--theme", &theme]);

    assert_pt(&config, "/styles/heading-1/size", 21.0);
    assert_eq!(config["styles"]["heading-1"]["weight"], "bold");
    assert_eq!(config["lists"]["bullets"], json!(["*"]));
    assert_eq!(config["pages"]["first"], Value::Null);
}

#[test]
fn rejects_null_where_the_type_does_not_allow_it() {
    let sandbox = Sandbox::new("null");
    let theme = sandbox.theme("theme.json", json!({"version": 1, "styles": {"body": {"size": null}}}));
    let document = sandbox.write("doc.md", "# Doc\n");
    let stderr = sandbox.rejection(&[&document, "--theme", &theme]);

    assert!(stderr.contains(&format!("{theme}: styles.body.size:")), "{stderr}");
}

#[test]
fn rejects_undefined_and_cyclic_token_references() {
    let sandbox = Sandbox::new("tokens");
    let document = sandbox.write("doc.md", "# Doc\n");

    let undefined = sandbox.theme(
        "undefined.json",
        json!({"version": 1, "tokens": {"colors": {"x": "$colors.nope"}}}),
    );
    let stderr = sandbox.rejection(&[&document, "--theme", &undefined]);
    assert!(
        stderr.contains("tokens.colors.x: undefined token $colors.nope"),
        "{stderr}"
    );

    let cyclic = sandbox.theme(
        "cyclic.json",
        json!({"version": 1, "tokens": {"colors": {"a": "$colors.b", "b": "$colors.a"}}}),
    );
    let stderr = sandbox.rejection(&[&document, "--theme", &cyclic]);
    assert!(stderr.contains("tokens.colors.a: cyclic token reference"), "{stderr}");
}

#[test]
fn resolves_em_against_the_body_size_and_rejects_other_units() {
    let sandbox = Sandbox::new("units");
    let document = sandbox.write("doc.md", "# Doc\n");
    let margin = |value: &str| {
        let theme = json!({"version": 1, "page": {"margins": {"top": value}}});
        sandbox.theme("theme.json", theme)
    };

    let theme = margin("4em");
    assert_pt(
        &sandbox.config(&[&document, "--theme", &theme]),
        "/page/margin-top",
        42.0,
    );

    for value in ["12px", "50%"] {
        let theme = margin(value);
        let stderr = sandbox.rejection(&[&document, "--theme", &theme]);
        assert!(stderr.contains("page.margins.top: length"), "{stderr}");
    }
}

#[test]
fn rejects_wrong_or_missing_versions_and_unknown_fields() {
    let sandbox = Sandbox::new("fields");
    let document = sandbox.write("doc.md", "# Doc\n");
    let cases = [
        (json!({"version": 2}), "version: unsupported theme version 2"),
        (json!({}), "version: missing theme version"),
        (json!({"version": 1, "bogus": 1}), "bogus: unknown field `bogus`"),
    ];
    for (theme, expected) in cases {
        let theme = sandbox.theme("theme.json", theme);
        let stderr = sandbox.rejection(&[&document, "--theme", &theme]);
        assert!(stderr.contains(&format!("{theme}: {expected}")), "{stderr}");
    }

    let front = sandbox.write("front.md", "---\ncolour: red\n---\n");
    let stderr = sandbox.rejection(&[&front]);
    assert!(
        stderr.contains(&format!("{front}:2:1: unknown field `colour`")),
        "{stderr}"
    );
}

#[test]
fn reports_yaml_errors_at_the_document_line() {
    let sandbox = Sandbox::new("yaml");
    let document = sandbox.write("doc.md", "---\ntitle: ok\ntoc: [unclosed\n---\n# Doc\n");
    let stderr = sandbox.rejection(&[&document]);

    assert!(stderr.starts_with(&format!("error: {document}:3:")), "{stderr}");
}

#[test]
fn reports_a_missing_theme_font_relative_to_the_theme() {
    let sandbox = Sandbox::new("missing-font");
    let theme = sandbox.theme(
        "theme.json",
        json!({"version": 1, "fonts": {"Ghost": {"regular": "fonts/ghost.otf"}}}),
    );
    let document = sandbox.write("doc.md", "# Doc\n");
    let stderr = sandbox.rejection(&[&document, "--theme", &theme]);

    let expected = format!(
        "fonts.Ghost.regular: resource not found: \"fonts/ghost.otf\" relative to the theme in {}",
        sandbox.root.display()
    );
    assert!(stderr.contains(&expected), "{stderr}");
}

const REPO: &str = env!("CARGO_MANIFEST_DIR");

/// Renders a document that must succeed and returns the PDF bytes.
fn render(sandbox: &Sandbox, args: &[&str], output: &Path) -> Vec<u8> {
    let run = sandbox.run(&[&["render"], args].concat());
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("wrote"), "{}", run.stdout);
    let pdf = fs::read(output).expect("PDF written");
    assert!(pdf.starts_with(b"%PDF-"));
    pdf
}

#[test]
fn renders_the_samples() {
    let sandbox = Sandbox::new("samples");
    for name in ["en", "de", "pagination", "columns", "images", "tables", "offer"] {
        let output = sandbox.root.join(format!("{name}.pdf"));
        let document = format!("{REPO}/samples/{name}.md");
        let pdf = render(&sandbox, &[&document, "-o", output.to_str().unwrap()], &output);
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(pdf.contains("/FontFile3"), "{name}: fonts are embedded");
        if name == "en" || name == "de" {
            assert!(pdf.contains("/URI"), "{name}: the link is clickable");
        }
    }
}

#[test]
fn renders_the_report_with_navigation_in_both_themes_the_same_every_time() {
    let sandbox = Sandbox::new("report");
    let document = format!("{REPO}/samples/report.md");
    let theme = format!("{REPO}/samples/themes/report.json");
    let output = sandbox.root.join("report.pdf");
    let default = render(&sandbox, &[&document, "-o", "../report.pdf"], &output);
    assert!(
        default == render(&sandbox, &[&document, "-o", "../report.pdf"], &output),
        "renders are identical"
    );
    let custom = render(
        &sandbox,
        &[&document, "--theme", &theme, "-o", "../report.pdf"],
        &output,
    );
    for pdf in [default, custom] {
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(pdf.contains("/Type/Outlines"), "headings are bookmarked");
        assert!(pdf.contains("/Title(5.1 Page numbers that settle)"));
        let internal = pdf
            .split("<<")
            .filter(|d| d.contains("/Subtype/Link") && d.contains("/Dest"))
            .count();
        assert!(
            internal > 30,
            "contents entries and references link inside the document: {internal}"
        );
    }
}

#[test]
fn check_reports_a_required_title_slot_without_a_value() {
    let sandbox = Sandbox::new("required-slot");
    let document = sandbox.write("doc.md", "---\ntitle-page: true\nauthor: Ada\n---\nText.\n");
    let stderr = sandbox.rejection(&[&document]);
    assert!(
        stderr.contains(&format!(
            "{document}: title-page.groups.0.slots.0.text: this required slot needs {{title}}; set title in the front matter"
        )),
        "{stderr}"
    );
}

#[test]
fn writes_next_to_the_document_by_default() {
    let sandbox = Sandbox::new("default-output");
    let document = sandbox.write("doc.md", "# Doc\n\nText.\n");
    render(&sandbox, &[&document], &sandbox.root.join("doc.pdf"));
    assert_eq!(fs::read_dir(&sandbox.cwd).unwrap().count(), 0);
}

#[test]
fn uses_font_files_relative_to_the_document() {
    let sandbox = Sandbox::new("font-files");
    fs::create_dir(sandbox.root.join("fonts")).unwrap();
    fs::copy(
        format!("{REPO}/fonts/LibertinusMono-Regular.otf"),
        sandbox.root.join("fonts/Body.otf"),
    )
    .unwrap();
    let front_matter = "---\nfont-files:\n  Doc Mono:\n    regular: fonts/Body.otf\nfonts:\n  mono: Doc Mono\n---\n";
    let document = sandbox.write("doc.md", &format!("{front_matter}Code in the document's own `font`.\n"));
    render(&sandbox, &[&document], &sandbox.root.join("doc.pdf"));

    sandbox.write("fonts/Body.otf", "not a font");
    let run = sandbox.run(&["render", &document]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr
            .contains("fonts.Doc Mono.regular: \"fonts/Body.otf\" relative to the document"),
        "{}",
        run.stderr
    );
}

#[test]
fn reports_unsupported_content_with_its_location_and_writes_nothing() {
    let sandbox = Sandbox::new("unsupported");
    let document = sandbox.write("doc.md", "---\ntitle: T\n---\n\nText.\n\n~~gone~~\n\n---\n");
    let run = sandbox.run(&["render", &document]);

    assert_eq!(run.code, 1);
    assert!(
        run.stderr
            .contains(&format!("{document}:7:1: strikethrough is not supported")),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains(&format!("{document}:9:1: thematic breaks are not supported")),
        "{}",
        run.stderr
    );
    assert!(!sandbox.root.join("doc.pdf").exists());
}

/// A sandbox whose `images` directory holds the image fixtures.
fn image_sandbox(name: &str) -> Sandbox {
    let sandbox = Sandbox::new(name);
    fs::create_dir(sandbox.root.join("images")).unwrap();
    for entry in fs::read_dir(format!("{FIXTURES}/images")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, sandbox.root.join("images").join(path.file_name().unwrap())).unwrap();
    }
    sandbox
}

#[test]
fn renders_images_relative_to_the_document_the_same_every_time() {
    let sandbox = image_sandbox("images");
    let body = "![A *pixel*](images/pixel.png)\n\n![](images/photo.jpg)\n\n![A drawing](images/drawing.svg)\n";
    let document = sandbox.write("doc.md", body);
    let output = sandbox.root.join("doc.pdf");
    let first = render(&sandbox, &[&document], &output);
    let second = render(&sandbox, &[&document], &output);

    assert_eq!(
        String::from_utf8_lossy(&first).matches("/Subtype/Image").count(),
        2,
        "PNG and JPEG"
    );
    assert!(first == second, "renders are identical");
}

#[test]
fn reports_image_problems_at_their_reference_with_the_resource() {
    let sandbox = image_sandbox("image-errors");
    let body = "Text.\n\n![a](images/missing.png)\n\n![b](images/truncated.png)\n\n![c](images/still.gif)\n\n\
        ![d](images/jpeg-data.png)\n\n![e](https://example.com/e.png)\n";
    let document = sandbox.write("doc.md", body);
    let run = sandbox.run(&["render", &document]);
    let dir = sandbox.root.display();
    let expected = [
        format!("{document}:3:1: image not found: \"images/missing.png\" relative to the document in {dir}"),
        format!(
            "{document}:5:1: image \"images/truncated.png\" relative to the document in {dir}: malformed PNG image"
        ),
        format!(
            "{document}:7:1: image \"images/still.gif\" relative to the document in {dir}: GIF images are not supported"
        ),
        format!(
            "{document}:9:1: image \"images/jpeg-data.png\" relative to the document in {dir}: \
            the file contains JPEG data but its name ends in .png"
        ),
    ];

    assert_eq!(run.code, 1);
    // Remote references are reported by the parser, before any file is read.
    assert!(
        run.stderr.contains(&format!(
            "{document}:11:1: remote images are not fetched; use a local file"
        )),
        "{}",
        run.stderr
    );
    assert!(!sandbox.root.join("doc.pdf").exists());

    let local = sandbox.write("local.md", &body.replace("![e](https://example.com/e.png)\n", ""));
    let run = sandbox.run(&["render", &local]);
    let expected = expected.map(|line| line.replace(&document, &local));
    for line in &expected {
        assert!(run.stderr.contains(line.as_str()), "{line}\n{}", run.stderr);
    }
}

#[test]
fn rejects_code_lines_wider_than_the_text_area() {
    let sandbox = Sandbox::new("wide-code");
    let long = "x".repeat(200);
    let document = sandbox.write("doc.md", &format!("Text.\n\n```\nshort\n{long}\n```\n"));
    let run = sandbox.run(&["render", &document]);

    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains(&format!("{document}:5:1: code line is")),
        "{}",
        run.stderr
    );
    assert!(!sandbox.root.join("doc.pdf").exists());
}

#[test]
fn set_paths_resolve_against_the_working_directory() {
    let sandbox = Sandbox::new("set-path");
    let document = sandbox.write("doc.md", "---\nbibliography: refs.bib\n---\n");
    sandbox.write("refs.bib", "");
    fs::write(sandbox.cwd.join("refs.bib"), "").unwrap();
    let config = sandbox.config(&[&document, "--set", "bibliography=refs.bib"]);

    assert_eq!(
        config["bibliography-file"],
        json!({"origin": "working-dir", "dir": sandbox.cwd, "path": "refs.bib"})
    );
}

#[test]
fn renders_the_reports_with_citations_in_both_styles_the_same_every_time() {
    let sandbox = Sandbox::new("citations");
    let output = sandbox.root.join("report.pdf");
    for name in ["report", "report-de"] {
        let document = format!("{REPO}/samples/{name}.md");
        for style in ["author-date", "numeric"] {
            let args = [
                &document,
                "--set",
                &format!("citation-style={style}"),
                "-o",
                "../report.pdf",
            ];
            let pdf = render(&sandbox, &args, &output);
            if name == "report-de" && style == "numeric" {
                assert!(pdf == render(&sandbox, &args, &output), "renders are identical");
            }
            let pdf = String::from_utf8_lossy(&pdf);
            let heading = if name == "report" { "References" } else { "Literatur" };
            assert!(
                pdf.contains(&format!("/Title({heading})")),
                "{name}: the bibliography is bookmarked"
            );
        }
    }
}

/// A sandbox with `doc.md` holding `body` after front matter that names `refs.bib`, which holds `bib`.
fn citation_sandbox(name: &str, body: &str, bib: &str) -> (Sandbox, String, String) {
    let sandbox = Sandbox::new(name);
    let document = sandbox.write("doc.md", &format!("---\nbibliography: refs.bib\n---\n{body}"));
    let bib = sandbox.write("refs.bib", bib);
    (sandbox, document, bib)
}

#[test]
fn reports_a_missing_bibliography_file_by_property() {
    let sandbox = Sandbox::new("missing-bib");
    let document = sandbox.write("doc.md", "---\nbibliography: refs.bib\n---\nText.\n");
    let stderr = sandbox.rejection(&[&document]);
    let dir = sandbox.root.display();
    assert!(
        stderr.contains(&format!(
            "bibliography: resource not found: \"refs.bib\" relative to the document in {dir}"
        )),
        "{stderr}"
    );
}

#[test]
fn reports_bibliography_errors_at_their_line_in_the_bib_file() {
    let bib = "@book{good, author = {A}, title = {T}, publisher = {P}, year = {2020}}\n\n@book{bad, title = {T}}\n";
    let (sandbox, document, bib) = citation_sandbox("bib-errors", "Text [@good].\n", bib);
    let run = sandbox.run(&["render", &document]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains(&format!(
            "{bib}:3:1: entry `bad` is missing required fields: `author or editor`, `publisher`, `year`"
        )),
        "{}",
        run.stderr
    );
}

#[test]
fn reports_citations_without_a_bibliography_or_entry_at_their_location() {
    let sandbox = Sandbox::new("no-bib");
    let document = sandbox.write("doc.md", "# Title\n\nText [@a].\n");
    let run = sandbox.run(&["render", &document]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains(&format!(
            "{document}:3:6: a citation needs a bibliography; set `bibliography` to a BibTeX file in the front matter"
        )),
        "{}",
        run.stderr
    );

    let bib = "@misc{web, title = {T}, url = {https://example.org}}\n";
    let (sandbox, document, bib) = citation_sandbox("missing-key", "Per @web and\n[@web; @nobody].\n", bib);
    let run = sandbox.run(&["render", &document]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr
            .contains(&format!("{document}:5:8: no entry in {bib} has the key `nobody`")),
        "{}",
        run.stderr
    );
    assert!(!sandbox.root.join("doc.pdf").exists());
}
