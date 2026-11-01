# doc-legacy 发布历史

## 0.2.0 — 2026-11-01 — abi 6

- **区域渲染（ABI v6）**：新增可选导出 `iv_render_region`，能力位改为
  `CAP_FULL | CAP_REGION`，metadata 新增成员级 `region_exts`=`cmx;ccx;wpg;zmf`。
  CMX/CCX、WPG、ZMF 的 SVG 场景按目标密度重画；宿主只请求视口那一块。
- **ZBR 不进区域通道**：它是文件内 100×100 的预览位图（没有矢量内容），
  区域请求返回 `ERR_UNSUPPORTED` 让位；宿主按 `region_exts` 也不会对它发请求。
- libcdr/libwpg/libzmf 转换出的 SVG 场景按字节指纹缓存：同一份文件连续平移/
  缩放不重复转换（契约 §十六 第 4 条）。

## 0.1.0 — 2026-10-29 — abi 6

- 从 `doc` 拆出 CMX/CCX、WPG、ZMF、ZBR，独立成 `doc-legacy`。
- C++ 静态库 `libcdr 0.1.9` / `libwpg 0.3.4` / `libzmf 0.0.2` + `librevenge 0.0.5`，
  通过 `extern "C"` shim 编入；构建目标 `x86_64-pc-windows-gnu`。
- ZBR 输出文件内 100×100 预览位图；其余格式走 SVG 后由 resvg 光栅化。
