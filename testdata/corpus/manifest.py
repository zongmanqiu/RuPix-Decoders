"""Manifest writing and size-uniqueness checks."""

from __future__ import annotations

import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

from .spec import Generated, Request


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def row(request: Request, family: str, generated: Generated) -> dict[str, object]:
    return {
        "ext": request.ext,
        "tier": request.tier,
        "target_bytes": request.target_bytes,
        "bytes": len(generated.data),
        "sha256": sha256(generated.data),
        "width": generated.width,
        "height": generated.height,
        "family": family,
        "capped": generated.capped,
        "note": generated.note,
    }


def write(out_dir: Path, seed: int, rows: list[dict[str, object]]) -> Path:
    """Write a stable, machine-readable manifest beside the tier directories."""
    by_tier: dict[str, dict[int, list[str]]] = {}
    for r in rows:
        by_tier.setdefault(str(r["tier"]), {}).setdefault(int(r["bytes"]), []).append(
            str(r["ext"])
        )
    for tier, sizes in by_tier.items():
        duplicates = {n: exts for n, exts in sizes.items() if len(exts) > 1}
        if duplicates:
            raise ValueError(f"duplicate sizes in {tier}: {duplicates}")

    path = out_dir / "manifest.json"
    payload = {
        "generator": "synthetic-corpus",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "seed": seed,
        "files": sorted(rows, key=lambda r: str(r["ext"])),
    }
    path.write_text(json.dumps(payload, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return path
