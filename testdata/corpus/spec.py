"""Types shared by the CLI, format modules, and validators."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


DEFAULT_TIER = "10M"
DEFAULT_SEED = 20261016
TIERS: dict[str, int] = {
    "1M": 1 << 20,
    "10M": 10 << 20,
    "100M": 100 << 20,
}


@dataclass(frozen=True)
class Request:
    """One format's generation request."""

    ext: str
    tier: str
    target_bytes: int
    out_dir: Path
    seed: int
    strict: bool = False


@dataclass(frozen=True)
class Generated:
    """Bytes returned by a writer.

    `capped` means the format cannot reach the requested target (for example a
    256px ICO or a 1-bit PBM). `note` is persisted in the manifest.
    """

    data: bytes
    width: int
    height: int
    note: str = ""
    capped: bool = False
