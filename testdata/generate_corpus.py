#!/usr/bin/env python3
"""Generate the synthetic multi-format test corpus.

Default invocation writes the 10M tier, all registered suffixes. The output
root is outside the source tree. Generated formats do not read a seed; the
DjVu writer is the explicit exception and reads the checked-in public-domain
fixture `golden/djvu_pd_scan.djvu` because no portable encoder is available.
A format module is imported only when its suffix is selected, so missing
HEIF/JXL/RAW tooling does not break `--only png`.

The writer contract is `write(request) -> corpus.spec.Generated`; the registry
is explicit in `corpus/registry.py`. Formats that cannot be generated and have
no redistributable sample are declared in `GENERATION_EXCEPTIONS.md` and
`corpus/registry.py::GENERATION_EXCEPTIONS`; they are never replaced by a
zero-byte or renamed file.
"""

from __future__ import annotations

import argparse
import importlib
import sys
from pathlib import Path

from corpus import manifest, registry
from corpus.cli_common import add_common_args, resolve_out, selected_tiers
from corpus.spec import DEFAULT_SEED, DEFAULT_TIER, Request


DEFAULT_OUT = Path(__file__).resolve().parents[2] / ".temp" / "testdata"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    add_common_args(parser)
    parser.add_argument("--only", default="", help="comma-separated suffixes")
    parser.add_argument("--list", action="store_true", help="list registered suffixes")
    parser.add_argument("--force", action="store_true", help="overwrite existing files")
    args = parser.parse_args(argv)
    args.out = resolve_out(args.out, DEFAULT_OUT)
    try:
        args.only_entries = registry.select(args.only or None)
    except ValueError as exc:
        parser.error(str(exc))
    return args


def list_formats() -> None:
    print(f"{len(registry.REGISTRY)} suffixes")
    for e in registry.REGISTRY:
        suffix = f"  # {e.note}" if e.note else ""
        if e.ext in registry.GENERATION_EXCEPTIONS:
            suffix = f"  # NOT PUBLIC: {registry.GENERATION_EXCEPTIONS[e.ext]}"
        print(f"{e.ext:8} {e.phase:2} {e.family:12} {e.import_path}{suffix}")


def load_writer(entry: registry.FormatEntry):
    return importlib.import_module(entry.import_path)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.list:
        list_formats()
        return 0

    rows: list[dict[str, object]] = []
    missing: list[str] = []
    for tier_name, target in selected_tiers(args.tier):
        tier_dir = args.out / tier_name
        tier_dir.mkdir(parents=True, exist_ok=True)
        for entry in args.only_entries:
            if entry.ext in registry.GENERATION_EXCEPTIONS:
                reason = registry.GENERATION_EXCEPTIONS[entry.ext]
                missing.append(f"{entry.ext}: {reason}")
                continue
            try:
                writer = load_writer(entry)
            except ModuleNotFoundError:
                missing.append(f"{entry.ext} -> {entry.import_path}")
                continue
            request = Request(
                ext=entry.ext,
                tier=tier_name,
                target_bytes=target,
                out_dir=tier_dir,
                seed=args.seed,
                strict=args.strict,
            )
            generated = writer.write(request)
            path = tier_dir / ("noise." + entry.ext)
            if path.exists() and not args.force:
                raise SystemExit(f"refusing to overwrite {path}; pass --force")
            path.write_bytes(generated.data)
            rows.append(manifest.row(request, entry.family, generated))
            print(f"{tier_name:5} {entry.ext:8} {len(generated.data):>10} B")

    if missing:
        print("\nmissing writers:", file=sys.stderr)
        for line in missing:
            print("  " + line, file=sys.stderr)
        if args.strict:
            return 1
    if not rows:
        return 1
    manifest.write(args.out, args.seed, rows)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
