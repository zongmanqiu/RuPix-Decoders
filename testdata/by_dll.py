#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Create a per-DLL view of the generated sample corpus.

The generator itself stays keyed by suffix (one file per suffix per tier). This
script reads `testdata/claims.tsv` to map each suffix to its owning DLL, then
hard-links (or copies) the generated file into:

    <out>/<dll-id>/<tier>/<noise.ext>

No bytes are duplicated when hard links are available. Missing files are
reported, not fabricated.
"""

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLAIMS = ROOT / "testdata" / "claims.tsv"


def suffix_owner():
    owners = {}
    for raw in CLAIMS.read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) != 6:
            continue
        tier, _member, suffixes = parts[0].strip(), parts[1].strip(), parts[2].strip()
        for ext in suffixes.split(";"):
            ext = ext.strip().lstrip(".").lower()
            if ext:
                owners[ext] = tier
    return owners


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--src", type=Path, default=ROOT.parent / ".temp" / "testdata")
    parser.add_argument("--out", type=Path, default=ROOT.parent / ".temp" / "by_dll")
    args = parser.parse_args()

    manifest = args.src / "manifest.json"
    if not manifest.is_file():
        print("missing %s; run: python testdata/make_samples.py" % manifest, file=sys.stderr)
        return 2
    owners = suffix_owner()
    data = json.loads(manifest.read_text(encoding="utf-8"))
    written = 0
    skipped = 0
    per_dll = {}
    for row in data.get("rows", []):
        ext = str(row.get("ext", "")).lstrip(".").lower()
        dll = owners.get(ext)
        source = Path(row.get("file", ""))
        if not dll or not source.is_file():
            skipped += 1
            continue
        tier = str(row.get("tier", "unknown"))
        target_dir = args.out / dll / tier
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / source.name
        if target.exists():
            target.unlink()
        try:
            os.link(source, target)
        except OSError:
            shutil.copyfile(source, target)
        per_dll.setdefault(dll, []).append({
            "tier": tier, "ext": "." + ext, "file": str(target),
            "bytes": target.stat().st_size, "claimed_by": row.get("claimed_by", ""),
        })
        written += 1

    args.out.mkdir(parents=True, exist_ok=True)
    for dll, rows in per_dll.items():
        (args.out / dll).mkdir(parents=True, exist_ok=True)
        (args.out / dll / "manifest.json").write_text(
            json.dumps({"keyed_by": "dll (view over the suffix corpus)", "rows": rows},
                       ensure_ascii=False, indent=1),
            encoding="utf-8",
        )
        print("%-8s %d files" % (dll, len(rows)))
    print("written: %d, skipped: %d" % (written, skipped))
    return 0


if __name__ == "__main__":
    sys.exit(main())
