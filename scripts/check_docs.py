#!/usr/bin/env python3
"""Docs coverage gate: every published decoder has docs/<id>.md, README links resolve,
and every `:NNN` line anchor lands on a real, non-blank line of the file it names.

Machine check for the rule in AGENTS.md section 6 step 7: a plugin without a detail doc is
a plugin nobody can maintain. Exits 1 on any miss.

The anchor check was widened on 2026-10-20 (m61): the old regex only matched a whole
backticked run of exactly `:N` or `:N-M`, so comma lists (`:86, 116`) and prefixed anchors
were never looked at, and every anchor was read against src/lib.rs even when the sentence
named 根 Cargo.toml. Measured on this repo: 261 runs resolved before, 373 runs / 678
numbers now (10 of them name a file outside this repo, so they are skipped by design).
Drift nobody can run into is not a gate: build.ps1 had no step that ran this script at all.

Usage: python scripts/check_docs.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "decoders"      # 每枚插件的文档 = decoders/<id>/README.md（2026-10-25：
                              # 撤销 docs/ 目录，文档跟着它讲的那枚插件走）
SPEC = ROOT / "插件开发规范.md"   # 契约正文真源，2026-10-25 从 docs/ 提到仓库根
DECODERS = ROOT / "decoders"
# 发布档（decoders/<id>）走**同一套**文档规矩：一枚 DLL 没有文档，维护者就只能
# 靠读代码猜它认领了什么（2026-10-25 m138 立）。单库在 bench/singles/，不发布，
# 不在这道门禁的管辖内。
PLUGIN_ROOTS = (DECODERS,)
README = ROOT / "README.md"

REQUIRED_HEADINGS = ("iv_probe",)
# 一个行锚 = 反引号里以 `:` 开头的一段数字（可以是 `:12`、`:12-30`，也可以是逗号列表
# `:61, 169-172`），或者反引号里自带文件名（`build.rs:86-97`）。
# 旧版的正则只认"`:N` 或 `:N-M`，整条反引号里不许有别的东西"，于是逗号列表
# （`:86, 116`）和带前缀的锚（`:61, 169-172`）统统查不到——2026-10-20 m61 实测：
# docs/*.md 共 399 个锚号，旧正则只解析出 342 个，漏掉的正是漂移最集中的那批。
ANCHOR = re.compile(r"`((?:[\w./\\-]+)?:[\d][\d,\s-]*)`")
FILE_IN_TEXT = re.compile(r"[\w.\-]+\.(?:rs|toml|lock|py|c|cc|h|hpp|md|json|cmake)\b")


def _is_plugin(path):
    """有 Cargo.toml，且真的产出一枚 DLL（`router` 是共用 rlib，不是插件）。"""
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


def plugins():
    """[(根目录, id)]：独立插件与合并档一视同仁（只要目录里有 Cargo.toml）。"""
    out = []
    for root in PLUGIN_ROOTS:
        if root.is_dir():
            out += [(root, p.name) for p in sorted(root.iterdir())
                    if p.is_dir() and _is_plugin(p)]
    return out


def decoder_ids():
    return sorted(p.name for p in DECODERS.iterdir() if p.is_dir())


def missing_docs():
    out = []
    if not SPEC.is_file():
        out.append("%s does not exist - the contract text belongs at the repo root"
                   % SPEC.name)
    for root, pid in plugins():
        doc = root / pid / "README.md"
        rel = doc.relative_to(ROOT).as_posix()
        if not doc.is_file():
            out.append("%s: %s does not exist" % (pid, rel))
            continue
        text = doc.read_text(encoding="utf-8")
        if len(text.splitlines()) < 20:
            out.append("%s: %s is a stub (%d lines)" % (pid, rel, len(text.splitlines())))
        if "iv_probe" not in text:
            out.append("%s: %s never explains the claim rule (iv_probe)" % (pid, rel))
    return out


def broken_readme_links():
    out = []
    if not README.is_file():
        return ["README.md does not exist"]
    text = README.read_text(encoding="utf-8")
    for target in re.findall(r"\]\((docs/[^)#]+)\)", text):
        if not (ROOT / Path(target)).is_file():
            out.append("README links to missing file: %s" % target)
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
    for root, pid in plugins():
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
    file as binary. Measured on 2026-10-20: `decoders/jpeg/src/lib.rs` held one such byte, which
    is why a repo-wide `grep -rn` for a call-site inventory skipped that crate's two call sites
    (host pitfalls K86). Byte-identical rewrite: spell it as `c"..."`/`\x00`, or in prose.
    """
    out = []
    roots = [ROOT / "abi", ROOT / "decoders", ROOT / "docs", ROOT / "scripts"]
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
    problems = (missing_docs() + broken_readme_links() + anchors
                + invisible_bytes() + non_ascii_scripts())
    for line in problems:
        print("MISS %s" % line)
    plugs = plugins()
    print("plugins: %d, docs: %d, missing: %d, "
          "anchors skipped (file not in this repo): %d"
          % (len(plugs),
             sum(1 for r, pid in plugs if (r / pid / "README.md").is_file()),
             len(problems), skipped))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
