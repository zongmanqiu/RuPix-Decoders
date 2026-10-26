#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Fail when a document still advertises an ABI other than abi/src/lib.rs.

The ABI version is a hard contract, but several docs spell it out for humans.
Those copies drifted to v4 after the v5 rename; this gate makes the next drift
visible in build.ps1 instead of in a release note.

Only current-version claims are checked. Historical statements such as
"v4 introduced ..., v5 keeps it" are intentionally allowed.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ABI_SOURCE = ROOT / "abi" / "src" / "lib.rs"

# (file, regex with one numeric capture). Each pattern must appear at least once
# and every captured value must equal the current ABI.
CHECKS = (
    ("README.md", r"ABI 冻结 \*\*v(\d+)\*\*"),
    ("docs/architecture.md", r"IV_ABI_VERSION`，当前 (\d+)"),
    ("docs/releasing.md", r"IV_ABI_VERSION`（当前 (\d+)）"),
    ("docs/history-policy.md", r"IV_ABI_VERSION`（当前 (\d+)）"),
    ("docs/catalog.md", r'"abi":\s*(\d+)'),
    ("docs/catalog.md", r"\|\s*`abi`\s*\|\s*`(\d+)`"),
    ("解码器开发规范.md", r"#define IV_ABI_VERSION (\d+)"),
    ("解码器开发规范.md", r"abi_version:\s*(\d+),"),
)


def current_abi():
    text = ABI_SOURCE.read_text(encoding="utf-8")
    match = re.search(r"IV_ABI_VERSION\s*:\s*u32\s*=\s*(\d+)", text)
    if not match:
        raise RuntimeError("IV_ABI_VERSION not found in %s" % ABI_SOURCE)
    return int(match.group(1))


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    abi = current_abi()
    violations = []
    checked = 0
    for rel, pattern in CHECKS:
        path = ROOT / rel
        if not path.is_file():
            violations.append("%s: file missing" % rel)
            continue
        text = path.read_text(encoding="utf-8")
        hits = re.findall(pattern, text)
        if not hits:
            violations.append("%s: expected a current-ABI statement matching %r" % (rel, pattern))
            continue
        for hit in hits:
            checked += 1
            if int(hit) != abi:
                violations.append("%s: states ABI %s, abi/src/lib.rs says %d" % (rel, hit, abi))
    for line in violations:
        print("  VIOLATION %s" % line)
    print("abi refs: %d checked, %d violation(s), current=%d" % (checked, len(violations), abi))
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
