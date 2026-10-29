#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Single-point check for every cargo workspace's release configuration.

The repo deliberately keeps one workspace per build graph (root tier /
bench / pro / classic / doc / doc-legacy / texture) so features never unify
across tiers. Their target dirs, target triples and static-CRT flags must not
drift; release profile differences must be exactly the intended ones (root,
pro, classic, texture, doc-legacy: opt-level=3; doc: "s" for size; bench:
z + per-package overrides).

This is a validator, not a generator: Cargo has no way to share a profile
across independent workspaces, so the next best thing is one gate that reads
all the manifests. Exit 0 clean, 1 on any drift.
"""

import sys
import tomllib
from pathlib import Path

from lib import ROOT
EXPECTED = {
    "root": {
        "config": ROOT / "workspace" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "Cargo.toml",
        "target": "../../.temp/decoder-target",
        "opt": 3,
    },
    "singles": {
        "config": ROOT / "workspace" / "singles" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "singles" / "Cargo.toml",
        "target": "../../../.temp/singles-target",
        "opt": "z",
    },
    "pro": {
        "config": ROOT / "workspace" / "decoders" / "pro" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "decoders" / "pro" / "Cargo.toml",
        "target": "../../../../.temp/pro-target",
        "opt": 3,
    },
    "classic": {
        "config": ROOT / "workspace" / "decoders" / "classic" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "decoders" / "classic" / "Cargo.toml",
        "target": "../../../../.temp/classic-target",
        "opt": 3,
    },
    "doc": {
        "config": ROOT / "workspace" / "decoders" / "doc" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "decoders" / "doc" / "Cargo.toml",
        "target": "../../../../.temp/doc-target",
        "opt": "s",
    },
    "doc-legacy": {
        "config": ROOT / "workspace" / "decoders" / "doc-legacy" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "decoders" / "doc-legacy" / "Cargo.toml",
        "target": "../../../../.temp/doc-legacy-target",
        "opt": 3,
        "triple": "x86_64-pc-windows-gnu",
        "static_crt": False,
    },
    "texture": {
        "config": ROOT / "workspace" / "decoders" / "texture" / ".cargo" / "config.toml",
        "manifest": ROOT / "workspace" / "decoders" / "texture" / "Cargo.toml",
        "target": "../../../../.temp/texture-target",
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
    if build.get("target") != spec.get("triple"):
        out.append("%s: build.target %r != %r"
                   % (name, build.get("target"), spec.get("triple")))
    if spec.get("static_crt", True):
        flags = data.get("target", {}).get("x86_64-pc-windows-msvc", {}).get("rustflags", [])
        if "-C" not in flags or "target-feature=+crt-static" not in flags:
            out.append("%s: missing +crt-static rustflags: %r" % (name, flags))
    return out


def check_profiles():
    out = []
    for name, spec in EXPECTED.items():
        manifest = spec["manifest"]
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
