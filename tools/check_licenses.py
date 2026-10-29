#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Supply-chain gate: every resolved package must declare a known license.

`cargo-deny` / `cargo-audit` are not installed on this machine, so this is the
local, dependency-free equivalent for the license half of that check. It reads
the three workspaces' full metadata (including transitive packages), fails on a
missing license, and fails on an SPDX id outside the allowlist. Adding a new
license must be a deliberate edit here and in docs/licensing.md.

Vulnerability advisories are not covered; run `cargo audit` when it is available.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

from lib import ROOT

MANIFESTS = [
    ROOT / "workspace" / "Cargo.toml",
    ROOT / "workspace" / "singles" / "Cargo.toml",
    ROOT / "workspace" / "decoders" / "pro" / "Cargo.toml",
    ROOT / "workspace" / "decoders" / "classic" / "Cargo.toml",
    ROOT / "workspace" / "decoders" / "texture" / "Cargo.toml",
    ROOT / "workspace" / "decoders" / "doc" / "Cargo.toml",
    ROOT / "workspace" / "decoders" / "doc-legacy" / "Cargo.toml",
]
ALLOWED = {
    "0BSD",
    "AGPL-3.0",
    "AGPL-3.0-only",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "IJG",
    "ISC",
    "LGPL-2.1",
    "LGPL-2.1-or-later",
    "LicenseRef-Imazen-Commercial",
    "MIT",
    "MPL-2.0",
    "NCSA",
    "Unicode-3.0",
    "Unlicense",
    "WTFPL",
    "Zlib",
}


def metadata(manifest):
    cmd = [
        "cargo", "metadata", "--format-version", "1",
        "--manifest-path", str(manifest),
    ]
    res = subprocess.run(cmd, cwd=str(ROOT), capture_output=True, text=True, encoding="utf-8")
    if res.returncode != 0:
        raise RuntimeError("%s: cargo metadata failed:\n%s" % (manifest, res.stderr))
    return json.loads(res.stdout)


def ids(expression):
    # Legacy slash expressions (MIT/Apache-2.0) and SPDX OR/AND/parentheses.
    text = expression.replace("/", " OR ")
    text = re.sub(r"[()]", " ", text)
    parts = re.split(r"\s+(?:OR|AND)\s+", text)
    return [re.sub(r"\s+", " ", p).strip() for p in parts if p.strip()]


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    violations = []
    seen = {}
    for manifest in MANIFESTS:
        try:
            data = metadata(manifest)
        except RuntimeError as exc:
            print("  VIOLATION %s" % exc)
            return 1
        for pkg in data.get("packages", []):
            key = (pkg.get("name"), pkg.get("version"))
            if key in seen:
                continue
            seen[key] = True
            license = pkg.get("license") or ""
            license_file = pkg.get("license_file") or ""
            if not license and not license_file:
                violations.append("%s %s: no license / license_file"
                                  % (pkg.get("name"), pkg.get("version")))
                continue
            if license:
                for ident in ids(license):
                    if ident not in ALLOWED:
                        violations.append("%s %s: license id %r not allowed (%s)"
                                          % (pkg.get("name"), pkg.get("version"), ident, license))
    for line in violations:
        print("  VIOLATION %s" % line)
    print("packages: %d, violations: %d" % (len(seen), len(violations)))
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
