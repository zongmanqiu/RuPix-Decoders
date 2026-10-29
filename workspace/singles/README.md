# 单库（workspace/singles）

33 个单库 = 每格式一枚的参考实现。它们同时是：

- 档的装配来源（档以 `default-features = false` 取 `*_impl`）；
- bench 横评的比对目标；
- A/B 排查时的现成实现。

**单库不发布**，也不单独写 `HISTORY.md`；历史由所属档覆盖。`README.md` 不是每个都必须有，
但本表必须登记；有独立实现细节/限制的单库应补 README。

## 总表

| 单库 | 库 / 实现 | 归属档 | README |
|---|---|---|---|
| `av` | heif-oxide + zenavif | modern | 有 |
| `classic` | image / image-extras / oxideav-pict / oxideav-iff | classic | 无（见 decoders/classic/README.md） |
| `core` | image + image-webp（GIF/APNG 动画会话也在此） | common | 有 |
| `doc-cad` | acadrust + dxf + dxf_to_svg | doc | 无（见 decoders/doc/README.md） |
| `doc-emf` | emfsdk + resvg | doc | 无 |
| `doc-hpgl` | nom-hpgl + resvg | doc | 无 |
| `doc-legacy` | libcdr / libwpg / libzmf（C++ 静态库）+ resvg | doc-legacy | 无 |
| `doc-pdf` | hayro | doc | 无 |
| `doc-print` | PostScript（hayro-postscript） | doc | 无 |
| `doc-xps` | justpdf-formats/xps | doc | 无 |
| `ff` | Farbfeld 自写 | modern | 无 |
| `gif` | gif | common | 有 |
| `icns` | image（icns）+ 尺寸档 | common | 无 |
| `jng` | jng | modern | 无 |
| `jpeg` | zune-jpeg | common | 有 |
| `jxl` | jxl-oxide | modern | 有 |
| `png` | png | common | 有 |
| `pro-ase` | aseprite-loader | pro | 无 |
| `pro-dicom` | dicom-pixeldata | pro | 无 |
| `pro-djvu` | djvu-rs | doc | 无 |
| `pro-exr` | exr | pro | 无 |
| `pro-hdr` | image 的 `hdr` feature | pro | 无 |
| `pro-jp2` | hayro-jpeg2000 | pro | 无 |
| `pro-jxr` | jxr | pro | 无 |
| `pro-psd` | zune-psd（头）+ 自解像素 | pro | 无 |
| `pro-sci` | 自写 NIfTI/NRRD/MRC | pro（无发布档认领） | 无 |
| `pro-wmf` | wmf-core + resvg | doc | 无 |
| `pro-xcf` | xcf | pro | 无 |
| `qoi` | 自研（零依赖） | modern | 有 |
| `rawpx` | 零依赖参数化裸数据 | raw | 有 |
| `texture` | image-extras/dds / ktx2 / texture2ddecoder / basisu / vtf / 自写 BLP | texture | 无（见 decoders/texture/README.md） |
| `texture-ktx2` | ktx2 + texture2ddecoder | texture | 无 |
| `tiff` | tiff | common | 有 |

缺 README 的 24 个：`classic`、`doc-cad`、`doc-emf`、`doc-hpgl`、`doc-legacy`、`doc-pdf`、
`doc-print`、`doc-xps`、`ff`、`icns`、`jng`、`pro-ase`、`pro-dicom`、`pro-djvu`、`pro-exr`、
`pro-hdr`、`pro-jp2`、`pro-jxr`、`pro-psd`、`pro-sci`、`pro-wmf`、`pro-xcf`、`texture`、
`texture-ktx2`。它们的当前行为由本表 + 所属档 README 覆盖；有独立限制的单库优先补。

`pro-sci` 与 `pro-dicom` 目前没有发布档认领（原 `medical` 档尚未建立），保留为 bench/A-B
与未来拆档的来源。

## 规则

- 目录名 = 解码器 id = `[lib] name`；不带连字符。
- `[features] default = ["standalone"]`；档依赖时关掉，避免 `LNK2005`。
- 单库不发布；`workspace/decoders/` 才是发布目录。
- 样本只放 `testdata/golden/` 或 `../.temp/testdata/`；单库目录不放图片。
- 单库测试与档端到端测试都要过；`build.ps1` 会跑 singles workspace。
