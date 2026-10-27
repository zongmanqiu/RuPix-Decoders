#!/usr/bin/env python3
"""History gate: release history is version/date/ABI + public changes only.

HISTORY.md is not a release dashboard. Upload progress, ZIP generation state,
release tags and URLs belong to decoders.json, never to the version history.

Exit 0 clean, 1 on any violation. Prints "violations: N" like the other gates.
"""

import json
import re
import sys
from datetime import date
from pathlib import Path

from lib import ROOT
DECODERS = ROOT / "workspace" / "decoders"
ABI = ROOT / "abi" / "src" / "lib.rs"
INDEX = ROOT / "decoders.json"

HEADING = re.compile(
    r"^##\s+(\d+\.\d+\.\d+)\s+[—–-]\s+(\d{4}-\d{2}-\d{2})\s+[—–-]\s+abi\s+(\d+)\s*$",
    re.MULTILINE,
)
VERSION = re.compile(r'(?m)^version\s*=\s*"([^"]+)"')

# Words that describe the release process, not the shipped behavior.
# Keep this list narrow: "包格式改为 ZIP" is a real public change and is allowed.
FORBIDDEN = (
    (re.compile(r"(?m)^\s*-\s*\*\*状态\*\*"), "状态 bullet"),
    (re.compile("待上传"), "待上传"),
    (re.compile("发布包已生成"), "发布包生成状态"),
    (re.compile("等待 Gitee|等待 GitHub|等待上传"), "上传等待状态"),
    (re.compile("未发布"), "未发布状态"),
    (re.compile("release tag", re.IGNORECASE), "release tag"),
    (re.compile(r"gitee\.com|github\.com", re.IGNORECASE), "release URL"),
)


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

    text = hist.read_text(encoding="utf-8")
    headings = HEADING.findall(text)
    if not headings:
        return ["%s: no '## <version> — <YYYY-MM-DD> — abi <N>' heading" % pid]
    newest_version, newest_date, newest_abi = headings[0]
    if newest_version != version:
        out.append("%s: HISTORY newest %s != Cargo.toml %s" % (pid, newest_version, version))
    if int(newest_abi) != abi:
        out.append("%s: HISTORY abi %s != IV_ABI_VERSION %s" % (pid, newest_abi, abi))

    versions = [h[0] for h in headings]
    if len(versions) != len(set(versions)):
        out.append("%s: duplicate version heading in HISTORY" % pid)

    for pattern, label in FORBIDDEN:
        if pattern.search(text):
            out.append("%s: HISTORY contains forbidden %s" % (pid, label))

    try:
        newest = date.fromisoformat(newest_date)
        if newest > date.today():
            out.append("%s: HISTORY release date is in the future" % pid)
    except ValueError:
        out.append("%s: HISTORY date is not YYYY-MM-DD: %r" % (pid, newest_date))

    if released and newest_date != released:
        out.append("%s: HISTORY date %r != decoders.json released %r"
                   % (pid, newest_date, released))
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
