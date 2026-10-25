# png — PNG 冠军插件（静态图）

一句话：`png` 后缀的选取景器，png crate 用 `ALPHA | STRIP_16` 变换直出 RGBA8；
**APNG 一律让位 `core`**，`apng` 后缀故意不写进认领表。

## 身份（`src/lib.rs:61-70` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `png` / rupix-png / **0.1.0** |
| name | `PNG (png crate)` |
| extensions | `png`（**不含 apng**，`lib.rs:64-65` 写明理由） |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `png_0.1.0.dll` |

依赖 `png_crate = { package = "png", version = "0.18" }`（锁 0.18.1，MIT / Apache-2.0）。
**别名是必须的**：`[lib] name = "png"` 与依赖同名，extern prelude 会打架（`Cargo.toml:27-29`）。
dev-dependency `image = "0.25"`（关默认特性、只开 png）当像素对拍基准。
根 `Cargo.toml:53-58, 78-89` 把 `png` 本体、`rupix-png` 插件自己、`flate2`/`miniz_oxide`/
`fdeflate`/`simd-adler32`/`crc32fast`/`color_quant` 全提 `opt-level = 3`——灰度 2→4 通道扩展是
本插件里的逐像素循环，所以插件本体也进了例外清单。

## 认领（`iv_probe`，`lib.rs:92-115`）

**纯魔数**，`_ext` 忽略。要求 `len >= 33`（8 字节签名 + IHDR 头 17 + 4 字节 CRC，`MIN_LEGAL_LEN :77`）；
签名 `89 50 4E 47 0D 0A 1A 0A`，且 **IHDR 必须是第一个块、长度字段必须等于 13** → 100，否则 0。
只有一档置信度。

## 解码路径

1. `read_head`（`:282-296`）：`Decoder::new` + `set_transformations(ALPHA | STRIP_16)` + `read_info()`
   （停在第一个 IDAT，不做 inflate）。顺手取 `is_animated()`。
2. **APNG 立刻让位**：`ERR_UNSUPPORTED "APNG 动图由 core 插件负责，本冠军插件让位"`
   （`:162-166`）。绝不"把动画第一帧当静图"糊过去。
3. `decode_into`（`:261-312`）：重开 reader，按 `output_color_type()` 分派：
   * `Rgba/8`（bpp 4）→ `next_frame(dst)` **直接写进宿主缓冲，零拷贝**（`:292-298`）。
   * `GrayscaleAlpha/8`（bpp 2）→ 解到临时 `Vec`，再手工 2→4 扩展（`:301-310`）。
   * 其他输出布局 → `ERR_UNSUPPORTED`（让 core 兜），并用库报的 `output_buffer_size()`
     与 `npixels * bpp` 交叉校验，不等就 UNSUPPORTED（`:280-289`）。

灰度 / RGB / 调色板 / 1-2-4 bit 全由 ALPHA 变换归一到 RGBA8。**16 bit** 走 `STRIP_16`
＝丢低位留高位（与 libpng 一致）；`image` 那条路是 ÷257，两者差 ≤1 LSB，这个口径差异
是明写的校准选择（`:37-39`，测试里两条断言都钉住）。

## 多帧 / 多页

只出静帧：`frame_count = 1`、`page_count = 1`、`frame_delay_ms = 0`（`:222-225`），
`req` 的 frame/page 索引忽略。动画语义全在 `core` 的 apng 会话里。

## 元数据 / 色彩

**`iCCP` 读并透传**（m53 起）：`read_head` 取出 `i.icc_profile`（`:285-290`），超过 `ICC_MAX` 整份丢弃、
报 `icc_len = 0`（不截半份）；像素写完后紧跟那段 ICC（`:210-216`），`fill_info` 申报 `icc_len`（`:257`）。
**gAMA / cHRM 仍然不做**——色彩管理只认嵌入的 profile，不读那两块灰阶/色度提示。
**方向读 `eXIf` 数据块**（m48）：`:185-186` 走 kit 的 `png_exif_orientation` 逐块扫到 IDAT 为止，
`fill_info` `:247, 251` 照实报 `orientation` + `already_oriented = 0`。旧注释"PNG 没有方向语义"
不准确——`eXIf` 是 PNG 第三版登记过的块，里面就是同一枚 274 标签；读不到才报 1。

## 错误码口径

入口两条与 jpeg 相同（1 / 4）。`:64MP` 与预算 → 7。`dec_err`（`:316-327`）：
`LimitsExceeded`→7、`Parameter`→4、`Format`/`IoError`→2，消息一律 `PNG 库报错 "{e}"` 前缀。
**"我不会解"（3 UNSUPPORTED）与"文件坏了"（2 CORRUPT）严格分开**——让位链靠这个区分工作
（`:314-315`）。

## 限制与已知空白

* `MAX_PIXELS = 64 MP`（`:75`）+ `budget_bytes`；没有递归防护（单块读，结构由库管）。
* 不做 APNG 合成 / dispose / blend：明确列为非目标，宁可让位也不假装会放（`:43-45`）。
* 不做 interlace 之外的优化；隔行样本已预验证（`lib.rs:717` 注释说明与 WIC 对拍过）。

## 测试（12 个 `#[test]`，`lib.rs:482-1152`）

最值钱的是 `parity_with_core_image_route_across_layouts`：16 种布局（gray1/2/4/8、
gray8_trns、grayalpha8/16、gray16、rgb8、rgb16_strip×2、rgba16_strip、palette4、
palette4_trns、palette8、interlaced_rgb8）逐字节对比 core 的 image 路线，16 bit 允许 ≤1 LSB。
其余：布局断言、魔数与伪魔数、查询阶段（`red_1x1` → `[255,0,0,255]`）、APNG 拒绝、
缓冲不足、预算、截断、签名后垃圾、自算 CRC32 改写 IHDR 成 100000×100000 的巨尺寸拒绝。
样本：**全仓共享**的 `testdata/golden/red_1x1.png`、`interlaced_16x12_rgb.png`（`include_bytes!`）。

## 维护注意

* 一次渲染会**打开 reader 两次**（查询一次、渲染一次）——宿主协议就是两阶段，别当性能 bug。
* 缓冲不变式：`npixels * 4 == bytes_needed`，且必须等于库自己报的尺寸，不等就是 CORRUPT。
