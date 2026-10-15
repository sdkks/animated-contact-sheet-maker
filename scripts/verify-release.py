#!/usr/bin/env python3
"""Local-only guards used before release staging/publication; no network calls."""
import hashlib
from pathlib import Path
import sys
import tomllib
import zipfile

PLATFORMS = ("windows-x86_64", "windows-aarch64", "linux-x86_64", "linux-x86_64-musl")
ROOT = Path(__file__).resolve().parent.parent


def verify_tag(tag, root=ROOT):
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    version = manifest["package"]["version"]
    locked = [p["version"] for p in lock["package"] if p["name"] == "acsm"]
    if tag != f"v{version}" or locked != [version]:
        raise ValueError("Release tag, Cargo.toml and acsm Cargo.lock version must match")


def verify_archives(directory):
    archives = {f"acsm-{platform}.zip" for platform in PLATFORMS}
    expected = archives | {f"{name}.sha256" for name in archives}
    # An interrupted finalization may already have uploaded the aggregate.
    actual = {p.name for p in directory.iterdir()} - {"SHA256SUMS"}
    if actual != expected:
        raise ValueError(f"Release inventory mismatch: missing={expected - actual}, extra={actual - expected}")
    lines = []
    for name in sorted(archives):
        path = directory / name
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        line = f"{digest}  {name}\n"
        if (directory / f"{name}.sha256").read_text(encoding="utf-8") != line:
            raise ValueError(f"Checksum mismatch: {name}")
        executable = "acsm.exe" if "windows" in name else "acsm"
        with zipfile.ZipFile(path) as archive:
            if sorted(archive.namelist()) != sorted([executable, "LICENSE", "FONT-LICENSE"]):
                raise ValueError(f"Unexpected ZIP entries: {name}")
            if archive.testzip() is not None:
                raise ValueError(f"ZIP integrity failure: {name}")
            if any(entry.file_size == 0 for entry in archive.infolist()):
                raise ValueError(f"Empty ZIP entry: {name}")
            for entry, source in (("LICENSE", "LICENSE"), ("FONT-LICENSE", "assets/fonts/LICENSE")):
                if archive.read(entry) != (ROOT / source).read_bytes():
                    raise ValueError(f"License notice mismatch: {name}/{entry}")
            if executable == "acsm" and not (archive.getinfo(executable).external_attr >> 16) & 0o111:
                raise ValueError(f"Missing executable permission: {name}")
        lines.append(line)
    return "".join(sorted(lines))


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in ("tag", "archives"):
        sys.exit("Usage: verify-release.py tag <tag> | archives <download-directory>")
    if sys.argv[1] == "tag":
        verify_tag(sys.argv[2])
    else:
        directory = Path(sys.argv[2])
        sums = verify_archives(directory)
        (directory / "SHA256SUMS").write_text(sums, encoding="utf-8", newline="\n")
        print(sums, end="")
