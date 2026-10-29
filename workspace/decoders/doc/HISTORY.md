# doc 发布历史

## 0.1.0 — 2026-10-29 — abi 6

- 新增 `doc` 合并档，承载常见文档/CAD 格式：
  PDF、PS/EPS/AI、DWG/DXF、WMF/WMZ、EMF/EMZ、HPGL、XPS/OXPS、DjVu/DjV。
- DWG/DXF 使用纯 Rust `acadrust` + `dxf` + `dxf_to_svg`。
- PDF 使用 `hayro`，PS/EPS/AI 使用 `stet`，XPS 使用 `justpdf-formats`。
- WMF/WMZ、DjVu/DjV 从 `pro` 迁入；`pro` 不再声明 `CAP_REGION`。
- 旧版矢量 CMX/CCX、WPG、ZMF、ZBR 拆到独立 `doc-legacy`。
- 当前为混合档：`is_vector=0`、`CAP_FULL`；区域渲染暂未声明。
