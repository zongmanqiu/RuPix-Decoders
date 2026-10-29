# av — HEIC / HEIF / AVIF 解码器

一句话：`heic;heif;hif;avif;avifs` 的唯一认领者，双后端（heif-oxide + zenavif，都是纯 Rust）。
它是 33 个单库里**自带第二套容器解析器**的那个，也是 AVIF 查询阶段必须真解码的那个。

## 身份（`src/lib.rs:37-46` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `av` / rupix-av / **0.3.0** |
| name | `Modern Formats (HEIC/AVIF)` |
| extensions | `heic;heif;hif;avif;avifs` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `av_0.3.0.dll` |

`crate-type = ["cdylib", "rlib"]`（`Cargo.toml:11`）——`rlib` **只为** `examples/smoke_av.rs`
能当库调 `av::iv_probe`，别的解码器没这么干。

依赖（`Cargo.toml:19-39`）：
* `heif-oxide = "0.1"`（锁 0.1.0）：ISO/IEC 23008-12 容器 + 纯 Rust HEVC（`rust_h265 0.1.0`、
  `yuv 0.8.19`），MIT / Apache-2.0（宿主 `THIRD-PARTY-NOTICES.md:83` 已确认）。
* `zenavif = "0.1"`（锁 **0.1.6**）：纯 Rust AV1（`rav1d-safe 0.5.7`、`rav1d-disjoint-mut 0.3.2`、
  `zenavif-parse 0.6.2`）+ `zenpixels 0.2.16`、`rgb 0.8.53`。
  ★ 许可 **`AGPL-3.0-only OR commercial`**（`NOTICES:84`，分发口径见同一文件 `NOTICES:127-128`）——整个 DLL 的
  许可基础就是它，换 libdav1d/libaom 属用户拍板（`.3rd/README.md` 的 GPL/LGPL 条目）。
* **没有链 libdav1d / libaom / libheif / libde265**：`.3rd/libheif-1.23.6`、
  `.3rd/libde265-0.2.3` 只是躺在盘上的备用源，`.3rd/README.md` 标着"未接入"，
  且 **libheif 那份还缺 `SOURCE.md`**（无 sha256 溯源）。
* 全仓零 `build.rs`、零 C 编译依赖（stb 已由纯 Rust 的 psd/hdr 替掉），本解码器无 nasm / cmake / 环境变量。

根 `Cargo.toml:86-100` 给 zenavif / rav1d-safe / rav1d-disjoint-mut / zenavif-parse / heif-oxide /
rust_h265 / yuv 全提 `opt-level = 3`；**`rupix-av` 自己不在例外清单**，它的像素格式分派循环跑在 `z` 上。

## 认领（`iv_probe`，`lib.rs:67-86`）

`_ext` 忽略，只防 `data.is_null()`；`sniff` → **100 或 0**，没有中间档。
`ftyp_brands`（`:259-271`）：`len >= 16` 且 `data[4..8] == "ftyp"`；major = `[8..12]`，
兼容 brand 从偏移 16 起，**扫描硬上限 64 项**（反滥用，`:266`）。
brand 表（`:273-277`）：AVIF = `avif, avis, av01, tmap, maf1`；
HEIF = `heic, heix, hevc, hevx, heim, heis, hevm, hevs, mif1, msf1, mif2`。
`sniff :261-267`：major 命中优先，否则按兼容表首个命中；**逐 brand 先判 AVIF 再判 HEIF**
（理由 `:289-290`：mif1 文件的兼容列表里会写 `avif`，HEIC 不会）。
非 `ftyp`（`isom` / `mp41` 之类）→ 0。

## 解码路径（`Kind::Heif | Kind::Avif` 双后端，`:58-62`）

**查询阶段**先走 header-only 尺寸恢复 `container_dimensions`（`:419-457`）：
`meta`（full-box 载荷）→ `iprp` → `ipco`，用 `box_parts`/`box_advance` **迭代**遍历兄弟盒子
（**不递归**，步长恒 ≥ 8 或 `b.len()`，所以"推进且线性"成立，规范 §六 第 10 条）；
`size == 0`（到 EOF）与 `size == 1`（64 位 largesize）都处理（`:304-329`、`:373-394`）。
★ 三层的头各不相同，别想当然：`meta` 是 FullBox、`iprp`/**`ipco` 都是普通 box**、
属性层 `ispe` 是 FullBox、`irot`/`imir` 是普通 box 且载荷就 1 字节。判据不是记忆：
`.3rd/libheif-1.23.6/libheif/box.h:757` `Box_ipco : public Box`、`box.cc:3637`（`Box_irot::parse`）与
`box.cc:3672`（`Box_imir::parse`）把
`parse_full_box_header(range)` **注释掉了**、`heif-oxide/src/boxes.rs:534-556` 只对 `ispe` 调
`full_box_header()`，加上实测盒树（真实 AVIF 里 `irot` size=9、`ipco` 后紧跟第一个属性）。
尺寸取 **`ispe` 里像素最多的那个**（`:406-418` 解释了为什么不走 pitm/ipma：ipma 关联容易挑中
小的 α 辅助 ispe；多出来的 idle ispe 只会让上限更保守，无害），再按 `container_transform`
（`:465-511`）折成 EXIF 方向：`irot` 原值是 **CCW×90° 档数**（0→1、1→8、2→3、3→6，奇数档换边），
单独 `imir` axis 0→2（左右翻）、1→4（上下翻），两者同时在场**保守报 1**（顺序要看 `ipma` 关联，
本解码器不解析——猜错方向比不转更难看）。

`query_needs_decode = Avif || w == 0 || h == 0`（`:154`）：**AVIF 在查询阶段必定真解码**，
因为"ispe 会撒谎"——grid 拼接结果无法从 ipco 反推（实测样本：ispe 写 80×80，zenavif 拼出 80×128），
而宿主只分配一次、不给重试（`:148-153`）。HEIC 除非 ispe 读不出来，否则查询是微秒级
（★ ipco 那刀剥错方向时，这条快路对**所有**真实文件都是死的：查询解一遍、渲染再解一遍）。

* HEIF 渲染：`heif_oxide::decode_bytes` → `to_rgba8()`；容器 demux、HEVC 解码、grid 拼接、
  8/10 bit、Display P3 → sRGB、`irot/imir/clap` 全在库里，输出即"显示就绪的 sRGB"。
  解完的尺寸与 ispe 交叉校验，允许 heif-oxide 应用 irot 造成的宽高互换，否则
  `ERR_CORRUPT "解出的尺寸与容器 ispe 声明不符"`（`:203-208`）。
* AVIF 渲染：`zenavif::decode(data)`，失败则退到 `decode_animation(...).frames` 取首帧（`:589-606`）；
  `pixel_buffer_to_rgba8`（`:618-709`）按 `zenpixels::descriptor::PixelFormat` 分派：
  `Rgba8` 拷贝、`Rgb8`/`Rgb16(>>8)`/`Gray8`/`Gray16(>>8)` 补 α=255 或复制灰度、`GrayA8` 展开、
  **`GrayA16` 与其他一律 `ERR_UNSUPPORTED`**（`:698-699`）；`as_contiguous_pixels` 处理行 stride。
  最后 `v.len() != w*h*4` → CORRUPT。非预乘直通 α（`:618` 注释）。
  ★ 库里**不应用** irot：`decode_avif` 现读容器方向（`:621-622`，上面 593-596 那四行注释就是这个理由）并声明 `already_oriented = 0`，
  由宿主按 EXIF 补转——谎报 1 就是"旋转过的 AVIF 躺平显示且宿主被告知别修"。

## 多帧 / 多页

`frame_index` **从不读**，两个后端都只出第一帧。`fill_info_oriented`（`:229-257`）里
`frame_count` / `frame_delay_ms` **根本没赋值**（见"坑"第 1 条），`page_count = 1`。

## 元数据 / 色彩

`icc_len = 0` 无条件（`:247`）：ICC 不透传，sRGB 由库里烤好。EXIF / XMP：absent。
`iv_render_raw`：absent。方向分两条口径：HEIF 查询报 `orientation=1 / already_oriented=1`
（库里已应用，`:222-227`），AVIF 报容器实读方向 + `already_oriented=0`（`:612`）。

## 错误码口径

1（空输入、ftyp brand 不匹配）、4（空 info、`out_cap` 不足）、7（64 MP、预算，查询两条分支与渲染都查）、
2（ispe 不一致、空动画、转换长度不符、heif/zenavif 文本兜底）、3（意外 PixelFormat、heif 文本命中）、
6（guard）。**`ERR_OOM` 从不返回——absent。**
分类是**小写子串**匹配：heif（`:573-582`）"unsupported"/"该特性不支持"/"codec"→3、
"图片过大"/"limit"→7、其余 2；`map_zenavif_err :689-698` "图片过大"→7、
"unsupported"/"该特性不支持"→3、其余 2。上游改文案 = 让位链断，升级要复核。

## ★ 维护者必须知道的 4 个坑

1. `fill_info_oriented`（`:229-257`）不写 `frame_count` / `frame_delay_ms`，第二次调用时它们
   **保留宿主缓冲里的旧值**；测试之所以过是因为传了 `IvRenderOut::default()`。
2. AVIF 静图解码的 `Err(_)` 被丢弃（`:591-592`）；若动画回退也失败，`iv_last_error` 说的是动画的错，
   不是原始错误。
3. ~~HEIF **每次渲染都重解**（`decode_cached` 只缓存 AVIF）~~ ✅ **已修（m77，2026-10-21）**，
   见下面"已修"第 4 条。留这一行是为了挡住"把短路加回去"。
4. `irot` + `imir` **同时**在场的文件保守报方向 1（`container_transform :480-491`）。真转要解析
   `ipma` 关联次序（MIAF 的"裁剪→旋转→镜像"），本解码器不解析——这是**已知空白**，不是 bug；
   单 irot、单 imir 都已覆盖并由测试锁住。

### 已修的几条，别再退回去（前三条＝缺陷②a/②b/⑤那一轮，第四条＝m77）

* **AVIF 方向谎报**：原代码返回 `(orientation=1, already_oriented=1)`，模块头却承诺报 EXIF 方向。
  现在 `decode_avif :583-590` 现读容器、报 `already_oriented=0`，由宿主 `core/geometry.rs::oriented`
  补转。判据：真实样本 `abc_color_irot_alpha_irot.avif`（ispe 512×256、irot=1）→ 256×512 / EXIF 8。
* **AVIF 缓存键**：原来是 `(data.as_ptr(), data.len())`——宿主在同一地址换一张同长度 AVIF 就**静默串图**。
  现在是 `(len, FNV-1a 全量内容指纹)`，`fingerprint` + `decode_cached`（现 `:548-569`），O(n) 一遍扫，
  与一次 AV1 解码相比可忽略。
* **`ipco` 不是 FullBox**：曾经按 FullBox 剥 4 字节，导致属性排整排错位、真实文件读出 (0,0)，
  HEIC 查询快路对**所有**真实文件失效（查询+渲染各解一遍）。证据链见"解码路径"一节。
* **HEIF 也进那份内容键缓存（m77，2026-10-21）**：`decode_cached` 原来开头一句
  `if kind == Kind::Heif { 每次真解 }`——写它的人只把缓存当成"查询→渲染"的一次性交接，
  没算到宿主**按档位会再请求一次同一张图**（按 `1` 上原图档）。端点实测（生产 exe + `build/stall_probe.ps1`）：
  3000×4000 的 HEIC 第一次请求 273.9 ms、第二次 **262.8 ms**＝整趟重解；改后第一次 266.5 ms（不动）、
  第二次 **47.1 ms**。同形状的 AVIF 一直是 ~30 ms（它本来就命中缓存），这条把 HEIF 拉平。
  用例 `heif_render_hits_the_shared_cache` + `same_length_different_content_cannot_hit_the_heif_cache`
  （先红后还原：把那句短路加回去，第一条当场 FAILED）。
  ★ **价格实测**（`<宿主>/examples/mem_chain_probe.rs` 同图配对双臂）：解码器线程本地多留一份全尺寸 RGBA——
  12 MP HEIC ⇒ 提交量 +45.8 MB、峰值工作集 197.6 → 243.3 MB，直到同线程解下一张图才换手。

## 限制与测试现状

`MAX_PIXELS = 64 MP`（`:54`）**只在查询分支强制**；HEIF 渲染第 2 阶段没有像素上限（只有预算 +
ispe 交叉校验）。`Vec::with_capacity(w*h*4)`（`:627`）没有 `checked_mul` 守。brand 扫描 64 项封顶。

测试 20 个 `#[test]`（`:735-1240` 一带，模块到文件尾 `:1256`）+ 1 个 example：brand 接受（含 mif1 major + avif compat → 100）、
非 HEIF 拒绝、AVIF 1×1 / 4×4 / 灰度 / grid+alpha / irot（irot 那条现在断
`orientation=8` + `already_oriented=0`，不再只断字节长度）、预算、短缓冲、扩展名自述、布局断言、
成功清 last-error，另有 5 条容器方向锁：`container_ispe_with_irot_reports_exif_orientation`
（0/1/2/3 → EXIF 1/8/3/6 + 奇数档换边）、`ipco_children_are_read_without_a_fullbox_strip`、
`real_avif_sample_rotation_matches_container_bytes`（真实样本收口）、
`imir_alone_maps_to_flip_and_combo_stays_conservative`（2 / 4 / 组合保守报 1）、
`heic_shaped_container_picks_main_image_and_rotates`（照 `orange.heic` 实测盒树复刻：三 ispe 取最大 +
irot=3 → 3000×4000 / EXIF 6）、`fingerprint_separates_same_length_different_content`（同长度不同内容必须换键）。
★ 5 个 AVIF（315 B–10.6 KB，来源见 `smoke_av.rs:6-8`）住在**全仓共享的 `testdata/golden/`**
（不在本 crate 里），且是**运行时 `std::fs::read`** 而非 `include_bytes!`——把仓库根挪走后
不重编也没关系，但样本文件必须在那儿。
`smoke_av.rs` 从 `<workspace>/../参考项目 读 HEIC/AVIF，缺文件打印 SKIP、真失败 `exit(1)`
（`:131-162`）；**没有 HEIC 夹具**，所以 `decode_heif` 端到端未被测过（容器头解析已由上面的
合成盒树锁住，缺的是 HEVC 载荷那条腿）。
生产路径无 `unwrap`/`expect`/`panic!`（唯一的 `panic!` 在测试加载器，example 里有 `expect`）。
