# `pro` — 专业/低频格式档（独立 workspace）

一枚 DLL 认领 **33 个后缀**，覆盖四类“日常不常用、但专业场景必须有”的格式。
构建体积 **8,267,776 B**（0.2.1，abi5，纯 Rust，无 C 依赖）；发布包已生成，等待上传。

> **独立 workspace**：`decoders-rust/decoders/pro` 有自己的 `[workspace]` / `Cargo.lock` / `.cargo/config.toml`。
> 不能并回根 workspace——它的 `image` / `exr` / `resvg` 依赖会与 common/raw 做 feature 统一，
> 把 EXR/AVIF 等后端无差别编进每一枚 DLL（2026-10-25 实测 common +685 KB、raw +834 KB）。

## 成员与后缀

| 成员 | 后缀 | 实现 |
|---|---|---|
| `pro-jp2` | jp2 j2k j2c jpc jpx jpf jpt | `hayro-jpeg2000`（纯 Rust） |
| `pro-jxr` | jxr wdp hdp | `jxr =0.2.1`（纯 Rust） |
| `pro-exr` | exr | `exr`（纯 Rust，线性 f32 → sRGB） |
| `pro-ktx2` | ktx ktx2 | KTX1 未压缩 RGBA/RGB/BGRA/R8 + KTX2（未压缩 + BC1/BC3/BC7） |
| `pro-dicom` | dcm dicom | `dicom-pixeldata`（首帧 + VOI normalize） |
| `pro-sci` | nii nrrd mrc mrcs ccp4 map | 自写解析（NIfTI-1 / NRRD / MRC 首层，min/max 拉伸） |
| `pro-small` | pcx pcc dcx icns wbmp xbm | `pcx` / `icns` / `wbmp` / `xbm` |
| `pro-xcf` | xcf | `xcf`（取面积最大图层） |
| `pro-ase` | ase aseprite | `aseprite-loader`（第 0 帧可见层） |
| `pro-djvu` | djvu djv | `djvu-rs`（MIT，纯 Rust） |
| `pro-wmf` | wmf | `wmf-core` 转 SVG + `resvg` 光栅化 |

## 认领（`iv_probe`）

每个成员先按自己的魔数/文件头认领；`ext = NULL` 时同样按内容认领。
`pro` 是把这些成员装进一枚 DLL，探针取最高分、渲染走让位链。
`map` 这种过于通用的后缀只有扩展名点名且 MRC 头结构成立才认。

## 已知缺口

- `pict/pct`：`pict` crate 需要 nightly（`allocator_api`），本仓稳定工具链不引。
- `swf`：`swf` crate 只解析不渲染，没有纯 Rust 光栅器。
- KTX2 只支持未压缩与 BC1/BC3/BC7；Basis Universal / ASTC 等返回 `UNSUPPORTED`。
- NIfTI 只支持 NIfTI-1（348 字节头）；NRRD 只支持内嵌数据；分离 `.nhdr` 需要读外部文件，宿主 ABI 不支持，因此不认领。
- DICOM 只解首帧；XCF 不合成图层偏移/蒙版；Aseprite 只取第 0 帧。

## 测试

```powershell
cd decoders-rust/decoders/pro
cargo test --release --workspace -- --test-threads=1
```

各成员自己的测试在 `decoders-rust/singles/pro-*/`；`pro` 只验装配与路由。

## 历史

对外可见变化见 [`HISTORY.md`](HISTORY.md)。本页只保留当前状态、成员与限制。
