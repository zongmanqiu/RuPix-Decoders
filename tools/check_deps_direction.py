#!/usr/bin/env python3
"""Dependency-direction gate, based on `cargo metadata` rather than text grep.

Rules:
  abi                 -> no in-repo path dependencies (it is the leaf)
  router/pro-support  -> only abi
  singles             -> only abi + pro-support; never a tier or another single
  decoders/<tier>     -> abi/router/pro-support + singles (pro: pro-* only)
  nothing             -> a tier from another tier

Exit 0 clean, 1 on any violation.
"""

import json
import subprocess
import sys
from pathlib import Path

from lib import ROOT
MANIFESTS = [
    (ROOT / "decoders-rust" / "Cargo.toml", "root"),
    (ROOT / "decoders-rust" / "singles" / "Cargo.toml", "singles"),
    (ROOT / "decoders-rust" / "decoders" / "pro" / "Cargo.toml", "pro"),
]


def metadata(manifest: Path):
    cmd = [
        "cargo", "metadata", "--no-deps", "--format-version", "1",
        "--manifest-path", str(manifest),
    ]
    res = subprocess.run(cmd, cwd=str(ROOT), capture_output=True, text=True, encoding="utf-8")
    if res.returncode != 0:
        raise RuntimeError("%s: cargo metadata failed:\n%s" % (manifest, res.stderr))
    return json.loads(res.stdout)


def rel(path: str):
    try:
        return Path(path).resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return None


def allowed_for(name: str, path: str):
    """Return the set of allowed in-repo dependency package names, or None if not classified."""
    p = rel(path) or ""
    if p == "abi":
        return set()
    if p in ("decoders-rust/shared/router", "decoders-rust/shared/pro-support"):
        return {"rupix-abi"}
    if p.startswith("decoders-rust/singles/"):
        return {"rupix-abi", "rupix-pro-support"}
    if p.startswith("decoders-rust/decoders/"):
        if p.startswith("decoders-rust/decoders/pro"):
            return {"rupix-abi", "rupix-merge-router", "rupix-pro-support"}
        return {"rupix-abi", "rupix-merge-router", "rupix-pro-support"}
    return None


def main():
    violations = []
    count = 0
    seen = set()
    for manifest, label in MANIFESTS:
        if not manifest.is_file():
            violations.append("%s: manifest missing" % label)
            continue
        try:
            data = metadata(manifest)
        except RuntimeError as exc:
            print("  VIOLATION %s" % exc)
            return 1
        for pkg in data.get("packages", []):
            name = pkg.get("name")
            manifest_path = pkg.get("manifest_path", "")
            if name in seen:
                continue
            seen.add(name)
            count += 1
            pkg_path = rel(manifest_path)
            allowed = allowed_for(name, manifest_path)
            if allowed is None:
                continue
            for dep in pkg.get("dependencies", []):
                dep_path = dep.get("path")
                if not dep_path:
                    continue
                dep_name = dep.get("name")
                dep_rel = rel(dep_path) or dep_path
                if dep_name in allowed:
                    continue
                # A tier may assemble singles; pro only assembles pro-* singles.
                if pkg_path.startswith("decoders-rust/decoders/pro"):
                    if dep_rel.startswith("decoders-rust/singles/pro-"):
                        continue
                elif pkg_path.startswith("decoders-rust/decoders/") and dep_rel.startswith("decoders-rust/singles/"):
                    continue
                violations.append(
                    "%s (%s) -> %s (%s): not allowed by direction rules"
                    % (name, pkg_path, dep_name, dep_rel)
                )
    for v in violations:
        print("  VIOLATION %s" % v)
    print("packages: %d, violations: %d" % (count, len(violations)))
    return 1 if violations else 0


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except AttributeError:
        pass
    sys.exit(main())
