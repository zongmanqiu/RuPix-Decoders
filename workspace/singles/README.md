# 单库（workspace/singles）

22 个单库 = 每格式一枚的参考实现。它们同时是：

- 档的装配来源（档以 `default-features = false` 取 `*_impl`）；
- bench 横评的比对目标；
- A/B 排查时的现成实现。

**单库不发布**，也不单独写 `HISTORY.md`；历史由所属档覆盖。`README.md` 不是每个都必须有，
但本表必须登记；有独立实现细节/限制的单库应补 README。

## 总表

| 单库 | 库 / 实现 | 归属档 | README |
|---|---|---|---|
| `av` | heif-oxide + zenavif | modern | 有 |
| `core` | image + image-webp | common | 有 |
| `gif` | gif | common | 有 |
| `hdr` | image 的 `hdr` feature | common | 缺 |
| `jpeg` | zune-jpeg | common | 有 |
| `jxl` | jxl-oxide | modern | 有 |
| `png` | png | common | 有 |
| `pro-ase` | aseprite-loader | pro | 缺 |
| `pro-dicom` | dicom-pixeldata | pro | 缺 |
| `pro-djvu` | djvu-rs | pro | 缺 |
| `pro-exr` | exr | pro | 缺 |
| `pro-jp2` | hayro-jpeg2000 | pro | 缺 |
| `pro-jxr` | jxr | pro | 缺 |
| `pro-ktx2` | ktx2 + texture2ddecoder | pro | 缺 |
| `pro-sci` | 自写 NIfTI/NRRD/MRC | pro | 缺 |
| `pro-small` | pcx / icns / wbmp / xbm | pro | 缺 |
| `pro-wmf` | wmf-core + resvg | pro | 缺 |
| `pro-xcf` | xcf | pro | 缺 |
| `psd` | zune-psd（头）+ 自解像素 | common | 缺 |
| `qoi` | 自研（零依赖） | common | 有 |
| `rawpx` | 零依赖参数化裸数据 | raw | 有 |
| `tiff` | tiff | common | 有 |

缺 README 的 13 个：`hdr`、`psd`、`pro-ase`、`pro-dicom`、`pro-djvu`、`pro-exr`、
`pro-jp2`、`pro-jxr`、`pro-ktx2`、`pro-sci`、`pro-small`、`pro-wmf`、`pro-xcf`。
它们的当前行为由本表 + 所属档 README 覆盖；有独立限制的单库优先补。

## 规则

- 目录名 = 解码器 id = `[lib] name`；不带连字符。
- `[features] default = ["standalone"]`；档依赖时关掉，避免 `LNK2005`。
- 单库不发布；`workspace/decoders/` 才是发布目录。
- 样本只放 `testdata/golden/` 或 `../.temp/testdata/`；单库目录不放图片。
- 单库测试与档端到端测试都要过；`build.ps1` 会跑 singles workspace。
