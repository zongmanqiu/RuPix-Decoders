#!/usr/bin/env python3
"""Patches gate: every third-party patch we ship must stay re-derivable from upstream.

我们对外发布的源码要让别人能重建发布物，而 `doc` 依赖的 `emfsdk` 是**打过补丁的仓库外副本**。
`patches/<crate>-<version>/` 就是那份补丁的公开登记（规则见 `patches/README.md`）。
本门禁核四件事，全部可离线核：

1. 登记齐全：`manifest.json` + `README.md` + 至少一份 `*.patch`。
2. 补丁干净：LF 行尾（CRLF 会让 `git apply` 出来的文件哈希全不对——本机实测差 583 字节）。
3. 补丁自洽：`manifest.json` 里记的「补丁后 blob」== 补丁 `index old..new` 的 new。
4. 副本对得上：本机 `../.3rd/<crate>-<version>/` 在场时，逐文件核 sha256 与 git blob；
   不在场只打印 SKIP（新克隆本来就没有 `.3rd`，那不是本仓的错）。

★ 还有一条时效：`upstream_check.checked_at` 超过 `interval_days` 就判红。上游发了新版我们
不知道这件事**不会自己显形**，只能靠日期逼一次人工核对；核对完改日期（上游没变也只改日期）。
`--online` 可代查 crates.io 的 `max_stable_version`（网络不可用时打印 SKIP，不算违规）。

Usage: python tools/check_patches.py [--online] [--selftest]
Exit 0 clean, 1 on any violation. Prints "patch registries: N, violations: M".
"""

import hashlib
import json
import re
import sys
import tempfile
import shutil
from datetime import date, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PATCHES = ROOT / "patches"
THIRD = ROOT.parent / ".3rd"

HEX64 = re.compile(r"^[0-9a-f]{64}$")
DIFF_GIT = re.compile(r"^diff --git a/(\S+) b/(\S+)$")
INDEX_LINE = re.compile(r"^index ([0-9a-f]{7,40})\.\.([0-9a-f]{7,40}) 100644$")

REQUIRED_TOP = ("crate", "version", "upstream", "crate_sha256", "vendor_dir", "used_by",
                "patches", "upstream_check")
REQUIRED_PATCH = ("id", "file", "date", "summary", "files")
REQUIRED_FILE = ("path", "upstream_sha256", "patched_sha256", "patched_blob")
REQUIRED_CHECK = ("checked_at", "upstream_latest_known", "interval_days", "how")


def blob_sha1(data: bytes) -> str:
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def parse_patch(text: str):
    """[(path, old_blob, new_blob)] in patch order; both blobs are None when absent."""
    out = []
    cur = None
    for line in text.splitlines():
        m = DIFF_GIT.match(line)
        if m:
            cur = {"path": m.group(2), "old": None, "new": None}
            out.append(cur)
            continue
        m = INDEX_LINE.match(line)
        if m and cur is not None:
            cur["old"], cur["new"] = m.group(1), m.group(2)
    return [(f["path"], f["old"], f["new"]) for f in out]


def iso_date(value, label, out):
    try:
        return date.fromisoformat(value)
    except (TypeError, ValueError):
        out.append("%s: %s 不是 YYYY-MM-DD: %r" % (label, "date", value))
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
    if not HEX64.match(str(data["crate_sha256"])):
        out.append("%s: crate_sha256 不是 64 位小写十六进制" % label)

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
            out.append("%s/%s: 补丁里有 CRLF（git apply 会写出 CRLF，哈希全对不上）"
                       % (label, pid))
        text = raw.decode("utf-8", "replace")
        parsed = parse_patch(text)
        if not parsed:
            out.append("%s/%s: 补丁里没有 'diff --git a/… b/…' 段" % (label, pid))
            continue

        recorded = {f.get("path"): f for f in entry["files"] if isinstance(f, dict)}
        for path, _old, new in parsed:
            rec = recorded.get(path)
            if rec is None:
                out.append("%s/%s: manifest 没登记补丁触及的 %s" % (label, pid, path))
                continue
            missing = [k for k in REQUIRED_FILE if k not in rec]
            if missing:
                out.append("%s/%s/%s: 文件条目缺 %s" % (label, pid, path, ", ".join(missing)))
                continue
            for key in ("upstream_sha256", "patched_sha256"):
                if not HEX64.match(str(rec[key])):
                    out.append("%s/%s/%s: %s 不是 64 位十六进制" % (label, pid, path, key))
            if new is None:
                out.append("%s/%s/%s: 补丁缺少 'index old..new' 行，无法自洽核对"
                           % (label, pid, path))
            elif not str(rec["patched_blob"]).startswith(new) and not new.startswith(
                    str(rec["patched_blob"])):
                out.append("%s/%s/%s: manifest 的 patched_blob %s != 补丁 new %s"
                           % (label, pid, path, rec["patched_blob"], new))
        for path in recorded:
            if path not in [p for p, _o, _n in parsed]:
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
            for rec in entry.get("files", []) if isinstance(entry, dict) else []:
                if not isinstance(rec, dict) or "path" not in rec:
                    continue
                target = vendor / str(rec["path"])
                if not target.is_file():
                    out.append("%s: 副本缺文件 %s" % (label, rec["path"]))
                    continue
                raw = target.read_bytes()
                if "patched_sha256" in rec and sha256(raw) != rec["patched_sha256"]:
                    out.append("%s/%s: 副本 sha256 与登记不符（副本被手改过，或补丁没重放到这项）"
                               % (label, rec["path"]))
                if "patched_blob" in rec and blob_sha1(raw) != rec["patched_blob"]:
                    out.append("%s/%s: 副本 git blob 与登记不符" % (label, rec["path"]))
    else:
        notes.append("%s: SKIP 副本 %s 不在场，只做了登记与补丁自洽核对" % (label, vendor))
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


def selftest():
    """Prove the gate can fail: each mutation must be reported, the good case must not."""
    tmp = Path(tempfile.mkdtemp(prefix="patch-gate-"))
    try:
        reg = tmp / "patches" / "demo-1.0"
        vendor = tmp / ".3rd" / "demo-1.0"
        (reg).mkdir(parents=True)
        (vendor / "src").mkdir(parents=True)
        good = b"fn main() {}\n"
        (vendor / "src" / "x.rs").write_bytes(good)
        patch = ("diff --git a/src/x.rs b/src/x.rs\n"
                 "index 0000000..%s 100644\n"
                 "--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1 +1 @@\n-fn main() {}\n+fn main() {}\n"
                 % blob_sha1(good)[:7])
        (reg / "E01.patch").write_bytes(patch.encode())
        (reg / "README.md").write_text("demo\n", encoding="utf-8")
        manifest = {
            "crate": "demo", "version": "1.0",
            "upstream": "https://example.invalid/demo",
            "crate_sha256": "0" * 64,
            "vendor_dir": ".3rd/demo-1.0",
            "used_by": "selftest",
            "patches": [{
                "id": "E01", "file": "E01.patch", "date": "2026-01-01", "summary": "selftest",
                "files": [{"path": "src/x.rs", "upstream_sha256": "0" * 64,
                           "patched_sha256": sha256(good), "patched_blob": blob_sha1(good)}],
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
        probe("copy hand-edited", lambda: (vendor / "src" / "x.rs").write_bytes(b"tampered\n"), True)
        probe("patch has CRLF", lambda: (reg / "E01.patch").write_bytes(patch.replace("\n", "\r\n").encode()), True)
        probe("README removed", lambda: (reg / "README.md").unlink(), True)
        probe("stale upstream check",
              lambda: _set_check(reg, {"checked_at": "2025-01-01"}), True)
        probe("bad blob in manifest",
              lambda: _set_blob(reg, "0" * 40), True)

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


def _set_blob(reg: Path, blob: str):
    path = reg / "manifest.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    data["patches"][0]["files"][0]["patched_blob"] = blob
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
