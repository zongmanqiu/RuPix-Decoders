#!/usr/bin/env python3
"""Build release ZIPs: DLL + <id>_<version>/ metadata, license, notices, source offer.

The ZIP is the download unit. `index.json` still describes the DLL (size/hash);
the ZIP hash is computed after packaging and is not written inside itself.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT.parent / ".temp" / "decoder-dist"
OUT = ROOT.parent / ".temp" / "decoder-packages"
INDEX = ROOT / "index.json"
LICENSE = ROOT / "LICENSE"
SOURCE_REPO = "https://gitee.com/qiuzongman/RuPix-Decoder"
SOURCE_ISSUES = SOURCE_REPO + "/issues"
RAWLER_VERSION = "0.7.2"
RAWLER_COMMIT = "ff7a4524cc7187554544269482e5fd5e68a6b4ab"
RAWLER_CRATE_SHA256 = "04f4cc35c23969a4a834e0b117c7da41ace812eb9053b5effc3fc5c77d114677"

TIERS = {
    "common": ("rupix-common", ROOT / "decoders-rust"),
    "modern": ("rupix-modern", ROOT / "decoders-rust"),
    "raw": ("rupix-raw", ROOT / "decoders-rust"),
    "pro": ("rupix-pro", ROOT / "decoders-rust" / "decoders" / "pro"),
}


def license_ids(expr: str):
    text = expr.replace("/", " OR ")
    text = re.sub(r"[()]", " ", text)
    return [x.strip() for x in re.split(r"\s+(?:OR|AND)\s+", text) if x.strip()]


def metadata_for(cwd):
    data = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(cwd / "Cargo.toml")],
        cwd=ROOT, text=True, encoding="utf-8"))
    return {(p["name"], p["version"]): p for p in data["packages"]}


def deps_for(cwd, package):
    tree = subprocess.check_output(
        ["cargo", "tree", "-p", package, "--edges", "normal", "--prefix", "none"],
        cwd=cwd, text=True, encoding="utf-8")
    packages = metadata_for(cwd)
    out = []
    for line in tree.splitlines():
        m = re.match(r"([A-Za-z0-9_.-]+) v([^ ]+)", line.strip())
        if not m:
            continue
        key = (m.group(1), m.group(2))
        pkg = packages.get(key)
        if pkg is None or Path(pkg["manifest_path"]).resolve().is_relative_to(ROOT):
            continue
        if key not in out:
            out.append(key)
    return out, packages


def chosen_license_ids(expr: str):
    """For a plain OR expression, pick one branch (MIT/Apache preferred). AND terms
    must all be represented; only the OR alternatives are optional."""
    ids = license_ids(expr)
    if " OR " in expr and " AND " not in expr:
        preferred = ("MIT", "Apache-2.0", "BSD-3-Clause", "BSD-2-Clause", "Zlib",
                     "0BSD", "Unlicense", "AGPL-3.0-only", "AGPL-3.0", "LGPL-2.1",
                     "MPL-2.0", "CC0-1.0", "IJG")
        for p in preferred:
            if p in ids:
                return [p]
    return ids

def source_offer(tier: str, version: str, dll_name: str) -> str:
    tag = f"{tier}_{version}"
    source = f"{SOURCE_REPO}/tree/{tag}/decoders-rust/decoders/{tier}"
    parts = [f"""# Source offer for `{dll_name}`

This DLL is distributed under AGPL-3.0. The corresponding source for this
decoder is available at:

- {source}
- tag: `{tag}`

This package does not contain a copy of the source. If the link above is not
usable, request the source at {SOURCE_ISSUES}. We will provide the complete
corresponding source for this decoder for at least three years from the date
you received this package, at no charge beyond the cost of delivery.

This file records the offer; it is not a legal opinion.
"""]
    if tier == "raw":
        parts.append(f"""
## rawler

`raw` statically links `rawler {RAWLER_VERSION}` (LGPL-2.1). Upstream source:

- https://github.com/dnglab/dnglab/tree/{RAWLER_COMMIT}/rawler
- crate: https://crates.io/crates/rawler/{RAWLER_VERSION}
- Cargo.lock checksum: `{RAWLER_CRATE_SHA256}`

A copy of LGPL-2.1 is in `licenses/LGPL-2.1.txt`. The written offer above also
covers the `rawler` source and the materials needed to rebuild `raw` with a
modified `rawler`; request them at {SOURCE_ISSUES}. This package does not bundle
`rawler` source.
""")
    return "\n".join(parts)


def license_text(pkg_dir: Path, lid: str):
    names = {
        "MIT": ("LICENSE-MIT", "LICENSE-MIT.md", "LICENSE"),
        "Apache-2.0": ("LICENSE-APACHE", "LICENSE-APACHE.md", "LICENSE"),
        "BSD-2-Clause": ("LICENSE-BSD", "LICENSE", "LICENSE.md"),
        "BSD-3-Clause": ("LICENSE.md", "LICENSE", "LICENSE-BSD"),
        "Zlib": ("LICENSE-ZLIB", "LICENSE"),
        "0BSD": ("LICENSE-0BSD",),
        "Unlicense": ("UNLICENSE", "LICENSE"),
        "Unicode-3.0": ("LICENSE-UNICODE",),
        "CC0-1.0": ("LICENSE-CC0", "LICENSE-CC0.md", "LICENSE"),
        "IJG": ("LICENSE-IJG", "LICENSE"),
        "MPL-2.0": ("LICENSE", "LICENSE.md"),
        "LGPL-2.1": ("LICENSE", "COPYING", "LICENSE.md"),
        "AGPL-3.0": ("LICENSE-AGPL3", "LICENSE"),
        "AGPL-3.0-only": ("LICENSE-AGPL3", "LICENSE"),
        "ISC": ("LICENSE", "LICENSE.md"),
    }
    for name in names.get(lid, ("LICENSE", "LICENSE.md", "COPYING", "UNLICENSE")):
        p = pkg_dir / name
        if p.is_file() and p.stat().st_size:
            return p
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="", help="comma-separated decoder ids")
    ap.add_argument("--out", default=str(OUT))
    args = ap.parse_args()
    only = {x.strip() for x in args.only.split(",") if x.strip()}
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    entries = json.loads(INDEX.read_text(encoding="utf-8"))
    by_id = {e["id"]: e for e in entries}
    built = 0
    for tier, (package, cwd) in TIERS.items():
        if only and tier not in only:
            continue
        entry = by_id.get(tier)
        if entry is None:
            print("skip %s: no index entry" % tier)
            continue
        missing = [key for key in ("version", "size", "sha256", "released", "gitee", "github")
                   if not entry.get(key)]
        if missing:
            raise SystemExit(
                "refusing to package %s: index.json has empty %s; "
                "set the release URL/date before packaging" % (tier, ", ".join(missing))
            )
        ver = entry["version"]
        dll_name = f"{tier}_{ver}.dll"
        dll = DIST / dll_name
        if not dll.is_file():
            raise SystemExit("missing DLL: %s (run build.ps1 first)" % dll)
        deps, packages = deps_for(cwd, package)
        pkg_name = f"{tier}_{ver}"
        stage = out / (pkg_name + ".stage")
        if stage.exists():
            shutil.rmtree(stage)
        folder = stage / pkg_name
        folder.mkdir(parents=True)
        shutil.copy2(dll, stage / dll_name)
        shutil.copy2(LICENSE, folder / "LICENSE")
        (folder / "metadata.json").write_text(
            json.dumps(entry, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
        lic_dir = folder / "licenses"
        lic_dir.mkdir()
        ids = sorted({lid for key in deps for lid in chosen_license_ids(packages[key].get("license") or "")})
        for lid in ids:
            if lid == "LicenseRef-Imazen-Commercial":
                continue
            found = None
            for key in deps:
                p = packages[key]
                if lid not in chosen_license_ids(p.get("license") or ""):
                    continue
                found = license_text(Path(p["manifest_path"]).parent, lid)
                if found:
                    break
            if found is None and lid == "CC0-1.0":
                fallback = Path.home() / ".cargo/registry/src/rsproxy.cn-e3de039b2554c837/constant_time_eq-0.4.2/LICENSE-CC0"
                if fallback.is_file():
                    found = fallback
            if found is None and lid == "Unlicense":
                fallback = Path.home() / ".cargo/registry/src/rsproxy.cn-e3de039b2554c837/byteorder-lite-0.1.0/UNLICENSE"
                if fallback.is_file():
                    found = fallback
            if found is None:
                raise SystemExit(f"{tier}: no license text for {lid}")
            shutil.copy2(found, lic_dir / (lid + ".txt"))
        rows = []
        for n, v in sorted(deps):
            p = packages[(n, v)]
            rows.append(f"| `{n}` | {v} | {p.get('license') or p.get('license_file') or 'see package'} |")
        notices = f"""# Third-party notices for `{dll_name}`

This DLL is distributed under AGPL-3.0. Third-party crates remain under their
own licenses. Full license texts are in `licenses/`.

| Crate | Version | License expression |
|---|---:|---|
""" + "\n".join(rows) + "\n"
        (folder / "THIRD-PARTY-NOTICES.md").write_text(notices, encoding="utf-8", newline="\n")
        (folder / "SOURCE-OFFER.md").write_text(
            source_offer(tier, ver, dll_name), encoding="utf-8", newline="\n")
        zip_path = out / (pkg_name + ".zip")
        with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, strict_timestamps=False) as z:
            z.write(stage / dll_name, dll_name)
            for p in sorted(folder.rglob("*")):
                if p.is_file():
                    z.write(p, p.relative_to(stage))
        digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
        print(f"{zip_path}  {zip_path.stat().st_size} B  sha256={digest}  deps={len(deps)} licenses={len(ids)}")
        built += 1
    if not built:
        raise SystemExit("no packages built")
    return 0


if __name__ == "__main__":
    sys.exit(main())
