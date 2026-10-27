# 许可证与供应链

## 策略

本仓库整体 **AGPL-3.0**。发布 DLL 的依赖必须能在这个项目里分发；当前允许的 SPDX
家族：

解码器由宿主联网下载 ZIP；ZIP 内附 DLL、同版本目录中的 `LICENSE`、`THIRD-PARTY-NOTICES.md`、
`licenses/` 与 `SOURCE-OFFER.md`。仓库根 `LICENSE` 是 AGPL-3.0 全文。
DLL 的自述键值表（`iv_metadata()`）带 `version` / `author=RuPix` / `license=AGPL-3.0` / `released`；第三方解码器可改自己的作者名，许可证说明不是许可证文件。

发布 ZIP 的单版本目录还含 `SOURCE-OFFER.md`：AGPL 源码位置、三年内书面提供源码的承诺、问题入口；
`raw` 还单独列出静态链接的 `rawler 0.7.2` 上游 commit、crate 校验值与重链接说明。
这是源码获取/重链接材料，不把第三方源码打进 ZIP，也不是法律意见。

- `MIT` / `Apache-2.0` / `BSD-2-Clause` / `BSD-3-Clause` / `0BSD` / `Zlib` / `ISC` 类；
- `CC0-1.0` / `Unlicense` / `WTFPL` / `MPL-2.0`（文件级 copyleft，动态分发可接受）；
- `LGPL-2.1` / `LGPL-2.1-or-later`（`rawler`；固定 0.7.2，单 DLL 静态链接，发布包附 LGPL 文本与 `SOURCE-OFFER.md`）；
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
| `rawler`（LGPL-2.1） | 固定 0.7.2，静态编进单枚 `raw` DLL；发布包附 LGPL-2.1 全文与 `SOURCE-OFFER.md`（上游 commit、crate 校验值、源码/重链接书面提供说明），不内置 rawler 源码 |
| GPL-2.0-only / GPL-3.0-only | 当前不在允许表；引入前必须单独评估 |

`raw` 的发布包按上述固定版本 + `SOURCE-OFFER.md` 方式分发；依赖版本与源码位置由 `Cargo.lock` 锁定。

## 门禁

```powershell
python tools/check_licenses.py
```

它读 root / singles / pro 三个 workspace 的完整 `cargo metadata`：

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
