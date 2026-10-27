# jxl — JPEG XL 解码器

一句话：`jxl` 的唯一认领者，jxl-oxide 纯 Rust 解码。**查询阶段就把整图解出来**并缓存，
渲染阶段只是搬运——这是全部 22 个单库里唯一反过来的时序，改它前必读下面两节。

## 身份（`src/lib.rs:40-47` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `jxl` / rupix-jxl / **0.3.0** |
| name | `JPEG XL` |
| extensions | `jxl` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `jxl_0.3.0.dll` |

依赖：`jxl-oxide = "0.12.6"`，`default-features = false`，features `["moxcms"]`
（`Cargo.toml:19-26`）。锁：jxl-oxide 0.12.6 / jxl-oxide-common 1.0.0 / moxcms 0.8.1。
**纯 Rust，不链任何 C 库**——选 `moxcms` 正是为了避开 `lcms2` 这个 C 依赖（`Cargo.toml:22-25` 注释）。
无 `[dev-dependencies]`、无 `build.rs`。
根 `Cargo.toml:45-46, 75-76` 给 `jxl-oxide` 与 `moxcms` 提 `opt-level = 3`；解码器本体留 `z`，
所以它的 f32→u8 循环是按体积编的。★ 2026-10-21 m77 复核：单库 workspace（`singles/Cargo.toml`）
**没有** `[profile.release.package.rupix-jxl]` 这一条，那句话仍然成立；对照读数与这条要不要补，
。（2026-10-25 重组后单库 profile 在 `singles/Cargo.toml`；发布档用全局 `opt-level = 3`。）
许可证：宿主 `THIRD-PARTY-NOTICES.md` §2.2 —— **m76（2026-10-21）已按 `cargo metadata` 读已发布包自己的
`license` 字段核掉**：jxl-oxide = `MIT OR Apache-2.0`、moxcms = `BSD-3-Clause OR Apache-2.0`、
rayon = `MIT OR Apache-2.0`（原 [待核] 三条全部转 [实测]；rayon 仍会被 `raw` 带进 workspace 解析，见下）。

## 线程模型（原"未结冲突"，已按规范 §六 第 5 条收口）

原来 `features = ["rayon"]` 让 jxl-oxide 自己起全局线程池，与"解码器不得自建线程池"直接冲突。
现在两道都堵上：

1. `Cargo.toml` 不再请求 `rayon` feature。
2. `src/lib.rs:328` 显式 `.pool(JxlThreadPool::none())`。

★ 第 2 道才是**真杠杆**，别删。jxl-oxide 的 `default_pool()`（上游 `jxl-oxide-0.12.6/src/lib.rs:187-195`）按 feature
二选一：开 = `rayon_global()`，关 = `none()`。而 `build.ps1:71` 是一次
`cargo build --release --workspace`，cargo 会做 **feature 统一**：`rawler`（rupix-raw 的依赖）
以 default features 依赖 jxl-oxide，`rayon` 会被它重新打开——`cargo tree --workspace -e features -i jxl-oxide`
实测确认。只有 builder 层传进来的 `none()` 能无视 feature 状态。

**性能代价（m77 首次有数，2026-10-21）**：真实照片级语料补齐后（`.temp/m77/samples`，来源，
`<宿主>/examples/bench_decode.rs` 走生产解码链量得 **2.46 MP → 366 ms / 1.5 MP → 169 ms / 8.3 MP（3840×2160）→ 861 ms**
⇒ **6.7–9.6 MPix/s**。同尺对照：JPEG **127 MPix/s**、HEIC **54 MPix/s** ⇒ 单线程那条路让 JXL 比 JPEG 慢**十几倍**，
这是"解码器不得自建线程池"（规范 §六 第 5 条）的定价，不是实现失误。端点另见下面"缓存"一节。
单线程路径本身已由 `decodes_known_1x1_red` / `decodes_gray_4x4_and_expands_alpha` 跑通
（`cargo test -p rupix-jxl` 单独构建时连 feature 都是关的）。
★ **排除掉的一条猜测（m77 实测，别再照它改）**：慢会不会是解码器本体那圈 `to_u8`（f32→u8）按 `z` 编的？
给 `rupix-jxl` 加 `opt-level = 3` 之后，同一张 2.46 MP 的首次请求 **400.1 / 443.5 → 406.3 / 409.4 ms**、
8.3 MP **947.0 / 900.5 → 895.7 ms**——差异在噪声内，**没有可复现收益**，那条 Cargo.toml 例外已撤回。
瓶颈在 jxl-oxide 内部的单线程重建（那层本来就是 `opt-level = 3`），不在这 20 行封装里。

## 认领（`iv_probe`，`lib.rs:58`）

`len < 2` → 0；`looks_like_jxl`（`:264-266`）认两种签名之一：
裸码流 `FF 0A`，或 12 字节 ISOBMFF 容器签名 `00 00 00 0C 4A 58 4C 20 0D 0A 87 0A`。
**100 或 0，没有中间档；`_ext` 忽略；不做 box / brand 解析**（与 `av` 不同）。

## 解码路径

冻结的 ABI 只能"查询报尺寸 → 宿主分配 → 渲染填像素"，**不给重试**。而 JXL 的方向会改变输出
宽高、真实尺寸只有解完才知道，所以本解码器把整解码放到查询阶段（`:10-18` 是这套理由的原文）：

* 查询（`:123-148`）：`decode(input)` → RGBA `Vec`，`bytes_needed = rgba.len()`，
  **预算在解完之后才查**，然后把 `{fingerprint, w, h, rgba}` 放进 thread-local `CACHE`（`:192-199`）。
* 渲染（`:152-180`）：`take()` 缓存，指纹匹配才复用，否则重解。
* `decode`（`:292-354`）：重查魔数 → `JxlImage::builder().pool(JxlThreadPool::none()).read` →
  `render_frame(0)` →
  `image_all_channels()`（**方向已在库里应用**，含额外通道）；`w/h` 为 0 → Corrupt；
  `checked_mul` 后 `> MAX_PIXELS(64 MP)` → TooLarge（这条上限守的是**分配**，不是解码）；
  通道数分派 1→灰、2→灰+α、3→RGB(α=1.0)、4→RGBA、**>4 → 取前 3 通道当不透明 RGB**
  （额外通道顺序未定义，宁可当不透明也别把黑通道读成 α，`:336-342`）；
  f32 [0,1] → u8：非有限值→0、夹取、+0.5 取整（`to_u8 :332-337`）。
  输出是**直通（非预乘）RGBA8**，色彩管理（线性→sRGB）由 moxcms 在库里做。

## 多帧 / 多页

`frame_index`、`page_index` **从不读**，`render_frame(0)` 写死，`fill_info` 硬报
`frame_count = 1`、`frame_delay_ms = 0`（`:194-195`）。
后果值得记住：**动图 JXL 会静悄悄显示第 0 帧当静图，且从不返回 UNSUPPORTED** —— 没有让位链
（对比 `gif` 是明确拒绝）。

## 元数据 / 色彩

`PF_RGBA8`、`bit_depth = 8`（16 bit / f32 源都降位）、`orientation = 1` + `already_oriented = 1`
（库里已应用）、`page_count = 1`、`icc_len = 0`，注释写着"ICC 不透传，jxl-oxide 的 ICC 出口后续再接"（`:196`）。
EXIF / XMP：absent。`iv_render_raw`：absent。

## 错误码口径

`JxlErr :251-268`：1 / 2 / 3 / 7 / **5 OOM**（`try_reserve_exact` 失败，`:355`）；
空 info 指针与 `out_cap` 不足 → 4；panic → guard 转 6。
库错误是**按小写消息子串**分类的：含 "unsupported" / "该特性不支持" / "unimplemented" → 3，
其余 → 2（`:281-288`）——上游改文案就会把 UNSUPPORTED 掉进 CORRUPT、掐断让位，升级要复核。
消息：`不是 JPEG XL 文件（魔数不对）`、`JPEG XL 数据损坏或被截断`、
`该 JPEG XL 特性不支持`、`图片超过像素上限`、`分配像素缓冲时内存不足`、
`图片太大，超出宿主给的内存预算`、`内部错误（宿主给的缓冲小于申报值）`。

## 限制与已知空白

* 64 MP 上限 + 预算两阶段都查；`checked_mul` / `saturating_mul` / `try_reserve_exact` 齐。
* 没有解码级 budget、没有画布上限（解析全在库里）。
* ~~**缓存指纹只覆盖前 4 KiB + 总长**，且 `take()` 是一次性的~~ ✅ **m77 两条一起改**：
  ① 指纹改成**整遍** FNV-1a（`fingerprint`，现 `:241`）——缓存如今会跨请求留着，弱键就成了
  "两张同长度、前 4 KiB 相同的图 ⇒ 静默串图"的窗口，用例 `fingerprint_covers_the_whole_input_not_just_a_prefix`
  钉住；② 渲染阶段不再 `take()`，改由 `cached_entry` **保留**，同一张图的第二次请求（按 `1` 上原图档）
  直接命中。端点（生产 exe + `<宿主>/build/stall_probe.ps1`）：3840×2160 第一次请求 947.0 ms、第二次
  **892.8 → 31.0 ms**。
* **内存驻留是这里最现实的坑（改后的形状要说清）**：留着的是一份全尺寸 RGBA，直到同线程解下一张图才换手——
  8.3 MP JXL = 33 MB、12 MP HEIC = 45.8 MB（`<宿主>/examples/mem_chain_probe.rs` 同图配对双臂实测：
  提交量 +45.8 MB、峰值工作集 197.6 → 243.3 MB）。换来的是一整趟解码不再重付。

## 测试（12 个 `#[test]`，`lib.rs:395-621`）

夹具两枚，全在**全仓共享**的 `testdata/golden/`（`include_bytes!` 编译期嵌入，不在本 crate 里）：
`1x1_red.jxl`（24 B）、`4x4_gray.jxl`（25 B）。原 `tests/fixtures/` 目录已于 2026-10-25 撤销
（那个 `tests/` 里只有夹具、没有一行 `.rs`）。覆盖：1×1 精确字节、4×4 灰度扩 α、魔数、坏魔数→1、
`FF 0A` 单截断→2、消息分类、`to_u8` 夹取与 NaN、布局断言、两阶段一致。
未覆盖：`budget_bytes`、`out_cap` 不足、`ERR_OOM`、`ch > 4` 分支、指纹不匹配的重解分支。
生产路径零 `unwrap`/`expect`/`panic!`。
