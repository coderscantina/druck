# Releasing Kyber

How to build, check, and package a release by hand. CI runs the same scripts. Nothing here publishes, tags, or pushes. Those steps need a separate decision each time.

## Open item: Kyber's own license

Kyber has no `LICENSE` file and no `license` field in `Cargo.toml`. The owner has not chosen one. Until then:

- Do not distribute binaries or archives outside the project. Third-party licenses are covered by the notices file, but the code itself has no grant.
- `scripts/package.py` ships any `LICENSE*` file it finds at the repository root, so adding the file is enough. Also set `license` in `Cargo.toml`.
- Record the choice in `README.md` and `docs/DECISIONS.md`.

## Prerequisites

- Rust stable, with the target you build for. Native builds need no cross tools.
- Python 3.11 or newer for the scripts. They use the standard library only.
- `pdffonts` and `pdftotext` (poppler) for the checks.
- The crates in `Cargo.lock` in the local cargo registry (`cargo fetch --locked`). The notices script reads license files from there.
- Minimum Rust version: dependencies declare 1.92 at most (`krilla`). Kyber's own minimum is not tested, so `rust-version` is not set.

## Procedure

1. Start from a clean checkout of the commit to release. CI must be green on it, including the cross-OS PDF comparison.
2. Bump `version` in `Cargo.toml`, then run `cargo build --locked` once so `Cargo.lock` follows. Commit both.
3. Regenerate the notices if dependencies changed: `python3 scripts/notices.py`. CI fails when the file is stale (`--check`). Commit the result.
4. Build: `cargo build --release --locked`. The release profile uses fat LTO, one codegen unit, and stripped symbols. Expect about a minute.
5. Check the binary:
   - Render all samples from another directory: `scripts/render-samples.sh target/release/kyber /tmp/kyber-pdfs`.
   - Run it without network. On macOS: put `(version 1)(allow default)(deny network*)` in a file and run `sandbox-exec -f FILE target/release/kyber render samples/de.md -o /tmp/de.pdf`. On Linux: `unshare -rn target/release/kyber render samples/de.md -o /tmp/de.pdf`. The output must equal the online render. Kyber has no network code, so this guards against a future dependency adding one.
   - Confirm fonts are embedded: `pdffonts /tmp/kyber-pdfs/report.pdf` must show `yes` in the `emb` column for every row (Libertinus Serif and Mono).
   - To compare platforms by hand, copy each platform's PDFs into `DIR/<platform>/` and run `scripts/check-pdfs.sh DIR`.
6. Package: `python3 scripts/package.py <target> target/release/kyber dist`. This writes `dist/kyber-<version>-<target>.tar.gz` (`.zip` for Windows) and a `.sha256` file with it. Verify with `shasum -a 256 -c` (macOS) or `sha256sum -c` (Linux) inside `dist/`.

## Archive layout

```
kyber-<version>-<target>/
  kyber (kyber.exe on Windows)
  README.md
  THIRD_PARTY_NOTICES.md
  OFL.txt
  LICENSE            only once the repository has one
```

The notices file satisfies the attribution terms of the bundled licenses. MIT, BSD, and Unicode-3.0 require the copyright line and permission text with every binary distribution. Apache-2.0 requires the license text and any `NOTICE` file, and the notices file includes both. The SIL OFL requires its copyright and license with the fonts, which `OFL.txt` and the notices file provide. The two hyphenation pattern licenses are reproduced in the notices file. Do not strip it from an archive.

## CI builds

`.github/workflows/release.yml` runs on a manual dispatch or a `v*` tag push. It builds `--release --locked` for five targets, renders the samples with each runnable binary, runs `scripts/package.py`, and uploads the archive and checksum as a workflow artifact:

| Target | Runner |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` (older glibc for wider compatibility) |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest`, cross-built and not run |
| `x86_64-pc-windows-msvc` | `windows-latest` |

The macOS and Windows binaries are not signed or notarized. Expect Gatekeeper and SmartScreen warnings until that is set up.

## Not automated

- Creating or pushing a tag.
- Creating a GitHub release or attaching artifacts to it.
- Publishing to crates.io (also blocked until a license is chosen).
- Signing, notarization, Homebrew or other package manager entries.
- Running the manual visual review and recording it in `docs/PROGRESS.md`.

## If PDF comparison across OSes fails

The renderer promises identical layout, not identical bytes. `scripts/check-pdfs.sh` starts with a byte comparison and prints page counts and the first differing text lines on a mismatch. If bytes differ but text and page counts match (for example from a compression difference in a dependency), switch the check to `pdftotext -layout` output plus page count and record why in `docs/DECISIONS.md`.
