#!/usr/bin/env python3
"""Build Druck.app and a DMG for the desktop app on macOS.

Usage: python3 scripts/package-desktop.py [--binary PATH] [--outdir DIR]

Builds `druck-desktop` in release mode (or takes --binary), writes OUTDIR/Druck.app, signs it, and packs
OUTDIR/druck-desktop-VERSION-HASH-TARGET.dmg with a .sha256 file. VERSION is the root Cargo.toml's, HASH the short
commit hash, as in `scripts/package.py`. Standard library plus the tools macOS ships: codesign, hdiutil, iconutil,
xcrun. Nothing here publishes, tags, or pushes.

Signing reads two environment variables, and the repository holds no secret:

  DRUCK_SIGN_IDENTITY   a "Developer ID Application: Name (TEAMID)" identity in a keychain codesign can reach
  DRUCK_NOTARY_PROFILE  a keychain profile made with `xcrun notarytool store-credentials`

With neither, the bundle is signed ad hoc and the script says so. It runs here and on the same Mac only after a
right-click Open. With an identity and no profile it is signed but not notarized. With both, the app and then the
DMG are notarized and stapled. docs/RELEASE.md has the owner's steps.

The icon is a PLACEHOLDER drawn by this script so the bundle has one. Replace `icon_png` or ship a real
Druck.icns before a release that reaches other people.
"""

import argparse
import hashlib
import json
import math
import os
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUNDLE_ID = "com.coderscantina.druck"
EXECUTABLE = "Druck"
TARGET = "aarch64-apple-darwin"
COPYRIGHT = "© 2026 Michael Wallner, Coders Cantina. MIT licensed."
ENTITLEMENTS = ROOT / "scripts" / "desktop.entitlements"
# Shipped beside the app: the notices the licenses of the crates and fonts require, and Druck's own license.
RESOURCES = {
    "THIRD_PARTY_NOTICES.md": "desktop/THIRD_PARTY_NOTICES.md",
    "OFL.txt": "fonts/OFL.txt",
    "LICENSE": "LICENSE",
}


def main() -> int:
    if sys.platform != "darwin":
        print("package-desktop.py builds a macOS app and runs on macOS only", file=sys.stderr)
        return 2
    parser = argparse.ArgumentParser(description="Build Druck.app and a DMG.")
    parser.add_argument("--binary", type=Path, help="a built druck-desktop to use instead of building one")
    parser.add_argument("--outdir", type=Path, default=ROOT / "dist-desktop")
    args = parser.parse_args()

    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    commit = run("git", "rev-parse", "--short=7", "HEAD", cwd=ROOT).strip()
    binary = args.binary or build()
    outdir = args.outdir.resolve()
    outdir.mkdir(parents=True, exist_ok=True)

    app = outdir / "Druck.app"
    shutil.rmtree(app, ignore_errors=True)
    assemble(app, binary, version)

    identity = os.environ.get("DRUCK_SIGN_IDENTITY", "").strip()
    profile = os.environ.get("DRUCK_NOTARY_PROFILE", "").strip()
    sign(app, identity)
    if identity and profile:
        notarize(app, profile, archive=True)

    dmg = outdir / f"druck-desktop-{version}-{commit}-{TARGET}.dmg"
    make_dmg(app, dmg)
    if identity:
        sign(dmg, identity)
        if profile:
            notarize(dmg, profile, archive=False)

    digest = hashlib.sha256(dmg.read_bytes()).hexdigest()
    (outdir / (dmg.name + ".sha256")).write_text(f"{digest}  {dmg.name}\n")
    report(app, dmg, digest, bool(identity), bool(profile))
    return 0


def run(*command: str | Path, cwd: Path | None = None, check: bool = True) -> str:
    done = subprocess.run([str(part) for part in command], cwd=cwd, capture_output=True, text=True)
    if check and done.returncode != 0:
        print(f"{' '.join(map(str, command))} failed:\n{done.stdout}{done.stderr}", file=sys.stderr)
        raise SystemExit(1)
    return done.stdout + done.stderr


def build() -> Path:
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "druck-desktop"], cwd=ROOT, check=True)
    meta = json.loads(run("cargo", "metadata", "--format-version", "1", "--no-deps", cwd=ROOT))
    return Path(meta["target_directory"]) / "release" / "druck-desktop"


def minimum_macos(binary: Path) -> str:
    """The system version the binary was built for, which is the lowest it can run on."""
    lines = run("otool", "-l", binary).splitlines()
    for at, line in enumerate(lines):
        if "LC_BUILD_VERSION" in line:
            for follow in lines[at : at + 8]:
                if follow.split()[:1] == ["minos"]:
                    return follow.split()[1]
    raise SystemExit(f"{binary} names no minimum macOS version")


def assemble(app: Path, binary: Path, version: str) -> None:
    contents = app / "Contents"
    for folder in ("MacOS", "Resources"):
        (contents / folder).mkdir(parents=True)
    shutil.copy2(binary, contents / "MacOS" / EXECUTABLE)
    for name, source in RESOURCES.items():
        shutil.copy2(ROOT / source, contents / "Resources" / name)
    make_icns(contents / "Resources" / "Druck.icns")
    plist = {
        "CFBundleName": "Druck",
        "CFBundleDisplayName": "Druck",
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleExecutable": EXECUTABLE,
        "CFBundleIconFile": "Druck",
        "CFBundlePackageType": "APPL",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": minimum_macos(binary),
        "LSApplicationCategoryType": "public.app-category.productivity",
        "NSHighResolutionCapable": True,
        "NSHumanReadableCopyright": COPYRIGHT,
        # Alternate: Druck shows up under Open With and takes drops, and leaves the default editor alone.
        "CFBundleDocumentTypes": [
            {
                "CFBundleTypeName": "Markdown document",
                "CFBundleTypeRole": "Editor",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": ["net.daringfireball.markdown"],
            },
            {
                "CFBundleTypeName": "Druck project folder",
                "CFBundleTypeRole": "Editor",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": ["public.folder"],
            },
        ],
        "UTImportedTypeDeclarations": [
            {
                "UTTypeIdentifier": "net.daringfireball.markdown",
                "UTTypeDescription": "Markdown document",
                "UTTypeConformsTo": ["public.plain-text"],
                "UTTypeTagSpecification": {
                    "public.filename-extension": ["md", "markdown", "mdown"],
                    "public.mime-type": ["text/markdown"],
                },
            }
        ],
    }
    with (contents / "Info.plist").open("wb") as out:
        plistlib.dump(plist, out)


def sign(path: Path, identity: str) -> None:
    """Signs with the hardened runtime. No identity means ad hoc (`-`), which has no timestamp."""
    command = ["codesign", "--force", "--options", "runtime", "--sign", identity or "-"]
    if identity:
        command += ["--timestamp"]
    if path.suffix == ".app":
        command += ["--entitlements", ENTITLEMENTS]
    run(*command, path)
    run("codesign", "--verify", "--strict", "--verbose=2", path)


def notarize(path: Path, profile: str, archive: bool) -> None:
    """Submits the app (as a zip) or the DMG, waits for the verdict, and staples the ticket to it."""
    with tempfile.TemporaryDirectory() as scratch:
        upload = path
        if archive:
            upload = Path(scratch) / (path.name + ".zip")
            run("ditto", "-c", "-k", "--keepParent", path, upload)
        verdict = run("xcrun", "notarytool", "submit", upload, "--keychain-profile", profile, "--wait", check=False)
        print(verdict)
        if "status: Accepted" not in verdict:
            raise SystemExit(f"notarization of {path.name} was not accepted")
    run("xcrun", "stapler", "staple", path)
    run("xcrun", "stapler", "validate", path)


def make_dmg(app: Path, dmg: Path) -> None:
    dmg.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        staging = Path(scratch) / "Druck"
        staging.mkdir()
        # ditto keeps the signature, extended attributes, and a stapled ticket.
        run("ditto", app, staging / app.name)
        (staging / "Applications").symlink_to("/Applications")
        run("hdiutil", "create", "-volname", "Druck", "-srcfolder", staging, "-format", "UDZO", "-ov", dmg)


def report(app: Path, dmg: Path, digest: str, signed: bool, notarized: bool) -> None:
    size = lambda path: sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / 1e6 if path.is_dir() else path.stat().st_size / 1e6
    print(f"{app}: {size(app):.1f} MB")
    print(f"{dmg}: {size(dmg):.1f} MB, sha256 {digest}")
    if notarized:
        print("Signed with your identity, notarized, and stapled.")
    elif signed:
        print("Signed with your identity, NOT notarized: set DRUCK_NOTARY_PROFILE to notarize. Gatekeeper will warn.")
    else:
        print("AD-HOC SIGNED, not notarized: no DRUCK_SIGN_IDENTITY was set.")
        print("This build runs on this Mac. On another Mac, Gatekeeper refuses it until it is opened with right-click, Open.")


# The placeholder icon: a white page with a folded corner and three lines of text on a dark blue rounded square.


def icon_png(size: int) -> bytes:
    """One RGBA PNG of the placeholder icon, anti-aliased by the distance to each shape's edge."""
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            row += bytes(pixel(x + 0.5, y + 0.5, size))
        rows.append(bytes(row))
    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


def pixel(x: float, y: float, size: int) -> tuple[int, int, int, int]:
    u, v = x / size, y / size
    # The rounded square keeps the margin macOS icons have, about 10%.
    coverage = clamp(0.5 - rounded_box(u, v, 0.5, 0.5, 0.40, 0.40, 0.18) * size)
    if coverage <= 0:
        return (0, 0, 0, 0)
    ink, paper, fold = (34, 52, 92), (246, 246, 242), (198, 204, 218)
    # The page is 0.30 to 0.70 wide and 0.23 to 0.77 tall, with its top right corner folded over.
    diagonal = ((u - 0.58) - (v - 0.23)) / math.sqrt(2)
    page = max(rounded_box(u, v, 0.5, 0.5, 0.20, 0.27, 0.02), diagonal)
    colour = mix(ink, paper, clamp(0.5 - page * size))
    flap = max(0.58 - u, v - 0.35, diagonal)
    colour = mix(colour, fold, clamp(0.5 - flap * size))
    for line_y, half in ((0.46, 0.13), (0.55, 0.13), (0.64, 0.09)):
        line = rounded_box(u, v, 0.5 - (0.13 - half), line_y, half, 0.012, 0.012)
        colour = mix(colour, ink, clamp(0.5 - line * size))
    return (*colour, round(coverage * 255))


def rounded_box(u: float, v: float, cx: float, cy: float, hw: float, hh: float, r: float) -> float:
    """Signed distance to a rounded box, negative inside."""
    dx, dy = abs(u - cx) - (hw - r), abs(v - cy) - (hh - r)
    return math.hypot(max(dx, 0), max(dy, 0)) + min(max(dx, dy), 0) - r


def clamp(value: float) -> float:
    return max(0.0, min(1.0, value))


def mix(a: tuple[int, ...], b: tuple[int, ...], amount: float) -> tuple[int, int, int]:
    return tuple(round(p + (q - p) * amount) for p, q in zip(a, b))  # type: ignore[return-value]


def make_icns(target: Path) -> None:
    """Writes the icon set at every size macOS asks for and has iconutil pack it."""
    with tempfile.TemporaryDirectory() as scratch:
        iconset = Path(scratch) / "Druck.iconset"
        iconset.mkdir()
        sizes = {16: "16x16", 32: "16x16@2x", 64: "32x32@2x", 128: "128x128", 256: "128x128@2x", 512: "256x256@2x", 1024: "512x512@2x"}
        for size, name in sizes.items():
            (iconset / f"icon_{name}.png").write_bytes(icon_png(size))
        # 32 and 256 and 512 are also the 1x sizes of the next names.
        shutil.copy(iconset / "icon_16x16@2x.png", iconset / "icon_32x32.png")
        shutil.copy(iconset / "icon_128x128@2x.png", iconset / "icon_256x256.png")
        shutil.copy(iconset / "icon_256x256@2x.png", iconset / "icon_512x512.png")
        run("iconutil", "-c", "icns", iconset, "-o", target)


if __name__ == "__main__":
    sys.exit(main())
