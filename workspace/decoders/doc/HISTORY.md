# doc 发布历史

## 0.2.3 — 2026-11-04 — abi 6

- **修复（EMF/EMZ 卡死）**：`doc-emf` 用的 `emfsdk 0.2.0` 补丁副本新增 E02。上游把
  `EmfPen::width` 当**像素半径**直接用，而它是世界坐标单位；缩放比小的 EMF+ 文档
  （本机实测 0.00018）里一条 12700 单位的笔宽就成了 12700 像素半径，一个描边点
  1.6 亿次落笔、一张图十亿级写入——渲染永不返回，还堵死单线程解码队列，之后换图
  全部无响应。E02 把笔宽经 world/viewport 变换换到设备空间并夹到画布，线段先按笔宽
  外扩、用 Liang–Barsky 裁剪再走 Bresenham，落笔方框也夹到画布；同一文件修后 17 ms 出图，
  画面与原意一致（方块、细连线、文字）。
- 补丁的公开登记与重放步骤在 `patches/emfsdk-0.2.0/`，随包许可说明同步登记 E02。
- 未变：后缀集合、能力位、ABI、其余成员行为。

## 0.2.2 — 2026-11-01 — abi 6

HP-GL 中文显示排查带出的三件事（其余格式不动）：

- **HP-GL 文字**：`nom-hpgl` 本来就解析 `LB`/`SI`/`DI`，只是本档从来没用过它们——
  标签被当成未知命令丢掉。现在按 `SI` 的字符高（⇒ `font-size` = 高 ÷ 0.72）、
  `DI` 的角度排字，`LB` 之后笔位按标签宽度推进（HP-GL 语义）；`LB` 正文里可以
  出现 `;`（只有 `<ETX>` 收尾，按 `;` 切会把标签切碎）。整份文件先试 UTF-8、
  解不通再按 **GBK** 解一次——绘图仪年代的 PLT 中文基本都是 GBK。
- **★ "场景里有 `<text>`"不等于"屏上有字"**：本档建树用的是
  `usvg::Options::default()`，它的字体库是**空的**，`<text>` 会被静默丢掉。同档其它
  成员（CAD/WMF/XPS）都装了系统字体库，只有 HP-GL 这条漏了。现在装一份进程内
  只扫一次的字体库，并加了一条**落在像素上**的用例（老代码上必红）。
- **EMF/EMZ 中文**：`emfsdk` 只用一款通用无衬线、还完全忽略 `LOGFONTW::lfFaceName`，
  汉字的 `glyph_index` 拿不到就整段退回 5×7 点阵（ASCII 也一起掉进去）。库内没有
  字体回调、也没有视口 API，外部改不了 ⇒ 按仓库规则在 `/.3rd/emfsdk-0.2.0` 留一份
  **带补丁 E01 的副本**：系统字体按 `OnceLock` 只索引一次、数据走 mmap 共享（不复制
  进堆）、文档点名的字族排最前、其余按兜底栈**逐字**找字形，全都没有才退回点阵。
  判据是"两个不同的汉字必须画出不同的字形"——上游会把它们画成同一个方框。

## 0.2.1 — 2026-11-01 — abi 6

0.2.0 复测确认的三处遗留（其余格式确认无损放大）：

- **DXF 文字与解析路径**：`.dxf` 输入此前也走 `acadrust → DxfWriter → dxf` 的二次
  转换（DWG 才需要它），DXF 本来就是 DXF，这一圈只会丢字段；现在 `.dxf` 直接交给
  `dxf` crate。`TEXT` 改由本档自己排字（组码 40 是**字高**，SVG 的 `font-size` 取
  字高 ÷ 0.72；字体栈带系统中文字体），不再用 `dxf_to_svg` 那份没有 `font-size`
  的 `<text>`；`\U+XXXX` 与 R2007+ 的原生 UTF-8 两种写法都认。顺带把
  `dxf_to_svg` 写死的 1.0 padding 收回到内容框 + 4%（原先图纸只占中间九分之一）。
- **HPGL 笔色**：`nom-hpgl` 不解析 `PC`，而它的顶层解析遇到不认识的命令就停 ⇒
  带 `PC` 的真实文件会被截断。现在先按 `;` 切一遍取出 `SP`/`PC` 建立"笔号 → 颜色"
  表并从流里删掉 `PC`（连分隔符一起删，只删命令会留下空命令同样截断），几何再交给
  `nom-hpgl`；场景按笔色分组输出多段 path/circle。没有 `PC` 的文件仍按默认黑笔。
  描边宽度按画布尺寸给下限（1 单位在千级画布上只有亚像素宽）。
- **EMF/EMZ 抗锯齿**：`emfsdk` 的栅格器没有抗锯齿（`set_pixel` 与扫描线填充都写
  硬像素），文字轮廓放大后"坑坑洼洼"。它也没有视口参数可调，所以本档在预算允许
  （2× 画布 ≤ 12 MP）时按 2× 请求、拿到结果盒式降采样回目标尺寸；超预算退回 1×。

## 0.2.0 — 2026-11-01 — abi 6

- **区域渲染（ABI v6）**：新增可选导出 `iv_render_region`，能力位改为
  `CAP_FULL | CAP_REGION`，metadata 新增成员级 `region_exts`
  （`dwg;dwt;dxf;wmf;wmz;pdf;ps…ai;xps;oxps;hp…gl2`）。
  宿主据此只让**真能按密度重画**的后缀走区域通道；EMF/EMZ 仍走整幅
  （`emfsdk` 只有目标宽高、没有视口 API）。
- **DWG/DXF 颜色**：`dxf_to_svg` 取的是 DXF 组码 430（`color_name`，几乎永远为空），
  不是组码 62 的颜色索引 ⇒ 旧版整张图都按黑色画。现在先把 ACI（含 BYLAYER）
  折算成 CSS 颜色，再交给它；并补一张白纸底（DXF 本身没有背景）。
- **HPGL**：补白纸底；SVG 场景按字节指纹缓存。
- **PS/EPS/AI**：旧版固定 72 dpi 渲染后最近邻放大（放大即像素块）。现改为
  解释成 display list 后按目标密度重画（视口渲染），查询阶段不再光栅化；
  整图与区域两条路线都合成到白纸。
- **XPS/OXPS**：旧版 `justpdf-formats` 的 `render_page` 是占位实现，永远返回纯白页。
  改为自研 OPC 链（`_rels/.rels` → fdseq → fdoc → fpage）+ FixedPage → SVG：
  Path / Canvas / Glyphs（系统字体近似）、SolidColorBrush / LinearGradientBrush
  （径向取中间色标）、RenderTransform / Clip；`Image` 与 `ImageBrush` 暂不渲染。
- **PDF**：`Pdf` 解析结果进程内缓存；区域渲染用 `render_into` 的仿射变换按密度重画。

## 0.1.0 — 2026-10-29 — abi 6

- 新增 `doc` 合并档，承载常见文档/CAD 格式：
  PDF、PS/EPS/AI、DWG/DXF、WMF/WMZ、EMF/EMZ、HPGL、XPS/OXPS、DjVu/DjV。
- DWG/DXF 使用纯 Rust `acadrust` + `dxf` + `dxf_to_svg`。
- PDF 使用 `hayro`，PS/EPS/AI 使用 `stet`，XPS 使用 `justpdf-formats`。
- WMF/WMZ、DjVu/DjV 从 `pro` 迁入；`pro` 不再声明 `CAP_REGION`。
- 旧版矢量 CMX/CCX、WPG、ZMF、ZBR 拆到独立 `doc-legacy`。
- 当前为混合档：`is_vector=0`、`CAP_FULL`；区域渲染暂未声明。
