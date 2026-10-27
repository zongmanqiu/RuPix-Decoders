"""Shared helpers for the master CLI and optional per-format runs."""

from __future__ import annotations

import argparse
from pathlib import Path

from .spec import DEFAULT_SEED, DEFAULT_TIER, TIERS


def add_common_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--tier",
        choices=[*TIERS, "all"],
        default=DEFAULT_TIER,
        help=f"target size tier (default: {DEFAULT_TIER})",
    )
    parser.add_argument("--out", type=Path, default=None, help="output root")
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--strict", action="store_true", help="fail on missing tools/writers")


def selected_tiers(name: str) -> list[tuple[str, int]]:
    if name == "all":
        return list(TIERS.items())
    return [(name, TIERS[name])]


def resolve_out(arg: Path | None, default: Path) -> Path:
    return (arg if arg is not None else default).expanduser().resolve()
