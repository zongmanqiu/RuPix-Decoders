# 第三方组件许可与说明（RuPix-Decoder）

> 本仓库自身以 **AGPL-3.0** 授权（`LICENSE`）。
> 本文件登记**随发布 DLL 分发的第三方解码库**及许可证；版本的权威全集是
> `Cargo.lock`（发布档 workspace）与 `workspace/singles/Cargo.lock`（单库 workspace）。
>
> **读取日期 2026-10-25；版本一律实读 `Cargo.toml` / `Cargo.lock`，不许凭记忆写。**
> 许可证依据：`[实读字段]` = crate 自己 `Cargo.toml` 的 `license`；`[实测]` = 仓内
> 下载 `.crate` 后的许可核验留档。分发二进制前，新引入的库必须补到本表。

## 一、发布档与成员依赖

| 档 | 版本 | 成员库 | 许可证 |
|---|---|---|---|
| `common` | 0.2.2 | `zune-jpeg` 0.5、`png` 0.18、`gif` 0.14、`tiff` 0.11、`zune-psd` 0.5、`image` 0.25（含 `hdr`）、`image-webp` 0.2、自研 `qoi` | 见 §二；`qoi` 为本仓自写，无第三方 |
| `modern` | 0.2.0 | `heif-oxide` 0.1、`zenavif` 0.1、`jxl-oxide` 0.12、`moxcms` 0.8、`rayon` 1.x | 见 §二；注意 `zenavif` / `rav1d-safe` 的 AGPL 分支 |
| `raw` | 0.5.1 | `rawler` 0.7.2、`flashraw` 0.1.1、自研 `rawpx` | `rawler` = **LGPL-2.1**（见 §三）；`flashraw` = MIT/Apache-2.0（X3F 回退）；`rawpx` 无第三方 |
| `pro` | 0.2.1 | `hayro-jpeg2000` 0.4、`jxr` 0.2、`exr` 1.74、`ktx2` 0.5 + `texture2ddecoder` 0.1、`dicom-pixeldata` 0.10、`image` 0.25、自研 NIfTI/NRRD/MRC、`pcx` 0.2、`wbmp` 0.1、`xbm` 0.3、`icns` 0.5、`xcf` 0.4、`aseprite-loader` 0.4、`djvu-rs` 0.42、`wmf-core` 0.1 + `resvg` 0.49 | 见 §二；`xcf` = Unlicense |
| `abi` / `router` | 0.5.0 / 0.1.0 | —（本仓自写） | AGPL-3.0 |

> 单库（`workspace/singles/*`）只用于 bench / A-B，**不发布**；它们的依赖是上面四档的子集，
> 不额外分发。四档均无 C 依赖。

## 二、第三方库

| 库 | 版本 | 许可证 | 依据 / 备注 |
|---|---|---|---|
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib | [实测] `jpeg` 整解 |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib | [实测] `zune-jpeg` / `zune-psd` 底座 |
| `zune-psd` | 0.5.1 | MIT OR Apache-2.0 OR Zlib | [实读字段] `psd` 头解析（像素由本仓自解） |
| `png` | 0.18.1 | MIT OR Apache-2.0 | [实测] |
| `gif` / `color_quant` | 0.14.2 / 1.1.0 | MIT OR Apache-2.0 | [实测] |
| `tiff` | 0.11.3 | MIT | [实测] |
| `image` | 0.25.10 | MIT OR Apache-2.0 | [实测]（`hdr` 走 image 的 `hdr` feature） |
| `image-webp` | 0.2.4 | MIT OR Apache-2.0 | [实测] `common` 的 WebP 纯 Rust 后端 |
| `flate2` / `miniz_oxide` / `fdeflate` / `simd-adler32` / `crc32fast` | 1.1.10 / 0.9.1 与 0.8.9 / 0.3.7 / 0.3.10 / 1.5.2 | MIT OR Apache-2.0 / MIT OR Zlib OR Apache-2.0 / MIT OR Apache-2.0 / MIT / MIT OR Apache-2.0 | [实测] 压缩与校验热路径 |
| `jxl-oxide` | 0.12.6 | MIT OR Apache-2.0 | [实测] |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 | [实测] |
| `rayon` | 1.12.0 | MIT OR Apache-2.0 | [实测]（由 rawler/jxl 传入；解码器代码不自建线程池） |
| `heif-oxide` | 0.1.0 | MIT OR Apache-2.0 | [实读字段] |
| `zenavif` | 0.1.6 | **AGPL-3.0-only OR 商业许可** | [实测] 本仓整体 AGPL-3.0，故走 AGPL 分支 |
| `zenavif-parse` | 0.6.2 | MPL-2.0 | [实测] |
| `rav1d-safe` | 0.5.7 | **AGPL-3.0-only OR LicenseRef-Imazen-Commercial** | [实测] 与 `zenavif` 同族；本项目 AGPL，故可用 |
| `rav1d-disjoint-mut` | 0.3.2 | BSD-2-Clause OR MIT OR Apache-2.0 | [实测] |
| `rust_h265` | 0.1.0 | MIT OR Apache-2.0 | [实测] |
| `yuv` | 0.8.19 | BSD-3-Clause OR Apache-2.0 | [实测] |
| `rgb` | 0.8.53 | MIT（单许可，不是双许可） | [实测] |
| `zenpixels` | 0.2.16 | Apache-2.0 OR MIT | [实测] |
| `rawler` | 0.7.2 | **LGPL-2.1** | [实读字段] 见 §三 |
| `flashraw` | 0.1.1 | MIT OR Apache-2.0 | [实读字段] X3F 回退；软件许可 |
| `hayro-jpeg2000` | 0.4.1 | Apache-2.0 OR MIT | [实读字段] `pro` 的 JP2/J2K |
| `jxr` | 0.2.1 | MIT OR Apache-2.0 | [实读字段] `pro` 的 JPEG XR |
| `exr` | 1.74.2 | BSD-3-Clause | [实读字段] `pro` 的 OpenEXR |
| `ktx2` / `texture2ddecoder` | 0.5.0 / 0.1.2 | Apache-2.0 / MIT OR Apache-2.0 | [实读字段] KTX2 容器 + BCn 解码 |
| `dicom-pixeldata` | 0.10.0 | MIT OR Apache-2.0 | [实读字段] DICOM 像素解码 |
| `pcx` / `wbmp` / `xbm` | 0.2.5 / 0.1.2 / 0.3.0 | MIT OR Apache-2.0 OR WTFPL / MIT OR Apache-2.0 / Apache-2.0 OR MIT | [实读字段] 小格式 |
| `icns` / `xcf` | 0.5.0 / 0.4.0 | MIT / Unlicense | [实读字段] macOS 图标 / GIMP 工程 |
| `aseprite-loader` / `djvu-rs` | 0.4.2 / 0.42.0 | MIT OR Apache-2.0 / MIT | [实读字段] Aseprite / DjVu |
| `wmf-core` / `resvg` / `tiny-skia` | 0.1.2 / 0.49.0 / 0.12.0 | MIT / Apache-2.0 OR MIT / BSD-3-Clause | [实读字段] WMF 矢量光栅化（与宿主 SVG 同栈但各自链接） |

## 三、LGPL / AGPL 组件说明

- **`rawler`（LGPL-2.1，固定 0.7.2）**：`rawler` 被静态编进 `raw` DLL，不再拆独立 DLL。
  发布包内含 LGPL-2.1 全文、`SOURCE-OFFER.md`（上游 commit `ff7a4524…`、crate 校验值、
  源码与重链接材料的书面提供说明）；不把 rawler 源码打进 ZIP。源码可直接取对应 crates.io 包
  `https://crates.io/crates/rawler/0.7.2` 或上游 dnglab 对应 commit。
- **`zenavif` / `rav1d-safe`（AGPL-3.0-only OR 商业许可）**：本仓库整体 AGPL-3.0，
  按 AGPL 分支使用；不再单独提供商业许可。`modern` 档的发布受同一许可约束。
- 所有解码库都编在各自 DLL 内；`rupix.exe` 本身只带 SVG（windui/resvg 栈，见宿主仓库
  `THIRD-PARTY-NOTICES.md`）。

## 四、分发义务

发布 release 时，release 说明必须写明 AGPL-3.0 并链接本文件；ZIP 内附 `LICENSE`、`THIRD-PARTY-NOTICES.md`、`licenses/` 与 `SOURCE-OFFER.md`。新增 / 更换解码库时：
1. 在 `workspace/singles/<id>/Cargo.toml` 与解码档 `Cargo.toml` 里实读版本与 `license`；
2. 更新本表，禁止 `[待核]` 长期挂着；
3. 若引入 C 库（历史如已撤销的 `jpegturbo`），同时登记其源码位置、版本与 sha256。
