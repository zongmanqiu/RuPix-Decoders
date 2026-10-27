# pro 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.2.1 — 2026-10-27 — abi 5

- **变更**：JXR 无 alpha（24bpp RGB）输入不再错误地请求 RGBA；先解 RGB 再补 alpha=255，
  普通 JPEG XR 现在可打开。带 alpha 的 BGRA JXR 路径保持不变。
- **证据**：`pro-jxr` 的 `decodes_no_alpha_jxr_as_rgba` 使用 `testdata/golden/jxr_rgb_64x64.jxr`；
  真实 800x600 BGRA fixture 仍通过档端到端。
- **认领**：后缀、成员与能力位不变。

## 0.2.0 — 2026-10-27 — abi 5

- **变更**：DLL 自述增加可扩展 `iv_metadata()`（version/author/license/released）；
  包格式改为 ZIP + 同版本许可证目录。
- **首版内容**：33 个后缀、11 个成员；`pro-small` 的 WBMP 探测修复；独立 workspace；
  纯 Rust；已知空白见 `README.md`。
- **认领**：33 个后缀、成员与能力位不变。
