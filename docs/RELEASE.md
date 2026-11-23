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
   - Run it without network: `scripts/check-offline.sh target/release/druck` renders every sample normally and with the network denied (`sandbox-exec` on macOS, `unshare -rn` on Linux, an outbound Windows Firewall rule for the binary on Windows) from an empty directory with an empty home, and the PDFs must be equal. Druck has no network code, so this guards against a future dependency adding one. On Windows, run it in Git Bash started as administrator, with the firewall on; CI's `render` job runs it on `windows-latest`.
   - Check the limits: `cargo test --release --locked --test performance -- --nocapture` renders a 100-page report and fails over 2 s or 150 MB of heap. CI runs it on every OS.
   - Confirm fonts are embedded: `pdffonts /tmp/druck-pdfs/report.pdf` must show `yes` in the `emb` column for every row (Libertinus Serif and Mono).
   - To compare platforms by hand, copy each platform's PDFs into `DIR/<platform>/` and run `scripts/check-pdfs.sh DIR`.
3. Package: `python3 scripts/package.py <target> target/release/druck dist`. This writes `dist/druck-<version>-<hash>-<target>.tar.gz` (`.zip` for Windows) and a `.sha256` file with it. Verify with `shasum -a 256 -c` (macOS) or `sha256sum -c` (Linux) inside `dist/`.

## Conformance validators by hand

CI does not run these. They need Java (veraPDF, epubcheck) or Node (Ace), and their results need reading. Run them before a release and record the results in `docs/PROGRESS.md`. On macOS:

```sh
brew install verapdf epubcheck
npm install -g @daisy/ace
cargo build --release --locked
out=$(mktemp -d)
for md in samples/*.md; do
  name=$(basename "$md" .md)
  target/release/druck render "$md" --set 'conformance=[pdf/a-2a, pdf/ua-1]' -o "$out/$name.pdf"
  target/release/druck render "$md" -o "$out/$name.epub"
done
verapdf --flavour 2a --format text "$out"/*.pdf
verapdf --flavour ua1 --format text "$out"/*.pdf
for epub in "$out"/*.epub; do epubcheck "$epub"; done
for epub in "$out"/*.epub; do ace --outdir "$out/ace-$(basename "$epub" .epub)" "$epub"; done
```

veraPDF prints `PASS` or `FAIL` per file with the failed rules. epubcheck ends each file with its count of errors and warnings. Ace writes `report.html` into each `ace-NAME` folder. A sample that `conformance` rejects, such as one with an image without a description, reports why and writes no PDF; that is the check working, not a validator failure. Screen readers (VoiceOver on macOS, NVDA on Windows) are tried by hand on `report.pdf` and `report.epub`.

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
  calendar version, without the hash) as `CFBundleShortVersionString`, the commit count as `CFBundleVersion`, the
  minimum macOS read from the binary, Sparkle's settings, and two document types, Markdown (`.md`, `.markdown`) and
  folders. Both are `Alternate`, so Druck is offered under Open With and takes drops on the Dock icon, and it does not
  become the default editor.
- `Contents/Frameworks/Sparkle.framework`, the updater, without its XPC services, which only sandboxed apps use.
- `Contents/Resources`: `Druck.icns`, `THIRD_PARTY_NOTICES.md` (the About box has a button that opens it), `OFL.txt`,
  `LICENSE`, and `Sparkle-LICENSE.txt`.

`CFBundleVersion` is what Sparkle compares, so it must only rise. The number of commits up to the release commit does:
each release commit descends from the one before, also for two releases on one day. The script refuses a shallow
clone, whose count would be too small.

The icon is a placeholder drawn by the script. Ship a real `Druck.icns` before a release that reaches other people.

### Signing and notarizing

Nothing secret is in the repository. The script reads these environment variables:

| Variable | Holds |
| --- | --- |
| `DRUCK_SIGN_IDENTITY` | The name of a "Developer ID Application" certificate in a keychain `codesign` can reach, such as `Developer ID Application: Name (TEAMID)`. |
| `DRUCK_NOTARY_PROFILE` | The name of a keychain profile made with `xcrun notarytool store-credentials`. |
| `DRUCK_NOTARY_KEYCHAIN` | Optional: the keychain file that holds the profile, when it is not the login keychain. CI sets it. |
| `DRUCK_SPARKLE_KEY` | With `--appcast` only: Sparkle's EdDSA private key. |

With neither, the app is signed ad hoc, without the hardened runtime, whose library validation would refuse the ad-hoc Sparkle framework. It runs on the Mac that built it and on others only
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

### Updates

The app updates itself with [Sparkle 2](https://sparkle-project.org). The script downloads the version pinned in
`SPARKLE_VERSION` and refuses the archive unless its SHA-256 matches `SPARKLE_SHA256`. The repository holds no
framework. To move to a newer Sparkle, change both constants and check the release notes for new Info.plist keys.

The app loads the framework from `Contents/Frameworks` when it starts, so a build run with cargo has no updater and no
menu item. In the bundle, Sparkle checks the feed once a day and shows its own window, with the release notes, before
it installs anything. It never installs on its own. A check that finds an update while the author is writing waits
until Druck is next brought to the front. "Check for updates…" in the Druck menu checks at once.

The feed is `https://github.com/coderscantina/druck/releases/download/appcast/appcast.xml`, the one asset of a
pre-release with the fixed tag `appcast`. A pre-release is never `latest`, so this one never stands in for a versioned
release. GitHub redirects the URL to the asset, and Sparkle follows the redirect. Each appcast has one item, the DMG on
its own versioned tag. The update, the feed, and its release notes are signed with an EdDSA key
(`SURequireSignedFeed`), whose public half is `SPARKLE_PUBLIC_KEY` in the script. While that is empty, the script
builds the bundle without Sparkle and says so.

A release reaches the updater like this: the tag push runs the `desktop` job, which builds, signs, notarizes, and
staples the app and the DMG, then runs Sparkle's `generate_appcast` with the newest `CHANGELOG.md` section as the
release notes. The `publish-desktop` job adds the DMG and its `.sha256` to the release the `publish` job created, and
then replaces `appcast.xml` on the `appcast` release (`gh release upload --clobber`), creating that release the first
time. From then on, each running Druck offers the new version at its next daily check. A release whose desktop job
fails, or a CLI-only release, leaves the feed as it was, naming the last DMG there is, so the updater keeps working.
Every tag the workflow publishes moves the feed, so marking a versioned release a pre-release afterwards does not hold
its update back. Do not delete the `appcast` release or its tag: installed apps read the feed from it.

`python3 scripts/package-desktop.py --appcast NOTES.md` writes the appcast locally as well, with the private key in
`DRUCK_SPARKLE_KEY`.

### Secrets for the release workflow

On a tag, the `desktop` job signs with secrets of the repository (Settings, Secrets and variables, Actions). Without
them the job fails and the CLI release goes out without the desktop app. The team id, `3XA3ND2F7P`, is in the
workflow, as every signature names it. The job imports the certificate and the notary key into a keychain of its own,
which it deletes at the end, and prints none of them.

| Secret | Holds |
| --- | --- |
| `MACOS_CERTIFICATE_P12` | The "Developer ID Application" certificate with its private key, as a `.p12` file in base64. |
| `MACOS_CERTIFICATE_PASSWORD` | The password the `.p12` file was exported with. |
| `MACOS_NOTARY_KEY` | The App Store Connect API key for notarization, the whole text of its `AuthKey_<id>.p8` file. |
| `MACOS_NOTARY_KEY_ID` | That key's id, ten characters. |
| `MACOS_NOTARY_ISSUER_ID` | The issuer id of the team's API keys, a UUID. |
| `SPARKLE_ED_PRIVATE_KEY` | Sparkle's EdDSA private key, the one line `generate_keys -x` writes. |

How to make each, once:

1. **The certificate.** In Keychain Access, under My Certificates, select "Developer ID Application: … (3XA3ND2F7P)"
   with its key, choose File, Export Items, save as `druck.p12`, and give it a strong password. Then
   `base64 -i druck.p12 | pbcopy` and paste it as `MACOS_CERTIFICATE_P12`, the password as
   `MACOS_CERTIFICATE_PASSWORD`. Delete `druck.p12` afterwards.
2. **The notary key.** At App Store Connect, Users and Access, Integrations, App Store Connect API, make a team key with
   the Developer role. Download `AuthKey_<id>.p8` (it can be downloaded once) and paste its text as `MACOS_NOTARY_KEY`,
   the key id as `MACOS_NOTARY_KEY_ID`, and the issuer id shown above the list as `MACOS_NOTARY_ISSUER_ID`. An API key
   was chosen over an Apple ID and app-specific password because it belongs to the team and can be revoked alone.
3. **The EdDSA key.** From a Sparkle release (the version in `SPARKLE_VERSION`), run `./bin/generate_keys`. It keeps
   the private key in the login keychain and prints the public key. Put the public key in `SPARKLE_PUBLIC_KEY` in
   `scripts/package-desktop.py` and commit it. Run `./bin/generate_keys -x sparkle-private-key`, paste the file's
   one line as `SPARKLE_ED_PRIVATE_KEY`, and delete the file. Keep the keychain item backed up: a lost key means the
   builds in use accept no further update. Never change the public key after a build with it has shipped.

The local steps under "Signing and notarizing" stay as they are for a build by hand.

## Not automated

- Publishing to crates.io.
- Homebrew or other package manager entries.
- The repository secrets for signing, and `SPARKLE_PUBLIC_KEY`, which only the owner can add.
- Running the manual visual review and recording it in `docs/PROGRESS.md`.

## If PDF comparison across OSes fails

The renderer promises identical layout, not identical bytes. `scripts/check-pdfs.sh` starts with a byte comparison and prints page counts and the first differing text lines on a mismatch. If bytes differ but text and page counts match (for example from a compression difference in a dependency), switch the check to `pdftotext -layout` output plus page count and record why in `docs/DECISIONS.md`.
