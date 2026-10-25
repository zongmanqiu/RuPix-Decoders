#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Suffix-claim gate for the RuPix decoder repo.

The host routes a suffix to the first claiming plugin by DLL file-name order. Two
published tiers claiming the same suffix is therefore not an error at runtime; it is
a silent choice nobody can see. This gate keeps that property true at the source
(tiers only; single libs under bench/ are never published).

Rule: every `extensions: "a;b;c"` literal in `decoders/*/src/lib.rs` must be
disjoint across tiers, and at least one tier must declare something.

Why this lives here and not in the host: the host base knows the ABI and the
request-kind routing, not which decoder DLLs exist. The inventory belongs to this
repo; the host only reads `iv_plugin_info()` at runtime.

Exit code: 0 clean, 1 violations. Prints "violations: N" like the other gates.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TIERS = ROOT / "decoders"
EXT_LITERAL = re.compile(r'extensions\s*:\s*"([^"]*)"')


def parse_exts(literal):
    return [e.strip().strip(".").lower() for e in literal.split(";") if e.strip()]


def scan():
    claims = {}
    scanned = []
    for lib in sorted(TIERS.glob("*/src/lib.rs")):
        tier = lib.parent.parent.name
        text = lib.read_text(encoding="utf-8", errors="replace")
        found = EXT_LITERAL.findall(text)
        if not found:
            continue
        scanned.append(tier)
        for lit in found:
            for ext in parse_exts(lit):
                claims.setdefault(ext, []).append(tier)
    return scanned, claims


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    scanned, claims = scan()
    bad = []
    if not scanned:
        bad.append("decoders/*/src/lib.rs 里找不到 extensions: 字面量 —— 扫描器失明，先修脚本")
    for ext, tiers in sorted(claims.items()):
        if len(tiers) > 1:
            bad.append(f".{ext} 被多个档认领：{', '.join(tiers)}")
    for v in bad:
        print(f"  VIOLATION {v}")
    print(
        "tiers: %d, suffixes: %d, violations: %d"
        % (len(scanned), len(claims), len(bad))
    )
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
