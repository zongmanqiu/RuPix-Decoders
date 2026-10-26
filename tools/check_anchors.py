#!/usr/bin/env python3
"""Advisory: which docs line anchors are SUSPICIOUS, i.e. pointing away from the code the
sentence names? Exit code is always 0 - this is a work list for a human, not a gate.

Why not a gate (measured, 2026-10-20, m62): the rule below flagged 48 of 169 judgeable anchors.
A human pass over all 48 found 26 genuinely wrong (now corrected) and 22 false alarms - all the
same shape: "the sentence names symbol X, the anchor points at the code that *uses* X, and X's
definition lives elsewhere". Tightening it (only judge when one uniquely-occurring symbol sits
right before the anchor) cut it to 4 reports but 2 of those 4 were still false alarms. So a hard
gate here would train people to ignore it, which is worse than not having it.

What the machine CAN prove stays in check_docs.py: anchors in range, not on blank lines, and no
raw NUL bytes in any shipped file.

Usage: python tools/check_anchors.py [pid] [window=6]
Read it as: for each line, is the code the sentence named anywhere near the numbers it cites?
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
import check_docs as C  # noqa: E402  (same resolution rules, so the two scripts can't disagree)

IDENT = re.compile(r"`([A-Za-z_][A-Za-z0-9_]{3,})`")


def suspect(pid, window):
    base = C.DOCS / pid
    doc = base / "README.md"
    if not doc.is_file():
        return []
    out = []
    for ln, line in enumerate(doc.read_text(encoding="utf-8").splitlines(), 1):
        named = IDENT.findall(line)
        if not named:
            continue
        for m in C.ANCHOR.finditer(line):
            run = m.group(1)
            cands = C.anchor_candidates(base, run, line, m.start())
            if not cands:
                continue
            path = next((p for p in cands if C.fits(p, C.anchor_spans(run))), cands[0])
            src = path.read_text(encoding="utf-8").splitlines()
            hits = {}
            for name in named:
                pat = re.compile(r"\b%s\b" % re.escape(name))
                rows = [i + 1 for i, s in enumerate(src) if pat.search(s)]
                if rows:
                    hits[name] = rows
            if not hits:
                continue
            for (a, b) in C.anchor_spans(run):
                if a < 1 or b > len(src):
                    continue
                if any(a - window <= n <= b + window for rows in hits.values() for n in rows):
                    continue
                at = src[a - 1].strip()[:60] or "<blank>"
                near = ", ".join("%s@%s" % (k, v[0]) for k, v in list(hits.items())[:3])
                out.append("doc%d :%d-%d  %s\n        NOW %s\n        NAMES %s"
                           % (ln, a, b, run, at, near))
    return out


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    window = int(sys.argv[2]) if len(sys.argv) > 2 else 6
    total = 0
    for pid in C.decoder_ids():
        if only and pid != only:
            continue
        s = suspect(pid, window)
        total += len(s)
        if s:
            print("== %s: %d suspect(s)" % (pid, len(s)))
            for line in s:
                print("   " + line)
    print("suspects=%d (advisory only; each one needs a human to say wrong-anchor or wrong-rule)"
          % total)
    return 0


if __name__ == "__main__":
    sys.exit(main())
