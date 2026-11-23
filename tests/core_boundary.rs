//! The library core reads no files, reads no environment, never exits, and never prints. Files come through a
//! resolver (`src/resource.rs`), and the binary (`src/main.rs`) is the only place that talks to the terminal.
//! Test code, test-only modules, and comments are exempt, as in the rule of `docs/ACCEPTANCE.md`.

use std::fs;
use std::path::{Path, PathBuf};

const SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
/// Files allowed to touch the filesystem or the terminal.
const BOUNDARY: [&str; 2] = ["main.rs", "resource.rs"];
const FORBIDDEN: [&str; 4] = ["std::fs", "std::env", "process::exit", "println!"];

#[test]
fn the_core_neither_touches_the_filesystem_nor_prints() {
    let mut files = Vec::new();
    rust_files(Path::new(SRC), &mut files);
    let test_only: Vec<PathBuf> = files.iter().flat_map(|file| test_modules(file)).collect();

    let mut found = Vec::new();
    for file in &files {
        let relative = file.strip_prefix(SRC).expect("below src");
        if BOUNDARY.iter().any(|name| relative == Path::new(name)) || test_only.contains(file) {
            continue;
        }
        let text = fs::read_to_string(file).expect("source file");
        for (number, line) in production_lines(&text) {
            if let Some(word) = FORBIDDEN.iter().find(|word| line.contains(*word)) {
                found.push(format!("src/{}:{number}: {word}", relative.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the core must go through the resolver and must not print:\n{}",
        found.join("\n")
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source folder") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The files of the modules `file` declares under `#[cfg(test)]` as `mod name;`.
fn test_modules(file: &Path) -> Vec<PathBuf> {
    let text = fs::read_to_string(file).expect("source file");
    let stem = file.file_stem().and_then(|stem| stem.to_str()).expect("file name");
    let parent = file.parent().expect("parent");
    let dir = if matches!(stem, "lib" | "main" | "mod") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    };
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines
        .windows(2)
        .filter(|pair| pair[0] == "#[cfg(test)]")
        .filter_map(|pair| pair[1].split_once("mod ")?.1.strip_suffix(';'))
        .map(|name| dir.join(format!("{name}.rs")))
        .collect()
}

/// Numbered lines outside comments and inline `#[cfg(test)]` modules. rustfmt, which CI enforces, closes a module
/// with a `}` at the indentation of its `mod` line.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut tests_end: Option<String> = None;
    let mut previous = "";
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(end) = &tests_end {
            if line == end {
                tests_end = None;
            }
        } else if previous == "#[cfg(test)]" && trimmed.starts_with("mod ") && trimmed.ends_with('{') {
            tests_end = Some(format!("{}}}", &line[..line.len() - line.trim_start().len()]));
        } else if !trimmed.starts_with("//") {
            lines.push((index + 1, line));
        }
        previous = trimmed;
    }
    lines
}
