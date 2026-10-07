//! Sets `DRUCK_VERSION` to the package version and the commit's short hash,
//! as in `26.10.7-1a2b3c4`, which matches the release tag without its `v`.
//! Builds outside a git checkout get the package version alone.

use std::process::Command;

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let full = match git(&["rev-parse", "--short=7", "HEAD"]) {
        Some(hash) => format!("{version}-{hash}"),
        None => version,
    };
    println!("cargo:rustc-env=DRUCK_VERSION={full}");

    // Rebuild when HEAD moves: a checkout changes HEAD, a commit changes the branch ref.
    for path in ["HEAD", "packed-refs"]
        .into_iter()
        .map(str::to_owned)
        .chain(git(&["symbolic-ref", "-q", "HEAD"]))
        .filter_map(|name| git(&["rev-parse", "--git-path", &name]))
    {
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    (output.status.success() && !text.trim().is_empty()).then(|| text.trim().to_owned())
}
