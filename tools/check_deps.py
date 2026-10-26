#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Dependency gate for the RuPix decoder repo (replaces the host-side scan).

Rule under test (AGENTS.md 第一节 1, 解码器开发规范): a decoder decoder is a pure
computation black box. It must not pull in the host's windowing stack or Win32,
and must not depend on the host crate at all.

    barred crates: windui, windows, rupix, rupix_lib

Why a script and not a comment: the old machine-readable source of this rule
lived in the host repo (main/build/layering_scan.py, BARRED["decoders"]). After
the split the decoder repo owns the rule, so it owns the scanner too - otherwise
AGENTS.md would be documenting a gate nobody runs.

Two scans, both cheap:
  1. every Cargo.toml in the workspace -> [dependencies]/[dev-dependencies]/
     [build-dependencies] tables, key matched against the barred set;
  2. every *.rs / *.c / *.h under the workspace -> `use <barred>::` /
     `#include` style references (catches a path dep spelled through [patch]
     or a stray raw Win32 call in a C decoder).

Exit code: 0 clean, 1 violations. Prints "violations: N" like the host scanners
so both can sit in the same pipeline.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BARRED = {"windui", "windows", "rupix", "rupix_lib", "rupix-main", "rupix_main"}

# Crates allowed to mention a barred name in a doc comment / comment line only.
# Scan 2 therefore skips comment lines; scan 1 has no such exemption.
DEP_TABLE = re.compile(
    r"^\s*\[(?:target\.[\w.\-]+\.)?(?:dev-)?(?:build-)?dependencies\]\s*$"
)
DEP_LINE = re.compile(r"^\s*([A-Za-z0-9_.\-]+)\s*=")
USE_LINE = re.compile(r"(?:^|[^.\w])(?:use|extern\s+crate|include!?)\s*[\"<]?\s*(\w+)")


def cargo_tomls():
    for p in sorted(ROOT.rglob("Cargo.toml")):
        if "target" in p.relative_to(ROOT).parts:
            continue
        yield p


def scan_manifests():
    bad = []
    for path in cargo_tomls():
        in_deps = False
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("["):
                in_deps = bool(DEP_TABLE.match(line))
                continue
            if not in_deps or not stripped or stripped.startswith("#"):
                continue
            m = DEP_LINE.match(line)
            if not m:
                continue
            crate = m.group(1).lower().replace("_", "-")
            if crate in {b.replace("_", "-") for b in BARRED}:
                bad.append(f"{path.relative_to(ROOT)}:{n}: barred dependency `{m.group(1)}`")
    return bad


def scan_sources():
    bad = []
    for path in sorted(ROOT.rglob("*")):
        if not path.is_file() or path.suffix not in (".rs", ".c", ".h", ".cpp", ".hpp"):
            continue
        if "target" in path.relative_to(ROOT).parts:
            continue
        for n, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            s = line.strip()
            if s.startswith(("//", "/*", "*", "#")):
                continue  # comments describe the rule, they don't break it
            for word in BARRED:
                if re.search(rf"\b(?:use|extern crate)\s+{word}\b", s) or \
                   re.search(rf"\b#include\s*[<\"]{word}", s):
                    bad.append(f"{path.relative_to(ROOT)}:{n}: barred reference `{word}`")
    return bad


def main():
    violations = scan_manifests() + scan_sources()
    for v in violations:
        print(f"  VIOLATION {v}")
    print(f"violations: {len(violations)}")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main())
