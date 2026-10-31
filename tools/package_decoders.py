#!/usr/bin/env python3
"""Build release ZIPs: DLL + <id>_<version>/ metadata, license, notices, README.

The ZIP is the download unit. `decoders.json` still describes the DLL (size/hash);
the ZIP hash is computed after packaging and is not written inside itself.

★ 包里带的是**这档自己的 `README.md`**（仓库里那份原文），末尾由本脚本追加一节
「许可与源码」。以前这里放的是一枚手写风格的 `SOURCE-OFFER.md`，含"三年内书面提供
源码"的要约——那是物理分发时代的写法，我们源码公开在两个镜像上，没有需要"另行提供"
的东西，所以整段撤掉。唯一不能跟着消失的是 `raw` 的 LGPL 重链接材料（上游 commit、
crate 校验值），它搬进了同一节。
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT.parent / ".temp" / "decoder-dist"
OUT = ROOT.parent / ".temp" / "decoder-packages"
INDEX = ROOT / "decoders.json"
LICENSE = ROOT / "LICENSE"
SOURCE_REPO = "https://gitee.com/qiuzongman/RuPix-Decoders"
MIRROR_REPO = "https://github.com/zongmanqiu/RuPix-Decoders"
RAWLER_VERSION = "0.7.2"
RAWLER_COMMIT = "ff7a4524cc7187554544269482e5fd5e68a6b4ab"
RAWLER_CRATE_SHA256 = "04f4cc35c23969a4a834e0b117c7da41ace812eb9053b5effc3fc5c77d114677"
# ZIP 条目时间戳钉死 ⇒ 包字节是内容的纯函数。z.write() 取真实 mtime、writestr(str) 取当前时钟，
# 一次零改动的重打包就会换 sha；那意味着"同一版本的包"不再由源码树唯一决定。
ZIP_FIXED_DATE = (1980, 1, 1, 0, 0, 0)

TIERS = {
    "common": ("rupix-common", ROOT / "workspace"),
    "modern": ("rupix-modern", ROOT / "workspace"),
    "raw": ("rupix-raw", ROOT / "workspace"),
    "pro": ("rupix-pro", ROOT / "workspace" / "decoders" / "pro"),
    "classic": ("rupix-classic-dll", ROOT / "workspace" / "decoders" / "classic"),
    "texture": ("rupix-texture-dll", ROOT / "workspace" / "decoders" / "texture"),
    "doc": ("rupix-doc", ROOT / "workspace" / "decoders" / "doc"),
    "doc-legacy": ("rupix-doc-legacy-tier", ROOT / "workspace" / "decoders" / "doc-legacy"),
}


def license_ids(expr: str):
    text = expr.replace("/", " OR ")
    text = re.sub(r"[()]", " ", text)
    return [x.strip() for x in re.split(r"\s+(?:OR|AND)\s+", text) if x.strip()]


def metadata_for(cwd):
    data = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(cwd / "Cargo.toml")],
        cwd=ROOT, text=True, encoding="utf-8"))
    return {(p["name"], p["version"]): p for p in data["packages"]}


def deps_for(cwd, package):
    tree = subprocess.check_output(
        ["cargo", "tree", "-p", package, "--edges", "normal", "--prefix", "none"],
        cwd=cwd, text=True, encoding="utf-8")
    packages = metadata_for(cwd)
    out = []
    for line in tree.splitlines():
        m = re.match(r"([A-Za-z0-9_.-]+) v([^ ]+)", line.strip())
        if not m:
            continue
        key = (m.group(1), m.group(2))
        pkg = packages.get(key)
        if pkg is None or Path(pkg["manifest_path"]).resolve().is_relative_to(ROOT):
            continue
        if key not in out:
            out.append(key)
    return out, packages


def chosen_license_ids(expr: str):
    """For a plain OR expression, pick one branch (MIT/Apache preferred). AND terms
    must all be represented; only the OR alternatives are optional."""
    ids = license_ids(expr)
    if " OR " in expr and " AND " not in expr:
        preferred = ("MIT", "Apache-2.0", "BSD-3-Clause", "BSD-2-Clause", "Zlib",
                     "0BSD", "Unlicense", "AGPL-3.0-only", "AGPL-3.0", "LGPL-2.1",
                     "MPL-2.0", "CC0-1.0", "IJG")
        for p in preferred:
            if p in ids:
                return [p]
    return ids

def license_section(tier: str, version: str) -> str:
    """追加在包内 `README.md` 末尾的一节：许可 + 对应源码在哪。

    ★ 只写事实，不写"三年内书面提供源码"那类要约：本仓源码公开在两个镜像上，
    收件人不需要向我们索取任何东西。`raw` 例外的一小段是 LGPL-2.1 要求的重链接材料
    （上游 commit + crate 校验值），那是义务不是仪式，删文件时不能把它删没。
    """
    tag = f"{tier}_{version}"
    path = f"workspace/decoders/{tier}"
    parts = [f"""

---

## 许可与源码

这枚 `{tier}` 由 RuPix 以 **AGPL-3.0** 分发，全文见同目录 `LICENSE`；它静态链接的第三方
crate 仍各自沿用自己的许可证——逐包清单在同目录 `THIRD-PARTY-NOTICES.md`，许可证正文在
`licenses/`。

本包对应的完整源码就是这个仓库在 tag `{tag}` 下的 `{path}/`，**公开可取，无需索取**：

- Gitee：{SOURCE_REPO}/tree/{tag}/{path}
- GitHub：{MIRROR_REPO}/tree/{tag}/{path}

依赖版本由该目录的 `Cargo.lock` 锁定，重建方式见 `docs/building.md`。
本节说明本仓的许可口径，不是法律意见。
"""]
    if tier == "raw":
        parts.append(f"""
### `raw` 的 LGPL 重链接材料

`raw` 静态链接 `rawler {RAWLER_VERSION}`（LGPL-2.1），LGPL 要求收件人能从源码重建这枚 DLL：

- 上游源码：https://github.com/dnglab/dnglab/tree/{RAWLER_COMMIT}/rawler
- crate 包：https://crates.io/crates/rawler/{RAWLER_VERSION}
- `Cargo.lock` 校验值：`{RAWLER_CRATE_SHA256}`
- LGPL-2.1 正文：`licenses/LGPL-2.1.txt`（本包内）

`rawler` 的源码不在本包里，按上面的地址公开可取；换库或改版本后重跑
`cargo build --release` 即可重建这枚 DLL。
""")
    return "\n".join(parts)


def license_text(pkg_dir: Path, lid: str):
    names = {
        "MIT": ("LICENSE-MIT", "LICENSE-MIT.md", "LICENSE"),
        "Apache-2.0": ("LICENSE-APACHE", "LICENSE-APACHE.md", "LICENSE"),
        "BSD-2-Clause": ("LICENSE-BSD", "LICENSE", "LICENSE.md"),
        "BSD-3-Clause": ("LICENSE.md", "LICENSE", "LICENSE-BSD"),
        "Zlib": ("LICENSE-ZLIB", "LICENSE"),
        "0BSD": ("LICENSE-0BSD",),
        "Unlicense": ("UNLICENSE", "LICENSE"),
        "Unicode-3.0": ("LICENSE-UNICODE",),
        "CC0-1.0": ("LICENSE-CC0", "LICENSE-CC0.md", "LICENSE"),
        "IJG": ("LICENSE-IJG", "LICENSE"),
        "MPL-2.0": ("LICENSE", "LICENSE.md"),
        "LGPL-2.1": ("LICENSE", "COPYING", "LICENSE.md"),
        "AGPL-3.0": ("LICENSE-AGPL3", "LICENSE"),
        "AGPL-3.0-only": ("LICENSE-AGPL3", "LICENSE"),
        "ISC": ("LICENSE", "LICENSE.md"),
    }
    for name in names.get(lid, ("LICENSE", "LICENSE.md", "COPYING", "UNLICENSE")):
        p = pkg_dir / name
        if p.is_file() and p.stat().st_size:
            return p
    return None


def add_entry(z: zipfile.ZipFile, arcname: str, data: bytes):
    info = zipfile.ZipInfo(arcname, date_time=ZIP_FIXED_DATE)
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = 0o644 << 16
    z.writestr(info, data)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="", help="comma-separated decoder ids")
    ap.add_argument("--out", default=str(OUT))
    args = ap.parse_args()
    only = {x.strip() for x in args.only.split(",") if x.strip()}
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    entries = json.loads(INDEX.read_text(encoding="utf-8"))
    by_id = {e["id"]: e for e in entries}
    built = 0
    for tier, (package, cwd) in TIERS.items():
        if only and tier not in only:
            continue
        entry = by_id.get(tier)
        if entry is None:
            print("skip %s: no index entry" % tier)
            continue
        missing = [key for key in ("version", "size", "sha256", "released", "gitee", "github")
                   if not entry.get(key)]
        if missing:
            raise SystemExit(
                "refusing to package %s: decoders.json has empty %s; "
                "set the release URL/date before packaging" % (tier, ", ".join(missing))
            )
        ver = entry["version"]
        dll_name = f"{tier}_{ver}.dll"
        dll = DIST / dll_name
        if not dll.is_file():
            raise SystemExit("missing DLL: %s (run build.ps1 first)" % dll)
        # 包内 README = 仓库里那档的 README 原文 + 本脚本追加的「许可与源码」一节。
        # 缺 README 直接拒打包：那等于发布物少了一份门面，静默补一份空的不如不出包。
        tier_readme = ROOT / "workspace" / "decoders" / tier / "README.md"
        if not tier_readme.is_file():
            raise SystemExit(
                "refusing to package %s: %s 不存在" % (tier, tier_readme.relative_to(ROOT))
            )
        deps, packages = deps_for(cwd, package)
        pkg_name = f"{tier}_{ver}"
        stage = out / (pkg_name + ".stage")
        if stage.exists():
            shutil.rmtree(stage)
        folder = stage / pkg_name
        folder.mkdir(parents=True)
        shutil.copy2(dll, stage / dll_name)
        shutil.copy2(LICENSE, folder / "LICENSE")
        (folder / "metadata.json").write_text(
            json.dumps(entry, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
        lic_dir = folder / "licenses"
        lic_dir.mkdir()
        ids = sorted({lid for key in deps for lid in chosen_license_ids(packages[key].get("license") or "")})
        for lid in ids:
            if lid == "LicenseRef-Imazen-Commercial":
                continue
            found = None
            for key in deps:
                p = packages[key]
                if lid not in chosen_license_ids(p.get("license") or ""):
                    continue
                found = license_text(Path(p["manifest_path"]).parent, lid)
                if found:
                    break
            if found is None and lid == "CC0-1.0":
                fallback = Path.home() / ".cargo/registry/src/rsproxy.cn-e3de039b2554c837/constant_time_eq-0.4.2/LICENSE-CC0"
                if fallback.is_file():
                    found = fallback
            if found is None and lid == "Unlicense":
                fallback = Path.home() / ".cargo/registry/src/rsproxy.cn-e3de039b2554c837/byteorder-lite-0.1.0/UNLICENSE"
                if fallback.is_file():
                    found = fallback
            if found is None:
                raise SystemExit(f"{tier}: no license text for {lid}")
            shutil.copy2(found, lic_dir / (lid + ".txt"))
        rows = []
        for n, v in sorted(deps):
            p = packages[(n, v)]
            rows.append(f"| `{n}` | {v} | {p.get('license') or p.get('license_file') or 'see package'} |")
        notices = f"""# Third-party notices for `{dll_name}`

This DLL is distributed under AGPL-3.0. Third-party crates remain under their
own licenses. Full license texts are in `licenses/`.

| Crate | Version | License expression |
|---|---:|---|
""" + "\n".join(rows) + "\n"
        (folder / "THIRD-PARTY-NOTICES.md").write_text(notices, encoding="utf-8", newline="\n")
        (folder / "README.md").write_text(
            tier_readme.read_text(encoding="utf-8").rstrip() + "\n" + license_section(tier, ver),
            encoding="utf-8", newline="\n")
        zip_path = out / (pkg_name + ".zip")
        with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, strict_timestamps=False) as z:
            add_entry(z, dll_name, (stage / dll_name).read_bytes())
            for p in sorted(folder.rglob("*")):
                if p.is_file():
                    add_entry(z, p.relative_to(stage).as_posix(), p.read_bytes())
        digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
        print(f"{zip_path}  {zip_path.stat().st_size} B  sha256={digest}  deps={len(deps)} licenses={len(ids)}")
        built += 1
    if not built:
        raise SystemExit("no packages built")
    return 0


if __name__ == "__main__":
    sys.exit(main())
