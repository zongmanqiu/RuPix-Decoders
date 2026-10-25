#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Pixel-cap gate for the RuPix plugin repo (added by host round m120).

Two numbers used to be kept in step by hand, and nothing read them:

  * every decoder declares **its own** `const MAX_PIXELS: u64 = 64 * 1024 * 1024;`
    (12 copies as of 2026-10-24; the 11 pro singles share one copy in `pro-support/`). Their doc comments cross-reference each other
    ("与 core / png / jpeg / tiff 同口径", "与 core 插件同一道闸") - a manual
    invariant with no machine behind it, which is exactly the shape that m56
    learned to collapse in the host repo.
  * `testdata/make_samples.py` mirrors the **host** bound
    (`main/src/core/pixels.rs`: `MAX_PIXELS = 40_000_000`) because a sample the
    viewer refuses is not a sample. That is a third copy of a host truth, in
    another language, hand-synced.

Same identifier, two different gates: the plugins' 64 MP is an *allocation*
guard inside each DLL, the host's 40 MP is the viewer's hard top. This script
deliberately does **not** unify them and does not rename either one (renaming
would mean rebuilding 12 DLLs to buy readability only). It only asserts that
each side is internally consistent.

Checks - equality only, never "which value is right", so there is no
false-positive shape to train people to ignore (contrast check_anchors.py,
which stays advisory precisely for that reason):

  1. every published tier (`decoders/<id>/src/lib.rs`), every single lib
     (`bench/singles/<id>/src/lib.rs`) and the shared `pro-support/src/lib.rs`
     that declares MAX_PIXELS declares the same value, and at least one crate declares it;
  2. if the host repo sits beside this one: host `pixels.rs` MAX_PIXELS ==
     `make_samples.py` MAX_PIXELS. Otherwise print a loud skip - the host bound
     is then simply not observable from here, not "fine".

Self-nail (K126): `python scripts/check_caps.py --selftest` mutates one value in
memory and asserts the detector goes red. A gate that has never been proven to
fail is a gate in name only.

Exit code: 0 clean, 1 violations. Prints "violations: N" like the host scanners.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RE_RUST = re.compile(r"^\s*(?:pub\s+)?const\s+MAX_PIXELS\s*:\s*u64\s*=\s*([^;]+);", re.M)
RE_PY = re.compile(r"^MAX_PIXELS\s*=\s*([^#\n]+)", re.M)
SAFE = re.compile(r"^[0-9_()*+\-/ ]+$")


def value(expr):
    """Evaluate a literal like `64 * 1024 * 1024` / `40_000_000`. Letters are rejected."""
    text = expr.strip().rstrip(",")
    if not SAFE.match(text):
        raise ValueError(f"无法求值的表达式：{expr!r}")
    return eval(text.replace("_", ""))


def rust_caps(path):
    out = []
    for i, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        m = RE_RUST.match(line)
        if m:
            out.append((path, i, value(m.group(1))))
    return out


def py_caps(path):
    out = []
    for i, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        m = RE_PY.match(line)
        if m:
            out.append((path, i, value(m.group(1))))
    return out


def collect_plugin():
    rows = []
    for pattern in ("decoders/*/src/lib.rs", "bench/singles/*/src/lib.rs", "pro-support/src/lib.rs"):
        for lib in sorted(ROOT.glob(pattern)):
            rows += rust_caps(lib)
    return rows


def collect_host():
    """The host bound, from both places that state it. Empty if the host repo is absent."""
    host = ROOT.parent / "main"
    py = host / "src/core/pixels.rs"
    if not py.is_file():
        return None, None
    samples = ROOT / "testdata/make_samples.py"
    return (rust_caps(py) or [None])[0], (py_caps(samples) or [None])[0]


def detect(plugin_rows, host_pair):
    bad = []
    if not plugin_rows:
        bad.append("没有任何插件声明 MAX_PIXELS - 词式变了，本门禁失明（先修脚本再用它）")
    for path, line, val in plugin_rows[1:]:
        base_path, base_line, base = plugin_rows[0]
        if val != base:
            bad.append(f"{path}:{line} = {val} 与 {base_path}:{base_line} = {base} 不同口径")
    host_row, sample_row = host_pair
    if host_row is None:
        print(f"  skip: 宿主仓不在此处（找 {ROOT.parent / 'main/src/core/pixels.rs'}），宿主闸无法核对")
    elif host_row[2] != sample_row[2]:
        bad.append(f"宿主 {host_row[0]}:{host_row[1]} = {host_row[2]} 与 "
                   f"{sample_row[0]}:{sample_row[1]} = {sample_row[2]} 不同口径（样本生成器必须跟着宿主走）")
    return bad


def selftest(plugin_rows, host_pair):
    """Prove the detector bites: mutate values in memory, expect red in both directions."""
    assert not detect(plugin_rows, host_pair), "自检失败：当前树本来就不干净，先修树再用它自证"
    if len(plugin_rows) < 2:
        raise AssertionError("自检失败：插件侧不足两枚声明，比对臂是空的")
    p, l, v = plugin_rows[-1]
    mutated = plugin_rows[:-1] + [(p, l, v + 1)]
    assert detect(mutated, host_pair), "自检失败：插件侧改了一个值却没报红 ⇒ 判据没接上"
    if host_pair[0] is not None and host_pair[1] is not None:
        hp, hl, hv = host_pair[1]
        flipped = (host_pair[0], (hp, hl, hv + 1))
        assert detect(plugin_rows, flipped), "自检失败：样本生成器与宿主错开一格却没报红 ⇒ 判据没接上"
    print("  selftest: 变异臂都报红 ✓")


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    plugin_rows = collect_plugin()
    host_pair = collect_host()
    mp = {v // (1024 * 1024) for _, _, v in plugin_rows}
    print(f"  插件侧：{len(plugin_rows)} 枚 crate 声明 MAX_PIXELS，值集 {sorted(mp)} MP")
    if host_pair[0] is not None and host_pair[1] is not None:
        print(f"  宿主侧：pixels.rs = {host_pair[0][2]:,} ＝ make_samples.py = {host_pair[1][2]:,}")
    if "--selftest" in sys.argv:
        selftest(plugin_rows, host_pair)
    bad = detect(plugin_rows, host_pair)
    for v in bad:
        print(f"  VIOLATION {v}")
    print(f"violations: {len(bad)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
