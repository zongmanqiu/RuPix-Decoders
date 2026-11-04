# 许可证与供应链

## 策略

本仓库整体 **AGPL-3.0**。发布 DLL 的依赖必须能在这个项目里分发；当前允许的 SPDX
家族：

解码器由宿主联网下载 ZIP；ZIP 内附 DLL、同版本目录中的 `README.md`、`LICENSE`、
`THIRD-PARTY-NOTICES.md`、`licenses/` 与 `metadata.json`。仓库根 `LICENSE` 是 AGPL-3.0 全文。
DLL 的自述键值表（`iv_metadata()`）带 `version` / `author=RuPix` / `license=AGPL-3.0` / `released`；第三方解码器可改自己的作者名，许可证说明不是许可证文件。

包内 `README.md` = 该档在仓库里的 README 原文 + 打包器追加的一节「许可与源码」：这枚 DLL
按 AGPL-3.0 分发、第三方 crate 各自的许可在哪查、对应源码 = tag `<id>_<ver>` 下的
`workspace/decoders/<id>/`（两个镜像各一条链接）。`raw` 那一节还多列 LGPL 的重链接材料
（上游 commit、crate 校验值、LGPL 正文位置）——那是 LGPL 要求的，不是仪式。
★ **以前这里是一枚单独的 `SOURCE-OFFER.md`，含"三年内书面提供源码"的要约**（2026-11-01 撤掉）：
本仓源码公开在两个镜像上，收件人不需要向我们索取，所以那张书面要约卡整个去掉，
只剩下"许可 + 源码在哪"这两件事实，并合进 README 一处，不再多一个文件。

- `MIT` / `Apache-2.0` / `BSD-2-Clause` / `BSD-3-Clause` / `BSL-1.0` / `0BSD` / `Zlib` / `ISC` 类；
- `CC0-1.0` / `Unlicense` / `WTFPL` / `MPL-2.0`（文件级 copyleft，动态分发可接受）；
- `LGPL-2.1` / `LGPL-2.1-or-later`（`rawler`；固定 0.7.2，单 DLL 静态链接，发布包附 LGPL 文本与 README 里的重链接材料）；
- `AGPL-3.0` / `AGPL-3.0-only`（本仓自身）；
- `LicenseRef-Imazen-Commercial`（`rav1d-safe` 家族；本项目按 AGPL 使用）；
- `IJG` / `NCSA` / `Unicode-3.0` / `Apache-2.0 WITH LLVM-exception`（上游特有条款）。

不在表里的许可证 = 门禁红；要新增必须同时改 `tools/check_licenses.py` 的
`ALLOWED` 和本文件，并在评审里说明分发依据。

## 组合兼容性（门禁不等于法律结论）

`check_licenses.py` 只回答“每个依赖的许可证 ID 在允许表里”，**不自动证明整枚 DLL 可以按 AGPL-3.0 分发**。
当前人工结论：

| 依赖类型 | 与 AGPL-3.0 组合的判断 |
|---|---|
| MIT / Apache-2.0 / BSD / Zlib / ISC / Unlicense / CC0 / WTFPL | 可作为 AGPL 作品的一部分分发；保留版权与许可声明 |
| MPL-2.0（`zenavif-parse`） | 可组合；MPL 文件本身继续受 MPL 约束，源码需可获得 |
| `zenavif` / `rav1d-safe` 的 AGPL 分支 | 与 AGPL 方向一致；`modern` 整枚 DLL 按 AGPL 分发，源码需可获得 |
| `rawler`（LGPL-2.1） | 固定 0.7.2，静态编进单枚 `raw` DLL；发布包附 LGPL-2.1 全文与包内 README 的「`raw` 的 LGPL 重链接材料」一节（上游 commit、crate 校验值、LGPL 正文位置），不内置 rawler 源码 |
| GPL-2.0-only / GPL-3.0-only | 当前不在允许表；引入前必须单独评估 |

`raw` 的发布包按上述固定版本 + README 里的重链接材料分发；依赖版本与源码位置由 `Cargo.lock` 锁定。

## 带补丁的第三方副本

少数第三方库的缺陷长在库内（绘制在库里完成、外部没有回调），只能按仓库规则在
**仓库外** `../.3rd/<name>-<ver>/` 留一份带补丁的副本，补丁逐条登记在该目录的
`SOURCE.md`（改了哪几段、为什么、上游原文的版本用来重做 diff）：

| 副本 | 许可 | 用在哪 | 补丁 |
|---|---|---|---|
| `emfsdk-0.2.0/` | MIT OR Apache-2.0 | `decoders/doc` 的 `emf`/`emz` 成员 | E01 字体栈 + 逐字兜底 + 字体库只索引一次；E02 笔宽换设备空间 + 描边裁剪 |

`.3rd` 在仓库外，所以**同一份补丁必须在本仓 `patches/<crate>-<version>/` 有公开登记**
（`README.md` + `manifest.json` + `*.patch`）：别人拿上游原文 + 这份补丁就能重建发布物，
这是"源码可用"承诺的一部分，不是可选文档。我们是**跟随上游的补丁，不是分叉**——
基线永远取上游同一版本，上游一变就重放补丁。
`tools/check_patches.py` 核三件可离线核的事：补丁 LF 行尾、补丁触及的路径与
`manifest.json` 的 `files` 一一对应、以及**上游核对日期没超过 90 天**（过期即红，逼一次人工核对）。

这些副本随 DLL 分发（crate 名与许可表达式由 `cargo metadata` 读出来，自动进
`THIRD-PARTY-NOTICES.md` 与 `licenses/`）；许可原文放在副本目录里，`tools/package_decoders.py`
打包时按名字取。补丁只加"缺的能力"，不改上游 API 形状，升级时按 `patches/` 与 `SOURCE.md`
两边同步的步骤重新叠加。

## 门禁

```powershell
python tools/check_licenses.py
```

它读全部七个 workspace 的完整 `cargo metadata`：

- 每个包必须有 `license` 或 `license_file`；
- `license` 按 SPDX 的 `OR` / `AND` / 旧式 `/` 拆分，每个 id 都必须在允许表里；
- 传递依赖也算，不只看直接依赖。

`build.ps1` 已接入；`violations: 0` 才算过。

## 未覆盖

`cargo-audit` / `cargo-deny` 本机未安装；本门禁只做**许可证**，不做漏洞公告。
有 `cargo audit` 时另跑：

```powershell
cargo audit
```

新增依赖、升级大版本、换许可证时，同步 `THIRD-PARTY-NOTICES.md` 与本文件。

## doc-legacy 的 C++ 静态成员

`libcdr` / `libzmf` / `librevenge` 为 MPL-2.0；`libwpg` 为 MPL-2.0 / LGPL-2.1+
（本项目按 MPL 分支使用，同时保留 LGPL 文本）。它们由 `.3rd` 源码静态编入
`doc.dll`；发布包必须附对应源码提供说明与许可证。Boost / ICU / libpng / lcms2 / zlib
按各自宽松许可证处理。
