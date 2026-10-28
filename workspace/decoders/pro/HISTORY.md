# pro 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.3.0 — 2026-10-28 — abi 6

- **变更**：① ABI 代次前移到 **6**（纯加法，既有导出签名与结构体布局逐字节未动）。② WMF/WMZ 成员实现可选导出 `iv_render_region`（**ABI v6 区域渲染**）：宿主给出"看得见的那一块"与目标画布，解码器按密度**重新光栅化**——矢量图放大到 4000% 也无损。③ 矢量路线改为**解码器自渲染**：不再实现 `iv_export_svg`、不再声明 `CAP_VECTOR`（矢量格式一律由解码器按目标尺寸渲染）。
- **能力位**：`CAP_FULL | CAP_REGION`。
- **证据**：`pro-wmf` 的 `region_render_is_rasterized_at_density_not_upscaled`（边缘锐利 = 重画而非插值）与 `region_outside_the_document_is_transparent`；pro 档的 `caps_declare_region_capability_and_not_vector_export` 与 `render_region_rejects_bytes_no_member_claims`。
- **回归**：后缀集合与像素格式不变；WMF/WMZ 仍走同一条 WMF → SVG → resvg 管线。

## 0.2.2 — 2026-10-28 — abi 5

- **变更**：WMF 成员新增可选导出 `iv_export_svg`（矢量场景导出为 UTF-8 SVG 文本字节，
  两阶段缓冲；`.wmz` 的 gzip 包装导出前先解包）并声明 `CAP_VECTOR`；`iv_render` 落实
  `req.target_w/target_h`（矢量场景等比缩进目标框，0 = 原始尺寸）；`iv_metadata` 末尾追加
  `vector_exts` = `wmf;wmz`（成员级矢量声明，合并档 `is_vector` 是 DLL 级单值）。
- **证据**：`pro-wmf` 的 `probe_claims_wmz_only_when_named` / `export_rejects_bytes_outside_the_vector_family`；
  pro 档的 `metadata_declares_member_level_vector_exts` / `export_svg_rejects_bytes_no_member_claims`。
- **认领**：后缀、成员不变；能力位新增 `CAP_VECTOR`（WMF/WMZ 成员）。
- **实现补齐（2026-10-28 同日）**：上面那条"声明 `CAP_VECTOR`"当时只写进了文档与本记录，
  `export_caps!` 实际还停在 `CAP_FULL` —— 宿主按规范 §十五「只应由声明了 `CAP_VECTOR` 的解码器
  实现 `iv_export_svg`」筛候选时，把整枚 DLL 排除掉，表现为「没有支持矢量文档导出的解码器认领
  wmf」。现补上 `CAP_FULL | CAP_VECTOR`，并加用例 `caps_declare_vector_export_capability` 钉住
  （版本号不变：与 0.2.2 同批落地）。

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
