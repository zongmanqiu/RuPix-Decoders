#!/usr/bin/env python3
"""生成 decoders.json —— 宿主「解码器 / 可下载」卡读的那份清单。

# 清单长什么样

**顶层就是一个数组，一项一枚 DLL。** 没有 `schema`、没有 `decoders`、没有 `updated`
包裹层——那些只是把"一个列表"翻译成"一个对象的字段"，宿主每读一次都要多剥两层，
人眼也得先跳过三行才知道正文在哪。清单里只有一种东西（DLL），那就只写这一种东西。

每项十三个字段，顺序固定：

    id        解码器标识（小写；停用表与下载目录都按它认人，**不从文件名反推**）
    name      解码器名称（DLL 自述的 display_name，**不是 DLL 文件名**）
    version   版本号（`<id>_<ver>.dll` 的 ver 段，build.ps1 从 [package] 盖的章）
    abi       ABI 大版本（`iv_abi_version()`，宿主按它决定收不收）
    size      字节数（下载后先按它挡截断/多收）
    sha256    DLL 的 SHA-256（小写十六进制；下载后逐字节核对，挡同字节数替换）
    released  发布时间 `YYYY-MM-DD`（未发布的项为空串）
    desc      一句话说明（DLL 自述）
    formats   认领的扩展名，**分号分隔、不带点**（与 ABI `IvDecoderInfo.extensions`
              同一个形状：`jpg;jpeg;jpe`——宿主读自述时怎么切，这里就怎么切）
    author    作者标识（来自 DLL 可选导出 `iv_metadata()`；旧 DLL 为空串）
    license   许可证标识（来自 DLL 可选导出 `iv_metadata()`；旧 DLL 为空串）
    gitee     DLL 下载地址（Gitee 源；未发布为空串）
    github    DLL 下载地址（GitHub 源；未发布为空串；默认由 gitee 推导，可显式覆盖）

# 为什么有 sha256、没有 file

`sha256` 与 `size` 不是二选一：`size` 挡截断/多收（用户能一眼看懂"清单写 1,967,104 B，
你下到 1,048,576 B"），`sha256` 挡**同字节数的替换/镜像篡改**；两者都由宿主在下载后核对。
`file` 仍然不写：文件名恒等于 `<id>_<version>.dll`，能从另外两个字段算出来。

# 生成器拥有什么、你可以手改什么

- **生成器覆盖**：id / name / version / abi / size / sha256 / desc / formats / author / license —— 全部从**发布目录里
  那枚 DLL 自己**读出来（`iv_decoder_info`），所以"清单说 A、DLL 说 B"这种脱节
  **物理上不可能发生**，不需要靠纪律维持。
- **你可以手改**：`gitee`、`github`、`released`。生成器没有网络，无从判断某个 release
  到底存不存在，所以它**不臆造地址**：缺 gitee 地址的项 `gitee` 留空，收尾会列成
  `MISSING gitee`；资产由人工上传，本仓不做网络校验。手填过的地址会被**原样保留**。
  `github` 默认按 gitee 推导（`derive_github_url`：**主机名和 owner 一起换**——Gitee
  `qiuzongman` / GitHub `zongmanqiu` 不是同一用户名，只换主机名会得到 404）。显式填了
  就覆盖推导值（`--set-github-url <id>=<url>`）。
  `released` 同理：已填的保留；`gitee` 为空的项一律清空（没发布就没有发布日）。

# 用法

    py tools/make_index.py                 # 用默认发布目录 <repo>/../.temp/decoder-dist
    py tools/make_index.py --dist DIR      # 指定发布目录（RUPIX_DIST_DIR 也行）
    py tools/make_index.py --out FILE      # 指定输出（默认 <repo>/decoders.json）
    py tools/make_index.py --set-url common=https://...   # 填 gitee 地址（可重复，自动推导 github）
    py tools/make_index.py --set-github-url common=https://...  # 显式覆盖 github（可重复；ID= 清空）

★ 用 **`py`** 跑，不要用 `python`：本机 `python` 是 WorkBuddy 自带的那套，加载不了
x64 DLL（ctypes 拿不到自述）。脚本会先自检位数再动手。
"""

from __future__ import annotations

import argparse
import ctypes
import datetime
import hashlib
import json
import os
import pathlib
import sys

# 清单里的字段顺序 = 契约。改顺序会让 git diff 变得难读，也可能骗过"我只看了一眼"的评审。
FIELDS = ("id", "name", "version", "abi", "size", "sha256", "released", "desc", "formats", "author", "license", "gitee", "github")


def derive_github_url(gitee_url: str) -> str:
    """从 Gitee 下载地址推 GitHub 地址：**主机名和 owner 一起换**。

    Gitee owner = `qiuzongman`，GitHub owner = `zongmanqiu`，不是同一个用户名；
    只把 `gitee.com` 换成 `github.com` 会得到 404（2026-10-26 实测）。对不上本仓前缀
    的地址返回空串——生成器不臆造地址。
    """
    if not gitee_url:
        return ""
    pairs = (
        ("https://gitee.com/qiuzongman/", "https://github.com/zongmanqiu/"),
        (
            "https://raw.giteeusercontent.com/qiuzongman/",
            "https://raw.githubusercontent.com/zongmanqiu/",
        ),
    )
    for old, new in pairs:
        if gitee_url.startswith(old):
            return new + gitee_url[len(old):]
    return ""


ROOT = pathlib.Path(__file__).resolve().parent.parent


class IvDecoderInfo(ctypes.Structure):
    """`rupix-abi` 的 `IvDecoderInfo`（`#[repr(C)]`，字段顺序即布局）。"""

    _fields_ = [
        ("id", ctypes.c_char_p),
        ("display_name", ctypes.c_char_p),
        ("abi_version", ctypes.c_uint32),
        ("extensions", ctypes.c_char_p),
        ("is_vector", ctypes.c_int32),
        ("has_alpha", ctypes.c_int32),
        ("description", ctypes.c_char_p),
    ]


class IvMetadataField(ctypes.Structure):
    _fields_ = [("key", ctypes.c_char_p), ("value", ctypes.c_char_p)]


class IvMetadata(ctypes.Structure):
    _fields_ = [
        ("size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("field_count", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
        ("fields", ctypes.POINTER(IvMetadataField)),
    ]


def read_metadata(ptr):
    """Read the optional `iv_metadata` key/value table; absent/invalid = empty."""
    if not ptr:
        return {}
    meta = ptr.contents
    if meta.size < ctypes.sizeof(IvMetadata) or meta.field_count > 256:
        return {}
    if meta.field_count and not meta.fields:
        return {}
    out = {}
    for i in range(meta.field_count):
        field = meta.fields[i]
        if not field.key or not field.value:
            continue
        key = field.key.decode("utf-8", "replace")
        if key and key not in out:
            out[key] = field.value.decode("utf-8", "replace")
    return out


def read_self_description(dll: pathlib.Path) -> dict:
    """加载 DLL，读它自己的 `iv_decoder_info()`。"""
    lib = ctypes.CDLL(str(dll.resolve()))
    lib.iv_decoder_info.restype = ctypes.POINTER(IvDecoderInfo)
    lib.iv_abi_version.restype = ctypes.c_uint32
    info = lib.iv_decoder_info().contents
    metadata = {}
    try:
        lib.iv_metadata.restype = ctypes.POINTER(IvMetadata)
        metadata = read_metadata(lib.iv_metadata())
    except AttributeError:
        pass  # optional export: older decoder / third-party DLL
    return {
        "id": info.id.decode("utf-8"),
        "name": info.display_name.decode("utf-8"),
        "abi": int(info.abi_version),
        "formats": info.extensions.decode("utf-8"),
        "desc": info.description.decode("utf-8"),
        "version": metadata.get("version", ""),
        "released": metadata.get("released", ""),
        "author": metadata.get("author", ""),
        "license": metadata.get("license", ""),
        "abi_fn": int(lib.iv_abi_version()),
    }


def parse_file_name(name: str) -> tuple[str, str] | None:
    """`common_0.1.0.dll` → (`common`, `0.1.0`)。不合形状返回 None（杂项夹具）。"""
    if not name.lower().endswith(".dll"):
        return None
    stem = name[:-4]
    if "_" not in stem:
        return None
    pid, ver = stem.rsplit("_", 1)
    if not pid or not ver or not (ver[0].isdigit()):
        return None
    return pid, ver


def load_previous(path: pathlib.Path) -> dict[str, dict]:
    """读上一份清单，按 id 建索引——`gitee` / `github` / `released` / 条目顺序从它继承。

    兼容旧结构（`{"decoders": [...]}`、单 `url` 字段）：这次结构精简之前留下的文件也能
    直接吃，省得手上一堆"结构改了、地址全丢"的返工。`url` 字段会被迁移成 `gitee`
    （`github` 按 `derive_github_url` 推导：主机名 + owner 一起换）。
    """
    if not path.is_file():
        return {}
    try:
        raw = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, ValueError) as e:
        print(f"  (上一份清单读不出来，当作空的：{e})")
        return {}
    entries = raw.get("decoders", []) if isinstance(raw, dict) else raw
    return {str(e.get("id")): e for e in entries if isinstance(e, dict) and e.get("id")}


def main() -> int:
    ap = argparse.ArgumentParser(description="生成 decoders.json（宿主可下载清单）")
    ap.add_argument("--dist", default=os.environ.get("RUPIX_DIST_DIR") or (ROOT.parent / ".temp" / "decoder-dist"))
    ap.add_argument("--out", default=ROOT / "decoders.json")
    ap.add_argument("--set-url", action="append", default=[], metavar="ID=URL",
                    help="顺手写入 gitee 下载地址（可重复；自动推导 github；写进去的地址之后每次生成都会被保留）")
    ap.add_argument("--set-github-url", action="append", default=[], metavar="ID=URL",
                    help="显式覆盖 github 下载地址（可重复；ID= 清空该来源推导值）")
    args = ap.parse_args()

    if ctypes.sizeof(ctypes.c_void_p) != 8:
        print("错误：请用 64 位解释器跑（本机 64 位 DLL 无法被 32 位进程加载）。", file=sys.stderr)
        return 1

    dist = pathlib.Path(args.dist).resolve()
    out = pathlib.Path(args.out).resolve()
    if not dist.is_dir():
        print(f"错误：发布目录不存在 —— {dist}\n  先跑 build.ps1。", file=sys.stderr)
        return 1

    previous = load_previous(out)
    today = datetime.date.today().isoformat()

    # 已知 id（decode 源码目录名）——发布目录里混进来的夹具/改名残留一律跳过。
    known = set()
    for base in (ROOT / "workspace" / "decoders", ROOT / "decoders-cpp" / "decoders"):
        if base.is_dir():
            known |= {p.name for p in base.iterdir() if p.is_dir()}

    entries: list[dict] = []
    skipped: list[str] = []
    for dll in sorted(dist.glob("*.dll")):
        parsed = parse_file_name(dll.name)
        if parsed is None:
            skipped.append(f"{dll.name}（文件名不是 <id>_<version>.dll）")
            continue
        pid, version = parsed
        if pid not in known:
            skipped.append(f"{dll.name}（decoders-*/decoders/ 下没有 {pid} 这个档）")
            continue

        sd = read_self_description(dll)
        if sd["abi"] != sd["abi_fn"]:
            print(f"错误：{dll.name} 的 abi_version 字段 {sd['abi']} 与 iv_abi_version() {sd['abi_fn']} 不一致",
                  file=sys.stderr)
            return 1
        if sd["id"] != pid:
            print(f"错误：{dll.name} 的自述 id 是 {sd['id']!r}，与文件名不符", file=sys.stderr)
            return 1
        if sd["version"] and sd["version"] != version:
            print(f"错误：{dll.name} 的自述版本是 {sd['version']!r}，与文件名 {version!r} 不符",
                  file=sys.stderr)
            return 1

        prev = previous.get(pid) or {}
        # A version bump invalidates the previous release URL: keeping it would make
        # the new manifest point at the old artifact. Upload/set the new URL explicitly.
        if prev.get("version") and prev.get("version") != version:
            prev = {}
        gitee = prev.get("gitee") or ""
        if isinstance(gitee, dict):                     # 旧结构：{"gitee": ..., "github": ...}
            gitee = gitee.get("gitee") or gitee.get("github") or ""
        elif not gitee and isinstance(prev.get("url"), str):   # 更旧：单 url 字段
            gitee = prev["url"]
        github = prev.get("github") or ""
        if isinstance(github, dict):
            github = github.get("github") or github.get("gitee") or ""
        if not github:
            github = derive_github_url(gitee)            # 主机名 + owner 一起换
        released = prev.get("released") or ""
        if not gitee:
            released = ""                               # 没发布就没有发布日
            github = ""                                 # 没发布就没有镜像地址
        elif not released:
            released = today

        entries.append({
            "id": sd["id"],
            "name": sd["name"],
            "version": version,
            "abi": sd["abi"],
            "size": dll.stat().st_size,
            "sha256": hashlib.sha256(dll.read_bytes()).hexdigest(),
            "released": released,
            "desc": sd["desc"],
            "formats": sd["formats"],
            "author": sd["author"],
            "license": sd["license"],
            "gitee": gitee,
            "github": github,
        })

    for item in args.set_url:
        pid, _, url = item.partition("=")
        hit = next((e for e in entries if e["id"] == pid.strip()), None)
        if hit is None:
            print(f"错误：--set-url 的 id {pid!r} 不在清单里", file=sys.stderr)
            return 1
        hit["gitee"] = url.strip()
        hit["github"] = derive_github_url(url.strip())   # 主机名 + owner 一起换
        hit["released"] = hit["released"] or today

    for item in args.set_github_url:
        pid, _, url = item.partition("=")
        hit = next((e for e in entries if e["id"] == pid.strip()), None)
        if hit is None:
            print(f"错误：--set-github-url 的 id {pid!r} 不在清单里", file=sys.stderr)
            return 1
        hit["github"] = url.strip()                      # 显式覆盖推导值（ID= 即清空）

    # 人工排过的顺序：按上一份清单里出现的先后，新条目接在后面。
    order = {pid: i for i, pid in enumerate(previous)}
    entries.sort(key=lambda e: (order.get(e["id"], len(order)), e["id"]))

    # 2 空格缩进、中文字符原样（不转 \uXXXX）、无 BOM、末尾一个换行。
    text = json.dumps(entries, indent=2, ensure_ascii=False) + "\n"
    out.write_text(text, encoding="utf-8", newline="\n")

    print(f"写入 {out}（{len(entries)} 枚，{len(text.encode('utf-8'))} B）")
    for e in entries:
        gstate = e["gitee"] or "未发布"
        hstate = e["github"] or "（无镜像）"
        print(f"  {e['id']:7} {e['version']:8} abi={e['abi']} {e['size']:>9,} B  "
              f"{len(e['formats'].split(';')):>2} 格式  gitee={gstate}  github={hstate}")
    for s in skipped:
        print(f"  跳过 {s}")
    missing = [e["id"] for e in entries if not e["gitee"]]
    if missing:
        print(f"MISSING gitee（还没上传 release）：{', '.join(missing)}")
        print("  上传后把 gitee 地址写进该条（或 --set-url <id>=<地址>），再跑一次即可；")
        print("  github 地址会按 gitee 推导（主机名 + owner 一起换）。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
