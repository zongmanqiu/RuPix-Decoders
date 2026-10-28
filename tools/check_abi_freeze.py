#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Fail when a *published* ABI item changes instead of being added.

The contract makes a permanent promise (abi/src/lib.rs, "compatibility pact"):
a decoder DLL that once loaded in RuPix must keep loading and behaving the same
after every host upgrade, without being recompiled or re-uploaded.  That only
holds if no published item is ever modified, so this gate compares the live
contract against a frozen fingerprint:

  * frozen export signature / struct field / error code / enum value changed
    or disappeared            -> VIOLATION (breaking change)
  * IV_ABI_VERSION went down  -> VIOLATION (version never decreases)
  * a brand new item appears  -> allowed, counted as "new"

Run ``--update`` only after *adding* something, to re-freeze the baseline.  The
diff of tools/abi_freeze.txt is then the human-readable record of what the
contract gained; it must never contain a changed or removed old line.

ASCII only on purpose: the build prints this under a GBK-era console.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ABI_SOURCE = ROOT / "abi" / "src" / "lib.rs"
BASELINE = ROOT / "tools" / "abi_freeze.txt"

# Known spellings of the same C type collapse to one token, so moving a type
# from `std::ffi` to `core::ffi` (a pure path refactor) does not trip the gate.
TYPE_ALIASES = (
    ("std::ffi::c_char", "c_char"),
    ("core::ffi::c_char", "c_char"),
    ("std::ffi::c_void", "c_void"),
    ("core::ffi::c_void", "c_void"),
)

STRUCT_RE = re.compile(r"#\[repr\(C\)\][\s\S]*?pub struct (\w+)\s*\{([^}]*)\}")
FIELD_RE = re.compile(r"^\s*pub\s+(\w+)\s*:\s*([^,]+),", re.M)
CONST_RE = re.compile(
    r"pub const ((?:ERR|PF|CAP|REQ|METADATA|IV_ABI)_\w*)\s*:\s*\w+\s*=\s*([^;]+);"
)
LAYOUT_RE = re.compile(r"assert!\(\s*size_of::<(\w+)>\(\)\s*==\s*(\d+)\s*\)")
EXPORT_RE = re.compile(r'pub extern "C" fn (iv_\w+)\s*\(([^)]*)\)\s*->\s*([^\{]+)\{')
FNTYPE_RE = re.compile(
    r'pub type (Iv\w+Fn)\s*=\s*(?:unsafe\s+)?extern "C" fn\s*\(([^)]*)\)\s*->\s*([^;]+);'
)

KIND_ORDER = ("abi", "layout", "struct", "const", "type", "export")


def norm(text):
    """One canonical spelling for a type expression."""
    t = re.sub(r"\s+", " ", text).strip().rstrip(",").strip()
    t = t.replace("$crate::", "")
    for src, dst in TYPE_ALIASES:
        t = t.replace(src, dst)
    return t


def eval_int(expr):
    """`7`, `1 << 6` -> int; anything else -> None (unfreezable, skip)."""
    e = norm(expr)
    m = re.fullmatch(r"(\d+)\s*<<\s*(\d+)", e)
    if m:
        return int(m.group(1)) << int(m.group(2))
    if re.fullmatch(r"\d+", e):
        return int(e)
    return None


def params(text):
    out = []
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        name, _, ty = part.partition(":")
        out.append("%s:%s" % (name.strip(), norm(ty)))
    return ", ".join(out)


def fingerprint(text):
    """-> {kind: {key: value}} for every published item in the contract."""
    items = {kind: {} for kind in KIND_ORDER}

    match = re.search(r"IV_ABI_VERSION\s*:\s*u32\s*=\s*(\d+)", text)
    items["abi"][""] = match.group(1) if match else None

    for name, size in LAYOUT_RE.findall(text):
        items["layout"][name] = size

    for name, body in STRUCT_RE.findall(text):
        fields = ["%s:%s" % (f, norm(t)) for f, t in FIELD_RE.findall(body)]
        items["struct"][name] = " ".join(fields)

    for name, expr in CONST_RE.findall(text):
        value = eval_int(expr)
        if value is not None:
            items["const"][name] = str(value)

    for name, arglist, ret in FNTYPE_RE.findall(text):
        items["type"][name] = "(%s)->%s" % (params(arglist), norm(ret))

    for name, arglist, ret in EXPORT_RE.findall(text):
        items["export"][name] = "(%s)->%s" % (params(arglist), norm(ret))

    return items


def render(items):
    lines = [
        "# Frozen ABI fingerprint - one line per published item.",
        "# A published line may NEVER change or disappear; only new lines may be",
        "# appended (run: python tools/check_abi_freeze.py --update).",
        "# Guarded by tools/check_abi_freeze.py inside workspace/build.ps1.",
    ]
    for kind in KIND_ORDER:
        for key in sorted(items[kind]):
            value = items[kind][key]
            if kind == "abi":
                lines.append("abi=%s" % value)
            elif key:
                lines.append("%s %s%s" % (kind, key, "=" + value if kind in ("layout", "const") else " " + value))
    return "\n".join(lines) + "\n"


def parse_baseline(text):
    items = {kind: {} for kind in KIND_ORDER}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        head, _, rest = line.partition(" ")
        if head.startswith("abi="):
            items["abi"][""] = head.split("=", 1)[1]
            continue
        if head in ("layout", "const"):
            name, _, value = rest.partition("=")
            items[head][name.strip()] = value.strip()
        elif head in ("struct", "type", "export"):
            name, _, value = rest.partition(" ")
            items[head][name.strip()] = value.strip()
    return items


def main(argv):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    text = ABI_SOURCE.read_text(encoding="utf-8")
    current = fingerprint(text)
    if current["abi"][""] is None:
        print("  VIOLATION abi/src/lib.rs: IV_ABI_VERSION not found")
        return 1

    if "--update" in argv:
        BASELINE.write_text(render(current), encoding="utf-8", newline="\n")
        print("abi freeze: baseline written to %s" % BASELINE)
        return 0

    if not BASELINE.is_file():
        print("  VIOLATION %s missing (python tools/check_abi_freeze.py --update)" % BASELINE)
        print("abi freeze: 0 frozen entries, 1 violation(s), abi=%s" % current["abi"][""])
        return 1

    frozen = parse_baseline(BASELINE.read_text(encoding="utf-8"))
    violations = []
    checked = 0

    base_abi = int(frozen["abi"].get("", "0") or 0)
    now_abi = int(current["abi"][""])
    checked += 1
    if now_abi < base_abi:
        violations.append("abi: IV_ABI_VERSION %d < frozen %d (version never decreases)" % (now_abi, base_abi))

    for kind in KIND_ORDER[1:]:
        for key, value in sorted(frozen[kind].items()):
            checked += 1
            live = current[kind].get(key)
            if live is None:
                violations.append("%s %s: frozen item is gone" % (kind, key))
            elif live != value:
                violations.append("%s %s: frozen %r -> now %r" % (kind, key, value, live))

    new = sum(
        1 for kind in KIND_ORDER[1:] for key in current[kind] if key not in frozen[kind]
    )
    for line in violations:
        print("  VIOLATION %s" % line)
    print(
        "abi freeze: %d frozen, %d new (allowed), %d violation(s), abi=%s"
        % (checked, new, len(violations), current["abi"][""])
    )
    if new and not violations:
        print("  note: %d item(s) are newer than the baseline; re-freeze with --update once reviewed" % new)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
