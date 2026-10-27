#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Suffix-claim and evidence gate for the RuPix decoder repo.

Two rules:
  1. every `extensions: "a;b;c"` literal in workspace/decoders/*/src/lib.rs is disjoint
     across tiers (a suffix has exactly one claiming tier);
  2. every claimed suffix appears exactly once in testdata/claims.tsv, and every
     ledger row is either:
       verified   -> evidence is `golden:<file>` (file exists and is referenced
                     by a .rs file) or `tests:<workspace/singles package>` (that
                     package has at least one #[test]);
       unverified -> evidence is `-` and a non-empty reason is recorded.

The ledger is the human-readable debt list: current gaps are allowed, new
claims are not allowed to enter without evidence or an explicit unverified row.
Exit code: 0 clean, 1 violations.
"""

import re
import sys
from pathlib import Path

from lib import ROOT
TIERS = ROOT / "workspace" / "decoders"
LEDGER = ROOT / "testdata" / "claims.tsv"
GOLDEN = ROOT / "testdata" / "golden"
EXT_LITERAL = re.compile(r'extensions\s*:\s*"([^"]*)"')
STATUSES = {"verified", "unverified"}


def parse_exts(literal):
    literal = literal.replace("\\\n", "").replace("\n", "")
    return [e.strip().strip(".").lower() for e in literal.split(";") if e.strip()]


def scan():
    claims = {}
    scanned = []
    for lib in sorted(TIERS.glob("*/src/lib.rs")):
        tier = lib.parent.parent.name
        text = lib.read_text(encoding="utf-8", errors="replace")
        found = EXT_LITERAL.findall(text)
        if not found:
            continue
        scanned.append(tier)
        for lit in found:
            for ext in parse_exts(lit):
                claims.setdefault(ext, []).append(tier)
    return scanned, claims


def load_ledger():
    if not LEDGER.is_file():
        return [], ["%s does not exist" % LEDGER.relative_to(ROOT)]
    rows = []
    errors = []
    for n, raw in enumerate(LEDGER.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) != 6:
            errors.append("claims.tsv:%d: expected 6 tab-separated columns, got %d"
                          % (n, len(parts)))
            continue
        tier, member, suffixes, status, evidence, note = [p.strip() for p in parts]
        rows.append({
            "line": n,
            "tier": tier,
            "member": member,
            "suffixes": parse_exts(suffixes.replace(" ", ";")),
            "status": status,
            "evidence": evidence,
            "note": note,
        })
    return rows, errors


def rust_sources():
    return list((ROOT / "workspace" / "decoders").rglob("*.rs")) + list((ROOT / "workspace" / "singles").rglob("*.rs"))


def member_is_known(tier, member):
    """The ledger's member name must exist in that tier's lib.rs or Cargo.toml."""
    tier_dir = TIERS / tier
    haystack = ""
    for name in ("src/lib.rs", "Cargo.toml"):
        path = tier_dir / name
        if path.is_file():
            haystack += path.read_text(encoding="utf-8", errors="replace")
    return member in haystack


def golden_is_referenced(name):
    needle = "testdata/golden/" + name
    for path in rust_sources():
        if needle in path.read_text(encoding="utf-8", errors="replace"):
            return True
    return False


def package_test_count(package):
    for manifest in sorted((ROOT / "workspace" / "singles").glob("*/Cargo.toml")):
        text = manifest.read_text(encoding="utf-8", errors="replace")
        m = re.search(r'(?m)^name\s*=\s*"([^"]+)"', text)
        if not m or m.group(1) != package:
            continue
        count = 0
        for path in sorted(manifest.parent.rglob("*.rs")):
            count += len(re.findall(r"#\[test\]", path.read_text(encoding="utf-8", errors="replace")))
        return count
    return None


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    scanned, claims = scan()
    rows, bad = load_ledger()
    if not scanned:
        bad.append("workspace/decoders/*/src/lib.rs has no extensions: literal - scanner is blind")

    seen = {}
    for row in rows:
        if not row["member"]:
            bad.append("claims.tsv:%d: empty member" % row["line"])
        elif not member_is_known(row["tier"], row["member"]):
            bad.append("claims.tsv:%d: member %r is not in decoders/%s/src/lib.rs or Cargo.toml"
                       % (row["line"], row["member"], row["tier"]))
        if row["status"] not in STATUSES:
            bad.append("claims.tsv:%d: status must be verified|unverified" % row["line"])
        if not row["suffixes"]:
            bad.append("claims.tsv:%d: empty suffix list" % row["line"])
        if row["status"] == "verified":
            ev = row["evidence"]
            if ev.startswith("golden:"):
                name = ev.split(":", 1)[1]
                path = GOLDEN / name
                if not path.is_file():
                    bad.append("claims.tsv:%d: golden missing: %s" % (row["line"], name))
                elif not golden_is_referenced(name):
                    bad.append("claims.tsv:%d: golden is not referenced by any .rs: %s"
                               % (row["line"], name))
            elif ev.startswith("tests:"):
                pkg = ev.split(":", 1)[1]
                count = package_test_count(pkg)
                if count is None:
                    bad.append("claims.tsv:%d: no workspace/singles package %s" % (row["line"], pkg))
                elif count == 0:
                    bad.append("claims.tsv:%d: package %s has no #[test]" % (row["line"], pkg))
            else:
                bad.append("claims.tsv:%d: verified needs golden:<file> or tests:<package>"
                           % row["line"])
        else:
            if row["evidence"] != "-":
                bad.append("claims.tsv:%d: unverified evidence must be '-'" % row["line"])
            if not row["note"]:
                bad.append("claims.tsv:%d: unverified needs a reason in note" % row["line"])
        for ext in row["suffixes"]:
            if ext in seen:
                bad.append("claims.tsv:%d: duplicate suffix .%s (also line %d)"
                           % (row["line"], ext, seen[ext]))
            seen[ext] = row["line"]
            tiers = claims.get(ext, [])
            if not tiers:
                bad.append("claims.tsv:%d: .%s is not claimed by any tier" % (row["line"], ext))
            elif row["tier"] not in tiers:
                bad.append("claims.tsv:%d: .%s is claimed by %s, ledger says %s"
                           % (row["line"], ext, ",".join(tiers), row["tier"]))
            elif len(tiers) > 1:
                bad.append(".%s is claimed by multiple tiers: %s" % (ext, ", ".join(tiers)))

    for ext, tiers in sorted(claims.items()):
        if ext not in seen:
            bad.append(".%s is claimed by %s but missing from claims.tsv"
                       % (ext, ", ".join(tiers)))

    verified = sum(1 for row in rows if row["status"] == "verified"
                   for _ in row["suffixes"])
    unverified = sum(1 for row in rows if row["status"] == "unverified"
                     for _ in row["suffixes"])
    for line in bad:
        print("  VIOLATION %s" % line)
    print("tiers: %d, suffixes: %d, verified: %d, unverified: %d, violations: %d"
          % (len(scanned), len(claims), verified, unverified, len(bad)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
