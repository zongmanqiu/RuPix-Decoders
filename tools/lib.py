"""Shared helpers for the decoder repo's Python gates.

Keep this tiny: only the repo root and the common violation printer live here.
Gate-specific parsing stays in its own script so a failure points at one file.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def print_violations(violations):
    for line in violations:
        print("  VIOLATION %s" % line)
