#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Single-point check for the three cargo workspaces' release configuration.

The repo deliberately keeps three workspaces (tier / bench / pro) for feature
isolation. Their target dirs and the static-CRT flag must not drift, and the
release profile differences must be exactly the intended ones (tiers/pro:
opt-level=3; bench: z + per-package overrides).

This is a validator, not a generator: Cargo has no way to share a profile
across independent workspaces, so the next best thing is one gate that reads
all three files. Exit 0 clean, 1 on any drift.
"""

import sys
import tomllib
from pathlib import Path

from lib import ROOT
EXPECTED = {
    "root": {
        "config": ROOT / "decoders-rust" / ".cargo" / "config.toml",
        "target": "../../.temp/decoder-target",
        "opt": 3,
    },
    "singles": {
        "config": ROOT / "decoders-rust" / "singles" / ".cargo" / "config.toml",
        "target": "../../../.temp/singles-target",
        "opt": "z",
    },
    "pro": {
        "config": ROOT / "decoders-rust" / "decoders" / "pro" / ".cargo" / "config.toml",
        "target": "../../../../.temp/pro-target",
        "opt": 3,
    },
}
COMMON = {
    "lto": True,
    "codegen-units": 1,
    "strip": True,
    "panic": "unwind",
    "debug": 1,
}


def check(name, spec):
    out = []
    path = spec["config"]
    if not path.is_file():
        return ["%s: %s missing" % (name, path.relative_to(ROOT))]
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        return ["%s: %s is not valid TOML: %s" % (name, path.relative_to(ROOT), exc)]
    build = data.get("build", {})
    if build.get("target-dir") != spec["target"]:
        out.append("%s: target-dir %r != %r"
                   % (name, build.get("target-dir"), spec["target"]))
    flags = data.get("target", {}).get("x86_64-pc-windows-msvc", {}).get("rustflags", [])
    if "-C" not in flags or "target-feature=+crt-static" not in flags:
        out.append("%s: missing +crt-static rustflags: %r" % (name, flags))
    return out


def check_profiles():
    out = []
    for name, spec in EXPECTED.items():
        manifest = {
            "root": ROOT / "decoders-rust" / "Cargo.toml",
            "singles": ROOT / "decoders-rust" / "singles" / "Cargo.toml",
            "pro": ROOT / "decoders-rust" / "decoders" / "pro" / "Cargo.toml",
        }[name]
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        profile = data.get("profile", {}).get("release", {})
        if profile.get("opt-level") != spec["opt"]:
            out.append("%s: opt-level %r != %r"
                       % (name, profile.get("opt-level"), spec["opt"]))
        for key, value in COMMON.items():
            if profile.get(key) != value:
                out.append("%s: profile.release.%s %r != %r"
                           % (name, key, profile.get(key), value))
    return out


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    bad = []
    for name, spec in EXPECTED.items():
        bad += check(name, spec)
    bad += check_profiles()
    for line in bad:
        print("  VIOLATION %s" % line)
    print("workspaces: %d, violations: %d" % (len(EXPECTED), len(bad)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
