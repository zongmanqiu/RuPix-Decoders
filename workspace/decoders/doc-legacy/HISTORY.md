# doc-legacy 发布历史

## 0.1.0 — 2026-10-29 — abi 6

- 从 `doc` 拆出 CMX/CCX、WPG、ZMF、ZBR，独立成 `doc-legacy`。
- C++ 静态库 `libcdr 0.1.9` / `libwpg 0.3.4` / `libzmf 0.0.2` + `librevenge 0.0.5`，
  通过 `extern "C"` shim 编入；构建目标 `x86_64-pc-windows-gnu`。
- ZBR 输出文件内 100×100 预览位图；其余格式走 SVG 后由 resvg 光栅化。
