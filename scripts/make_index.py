#!/usr/bin/env python3
"""生成 index.json —— 宿主「解码器 / 可下载」卡读的那份清单。

# 清单长什么样

**顶层就是一个数组，一项一枚 DLL。** 没有 `schema`、没有 `plugins`、没有 `updated`
包裹层——那些只是把"一个列表"翻译成"一个对象的字段"，宿主每读一次都要多剥两层，
人眼也得先跳过三行才知道正文在哪。清单里只有一种东西（DLL），那就只写这一种东西。

每项九个字段，顺序固定：

    id        解码器标识（小写；停用表与下载目录都按它认人，**不从文件名反推**）
    name      解码器名称（DLL 自述的 display_name，**不是 DLL 文件名**）
    version   版本号（`<id>_<ver>.dll` 的 ver 段，build.ps1 从 [package] 盖的章）
    abi       ABI 大版本（`iv_abi_version()`，宿主按它决定收不收）
    size      字节数（下载后按它比对，**这就是完整性校验**，见下）
    released  发布时间 `YYYY-MM-DD`（未发布的项为空串）
    desc      一句话说明（DLL 自述）
    formats   认领的扩展名，**分号分隔、不带点**（与 ABI `IvPluginInfo.extensions`
              同一个形状：`jpg;jpeg;jpe`——宿主读自述时怎么切，这里就怎么切）
    url       DLL 下载地址（未发布为空串）

# 为什么没有 sha256

早先每项带 `sha256` + `file`。两样都被 `size` 顶掉了：下载拿到的字节数不等于清单里的
`size`，那枚 DLL 就已经废了（下载中断 / 代理插了错误页），而**半截 DLL 必定过不了
`LoadLibrary` + 自述校验**，宿主那边本来就会拒绝安装。真出问题的时候，`size` 给出的
判据（"清单写 1,967,104 B，你下到 1,048,576 B"）比一串 64 位哈希更能让人一眼看懂。
`file` 同理：文件名恒等于 `<id>_<version>.dll`，能从另外两个字段算出来，写进清单只是
同一件事说两遍。

# 生成器拥有什么、你可以手改什么

- **生成器覆盖**：id / name / version / abi / size / desc / formats —— 全部从**发布目录里
  那枚 DLL 自己**读出来（`iv_plugin_info`），所以"清单说 A、DLL 说 B"这种脱节
  **物理上不可能发生**，不需要靠纪律维持。
- **你可以手改**：`url`、`released`。生成器没有网络，无从判断某个 release 到底存不存在，
  所以它**不臆造地址**：缺地址的项 `url` 留空，收尾会列成 `MISSING url`，由
  `scripts/check_urls.py` 真下载一遍核对。手填过的地址会被**原样保留**。
  `released` 同理：已填的保留；`url` 为空的项一律清空（没发布就没有发布日）。

# 用法

    py scripts/make_index.py                 # 用默认发布目录 <repo>/../.temp/decoder-dist
    py scripts/make_index.py --dist DIR      # 指定发布目录（RUPIX_DIST_DIR 也行）
    py scripts/make_index.py --out FILE      # 指定输出（默认 <repo>/index.json）
    py scripts/make_index.py --set-url common=https://...   # 顺手填地址（可重复）

★ 用 **`py`** 跑，不要用 `python`：本机 `python` 是 WorkBuddy 自带的那套，加载不了
x64 DLL（ctypes 拿不到自述）。脚本会先自检位数再动手。
"""

from __future__ import annotations

import argparse
import ctypes
import datetime
import json
import os
import pathlib
import sys

# 清单里的字段顺序 = 契约。改顺序会让 git diff 变得难读，也可能骗过"我只看了一眼"的评审。
FIELDS = ("id", "name", "version", "abi", "size", "released", "desc", "formats", "url")

ROOT = pathlib.Path(__file__).resolve().parent.parent


class IvPluginInfo(ctypes.Structure):
    """`rupix-abi` 的 `IvPluginInfo`（`#[repr(C)]`，字段顺序即布局）。"""

    _fields_ = [
        ("id", ctypes.c_char_p),
        ("display_name", ctypes.c_char_p),
        ("abi_version", ctypes.c_uint32),
        ("extensions", ctypes.c_char_p),
        ("is_vector", ctypes.c_int32),
        ("has_alpha", ctypes.c_int32),
        ("description", ctypes.c_char_p),
    ]


def read_self_description(dll: pathlib.Path) -> dict:
    """加载 DLL，读它自己的 `iv_plugin_info()`。"""
    lib = ctypes.CDLL(str(dll.resolve()))
    lib.iv_plugin_info.restype = ctypes.POINTER(IvPluginInfo)
    lib.iv_abi_version.restype = ctypes.c_uint32
    info = lib.iv_plugin_info().contents
    return {
        "id": info.id.decode("utf-8"),
        "name": info.display_name.decode("utf-8"),
        "abi": int(info.abi_version),
        "formats": info.extensions.decode("utf-8"),
        "desc": info.description.decode("utf-8"),
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
    """读上一份清单，按 id 建索引——`url` / `released` / 条目顺序从它继承。

    兼容旧结构（`{"plugins": [...]}`）：这次结构精简之前留下的文件也能直接吃，
    省得手上一堆"结构改了、地址全丢"的返工。
    """
    if not path.is_file():
        return {}
    try:
        raw = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, ValueError) as e:
        print(f"  (上一份清单读不出来，当作空的：{e})")
        return {}
    entries = raw.get("plugins", []) if isinstance(raw, dict) else raw
    return {str(e.get("id")): e for e in entries if isinstance(e, dict) and e.get("id")}


def main() -> int:
    ap = argparse.ArgumentParser(description="生成 index.json（宿主可下载清单）")
    ap.add_argument("--dist", default=os.environ.get("RUPIX_DIST_DIR") or (ROOT.parent / ".temp" / "decoder-dist"))
    ap.add_argument("--out", default=ROOT / "index.json")
    ap.add_argument("--set-url", action="append", default=[], metavar="ID=URL",
                    help="顺手写入下载地址（可重复；写进去的地址之后每次生成都会被保留）")
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
    known = {p.name for p in (ROOT / "decoders").iterdir() if p.is_dir()}

    entries: list[dict] = []
    skipped: list[str] = []
    for dll in sorted(dist.glob("*.dll")):
        parsed = parse_file_name(dll.name)
        if parsed is None:
            skipped.append(f"{dll.name}（文件名不是 <id>_<version>.dll）")
            continue
        pid, version = parsed
        if pid not in known:
            skipped.append(f"{dll.name}（decoders/ 下没有 {pid} 这个档）")
            continue

        sd = read_self_description(dll)
        if sd["abi"] != sd["abi_fn"]:
            print(f"错误：{dll.name} 的 abi_version 字段 {sd['abi']} 与 iv_abi_version() {sd['abi_fn']} 不一致",
                  file=sys.stderr)
            return 1
        if sd["id"] != pid:
            print(f"错误：{dll.name} 的自述 id 是 {sd['id']!r}，与文件名不符", file=sys.stderr)
            return 1

        prev = previous.get(pid) or {}
        url = prev.get("url") or ""
        if isinstance(url, dict):                       # 旧结构：{"gitee": ..., "github": ...}
            url = url.get("gitee") or url.get("github") or ""
        released = prev.get("released") or ""
        if not url:
            released = ""                               # 没发布就没有发布日
        elif not released:
            released = today

        entries.append({
            "id": sd["id"],
            "name": sd["name"],
            "version": version,
            "abi": sd["abi"],
            "size": dll.stat().st_size,
            "released": released,
            "desc": sd["desc"],
            "formats": sd["formats"],
            "url": url,
        })

    for item in args.set_url:
        pid, _, url = item.partition("=")
        hit = next((e for e in entries if e["id"] == pid.strip()), None)
        if hit is None:
            print(f"错误：--set-url 的 id {pid!r} 不在清单里", file=sys.stderr)
            return 1
        hit["url"] = url.strip()
        hit["released"] = hit["released"] or today

    # 人工排过的顺序：按上一份清单里出现的先后，新条目接在后面。
    order = {pid: i for i, pid in enumerate(previous)}
    entries.sort(key=lambda e: (order.get(e["id"], len(order)), e["id"]))

    # 2 空格缩进、中文字符原样（不转 \uXXXX）、无 BOM、末尾一个换行。
    text = json.dumps(entries, indent=2, ensure_ascii=False) + "\n"
    out.write_text(text, encoding="utf-8", newline="\n")

    print(f"写入 {out}（{len(entries)} 枚，{len(text.encode('utf-8'))} B）")
    for e in entries:
        state = e["url"] or "未发布"
        print(f"  {e['id']:7} {e['version']:8} abi={e['abi']} {e['size']:>9,} B  "
              f"{len(e['formats'].split(';')):>2} 格式  {state}")
    for s in skipped:
        print(f"  跳过 {s}")
    missing = [e["id"] for e in entries if not e["url"]]
    if missing:
        print(f"MISSING url（还没上传 release）：{', '.join(missing)}")
        print("  上传后把地址写进该条的 url（或 --set-url <id>=<地址>），再跑一次即可；地址会被保留。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
