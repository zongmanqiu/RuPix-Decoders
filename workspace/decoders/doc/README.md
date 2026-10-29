# doc — 常见文档 / CAD 档

一枚 DLL，承接日常最常见的文档类格式。旧版矢量 `CMX/CCX、WPG、ZMF、ZBR`
已拆到独立的 [`doc-legacy`](../doc-legacy/README.md)，避免默认下载背着约 50 MB 的
C++ 静态库。

当前 **30 个后缀 / 11 个格式组**：

| 成员 | 后缀 | 库 | 语言 | 许可 |
|---|---|---|---|---|
| `cad` | dwg/dwt/dxf | `acadrust` + `dxf` + `dxf_to_svg` | Rust | `acadrust` MPL-2.0；`dxf` / `dxf_to_svg` MIT |
| `meta` | wmf/wmz | 复用 `pro-wmf`（`wmf-core` + resvg） | Rust | MIT / Apache-2.0 / MPL-2.0 |
| `djvu` | djvu/djv | 复用 `pro-djvu`（`djvu-rs`） | Rust | MIT |
| `emf` | emf/emz | `emfsdk`（render） | Rust | MIT OR Apache-2.0 |
| `hpgl` | hp/hpg/plo/hg/hgl/plt/hp1/hp2/hpgl/hpgl2/gl2 | `nom-hpgl` + resvg | Rust | Apache-2.0 / MPL-2.0 |
| `pdf` | pdf | `hayro` | Rust | Apache-2.0 OR MIT |
| `print` | ps/ps1/ps2/ps3/prn/eps/ai | `stet` | Rust | Apache-2.0 OR MIT |
| `xps` | xps/oxps | `justpdf-formats`（xps） | Rust | MIT OR Apache-2.0 |

DWG/DXF 使用纯 Rust `acadrust`；PDF/PS/EPS/AI/XPS 也都由纯 Rust 库处理。
`doc` 仍用 MSVC 构建，不依赖 MSYS2 或 C++ 工具链。

## 约束

- 只发布一枚 `doc.dll`；不新增外部解码器 DLL。
- 当前 `is_vector=0`（混合档），能力位只有 `CAP_FULL`；暂不声明 `CAP_REGION`。
- ABI v6；字体兜底、越界透明按 `docs/vector-decoder.md` 执行。
- 所有第三方许可证和源码提供义务必须随 ZIP 附上；见仓库根 `THIRD-PARTY-NOTICES.md`。
- 旧版矢量格式去 `doc-legacy`，不要把 `libcdr`/`libwpg`/`libzmf` 重新编回本档。

## 认领规则（iv_probe）

`doc` 的 `iv_probe` 由装配层按 `MEMBERS` 顺序调用各成员：成员先看魔数，
需要后缀的成员（HPGL、XPS、PS/EPS/AI）再核对后缀；两者都满足才返回 `100`。
`.wmz/.emz` 这类通用 gzip 包装只在后缀点名时认领，普通 `.wmf/.emf` 按魔数认领。
认领失败返回 `0`，让宿主换下一家；认领成功但解析失败返回 `ERR_CORRUPT`。

## 测试

```powershell
cd workspace/decoders/doc
cargo test --release -- --test-threads=1
```

已用真实样本验证：WMF/WMZ、DjVu、PDF、PS、EPS、HPGL、DXF/DWG、XPS。
