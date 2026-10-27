#!/usr/bin/env python3
"""Docs coverage gate: every published decoder has docs/<id>.md, README links resolve,
and every `:NNN` line anchor lands on a real, non-blank line of the file it names.

Machine check for the rule in AGENTS.md section 6 step 7: a decoder without a detail doc is
a decoder nobody can maintain. Exits 1 on any miss.

The anchor check was widened on 2026-10-20 (m61): the old regex only matched a whole
backticked run of exactly `:N` or `:N-M`, so comma lists (`:86, 116`) and prefixed anchors
were never looked at, and every anchor was read against src/lib.rs even when the sentence
named 根 Cargo.toml. Measured on this repo: 261 runs resolved before, 373 runs / 678
numbers now (10 of them name a file outside this repo, so they are skipped by design).
Drift nobody can run into is not a gate: build.ps1 had no step that ran this script at all.

Usage: python tools/check_docs.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "workspace" / "decoders"      # per-DLL docs live next to the code
SPEC = ROOT / "解码器开发规范.md"   # ABI contract text, kept at the repo root
DECODERS = ROOT / "workspace" / "decoders"
DOCS_DIR = ROOT / "docs"      # cross-module process/design docs only
MANIFEST = ROOT / "DOCS_MANIFEST.md"
BENCH_README = ROOT / "workspace" / "singles" / "README.md"
# 发布档（decoders/<id>）走**同一套**文档规矩：一枚 DLL 没有文档，维护者就只能
# 靠读代码猜它认领了什么（2026-10-25 m138 立）。单库在 singles/，不发布，
# 不在这道门禁的管辖内。
DECODER_ROOTS = (DECODERS,)
README = ROOT / "README.md"
DOCS_REQUIRED = (
    "README.md", "architecture.md", "building.md", "releasing.md",
    "testing.md", "catalog.md", "contributing.md", "history-policy.md",
    "pitfalls.md", "design-adoption.md", "licensing.md",
)
MAX_LINES = {
    "README.md": 90,
    "DOCS_MANIFEST.md": 300,
    "workspace/singles/README.md": 250,
    "docs/pitfalls.md": 500,
    "docs/history-policy.md": 200,
    "docs/catalog.md": 200,
    "docs/testing.md": 200,
    "docs/design-adoption.md": 200,
    "docs/licensing.md": 200,
    "docs/contributing.md": 250,
    "docs/architecture.md": 200,
    "docs/building.md": 250,
    "docs/releasing.md": 200,
    "docs/README.md": 120,
    "workspace/decoders/raw/DESIGN.md": 300,
}
STALE = ("RuPix-Decoder/docs/", "make_index.ps1", "docs/<id>.md")
LINK = re.compile(r"\]\(([^)\s]+)\)")

REQUIRED_HEADINGS = ("iv_probe",)
# 一个行锚 = 反引号里以 `:` 开头的一段数字（可以是 `:12`、`:12-30`，也可以是逗号列表
# `:61, 169-172`），或者反引号里自带文件名（`build.rs:86-97`）。
# 旧版的正则只认"`:N` 或 `:N-M`，整条反引号里不许有别的东西"，于是逗号列表
# （`:86, 116`）和带前缀的锚（`:61, 169-172`）统统查不到——2026-10-20 m61 实测：
# docs/*.md 共 399 个锚号，旧正则只解析出 342 个，漏掉的正是漂移最集中的那批。
ANCHOR = re.compile(r"`((?:[\w./\\-]+)?:[\d][\d,\s-]*)`")
FILE_IN_TEXT = re.compile(r"[\w.\-]+\.(?:rs|toml|lock|py|c|cc|h|hpp|md|json|cmake)\b")


def _is_decoder(path):
    """有 Cargo.toml，且真的产出一枚 DLL（`router` 是共用 rlib，不是解码器）。"""
    toml = path / "Cargo.toml"
    if not toml.is_file():
        return False
    try:
        text = toml.read_text(encoding="utf-8")
    except OSError:
        return False
    # 只认 `crate-type = [... "cdylib" ...]` 这一行：注释里出现 "cdylib" 不算
    # （router 的 manifest 就解释过自己"不产出 DLL"）。
    return any(
        "cdylib" in line and line.lstrip().startswith("crate-type")
        for line in text.splitlines()
    )


def decoders():
    """[(根目录, id)]：独立解码器与合并档一视同仁（只要目录里有 Cargo.toml）。"""
    out = []
    for root in DECODER_ROOTS:
        if root.is_dir():
            out += [(root, p.name) for p in sorted(root.iterdir())
                    if p.is_dir() and _is_decoder(p)]
    return out


def decoder_ids():
    return sorted(p.name for p in DECODERS.iterdir() if p.is_dir())


def missing_docs():
    out = []
    if not SPEC.is_file():
        out.append("%s does not exist - the contract text belongs at the repo root"
                   % SPEC.name)
    for name in ("AGENTS.md", "DOCS_MANIFEST.md", "workspace/decoders/README.md"):
        if not (ROOT / name).is_file():
            out.append("%s does not exist" % name)
    if not DOCS_DIR.is_dir():
        out.append("docs/ does not exist")
    else:
        for name in DOCS_REQUIRED:
            if not (DOCS_DIR / name).is_file():
                out.append("docs/%s does not exist" % name)
    if not BENCH_README.is_file():
        out.append("workspace/singles/README.md does not exist")
    for root, pid in decoders():
        for name in ("README.md", "HISTORY.md"):
            doc = root / pid / name
            rel = doc.relative_to(ROOT).as_posix()
            if not doc.is_file():
                out.append("%s: %s does not exist" % (pid, rel))
                continue
            if name == "README.md":
                text = doc.read_text(encoding="utf-8")
                if len(text.splitlines()) < 20:
                    out.append("%s: %s is a stub (%d lines)"
                               % (pid, rel, len(text.splitlines())))
                if "iv_probe" not in text:
                    out.append("%s: %s never explains the claim rule (iv_probe)"
                               % (pid, rel))
    return out


def markdown_files():
    files = list(ROOT.glob("*.md"))
    if DOCS_DIR.is_dir():
        files += list(DOCS_DIR.glob("*.md"))
    files += list(DECODERS.glob("*/README.md"))
    files += list(DECODERS.glob("*/HISTORY.md"))
    files += list(DECODERS.glob("*/DESIGN.md"))
    if BENCH_README.is_file():
        files.append(BENCH_README)
    testdata = ROOT / "testdata" / "README.md"
    if testdata.is_file():
        files.append(testdata)
    return sorted(set(files))


def broken_links():
    out = []
    for doc in markdown_files():
        text = doc.read_text(encoding="utf-8")
        for target in LINK.findall(text):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            target = target.split("#", 1)[0].split("?", 1)[0]
            if not target:
                continue
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            if not (doc.parent / target).exists():
                out.append("%s links to missing file: %s"
                           % (doc.relative_to(ROOT).as_posix(), target))
    return out


def doc_budget():
    out = []
    for rel, cap in MAX_LINES.items():
        path = ROOT / rel
        if not path.is_file():
            continue
        lines = len(path.read_text(encoding="utf-8").splitlines())
        if lines > cap:
            out.append("%s is %d lines (cap %d) - move detail to the owning doc"
                       % (rel, lines, cap))
    for root, pid in decoders():
        for name, cap in (("README.md", 150), ("HISTORY.md", 400)):
            path = root / pid / name
            if path.is_file():
                lines = len(path.read_text(encoding="utf-8").splitlines())
                if lines > cap:
                    out.append("%s is %d lines (cap %d)"
                               % (path.relative_to(ROOT).as_posix(), lines, cap))
    return out


def stale_refs():
    out = []
    for doc in markdown_files():
        text = doc.read_text(encoding="utf-8")
        for token in STALE:
            if token in text:
                out.append("%s mentions stale path/term %r"
                           % (doc.relative_to(ROOT).as_posix(), token))
    return out


def manifest_coverage():
    if not MANIFEST.is_file():
        return ["DOCS_MANIFEST.md does not exist"]
    text = MANIFEST.read_text(encoding="utf-8")
    out = []
    required = ["README.md", "AGENTS.md", "解码器开发规范.md",
                "workspace/decoders/README.md", "workspace/decoders/raw/DESIGN.md",
                "workspace/singles/README.md", "testdata/README.md"]
    required += ["docs/%s" % name for name in DOCS_REQUIRED]
    for rel in required:
        if rel not in text:
            out.append("DOCS_MANIFEST.md does not register %s" % rel)
    return out


def anchor_candidates(base, run, line, col):
    """Files this anchor may be read against, most specific first (default: its own lib.rs).

    A named file we do not ship -- a crates.io manifest, a vendored `.3rd` source, the host
    repo's `THIRD-PARTY-NOTICES.md` -- yields no candidate: those line numbers live outside
    this repository and cannot be verified here, so the anchor is skipped, not reported.
    """
    named = run.split(":", 1)[0]
    if not named:
        hits = FILE_IN_TEXT.findall(line[:col])
        named = hits[-1] if hits else None
    if named is None:
        return [base / "src" / "lib.rs"]
    rel = named.replace("\\", "/").lstrip("./")
    cands = []
    for root in (base, ROOT, base / "src"):
        p = root / rel
        if p.is_file() and p not in cands:
            cands.append(p)
    if "/" not in rel:  # prose names only the basename: `smoke_av.rs:18`, `box.cc:3637`
        for p in sorted(base.rglob(rel)):
            if p not in cands:
                cands.append(p)
        for p in sorted(ROOT.glob("*/" + rel)):
            if p not in cands:
                cands.append(p)
    return cands


def anchor_spans(run):
    """:a-b, c, d -> [(a,b),(c,c),(d,d)]; numbers are 1-based inclusive."""
    spans = []
    body = run.split(":", 1)[1]
    for part in body.split(","):
        nums = re.findall(r"\d+", part)
        if not nums:
            continue
        spans.append((int(nums[0]), int(nums[-1])))
    return spans


def fits(path, spans):
    try:
        n = len(path.read_text(encoding="utf-8").splitlines())
    except OSError:
        return False
    return all(a >= 1 and b <= n for (a, b) in spans)


def drifting_anchors():
    """Line anchors in docs/<id>.md must resolve to real, non-blank lines.

    Two failure modes are machine-decidable, and they are the ones a reformat actually
    produces: the range runs past the end of the file, or every line inside it is blank.
    Anything subtler (in range, but pointing at a different function) still needs a human.

    An anchor that lives in another file (build.rs, 根 Cargo.toml, a .3rd source) names that
    file, and the numbers are read against it. `skipped` counts the anchors we cannot check
    because the file they name is not in this repo.
    """
    out = []
    skipped = [0]
    cache = {}
    for root, pid in decoders():
        doc = root / pid / "README.md"
        if not doc.is_file():
            continue
        for lineno, line in enumerate(doc.read_text(encoding="utf-8").splitlines(), 1):
            for m in ANCHOR.finditer(line):
                run = m.group(1)
                spans = anchor_spans(run)
                cands = anchor_candidates(root / pid, run, line, m.start())
                if not cands:
                    skipped[0] += 1
                    continue
                path = next((p for p in cands if fits(p, spans)), cands[0])
                if path not in cache:
                    cache[path] = path.read_text(encoding="utf-8").splitlines()
                lines = cache[path]
                label = "`%s`" % run
                for (a, b) in spans:
                    if a < 1 or b > len(lines):
                        out.append("%s doc%d: anchor %s does not fit %s (%d lines) "
                                   "-- re-anchor it, or name the file it lives in"
                                   % (pid, lineno, label, path.name, len(lines)))
                    elif not any(s.strip() for s in lines[a - 1:b]):
                        out.append("%s doc%d: anchor %s covers only blank lines in %s"
                                   % (pid, lineno, label, path.name))
    return out, skipped[0]


def invisible_bytes():
    """Raw NUL bytes in a source or doc file are a defect of their own.

    Why this is machine-decidable: a NUL inside `b"..."` behaves like the escape, so nothing
    breaks at compile time - but every text tool (grep, diff, editors) silently reclassifies the
    file as binary. Measured on 2026-10-20: `workspace/decoders/jpeg/src/lib.rs` held one such byte, which
    is why a repo-wide `grep -rn` for a call-site inventory skipped that crate's two call sites
    (host pitfalls K86). Byte-identical rewrite: spell it as `c"..."`/`\x00`, or in prose.
    """
    out = []
    roots = [ROOT / "abi", ROOT / "workspace" / "decoders", ROOT / "docs", ROOT / "tools"]
    files = [p for r in roots if r.is_dir() for p in r.rglob("*")
             if p.is_file() and p.suffix in (".rs", ".md", ".toml", ".py", ".h", ".c")]
    files += [p for p in ROOT.glob("*.md") if p.is_file()]
    for f in files:
        n = f.read_bytes().count(b"\x00")
        if n:
            rel = f.relative_to(ROOT)
            out.append("%s: %d raw NUL byte(s) - text tools will treat this file as binary"
                       % (rel, n))
    return out


def non_ascii_scripts():
    """`.ps1` in this repo must be pure ASCII - not a style preference, a parse rule.

    PowerShell 5.1 reads a BOM-less `.ps1` as **ANSI** (the active code page), so a CJK
    comment is decoded into different bytes than it was written as. The kicker is WHERE it
    bites: the mis-decoded run can swallow a quote or a line break, so the error surfaces
    on a *neighbouring* statement that looks perfectly fine (host pitfalls K27, and again
    on 2026-10-25: a Chinese comment above the symbol-archiving block made the next
    `Test-Path` bind a null `-Path`, reported as "ForEach-Object ... parameter Path is
    null" - three lines away from anything the message points at).
    """
    out = []
    for f in sorted(ROOT.rglob("*.ps1")):
        if not f.is_file():
            continue
        data = f.read_bytes()
        if data.startswith(b"\xef\xbb\xbf"):
            out.append("%s: has a UTF-8 BOM - PowerShell 5.1 wants ASCII (or UTF-16) here"
                       % f.relative_to(ROOT))
            continue
        bad = [(i + 1, line) for i, line in enumerate(data.splitlines())
               if any(b > 127 for b in line)]
        for lineno, line in bad[:5]:
            out.append("%s:%d: non-ASCII byte(s) - PS 5.1 reads this file as ANSI "
                       "(K27); write it in English: %r"
                       % (f.relative_to(ROOT), lineno, line[:60]))
    return out


def main():
    anchors, skipped = drifting_anchors()
    problems = (missing_docs() + broken_links() + anchors
                + invisible_bytes() + non_ascii_scripts()
                + doc_budget() + stale_refs() + manifest_coverage())
    for line in problems:
        print("MISS %s" % line)
    plugs = decoders()
    print("decoders: %d, docs: %d, missing: %d, "
          "anchors skipped (file not in this repo): %d"
          % (len(plugs),
             sum(1 for r, pid in plugs if (r / pid / "README.md").is_file()),
             len(problems), skipped))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
