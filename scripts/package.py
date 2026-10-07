#!/usr/bin/env python3
"""Package a release binary with the files that must ship with it.

Usage: python3 scripts/package.py TARGET BINARY OUTDIR

Writes OUTDIR/druck-VERSION-TARGET.tar.gz (.zip for Windows targets) and a
matching .sha256 file. The archive holds one folder with the binary, README.md,
THIRD_PARTY_NOTICES.md, OFL.txt, and LICENSE if the repository has one.
Standard library only, so it runs the same on every platform.
"""

import hashlib
import sys
import tarfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FILES = {"README.md": "README.md", "THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md", "OFL.txt": "fonts/OFL.txt"}


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    target, binary, outdir = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    windows = "windows" in target
    folder = f"druck-{version}-{target}"

    members = {binary: f"druck{'.exe' if windows else ''}"}
    members |= {ROOT / src: name for name, src in FILES.items()}
    for license in sorted(ROOT.glob("LICENSE*")):
        members[license] = license.name
    missing = [str(path) for path in members if not path.is_file()]
    if missing:
        print(f"missing files: {', '.join(missing)}", file=sys.stderr)
        return 1

    outdir.mkdir(parents=True, exist_ok=True)
    archive = outdir / (folder + (".zip" if windows else ".tar.gz"))
    if windows:
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            for path, name in members.items():
                zf.write(path, f"{folder}/{name}")
    else:
        with tarfile.open(archive, "w:gz") as tf:
            for path, name in members.items():
                tf.add(path, f"{folder}/{name}", filter=lambda info: _normalize(info, name))

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (outdir / (archive.name + ".sha256")).write_text(f"{digest}  {archive.name}\n")
    print(f"{archive} {digest}")
    return 0


def _normalize(info: tarfile.TarInfo, name: str) -> tarfile.TarInfo:
    """Drop build users and timestamps from the entries."""
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mtime = 0
    info.mode = 0o755 if name == "druck" else 0o644
    return info


if __name__ == "__main__":
    sys.exit(main())
