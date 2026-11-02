#!/usr/bin/env python3
"""Patches gate: every third-party patch we ship must stay re-derivable from upstream.

我们对外发布的源码要让别人能重建发布物，而 `doc` 依赖的 `emfsdk` 是**打过补丁的仓库外副本**。
`patches/<crate>-<version>/` 就是那份补丁的公开登记（规则见 `patches/README.md`）。
本门禁核四件事，全部可离线核，**不含任何哈希**：

1. 登记齐全：`manifest.json` + `README.md` + 至少一份 `*.patch`。
2. 补丁干净：LF 行尾（CRLF 会让 `git apply` 写出 CRLF 副本）。
3. 补丁与登记对齐：补丁里 `diff --git` 触及的路径集合 == `manifest.json` 的 `files`。
4. 副本在场：`../.3rd/<crate>-<version>/` 存在时，补丁触及的每个文件都得在
   （**只查存在，不比内容**）。

★ 为什么没有哈希（2026-11-02 用户口径）

这些都是官方维护的产物：补丁由本仓作者写、副本由本仓作者放到 `.3rd`、release 也只有作者
能发，指纹要挡的"第三方篡改"根本不存在。而它的成本是实打实的——MSVC 每次链接换 PDB GUID、
`rustfmt` 一次重排就换字节，于是"登记一串十六进制"变成每次动代码都要重算、算错就误报门禁红。
**识别一份补丁只认 `crate + version + 补丁编号 + 触及路径`**，那四样才是人真正关心的身份。
撤销的登记字段：`crate_sha256` / `upstream_sha256` / `patched_sha256` / `patched_blob`。

★ 还有一条时效：`upstream_check.checked_at` 超过 `interval_days` 就判红。上游发了新版我们
不知道这件事**不会自己显形**，只能靠日期逼一次人工核对；核对完改日期（上游没变也只改日期）。
`--online` 可代查 crates.io 的 `max_stable_version`（网络不可用时打印 SKIP，不算违规）。

Usage: python tools/check_patches.py [--online] [--selftest]
Exit 0 clean, 1 on any violation. Prints "patch registries: N, violations: M".
"""

import json
import re
import shutil
import sys
import tempfile
from datetime import date, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PATCHES = ROOT / "patches"

DIFF_GIT = re.compile(r"^diff --git a/(\S+) b/(\S+)$")

REQUIRED_TOP = ("crate", "version", "upstream", "vendor_dir", "used_by",
                "patches", "upstream_check")
REQUIRED_PATCH = ("id", "file", "date", "summary", "files")
REQUIRED_CHECK = ("checked_at", "upstream_latest_known", "interval_days", "how")


def parse_patch_paths(text: str):
    """补丁触及的路径，按出现顺序去重。"""
    out = []
    for line in text.splitlines():
        m = DIFF_GIT.match(line)
        if m:
            p = m.group(2)
            if p not in out:
                out.append(p)
    return out


def iso_date(value, label, out):
    try:
        return date.fromisoformat(value)
    except (TypeError, ValueError):
        out.append("%s: date 不是 YYYY-MM-DD: %r" % (label, value))
        return None


def check_registry(root: Path, reg: Path, today: date):
    """Returns (violations, notes). notes are SKIP lines a human should see."""
    out, notes = [], []
    label = reg.name
    manifest_path = reg / "manifest.json"
    if not manifest_path.is_file():
        return ["%s: manifest.json missing" % label], notes
    try:
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        return ["%s: manifest.json 不是合法 JSON (%s)" % (label, exc)], notes

    for key in REQUIRED_TOP:
        if key not in data:
            out.append("%s: manifest 缺 %s" % (label, key))
    if out:
        return out, notes

    if not (reg / "README.md").is_file():
        out.append("%s: README.md missing（人读的「为什么改 / 怎么重放 / 上游怎么核对」）" % label)

    patches = data["patches"]
    if not isinstance(patches, list) or not patches:
        out.append("%s: patches 必须是非空数组" % label)
        patches = []

    seen_ids = set()
    for entry in patches:
        if not isinstance(entry, dict):
            out.append("%s: patches 条目不是对象" % label)
            continue
        missing = [k for k in REQUIRED_PATCH if k not in entry]
        if missing:
            out.append("%s: 补丁条目缺 %s" % (label, ", ".join(missing)))
            continue
        pid = str(entry["id"])
        if pid in seen_ids:
            out.append("%s: 补丁编号重复 %s" % (label, pid))
        seen_ids.add(pid)
        iso_date(entry["date"], "%s/%s" % (label, pid), out)

        patch_file = reg / str(entry["file"])
        if not patch_file.is_file():
            out.append("%s/%s: 补丁文件不存在 %s" % (label, pid, entry["file"]))
            continue
        raw = patch_file.read_bytes()
        if b"\r\n" in raw:
            out.append("%s/%s: 补丁里有 CRLF（git apply 会写出 CRLF 副本）" % (label, pid))
        text = raw.decode("utf-8", "replace")
        touched = parse_patch_paths(text)
        if not touched:
            out.append("%s/%s: 补丁里没有 'diff --git a/… b/…' 段" % (label, pid))
            continue

        declared = entry["files"]
        if not isinstance(declared, list) or not all(isinstance(x, str) for x in declared):
            out.append("%s/%s: files 必须是路径字符串数组" % (label, pid))
            continue
        for path in touched:
            if path not in declared:
                out.append("%s/%s: manifest 没登记补丁触及的 %s" % (label, pid, path))
        for path in declared:
            if path not in touched:
                out.append("%s/%s: manifest 登记了补丁没触及的 %s" % (label, pid, path))

    check = data["upstream_check"]
    if not isinstance(check, dict):
        out.append("%s: upstream_check 不是对象" % label)
        return out, notes
    missing = [k for k in REQUIRED_CHECK if k not in check]
    if missing:
        out.append("%s: upstream_check 缺 %s" % (label, ", ".join(missing)))
    else:
        checked = iso_date(check["checked_at"], "%s/upstream_check" % label, out)
        interval = check["interval_days"]
        if not isinstance(interval, int) or interval <= 0:
            out.append("%s: upstream_check.interval_days 必须是正整数" % label)
        elif checked is not None:
            if checked > today:
                out.append("%s: upstream_check.checked_at 在未来 (%s)" % (label, checked))
            elif today - checked > timedelta(days=interval):
                days = (today - checked).days
                out.append("%s: 上游已 %d 天没核对（上限 %d）——去 %s 看一眼有没有新版，"
                           "核对完把 checked_at 改成今天（上游没变也只改日期）"
                           % (label, days, interval, data["upstream"]))

    vendor = (root / str(data["vendor_dir"])).resolve()
    if vendor.is_dir():
        for entry in patches:
            if not isinstance(entry, dict):
                continue
            for path in entry.get("files", []) if isinstance(entry.get("files"), list) else []:
                if not isinstance(path, str):
                    continue
                if not (vendor / path).is_file():
                    out.append("%s: 副本缺文件 %s" % (label, path))
    else:
        notes.append("%s: SKIP 副本 %s 不在场，只做了登记与补丁结构核对" % (label, vendor))
    return out, notes


def online_latest(crate: str):
    """(version, None) on success, (None, reason) when it could not be answered."""
    import urllib.request
    url = "https://crates.io/api/v1/crates/%s" % crate
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "rupix-patch-gate"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        return data["crate"]["max_stable_version"], None
    except Exception as exc:                     # noqa: BLE001 - any failure is a SKIP
        return None, str(exc)


def run(root: Path, today: date, online: bool):
    regs = sorted(p for p in (root / "patches").iterdir() if p.is_dir()) \
        if (root / "patches").is_dir() else []
    violations, notes = [], []
    for reg in regs:
        v, n = check_registry(root, reg, today)
        violations += v
        notes += n
        if online and not v:
            data = json.loads((reg / "manifest.json").read_text(encoding="utf-8"))
            latest, why = online_latest(str(data["crate"]))
            if why is not None:
                notes.append("%s: SKIP 上游在线查询失败（%s）" % (reg.name, why))
            elif latest != data["version"]:
                violations.append("%s: 上游已有 %s（我们登记的是 %s）——按 README 的"
                                  "「升级上游」重放补丁，再改 checked_at"
                                  % (reg.name, latest, data["version"]))
    return regs, violations, notes


DEMO_PATCH = (
    "diff --git a/src/x.rs b/src/x.rs\n"
    "--- a/src/x.rs\n"
    "+++ b/src/x.rs\n"
    "@@ -1 +1 @@\n"
    "-fn main() {}\n"
    "+fn main() { let _ = 1; }\n"
)


def selftest():
    """Prove the gate can fail: each mutation must be reported, the good case must not."""
    tmp = Path(tempfile.mkdtemp(prefix="patch-gate-"))
    try:
        reg = tmp / "patches" / "demo-1.0"
        vendor = tmp / ".3rd" / "demo-1.0"
        reg.mkdir(parents=True)
        (vendor / "src").mkdir(parents=True)
        (vendor / "src" / "x.rs").write_bytes(b"fn main() { let _ = 1; }\n")
        (reg / "E01.patch").write_bytes(DEMO_PATCH.encode())
        (reg / "README.md").write_text("demo\n", encoding="utf-8")
        manifest = {
            "crate": "demo", "version": "1.0",
            "upstream": "https://example.invalid/demo",
            "vendor_dir": ".3rd/demo-1.0",
            "used_by": "selftest",
            "patches": [{
                "id": "E01", "file": "E01.patch", "date": "2026-01-01",
                "summary": "selftest", "files": ["src/x.rs"],
            }],
            "upstream_check": {"checked_at": "2026-01-01",
                               "upstream_latest_known": "1.0", "interval_days": 90,
                               "how": "selftest"},
        }
        (reg / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        today = date(2026, 1, 2)

        cases = []

        def probe(name, mutate, expect_fail):
            backup = {}
            for f in reg.rglob("*"):
                if f.is_file():
                    backup[f] = f.read_bytes()
            mutate()
            _regs, violations, _notes = run(tmp, today, False)
            hit = bool(violations) == expect_fail
            cases.append((name, hit, violations))
            for f, raw in backup.items():
                f.write_bytes(raw)

        probe("good registry passes", lambda: None, False)
        probe("copy file missing", lambda: (vendor / "src" / "x.rs").unlink(), True)
        probe("patch has CRLF",
              lambda: (reg / "E01.patch").write_bytes(DEMO_PATCH.replace("\n", "\r\n").encode()), True)
        probe("README removed", lambda: (reg / "README.md").unlink(), True)
        probe("stale upstream check",
              lambda: _set_check(reg, {"checked_at": "2025-01-01"}), True)
        probe("manifest names a path the patch never touches",
              lambda: _set_files(reg, ["src/x.rs", "src/y.rs"]), True)

        failed = [name for name, ok, _v in cases if not ok]
        for name, ok, violations in cases:
            print("  %s %s%s" % ("OK " if ok else "FAIL", name,
                                 "" if ok or not violations else " -> %s" % violations[0]))
        print("selftest cases: %d, failed: %d" % (len(cases), len(failed)))
        return 0 if not failed else 1
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _set_check(reg: Path, patch: dict):
    path = reg / "manifest.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    data["upstream_check"].update(patch)
    path.write_text(json.dumps(data), encoding="utf-8")


def _set_files(reg: Path, files):
    path = reg / "manifest.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    data["patches"][0]["files"] = files
    path.write_text(json.dumps(data), encoding="utf-8")


def main(argv):
    if "--selftest" in argv:
        return selftest()
    regs, violations, notes = run(ROOT, date.today(), "--online" in argv)
    for line in notes:
        print("  %s" % line)
    for line in violations:
        print("  VIOLATION %s" % line)
    print("patch registries: %d, violations: %d" % (len(regs), len(violations)))
    return 1 if violations else 0


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except AttributeError:
        pass
    sys.exit(main(sys.argv[1:]))
