# pro 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.2.1 — 2026-10-27 — abi 5

- **状态**：发布包已生成；等待 Gitee/GitHub 上传。
- **变更**：JXR 无 alpha（24bpp RGB）输入不再错误地请求 RGBA；先解 RGB 再补 alpha=255，
  普通 JPEG XR 现在可打开。带 alpha 的 BGRA JXR 路径保持不变。
- **证据**：`pro-jxr` 的 `decodes_no_alpha_jxr_as_rgba` 使用 `testdata/golden/jxr_rgb_64x64.jxr`；
  真实 800x600 BGRA fixture 仍通过档端到端。
- **认领**：后缀、成员与能力位不变。

## 0.2.0 — 2026-10-27 — abi 5

- **状态**：发布包已生成；等待 Gitee/GitHub 上传。
- **变更**：DLL 自述增加可扩展 `iv_metadata()`；包格式改为 ZIP + 同版本许可证目录。
- **认领**：33 个后缀、成员与能力位不变。

## 0.1.0 — 未发布 — abi 5

- **状态**：未发布。`index.json` 的 `released` / `gitee` / `github` 为空；
  DLL 已可本地构建。
- **WBMP 探测修复**：`pro-small` 的 XBM 文本分支不再用 `from_utf8(...)?` 提前返回；
  非 UTF-8 的二进制 WBMP 现在能被后续分支识别（回归：`wbmp_binary_is_recognized`）。
- **认领**：33 个后缀，覆盖四类专业/低频格式：JPEG2000、JPEG XR、EXR、KTX2、
  DICOM、科学影像（NIfTI/NRRD/MRC）、设计源文件（XCF/Aseprite）、WMF、DjVu。
- **成员**：11 个（pro-jp2、pro-jxr、pro-exr、pro-ktx2、pro-dicom、pro-sci、
  pro-small、pro-xcf、pro-ase、pro-djvu、pro-wmf）。
- **独立 workspace**：`decoders-rust/decoders/pro` 有自己的 `[workspace]` / `Cargo.lock` /
  `.cargo/config.toml`。并回根 workspace 会让 `image`/`exr`/`resvg` 的 feature
  统一把 EXR/AVIF 等后端编进 common/raw（实测 common +685 KB、raw +834 KB）。
- **纯 Rust**：无 C 依赖。
- **已知缺口**：`pict/pct` 需要 nightly；`swf` 没有纯 Rust 光栅器；
  KTX2 只支持未压缩 + BC1/BC3/BC7；NIfTI 只支持 NIfTI-1；NRRD 只支持内嵌数据；
  DICOM 只解首帧；XCF 不合成图层偏移/蒙版；Aseprite 只取第 0 帧。
- **证据**：`cargo test --release --workspace -- --test-threads=1`（在
  `decoders-rust/decoders/pro` 下）；各成员测试在 `decoders-rust/singles/pro-*/`。
- **升级建议**：首个版本；未发布。
