# qoi — QOI 解码器（零依赖手写）

一句话：`qoi` 后缀的唯一持有者，**不引任何第三方 crate**，QOI 规范逐字节手写在 `src/lib.rs` 里。
它是 33 个单库里生产路径最干净的一个：没有 `unwrap`、没有 `expect`、没有 `panic!`。

## 身份（`src/lib.rs:23-30` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `qoi` / rupix-qoi / **0.3.0** |
| name | `Quite OK Image` |
| extensions | `qoi` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `qoi_0.3.0.dll` |

`Cargo.toml:13-15` 只有 `rupix-abi`（契约 + kit，样板已并入）（workspace path 依赖）；
`qoi` / `deqoi` / `image` 全都不在依赖树里，头注释写着"零第三方依赖，只用 std"（`:5`）。
`crate-type = ["cdylib"]`（`Cargo.toml:11`）。无 `build.rs`、无 C、无 COM、无 `testdata/`、无 `examples/`。

## 认领（`iv_probe`，`lib.rs:40-58`）

`len >= 4` 且以 `qoi f`（`b"qoif"`）开头 → **100**，否则 0。`_ext` 忽略，纯魔数，单档置信度。

## 解码路径（规范逐条对位，`lib.rs:140-322`）

* `parse_header`（`:204-233`）：14 字节头 + 魔数，宽高是大端 u32，`channels ∈ {3,4}` 否则 Unsupported，
  `colorspace <= 1` 否则 Unsupported，零尺寸 → Corrupt，`checked_mul` 溢出防护，
  **> 64 MP → `TooLarge` → 7 ERR_TOO_LARGE**（`:228-230`；原先判 2 Corrupt，与规范 §五 校验表和
  core / jpeg / thumbdoc 的口径不一致，已统一——"文件没坏，是我们不接"必须分开报）。
* 操作码（`:257-319`）：精确字节 `QOI_OP_RGB 0xFE` / `QOI_OP_RGBA 0xFF`；
  其余按 `b1 & 0xC0` 分类 `INDEX 0x00` / `DIFF 0x40` / `LUMA 0x80` / `RUN 0xC0`；
  `_ => Corrupt` 兜底（`:314`，2 bit 掩码其实已穷尽）。
* 哈希 `(3r + 5g + 7b + 11a) % 64`（`:234-237`），索引表 `[[u8;4];64]`（`:249`），
  起始像素 `(0,0,0,255)`（`:250`）；RUN 长度 = `b1 & 0x3f` 个额外像素；
  **索引表只在新像素产生后更新，run 连续期间不更新**（`:317-318`，规范正确行为）。
  每个多字节操作都有边界检查（`:299, 306, 314, 336`）。
* 输出缓冲用 `Vec::try_reserve_exact`，失败 → **`ERR_OOM`**（`:246`）——
  这是三个手写解码器里唯一会发 ERR_OOM 的。

★ 色彩口径：`colorspace` 字节会校验（0 = srgb、1 = linear），但**不做任何 gamma 展开**——
按存的值直接吐像素；`channels` 字节也不影响输出（恒 RGBA8）。
`linear`（1）目前被静默接受并标为"支持"（`:222`）。
★ 结束标记（7 个 `00` + `01`）**不校验**：解码写到 `npixels` 就停，尾部字节忽略（`:257` 循环界）。

## 多帧 / 多页 / 元数据

无：`frame_index`、`page_index` 从不读（`req` 里只取 `budget_bytes`，`:78-82`）。
`fill_info`（`:123-140`）硬写 `frame_count = 1`、`page_count = 1`、`frame_delay_ms = 0`、
`orientation = 1`、`already_oriented = 1`、`icc_len = 0`。EXIF / ICC：absent。`iv_render_raw`：absent。

## 错误码口径

`QoiError → (码, 消息)`（`:174-201`）：
`Unrecognized`→1 `不是 QOI 文件（魔数不对）`、`Corrupt`→2 `QOI 数据损坏或被截断`、
`Unsupported`→3 `该 QOI 子格式不支持`、`TooLarge`→7 `图片超过 6400 万像素的解码器上限`、
`Oom`→5 `分配像素缓冲时内存不足`。
渲染层另有：`内部错误（宿主传入空数据）`→1、`图片太大，超出宿主给的内存预算`→7、
`内部错误（宿主传入空输出指针）`→4、`内部错误（宿主给的缓冲小于申报值）`→4（`:65-118`）。

超尺寸口径现在与 core / thumbdoc / jpeg **一致**（都是 7）：以前这里报 2 CORRUPT，宿主 UI 会把
"文件太大"念成"文件坏了"。规范依据：§四「错误码（必须可区分）」推论第 2 条
（"内存 / 尺寸 / 预算类 → `ERR_TOO_LARGE`"）与 §五 校验表（"超 64MP / budget → ERR_TOO_LARGE"）。
`CORRUPT` 与 `TOO_LARGE` 都会中止让位链，但给用户的话术完全不同，别再改回去。

## 限制与防挂死口径

* `MAX_PIXELS = 64 MP`（`:203`）；预算在解码**之前**检查。
* 除逐操作边界检查外没有额外预算：推进严格线性——每轮迭代要么消耗 ≥ 1 字节、要么产出 ≥ 1 像素，
  RUN 恒推进 `written`。符合规范 §六 第 10 条"对输入字节推进且线性"。
* 一个 15 字节的文件声称 64 MP 现在**当场被头部闸拦下**（7 TOO_LARGE，不再走进像素循环）；
  略低于上限的谎报尺寸仍会过头部检查、在第一个操作就 Corrupt，`try_reserve` 保证不会变成分配炸弹。

## 测试（10 个 `#[test]`，`lib.rs:376-549`）

夹具 `RED_2X2` 是手搓字节（`:387-397`：头 + RGB + RUN(2) + 结束标记）。
覆盖：2×2 纯红 16 字节 RGBA 精确、魔数探测（含空指针与 `len = 2` → 0）、坏魔数 → 1、
截断头 → 2、零尺寸拒绝、非法 `channels` → 3、截断像素流 → 2、布局断言（24/48/abi 4）、
两阶段一致（查询与渲染的尺寸 + 成功后 last-error 为空），
以及 `oversized_dimensions_are_too_large`（`:466`：把头改成 10000×10000，断 `parse_header` → 7、
`iv_render` 查询 → `ERR_TOO_LARGE`、且 `iv_last_error` 文案是 `图片超过 6400 万像素的解码器上限`）。

## 维护注意

unsafe 只有：裸切片（`:53`、`:140`）、`req` 指针读（`:144`）、`&mut *info`（`:151`）。
像素写出走 `copy_from_slice`（`:115`），不是裸指针拷贝。唯一的 `panic!` 在测试的匹配器里（`:429`）。
加操作码时记得同时补 `expected` 的边界检查与对应测试——本解码器没有第三方库替你兜着。
