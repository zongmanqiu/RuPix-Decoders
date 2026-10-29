# `pro` — 专业格式档（独立 workspace）

一枚 DLL 认领 **16 个后缀**：JPEG2000 家族、JPEG XR 家族、EXR、PSD、HDR、XCF、ASE/ASEPRITE。
纯 Rust，无 C 依赖；当前源码版本 0.4.2（发布状态见 `decoders.json`，远端仍为旧版）。

> **独立 workspace**：`workspace/decoders/pro` 有自己的 `[workspace]` / `Cargo.lock` / `.cargo/config.toml`。
> 不能并回根 workspace——它的 `image` / `exr` / `resvg` 依赖会与 common/raw 做 feature 统一，
> 把 EXR/AVIF 等后端无差别编进每一枚 DLL（2026-10-25 实测 common +685 KB、raw +834 KB）。

## 成员与后缀

| 成员 | 后缀 | 实现 |
|---|---|---|
| `pro-jp2` | jp2 j2k j2c jpc jpx jpf jpt | `hayro-jpeg2000`（纯 Rust） |
| `pro-jxr` | jxr wdp hdp | `jxr =0.2.1`（纯 Rust） |
| `pro-exr` | exr | `exr`（纯 Rust，线性 f32 → sRGB） |
| `pro-psd` | psd | `zune-psd`（头）+ 自解像素；8/16-bit RGB/RGBA/灰度 |
| `pro-hdr` | hdr | `image` 的 `hdr` feature（Radiance RGBE → 线性 f32 → u8） |
| `pro-xcf` | xcf | `xcf`（取面积最大图层） |
| `pro-ase` | ase aseprite | `aseprite-loader`（第 0 帧可见层） |

## 已移出 `pro`

- `ktx` / `ktx2` → 目标 `texture`。
- `dcm` / `dicom`、`nii`、`nrrd`、`mrc` / `mrcs` / `ccp4` / `map` → 目标 `medical`。
- `icns` → `common`；`pcx/pcc/dcx/wbmp/xbm` → `classic` 0.1.0。
- `wmf` / `wmz`、`djvu` / `djv` → `doc`。
- 上述移出项不再出现在本档 `extensions`；分别归 `texture` / `medical`。

## 认领（`iv_probe`）

每个成员先按自己的魔数/文件头认领；`ext = NULL` 时同样按内容认领。
`pro` 是把这些成员装进一枚 DLL，探针取最高分、渲染走让位链。

## 已知缺口

- `pict/pct`：`pict` crate 需要 nightly（`allocator_api`），本仓稳定工具链不引。
- `swf`：`swf` crate 只解析不渲染，没有纯 Rust 光栅器。
- PSD 只取合成预览/像素，不展开完整图层树；XCF 不合成图层偏移/蒙版；Aseprite 只取第 0 帧。

## 测试

```powershell
cd workspace/decoders/pro
cargo test --release --workspace -- --test-threads=1
```

各成员自己的测试在 `workspace/singles/pro-*/`；`pro` 只验装配与路由。

## 历史

对外可见变化见 [`HISTORY.md`](HISTORY.md)。本页只保留当前状态、成员与限制。
