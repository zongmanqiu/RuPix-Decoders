# jpeg — JPEG 冠军解码器

一句话：`jpg/jpeg/jpe/jfif/jif/jfi/exif/jps/mpo` 的首选取景器，zune-jpeg 直出 RGBA8，**不做** EXIF 旋转、
不吃 CMYK（让位 `core`）。

## 身份（`src/lib.rs:30-40` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `jpeg` / rupix-jpeg / **0.1.0**（`Cargo.toml:2-5`） |
| name | `JPEG (zune-jpeg)` |
| extensions | `jpg;jpeg;jpe;jfif;jif;jfi;exif;jps;mpo`（`exif` 也在认领表里） |
| is_vector / has_alpha | `0` / `0` |
| 产物 | `jpeg_0.1.0.dll` |

依赖：`zune-jpeg = "0.5"`（锁 0.5.15）+ `zune-core = "0.5"`，MIT / Apache-2.0 / Zlib。
根 `Cargo.toml:38-58` 给这两个库提 `opt-level = 3`；**解码器本体留在 `z`**（解码在库里）。
无 `build.rs`、无 C 源。

## 认领（`iv_probe`，`lib.rs:59-72`）

**纯魔数**，`_ext` 完全忽略——`ext == NULL`（改名 / 无后缀）也照样认领，符合规范 §三 的嗅探约定。
判据：`len >= 4` 且 `FF D8 FF` 且第 4 字节 `!= 0x00` → **100**，否则 0。只有一档，没有 50 兜底分。

## 解码路径

两阶段，与宿主「查询 → 分配 → 渲染」协议一一对应：

1. `header_dimensions`（`lib.rs:258-270`）：`JpegDecoder::new(ZCursor::new(data))` +
   `decode_headers()`——只读头不解 IDCT，零拷贝借用输入字节。尺寸为 0 直接判坏。
2. `decode_rgba`（`lib.rs:273-288`）：`DecoderOptions.jpeg_set_out_colorspace(ColorSpace::RGBA)`
   后 `decode()`，库直接吐 RGBA8（alpha 恒 255），解码器侧**没有逐像素转换**，
   像素与 ICC 两段 `copy_nonoverlapping` 直接写进宿主缓冲（`lib.rs:207, 218`）。

解码长度与头部 `w*h*4` 不一致 → `ERR_CORRUPT "像素数与文件头不符"`。

## 多帧 / 多页

MPO（多图 JPEG）**按 `page_index` 取子图**（0.1.1 起，由 `common` 0.2.6 发布）：
`mpo_layout`（`lib.rs:272` 起）沿 JPEG 段链走一遍，数出几份完整 JPEG 并记下各自的
`(start, len)`；查询回填真实 `page_count`，渲染按 `page_index` 切出那一份再交给解码库。
越界页报 `ERR_CORRUPT`（不冒充首页）。

单张 JPEG 就是 `page_count = 1` 的一条切片，取 page 0 得回原切片——与旧版逐字节相同。

两条特意没走的路：**不读 MPF（APP2）里的 MPEntry 偏移**（实测 Pillow 生成的 MPO，
APP2 长度字段少报 26 字节、MPF 的 TIFF 头字节序位全 0）；**不裸扫 FFD8**
（EXIF 缩略图本身就是内嵌的完整 JPEG，裸扫会让每张带缩略图的照片凭空多出一页）。
段链走法让 APP1 按长度整段跳过，页数恒为 1。

`frame_index` 仍不读（JPEG/MPO 没有动画帧概念）。

## 元数据 / 色彩

**ICC 照实透传**（m53 起）：`rupix_abi::jpeg_icc_profile` 取出 APP2 里的 profile（`:169`），查询腿与
渲染腿都把它算进 `bytes_needed + icc_len`（`:173-190`），像素写完后由 `copy_icc` 追加在像素段之后
（`:215-219`），`fill_info` 申报 `icc_len`（`:223-248`）。**方向照实回报**（m48）：`iv_render` `:167` 用 kit 的
`jpeg_exif_orientation` 沿段表找 APP1 里的 `Exif` 后接两个 0 字节（只走段表、不碰熵数据，微秒级），
`fill_info` `:237, 243` 把 274 标签写进 `orientation` 并声明 `already_oriented = 0`——本解码器从不
摆正像素，转正归宿主 post 阶段那条有矩阵用例锁定的路。旧口径"报 `1/1` 免得宿主二次旋转"已作废：
二次旋转只在"转了却不声明"时发生，如实回报才是契约给的路。EXIF 拇指层、重建标记仍一概忽略。

## 错误码口径

| 情形 | 码 | 消息 |
|---|---|---|
| 空输入 | 1 UNRECOGNIZED | `内部错误（宿主传入空数据）` |
| info 指针为空 | 4 BUFFER_TOO_SMALL | `内部错误（宿主传入空输出指针）` |
| 头解析失败 / 无 SOF / 零尺寸 | 2 CORRUPT | `JPEG 文件头解析失败` 等 |
| > 64 MP | 7 TOO_LARGE | `图片超过 6400 万像素的解码器上限` |
| 超 `budget_bytes` | 7 TOO_LARGE | `图片太大，超出宿主给的内存预算` |
| `out_cap` 不够 | 4 BUFFER_TOO_SMALL | `内部错误（宿主给的缓冲小于申报值）` |
| CMYK / 算术编码等库不支持 | 3 UNSUPPORTED | 由 `classify()` 前缀判定 |

`classify()`（`lib.rs:230-240`）**按 `format!("{:?}", e)` 的字符串前缀**分派：
`LargeDimensions`→7、`Unsupported`→3、`IllegalMagicBytes`→1，其余 2。
zune-jpeg 改枚举名就会退化成一律 CORRUPT，进而**掐断让位链**——升级依赖要复核这张表。

## 限制与已知空白

* `MAX_PIXELS = 64 * 1024 * 1024`（`lib.rs:46`），在解码/分配**之前**检查（`lib.rs:125`）。
* `out == NULL` 是合法的查询阶段：只填 info 返回 ERR_OK，不解码（`lib.rs:141-144`）。
* CMYK JPEG 不解，交给 `core`（让位链见规范 §三）。无 progressive 特殊分支（库内处理）。
* 不做 DCT 缩放：`target_w/target_h` 忽略，缩放是宿主的活。

## 测试（18 个 `#[test]`，`lib.rs:479-1035`）

布局钉子 / 魔数与伪魔数 / 查询阶段报尺寸 / 渲染与查询一致且 alpha 全 255 / 缓冲不足不越界 /
预算生效 / 巨尺寸在分配前拒绝（自己改 SOF 字节）/ 截断流是 CORRUPT 不是挂死 / 垃圾输入 /
失败必带 last-error、成功必清空 / 缩放腿（`base_64x48` → 40×30、`grey_8x8` → 1×1、
`prog_64x48` → 8×6，三种编码各走一遍） / 渐进式截断。
样本 4 枚全在**全仓共享**的 `testdata/golden/`：`golden_16x12.jpg`、`base_64x48.jpg`、
`prog_64x48.jpg`（渐进式）、`grey_8x8.jpg`（灰度），均 `include_bytes!` 编译期嵌入。

## 维护注意

* 选型依据（配对实测台账）在宿主仓库 `本仓性能记录`，本文件不复述数字。
* 生产路径零 `unwrap/panic`；`expect` 只出现在测试里。
