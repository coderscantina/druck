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

Before releasing, make sure CI is green on the commit, including the cross-OS PDF comparison, and that the notices are current: run `python3 scripts/notices.py` and `python3 scripts/notices.py --desktop` after dependency changes and commit the result. CI fails when the file is stale (`--check`).

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
   - Run it without network: `scripts/check-offline.sh target/release/druck` renders every sample normally and with the network denied (`sandbox-exec` on macOS, `unshare -rn` on Linux) from an empty directory with an empty home, and the PDFs must be equal. Druck has no network code, so this guards against a future dependency adding one. Windows has no script: check by hand.
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

The macOS and Windows command line binaries are not signed or notarized. Expect Gatekeeper and SmartScreen warnings until that is set up. The desktop app has its own script, below.

## Desktop app (macOS)

`scripts/package-desktop.py` builds `Druck.app` and a DMG. It runs on macOS only and uses nothing but Python's standard
library and the tools macOS ships (`codesign`, `hdiutil`, `iconutil`, `xcrun`). It never publishes, tags, or pushes.

```
python3 scripts/package-desktop.py [--outdir dist-desktop] [--binary PATH]
```

It builds `druck-desktop` in release mode (or takes `--binary`), then writes `Druck.app`, signs it, and writes
`druck-desktop-<version>-<hash>-aarch64-apple-darwin.dmg` with a `.sha256` file. The last lines it prints say plainly
whether the result is ad-hoc signed, signed but not notarized, or notarized and stapled.

The bundle holds:

- `Contents/MacOS/Druck`, the app. Its fonts (Libertinus) and icons are compiled into it.
- `Contents/Info.plist`: name `Druck`, identifier `com.coderscantina.druck`, the version of the root `Cargo.toml` (the
  calendar version, without the hash), the minimum macOS read from the binary, and two document types, Markdown
  (`.md`, `.markdown`) and folders. Both are `Alternate`, so Druck is offered under Open With and takes drops on the
  Dock icon, and it does not become the default editor.
- `Contents/Resources`: `Druck.icns`, `THIRD_PARTY_NOTICES.md` (the About box has a button that opens it), `OFL.txt`, and
  `LICENSE`.

The icon is a placeholder drawn by the script. Ship a real `Druck.icns` before a release that reaches other people.

### Signing and notarizing

Nothing secret is in the repository. The script reads two environment variables:

| Variable | Holds |
| --- | --- |
| `DRUCK_SIGN_IDENTITY` | The name of a "Developer ID Application" certificate in a keychain `codesign` can reach, such as `Developer ID Application: Name (TEAMID)`. |
| `DRUCK_NOTARY_PROFILE` | The name of a keychain profile made with `xcrun notarytool store-credentials`. |

With neither, the app is signed ad hoc with the hardened runtime. It runs on the Mac that built it and on others only
after right-click, Open. The script says so.

The owner's steps, once:

1. Have an Apple Developer Program membership and a "Developer ID Application" certificate with its private key in the
   login keychain. `security find-identity -v -p codesigning` lists it.
2. Make an app-specific password at account.apple.com, then store the credentials once:
   `xcrun notarytool store-credentials druck-notary --apple-id <apple id> --team-id <TEAMID>` (it asks for the password).

For each release:

```
export DRUCK_SIGN_IDENTITY="Developer ID Application: Name (TEAMID)"
export DRUCK_NOTARY_PROFILE=druck-notary
python3 scripts/package-desktop.py
```

The script then signs the app with `codesign --options runtime --timestamp` and the entitlements in
`scripts/desktop.entitlements`, which hold none: the app is not sandboxed and generates no code at run time. It zips the
app, submits it with `notarytool submit --wait`, and staples the ticket. It builds the DMG from that app, signs the
DMG, notarizes it, and staples it. It stops on the first step that is not accepted. Check the result by hand:

```
spctl --assess --type execute --verbose dist-desktop/Druck.app
xcrun stapler validate dist-desktop/druck-desktop-*.dmg
```

`spctl` must say `accepted, source=Notarized Developer ID`. Open the DMG on a Mac that did not build it, from a
download, to see that Gatekeeper opens the app with no warning.

The release workflow builds the app in a `desktop` job on every tag and manual run, ad-hoc signed, and keeps it as the
workflow artifact `desktop-aarch64-apple-darwin`. The publish job does not take it. Signing in CI needs the certificate
and the notary credentials as secrets and a step that imports them into a temporary keychain. That is not set up.

## Not automated

- Publishing to crates.io.
- Signing and notarizing the desktop app (a script does it, the owner runs it), Homebrew or other package manager entries.
- Running the manual visual review and recording it in `docs/PROGRESS.md`.

## If PDF comparison across OSes fails

The renderer promises identical layout, not identical bytes. `scripts/check-pdfs.sh` starts with a byte comparison and prints page counts and the first differing text lines on a mismatch. If bytes differ but text and page counts match (for example from a compression difference in a dependency), switch the check to `pdftotext -layout` output plus page count and record why in `docs/DECISIONS.md`.
