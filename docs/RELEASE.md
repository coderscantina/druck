# Releasing Druck

Druck uses calendar versions: `YY.M.D` in `Cargo.toml`, and a tag `vYY.M.D-HASH`, where `HASH` is the release commit's seven-character short hash, as in `v26.10.7-1a2b3c4`. The hash keeps tags unique when there are two releases on one day. `build.rs` adds the hash to the version, so `druck --version` prints `druck 26.10.7-1a2b3c4`, and the archives carry the same name.

Druck is MIT licensed (`LICENSE`), © 2026 Michael Wallner, Coders Cantina.

## Cutting a release

Run `scripts/release.sh` on an up-to-date `main` with a clean working tree. It:

1. Sets today's version in `Cargo.toml` and `Cargo.lock`.
2. Prepends a section to `CHANGELOG.md` with the commits since the last tag, grouped by gitmoji: features (✨), fixes (🐛, 🚑️, 🩹, 🔒️), changes (💄, ⚡️, ♻️, 🎨, ⏪️, 🚸, 💬, 🌐, ♿️, 🔥, 🗑️), and dependencies (➕, ➖, ⬆️, ⬇️, 📌). Other commits, such as docs, merges, tests, and CI, are left out.
3. Commits as `🔖 release YY.M.D`, tags that commit, and pushes `main` and the tag.

`scripts/release.sh --dry-run` prints the changelog section and changes nothing. Read it first, because the script itself does not stop for review. A second release on the same day gets its own section under the same version heading.

The tag push starts `.github/workflows/release.yml`. It builds and packages five targets, checks that the tag matches the version and commit, and creates the GitHub release with the archives, their checksums, and the newest changelog section as notes. Check that the workflow finished before announcing the release.

Before releasing, make sure CI is green on the commit, including the cross-OS PDF comparison, and that the notices are current: run `python3 scripts/notices.py` after dependency changes and commit the result. CI fails when the file is stale (`--check`).

## Prerequisites

- Rust stable, with the target you build for. Native builds need no cross tools.
- Python 3.11 or newer for the scripts. They use the standard library only.
- `pdffonts` and `pdftotext` (poppler) for the checks.
- The crates in `Cargo.lock` in the local cargo registry (`cargo fetch --locked`). The notices script reads license files from there.
- Minimum Rust version: dependencies declare 1.92 at most (`krilla`). Druck's own minimum is not tested, so `rust-version` is not set.

## Building and checking by hand

CI runs the same scripts. Nothing in this section publishes, tags, or pushes.

1. Build: `cargo build --release --locked`. The release profile uses fat LTO, one codegen unit, and stripped symbols. Expect about a minute.
2. Check the binary:
   - Render all samples from another directory: `scripts/render-samples.sh target/release/druck /tmp/druck-pdfs`.
   - Run it without network. On macOS: put `(version 1)(allow default)(deny network*)` in a file and run `sandbox-exec -f FILE target/release/druck render samples/de.md -o /tmp/de.pdf`. On Linux: `unshare -rn target/release/druck render samples/de.md -o /tmp/de.pdf`. The output must equal the online render. Druck has no network code, so this guards against a future dependency adding one.
   - Confirm fonts are embedded: `pdffonts /tmp/druck-pdfs/report.pdf` must show `yes` in the `emb` column for every row (Libertinus Serif and Mono).
   - To compare platforms by hand, copy each platform's PDFs into `DIR/<platform>/` and run `scripts/check-pdfs.sh DIR`.
3. Package: `python3 scripts/package.py <target> target/release/druck dist`. This writes `dist/druck-<version>-<hash>-<target>.tar.gz` (`.zip` for Windows) and a `.sha256` file with it. Verify with `shasum -a 256 -c` (macOS) or `sha256sum -c` (Linux) inside `dist/`.

## Archive layout

```
druck-<version>-<hash>-<target>/
  druck (druck.exe on Windows)
  README.md
  THIRD_PARTY_NOTICES.md
  OFL.txt
  LICENSE
```

`LICENSE` covers Druck itself. The notices file satisfies the attribution terms of the bundled licenses. MIT, BSD, and Unicode-3.0 require the copyright line and permission text with every binary distribution. Apache-2.0 requires the license text and any `NOTICE` file, and the notices file includes both. The SIL OFL requires its copyright and license with the fonts, which `OFL.txt` and the notices file provide. The two hyphenation pattern licenses are reproduced in the notices file. Do not strip it from an archive.

## CI builds

`.github/workflows/release.yml` runs on a manual dispatch or a `v*` tag push. It builds `--release --locked` for five targets, renders the samples with each runnable binary, runs `scripts/package.py`, and uploads the archive and checksum as a workflow artifact. On a tag push, the `publish` job then creates the GitHub release from them:

| Target | Runner |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` (older glibc for wider compatibility) |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest`, cross-built and not run |
| `x86_64-pc-windows-msvc` | `windows-latest` |

The macOS and Windows binaries are not signed or notarized. Expect Gatekeeper and SmartScreen warnings until that is set up.

## Not automated

- Publishing to crates.io.
- Signing, notarization, Homebrew or other package manager entries.
- Running the manual visual review and recording it in `docs/PROGRESS.md`.

## If PDF comparison across OSes fails

The renderer promises identical layout, not identical bytes. `scripts/check-pdfs.sh` starts with a byte comparison and prints page counts and the first differing text lines on a mismatch. If bytes differ but text and page counts match (for example from a compression difference in a dependency), switch the check to `pdftotext -layout` output plus page count and record why in `docs/DECISIONS.md`.
