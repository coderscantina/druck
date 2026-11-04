#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.md from Cargo.lock and the bundled data.

Usage: python3 scripts/notices.py [--desktop] [--check]

Runs `cargo metadata --locked` once per release target and takes the union of
the crates that are linked into the binary: normal dependencies only, no build
scripts, no dev dependencies, no proc macros. License texts come from the
crate sources in the local cargo registry. Standard library only.

With --desktop it covers the druck-desktop app instead: the crates it links on
macOS, written to desktop/THIRD_PARTY_NOTICES.md.

With --check the file is not written; the exit code is 1 when it is stale.
"""

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "THIRD_PARTY_NOTICES.md"
DESKTOP_OUTPUT = ROOT / "desktop" / "THIRD_PARTY_NOTICES.md"

# Keep in sync with the release matrix in .github/workflows/release.yml.
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
]

# The desktop app is built for macOS only, see the desktop job in ci.yml.
DESKTOP_TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"]

# Crates that publish no license file. Their texts are copied from the upstream
# repository into scripts/license-overrides/<dir>/ and used instead.
OVERRIDES = {"krilla": "krilla", "krilla-svg": "krilla"}
OVERRIDE_DIR = ROOT / "scripts" / "license-overrides"

# Standard texts (SPDX) for a crate that declares one of these licenses but ships no license file. Only the licenses a
# crate declares are used, never a guess. Zlib asks for no notice with a binary, so it has no text here.
SPDX_DIR = OVERRIDE_DIR / "spdx"
SPDX_IDS = ("MIT", "Apache-2.0")

LICENSE_FILE = re.compile(r"^(licen[sc]e|copying|notice|unlicense)([-_.].*)?$", re.I)

# Pattern licenses for the hypher features we build ("english" and "german"),
# copied from the pattern file headers in the hypher repository (patterns/).
HYPHENATION = """\
### hyph-en-us (American English)

Copyright (C) 1990, 2004, 2005 Gerard D.C. Kuiken

Copying and distribution of this file, with or without modification, are
permitted in any medium without royalty provided the copyright notice and this
notice are preserved.

### hyph-de-1996 (German, reformed orthography)

Copyright (c) 2013-2024 Stephan Hennig, Werner Lemberg, Günter Milde, Sander van
Geloven, Georg Pfeiffer, Gisbert W. Selke, Tobias Wendorf, Keno Wehr
(Deutschsprachige Trennmustermannschaft), version 2024-02-28. MIT License:

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""


def metadata(target: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=ROOT, check=True, capture_output=True, text=True,
    )
    return json.loads(out.stdout)


def linked_crates(meta: dict, root: str) -> set[str]:
    """Package ids reachable from `root` through normal dependency edges."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    seen: set[str] = set()
    stack = [root]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    return seen


def collect(targets: list[str], package: str | None) -> dict[tuple[str, str], dict]:
    """Return {(name, version): package} for the crates linked on any target, from the root package or `package`."""
    found: dict[tuple[str, str], dict] = {}
    for target in targets:
        meta = metadata(target)
        packages = {p["id"]: p for p in meta["packages"]}
        root = meta["resolve"]["root"]
        if package:
            root = next(p["id"] for p in meta["packages"] if p["name"] == package)
        for pid in linked_crates(meta, root):
            pkg = packages[pid]
            if pid == root or any("proc-macro" in t["kind"] for t in pkg["targets"]):
                continue
            found[(pkg["name"], pkg["version"])] = pkg
    return found


def read_license_files(folder: Path) -> list[tuple[str, str]]:
    return [
        (path.name, path.read_text(encoding="utf-8", errors="replace").strip())
        for path in sorted(folder.iterdir())
        if path.is_file() and LICENSE_FILE.match(path.name)
    ]


def license_files(pkg: dict) -> list[tuple[str, str]]:
    files = read_license_files(Path(pkg["manifest_path"]).parent)
    if not files and pkg["name"] in OVERRIDES:
        files = read_license_files(OVERRIDE_DIR / OVERRIDES[pkg["name"]])
    return files


def declared(expression: str) -> list[str]:
    """The SPDX ids in a license expression, with `/` read as `OR` as older crates write it."""
    return [part for part in re.split(r"[\s/()]+", expression) if part and part not in ("OR", "AND", "WITH")]


def render(desktop: bool) -> tuple[str, list[str]]:
    targets = DESKTOP_TARGETS if desktop else TARGETS
    crates = collect(targets, "druck-desktop" if desktop else None)
    warnings: list[str] = []
    rows = []
    texts: dict[str, dict] = {}
    # Crates with no license file: their declared licenses and authors, and the standard texts of those licenses.
    bare: list[str] = []
    standard: dict[str, list[str]] = {}

    for name, version in sorted(crates):
        pkg = crates[(name, version)]
        license = pkg["license"] or "(none declared)"
        if not pkg["license"]:
            warnings.append(f"{name} {version} declares no license")
        rows.append(f"| {name} | {version} | {license} |")
        files = license_files(pkg)
        if not files:
            ids = [i for i in declared(pkg["license"] or "") if i in SPDX_IDS]
            note = f"; standard text of {', '.join(ids)} added" if ids else ""
            warnings.append(f"{name} {version} ships no license file ({license}){note}")
            authors = ", ".join(a.split("<")[0].strip() for a in pkg["authors"]) or "(none declared)"
            bare.append(f"| {name} | {version} | {license} | {authors} |")
            for spdx in ids:
                standard.setdefault(spdx, []).append(f"{name} {version}")
        for filename, text in files:
            sha = hashlib.sha256(text.encode()).hexdigest()
            entry = texts.setdefault(sha, {"text": text, "names": set(), "crates": []})
            entry["names"].add(filename)
            entry["crates"].append(f"{name} {version}")

    ofl = (ROOT / "fonts" / "OFL.txt").read_text(encoding="utf-8").strip()
    out = [
        "# Third-party notices",
        "",
        f"{'Druck Desktop' if desktop else 'Druck'}'s release binaries embed the fonts, hyphenation patterns, and Rust crates listed here.",
        "Their licenses require that the copyright notices and license texts travel with the binary.",
        "This file does that. Ship it next to the executable.",
        "",
        "Generated by `python3 scripts/notices.py`. Do not edit by hand.",
        "",
        "## Bundled data",
        "",
        "### Libertinus fonts",
        "",
        "Libertinus Serif (regular, italic, bold, bold italic) and Libertinus Mono (regular), version 7.051,",
        "are embedded in the binary and, as subsets, in PDFs that use them. They are licensed under the",
        "SIL Open Font License 1.1, reproduced here and in `fonts/OFL.txt`.",
        "",
        "```text",
        ofl,
        "```",
        "",
        "### ICC color profiles",
        "",
        "`krilla` embeds four small sRGB and gray ICC profiles from saucecontrol/Compact-ICC-Profiles,",
        "released under CC0 1.0. CC0 asks for no attribution.",
        "",
        "## Hyphenation patterns",
        "",
        "The `hypher` crate compiles TeX hyphenation patterns into the binary. Only the English and German",
        "patterns are built. Their licenses are separate from the crate's own license.",
        "",
        HYPHENATION.rstrip(),
        "",
        "## Rust crates",
        "",
        f"{len(rows)} crates are linked into the binary on at least one of: {', '.join(targets)}.",
        "Build scripts, procedural macros, and test-only crates are not linked and are not listed.",
        "Where a crate offers a choice of licenses (`OR`), either one applies.",
        "",
        "| Crate | Version | License |",
        "| --- | --- | --- |",
        *rows,
        "",
        *(
            [
                "## Crates without a license file",
                "",
                f"{len(bare)} crates declare a license in their manifest but ship no license text. Their copyright",
                "holders are the authors their manifests name, or the contributors of the repository they list.",
                "The standard texts of the licenses they declare follow the other license texts. These are not copies",
                "from the crates.",
                "",
                "| Crate | Version | License | Authors |",
                "| --- | --- | --- | --- |",
                *bare,
                "",
            ]
            if bare
            else []
        ),
        "## License texts",
        "",
        "Texts are copied from each crate's source. Identical texts are shown once.",
        "",
    ]
    ordered = sorted(texts.values(), key=lambda e: (-len(e["crates"]), sorted(e["crates"])[0]))
    for entry in ordered:
        label = ", ".join(sorted(entry["names"]))
        used_by = ", ".join(sorted(entry["crates"]))
        out += [f"### {label}", "", f"Used by: {used_by}", "", "```text", entry["text"], "```", ""]
    for spdx in SPDX_IDS:
        if spdx in standard:
            text = (SPDX_DIR / f"{spdx}.txt").read_text(encoding="utf-8").strip()
            used_by = ", ".join(sorted(standard[spdx]))
            out += [f"### {spdx} (standard text)", "", f"Declared by: {used_by}", "", "```text", text, "```", ""]
    return "\n".join(out).rstrip() + "\n", warnings


def main() -> int:
    desktop = "--desktop" in sys.argv
    output = DESKTOP_OUTPUT if desktop else OUTPUT
    text, warnings = render(desktop)
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    if "--check" in sys.argv:
        current = output.read_text(encoding="utf-8") if output.exists() else ""
        if current != text:
            print(f"{output.name} is stale: run python3 scripts/notices.py{' --desktop' if desktop else ''}", file=sys.stderr)
            return 1
        return 0
    output.write_text(text, encoding="utf-8")
    print(f"wrote {output.relative_to(ROOT)}: {len(text.encode()) // 1024} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
