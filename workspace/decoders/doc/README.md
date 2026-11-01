# doc — 常见文档 / CAD 档

一枚 DLL，承接日常最常见的文档类格式。旧版矢量 `CMX/CCX、WPG、ZMF、ZBR`
已拆到独立的 [`doc-legacy`](../doc-legacy/README.md)，避免默认下载背着约 50 MB 的
C++ 静态库。

认领的后缀与成员如下（共 30 个后缀、8 个成员）：

| 成员 | 后缀 | 库 | 语言 | 许可 |
|---|---|---|---|---|
| `cad` | dwg/dwt/dxf | `acadrust` + `dxf` + `dxf_to_svg` | Rust | `acadrust` MPL-2.0；`dxf` / `dxf_to_svg` MIT |
| `meta` | wmf/wmz | 复用 `pro-wmf`（`wmf-core` + resvg） | Rust | MIT / Apache-2.0 / MPL-2.0 |
| `djvu` | djvu/djv | 复用 `pro-djvu`（`djvu-rs`） | Rust | MIT |
| `emf` | emf/emz | `emfsdk`（render） | Rust | MIT OR Apache-2.0 |
| `hpgl` | hp/hpg/plo/hg/hgl/plt/hp1/hp2/hpgl/hpgl2/gl2 | `nom-hpgl` + resvg | Rust | Apache-2.0 / MPL-2.0 |
| `pdf` | pdf | `hayro` | Rust | Apache-2.0 OR MIT |
| `print` | ps/ps1/ps2/ps3/prn/eps/ai | `stet` | Rust | Apache-2.0 OR MIT |
| `xps` | xps/oxps | 自研 FixedPage → SVG（`zip` + `roxmltree` + resvg） | Rust | MIT / MPL-2.0 |

DWG/DXF 使用纯 Rust `acadrust`；PDF/PS/EPS/AI/XPS 也都由纯 Rust 库处理。
`doc` 仍用 MSVC 构建，不依赖 MSYS2 或 C++ 工具链。

`.dxf` 输入**直接**交给 `dxf` crate（`acadrust` 只服务 DWG：它把 DWG 写成 DXF 是
必由之路）；`TEXT` 由本档自己排字——组码 40 是字高，SVG 的 `font-size` 取
`字高 ÷ 0.72`，字体栈带系统中文字体，`\U+XXXX` 与 R2007+ 的原生 UTF-8 两种写法都认。

HP-GL 的 `PC`（笔色）由本档自己扫一遍：`nom-hpgl` 不认它、且遇到不认识的命令就
停止解析，所以先把 `SP`/`PC` 摘成"笔号 → 颜色"表并从命令流里删掉，几何再交给它。
没有 `PC` 的文件按默认黑笔（绘图仪默认笔色是设备相关的，不臆造）。`LB` 标签按
`SI` 的字符高（`font-size` = 高 ÷ 0.72）与 `DI` 的角度排字，`LB` 正文里的 `;`
不算分隔符（`<ETX>` 才收尾）；整份文件先试 UTF-8，解不通再按 GBK 解一次。

EMF/EMZ 的 `emfsdk` 没有抗锯齿也没有视口参数：本档在预算允许时按 **2× 画布**请求、
再盒式降采样回目标尺寸（4 个样本一平均 = 抗锯齿），超预算退回 1×。它原本只用一款
通用无衬线、且忽略 `LOGFONTW` 的字族名（汉字会整段掉进 5×7 点阵），本档用的是
`/.3rd/emfsdk-0.2.0` 里**带补丁 E01** 的副本：系统字体只索引一次、按字族名优先、
逐字兜底（雅黑 / 宋体 / Noto CJK / 苹方 / 正黑 / MS Gothic / Malgun / Segoe UI）。

XPS 由本档自己渲染：OPC 链（`_rels/.rels` → `FixedDocumentSequence` →
`FixedDocument` → `FixedPage`）+ FixedPage 的 Path/Canvas/Glyphs 直接映射成 SVG。
`Image` 与 `ImageBrush` 暂不渲染；`Glyphs` 用系统字体近似（XPS 内嵌 ODTTF 字体
还没做反混淆），跨机像素可能不同——这是 ABI §十六 已登记的字体例外。

## 区域渲染（ABI v6）

`iv_render_region` 由 `cad` / `meta` / `hpgl` / `pdf` / `print` / `xps` 六个成员
提供：宿主只给视口那一块，解码器按目标密度**重画**（不是渲大图再缩）。
`emf`/`emz` 的 `emfsdk` 只有目标宽高、没有视口 API，因此不在 `region_exts` 里，
宿主对它们走整幅路线（m189 的整图重渲）。

## 约束

- 只发布一枚 `doc.dll`；不新增外部解码器 DLL。
- `is_vector=0`（混合档），能力位 `CAP_FULL | CAP_REGION`；哪些后缀真能按区域
  重画由 metadata `region_exts` 精确声明（EMF/EMZ 不在其中）。
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
