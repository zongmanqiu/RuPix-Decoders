#!/usr/bin/env python3
"""History gate: each published decoder has a HISTORY.md whose newest version and
ABI match the code, and whose release status matches index.json.

Exit 0 clean, 1 on any violation. Prints "violations: N" like the other gates.
"""

import json
import re
import sys
from datetime import date
from pathlib import Path

from lib import ROOT
DECODERS = ROOT / "decoders-rust" / "decoders"
ABI = ROOT / "abi" / "src" / "lib.rs"
INDEX = ROOT / "index.json"

HEADING = re.compile(
    r"^##\s+(\d+\.\d+\.\d+)\s+[—–-]\s+(.+?)\s+[—–-]\s+abi\s+(\d+)\s*$",
    re.MULTILINE,
)
VERSION = re.compile(r'(?m)^version\s*=\s*"([^"]+)"')


def package_version(manifest: Path):
    text = manifest.read_text(encoding="utf-8")
    m = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", text)
    if not m:
        return None
    v = VERSION.search(m.group(1))
    return v.group(1) if v else None


def abi_version():
    m = re.search(r"IV_ABI_VERSION:\s*u32\s*=\s*(\d+)", ABI.read_text(encoding="utf-8"))
    return int(m.group(1)) if m else None


def released_map():
    if not INDEX.is_file():
        return {}
    data = json.loads(INDEX.read_text(encoding="utf-8"))
    return {entry.get("id"): entry.get("released", "") for entry in data}


def check_decoder(pid: str, released: str, abi: int):
    out = []
    hist = DECODERS / pid / "HISTORY.md"
    manifest = DECODERS / pid / "Cargo.toml"
    if not hist.is_file():
        return ["%s: HISTORY.md missing" % pid]
    if not manifest.is_file():
        return ["%s: Cargo.toml missing" % pid]
    version = package_version(manifest)
    if not version:
        return ["%s: no [package] version" % pid]
    headings = HEADING.findall(hist.read_text(encoding="utf-8"))
    if not headings:
        return ["%s: no '## <version> — <date|未发布> — abi <N>' heading" % pid]
    newest_version, status, newest_abi = headings[0]
    if newest_version != version:
        out.append("%s: HISTORY newest %s != Cargo.toml %s" % (pid, newest_version, version))
    if int(newest_abi) != abi:
        out.append("%s: HISTORY abi %s != IV_ABI_VERSION %s" % (pid, newest_abi, abi))
    versions = [h[0] for h in headings]
    if len(versions) != len(set(versions)):
        out.append("%s: duplicate version heading in HISTORY" % pid)
    if released:
        if status.strip() != released:
            out.append("%s: HISTORY status %r != index.json released %r"
                       % (pid, status.strip(), released))
        try:
            if date.fromisoformat(status.strip()) > date.today():
                out.append("%s: HISTORY release date is in the future" % pid)
        except ValueError:
            out.append("%s: HISTORY status is neither a date nor 未发布" % pid)
    else:
        if "未发布" not in status and status.strip().lower() not in ("unreleased", "not released"):
            out.append("%s: index.json says unreleased but HISTORY says %r" % (pid, status.strip()))
    return out


def main():
    abi = abi_version()
    if abi is None:
        print("violations: 1")
        print("  VIOLATION abi/src/lib.rs: IV_ABI_VERSION not found")
        return 1
    released = released_map()
    violations = []
    ids = sorted(p.name for p in DECODERS.iterdir() if p.is_dir())
    for pid in ids:
        violations += check_decoder(pid, released.get(pid, ""), abi)
    for v in violations:
        print("  VIOLATION %s" % v)
    print("histories: %d, violations: %d" % (len(ids), len(violations)))
    return 1 if violations else 0


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except AttributeError:
        pass
    sys.exit(main())
