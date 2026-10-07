#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.md from Cargo.lock and the bundled data.

Usage: python3 scripts/notices.py [--check]

Runs `cargo metadata --locked` once per release target and takes the union of
the crates that are linked into the binary: normal dependencies only, no build
scripts, no dev dependencies, no proc macros. License texts come from the
crate sources in the local cargo registry. Standard library only.

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

# Keep in sync with the release matrix in .github/workflows/release.yml.
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
]

# Crates that publish no license file. Their texts are copied from the upstream
# repository into scripts/license-overrides/<dir>/ and used instead.
OVERRIDES = {"krilla": "krilla", "krilla-svg": "krilla"}
OVERRIDE_DIR = ROOT / "scripts" / "license-overrides"

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


def linked_crates(meta: dict) -> set[str]:
    """Package ids reachable from the root through normal dependency edges."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    seen: set[str] = set()
    stack = [meta["resolve"]["root"]]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    return seen


def collect() -> dict[tuple[str, str], dict]:
    """Return {(name, version): package} for the crates linked on any target."""
    found: dict[tuple[str, str], dict] = {}
    for target in TARGETS:
        meta = metadata(target)
        packages = {p["id"]: p for p in meta["packages"]}
        root = meta["resolve"]["root"]
        for pid in linked_crates(meta):
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


def render() -> tuple[str, list[str]]:
    crates = collect()
    warnings: list[str] = []
    rows = []
    texts: dict[str, dict] = {}

    for name, version in sorted(crates):
        pkg = crates[(name, version)]
        license = pkg["license"] or "(none declared)"
        if not pkg["license"]:
            warnings.append(f"{name} {version} declares no license")
        rows.append(f"| {name} | {version} | {license} |")
        files = license_files(pkg)
        if not files:
            warnings.append(f"{name} {version} ships no license file ({license})")
        for filename, text in files:
            sha = hashlib.sha256(text.encode()).hexdigest()
            entry = texts.setdefault(sha, {"text": text, "names": set(), "crates": []})
            entry["names"].add(filename)
            entry["crates"].append(f"{name} {version}")

    ofl = (ROOT / "fonts" / "OFL.txt").read_text(encoding="utf-8").strip()
    out = [
        "# Third-party notices",
        "",
        "Kyber's release binaries embed the fonts, hyphenation patterns, and Rust crates listed here.",
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
        f"{len(rows)} crates are linked into the binary on at least one of: {', '.join(TARGETS)}.",
        "Build scripts, procedural macros, and test-only crates are not linked and are not listed.",
        "Where a crate offers a choice of licenses (`OR`), either one applies.",
        "",
        "| Crate | Version | License |",
        "| --- | --- | --- |",
        *rows,
        "",
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
    return "\n".join(out).rstrip() + "\n", warnings


def main() -> int:
    text, warnings = render()
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    if "--check" in sys.argv:
        current = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
        if current != text:
            print(f"{OUTPUT.name} is stale: run python3 scripts/notices.py", file=sys.stderr)
            return 1
        return 0
    OUTPUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUTPUT.name}: {len(text.encode()) // 1024} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
