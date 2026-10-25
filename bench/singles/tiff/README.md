# tiff — TIFF 冠军插件（单页，连续采样）

一句话：`tif/tiff` 的选取景器，tiff crate 直调，只吃 **Gray / GrayA / RGB / RGBA × 8/16 bit ×
连续采样（planar=1）**；调色板、CMYK、YCbCr、Multiband、分平面全部 `ERR_UNSUPPORTED` 让位 `core`。

## 身份（`src/lib.rs:59-66` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `tiff` / rupix-tiff / **0.1.0** |
| name | `TIFF (tiff crate)` |
| extensions | `tif;tiff` |
| is_vector / has_alpha | `0` / `1` |
| 能力位图（`iv_caps`，`:68-72`） | `CAP_FULL \| CAP_SCALED` |
| 产物 | `tiff_0.1.0.dll` |

依赖 `tiff = "0.11"`（锁 0.11.3，MIT）。根 `Cargo.toml` 给**两层**都提 `opt-level = 3`：
上游 `tiff` 库（`Cargo.toml:63-64`）与**插件本体 `rupix-tiff`**（`Cargo.toml:69-70`）。后者是 m89 补上的——profile
覆盖按 **package 名**匹配，`[lib] name = "tiff"` 只是插件 id，此前那条 `package.tiff` 根本没
命中的插件自己的行寻址 / 面积平均循环（一直是默认 `z`）。无 `build.rs`、无 C 源。

## 认领（`iv_probe`，`lib.rs:102-120`）

**纯魔数**，`_ext` 忽略，`len >= 8`。`is_tiff_magic`（`:86-97`）认四种组合：
`II 2A 00` / `MM 00 2A`（经典 v42）与 `II 2B 00` / `MM 00 2B`（**BigTIFF v43**）→ 100。

★ 注意：**BigTIFF 在 probe 层就被认领**，能不能真解取决于 tiff crate；probe 不区分。
所以"认了但解不出"的路径是存在的，测样时要用真 BigTIFF 验一次。

## 解码路径

`iv_render`（`:124-262`）先重查魔数（不符 → `ERR_UNRECOGNIZED "TIFF 魔数不对（应为 II/MM + 42）"`），
再读 `req.page_index`，然后**先定档再解码**（`plan`，见下一节）：

1. `read_shape`（`:717-759`）：`tiff::Decoder::new(Cursor)`；**`page != 0` 直接
   `ERR_UNSUPPORTED "本插件只解第一页，不支持请求第 N 页"`**；`dimensions()` 为 0 → CORRUPT；
   `colortype()` 过 `layout_of` 白名单；再读 tag 284 `PlanarConfiguration`，`!= 1` → UNSUPPORTED
   （缺 tag 当 1 处理）。顺带算出**行寻址表**（下一节）。
2. `decode_rgba`（`:867-889`）：**重开一次 decoder**，再读尺寸与色彩类型，两趟结果不一致
   → `ERR_CORRUPT`；`read_image()` 拿到 `DecodingResult::U8` → `to_rgba8`，`U16` → `to_rgba8_16`，
   其他（有符号 / 浮点）→ UNSUPPORTED。
3. 通道展开的**唯一一张表**是 `put_px8`（`:894-916`）：RGBA 同布局；1 通道 → 灰复制 + α=255；
   2 通道 → 灰 + α=src[1]；3 通道 → +α=255；4 通道 → 原样。**16 bit 取高字节**（`v >> 8`，
   `to_rgba8_16` `:932`）。直写腿与库腿共用它——两张表就会在"档位图 ↔ 原图档"之间变色。

## 原生降采样与直写（m36，`CAP_SCALED`）

未压缩（Compression=1）+ 连续采样 + 8bit + **条带式**（非瓦片）+ 正向光度的 TIFF，
字节就是像素本身，于是 `strip_table`（`:764-864`）算出一张行寻址表
（`Strips`，`:631-637`：`offsets[r/rows] + (r%rows)·row_bytes`）。有了它就能**不建整图中转**：
`render_strips_into`（`:360-412`）直接把 RGBA8 写进宿主缓冲。

闸门（任何一条不成立 → `strips = None` → 走上面第 2 步那条逐字节不变的老路）：
压缩、预测器≠1、瓦片、SampleFormat≠无符号、BitsPerSample 缺省或非 8、**反相光度**
（0 = WhiteIsZero 由库反相，直写不复制那套口径）、277 样本数与通道数不符、
RowsPerStrip=0、条带数 > `MAX_STRIPS`（`:655`，上界取自声明尺寸而非文件里的计数）、
条带跨度出文件。

档位（`plan` `:295-329` + `scaled_step` `:338-348`）：只在宿主点亮 `REQ_ALLOW_SCALED` 时缩，
步长 `s = ⌊max(w/tw, h/th)⌋`（**保证宿主不会被迫放大**，推导与 7×5 尺寸矩阵单测见
`tests::scaled_plan_never_lets_the_host_upscale`）；源图 < `MIN_SCALE_PIXELS`（`:659`）不缩，
缩后单边 < `MIN_OUT_EDGE`（`:663`）不缩。

两条缩放腿（`Fast::sample`）：

| 腿 | 条件 | 做什么 | 实测 |
|---|---|---|---|
| 取样 | 显示框长边 ≤ `SAMPLE_BOX_MAX = 512`（`:293`） | 每 `s` 行只读 1 行，行内 `s` 列平均 | 240×180 缩略图 **5.4 ms / 47×**（m36） |
| 真平均 | 更大的框（主画面） | `s × s` 块全读，逐格累加器（`ow × ch` 个 u32，不驻留整图） | 2K 视口档 **134.7 ms**；旧臂 241.9 ms（m89 同会话复测） |

★ 为什么主画面不许跳行：未压缩 TIFF 的垂直取样是**取样不是平均**（JPEG 的 DCT 缩放没这笔
账——它的 1/8 档天生是 8×8 块均值）。实测逐像素 MAD（与"整解再面积缩小"比）：
噪声内容 `1/11` 档 **17/255**（83.75% 像素差 >16，看得见颗粒），照片 **2.37/255**（0% 超限），
平坦渐变 **0.00**。所以判据是"给谁看"：小格子看不出来 → 换 47×；主画面看得出来 → 走真平均。

`step == 1`（不缩放）也走直写腿：省掉老路"3 通道 Vec → 4 通道 Vec → memcpy 进宿主缓冲"
三趟中转。6055² 整解在 m89 终版实测 **121–131 ms**（784×561 视口；旧 DLL 194–215 ms）。

### m89：编译档位与通道特化

热循环确实在本插件体内（m89 分段：`render_ms` 占端点的 85% 以上），因此两件事都按
"输出逐字节不变"落地：

1. **`[profile.release.package.rupix-tiff] opt-level = 3`**——此前只有上游 `tiff` 库在例外
   清单里（package 名匹配的坑见上文）。同一份代码只换档位：6055² 降采样 177→121 ms。
2. **`ch = 3/4` 特化**（`accumulate_row` `:471`、`write_averaged_row` `:425`、`write_row_plain`
   `:532`）：把逐像素的运行时通道循环与 `put_px8` 调用拆成三条/四条独立累加链。求和方式、
   `(sum + cell/2)/cell` 取整、边缘格按真实样本数除——**一字未改**。再降到 **83.6–84.2 ms**。

端点合计（784×561，n=7 中位，正反序）：**177→84 ms**；2K 视口 230–242→**134 ms**，
且从"降采样比整解还慢"（0.89–0.95×）翻回 1.10–1.16×。真实文件三条腿（整解 / 真平均档 /
取样缩略图）的 RGBA `fnv1a64` 与旧 DLL 逐一相同；用例 `scaled_modes_handle_ragged_edges_for_every_layout`
覆盖 1/3/4 通道 × 三组不整除尺寸的两种缩放腿，逐像素对照独立参考值。

## 多帧 / 多页

**单页插件**：`frame_count = 1`、`page_count = 1`、`frame_delay_ms = 0` 硬写（`fill_info`
`:584-609` 注释，`:606`："只出首页……宿主今天也一律发 `page_index = 0`"）。第二页测试明确断言返回
UNSUPPORTED 而**不是**偷偷再解一遍首页（`tests::second_page_reports_unsupported_instead_of_serving_page_zero`）。
没有页数发现。

## 元数据 / 色彩

**ICC 读并透传**（m53 起）：IFD0 的 `IccProfile`(34675) 在 `read_shape` 里整取（`:746-751`），
超过 `ICC_MAX` 整份丢弃、报 `icc_len = 0`（不截半份），像素写完后由 `copy_icc` 追加在像素段之后
（`:264-270`）。**方向读**（m48）：整个 TIFF 文件就是一枚 EXIF 块
（II/MM + 42 + IFD0），`iv_render` `:195` 交给 kit 的 `exif_orientation`，`fill_info`
`:599-603` 把 274 的值照实报进 `orientation` + `already_oriented = 0`，转正由宿主做。
本插件只解首页（第二页在 `read_shape` 就 `ERR_UNSUPPORTED`），所以"IFD0 的方向"与"这张页的方向"
是同一件事。旧口径写死的 `1/1`（还把它论证成"避免宿主二次旋转"）已作废。

## 错误码口径

入口两条同 jpeg/png（1 / 4）。64MP 与预算 → 7（预算按**档位**的 `bytes_needed` 判，不是原图）。
`dec_err`：`UnsupportedError`→3、`LimitsExceeded`→7、`IntSizeError`→7 `"尺寸计算溢出"`、
`Io/Format/Usage`→2。直写腿的行越界 → 2（`ERR_CORRUPT`，绝不 panic）。
UNSUPPORTED 与 CORRUPT 分开是**契约**：core 会以 50 分嗅探兜底（模块文档 `:19-26`）。

## 限制与已知空白

* `MAX_PIXELS = 64 MP`，**必须在 `read_image()` 之前**判——库自己按尺寸分配（`:78`）。
  这道闸看的是**源图**，缩放档不放宽（宿主侧 40MP 闸更早，见宿主 `core/pixels.rs`）。
* 不分平面交织（planar=2 拒绝）、调色板 / CMYK / YCbCr / Multiband / 1·4 bit 打包全拒绝。
* 多页导航未实现。
* **瓦片（tiled）TIFF 不走直写**：那是二维寻址、比条带更省，另开一轮再接。
* BigTIFF 的条带偏移按 u64 读，但 `find_tag_unsigned_vec::<u64>` 失败即让路 → 回老路。

## 测试（26 个 `#[test]`，`lib.rs:974-2103`）

布局断言 / 两端序魔数 + BigTIFF（并拒 `MM` + 44）/ LZW RGB 往返 α=255 / RGBA 源保 alpha /
灰度与 Gray16 扩展 / 调色板与 CMYK → UNSUPPORTED / 查询与渲染一致 / 缓冲不足 / 预算 /
巨尺寸 / 截断与垃圾 / 非 TIFF 输入 UNRECOGNIZED / 第二页 UNSUPPORTED / 成功清空 last-error。

m36 新增 9 个测试，外加 `raw_tiff` 造样本工具（含"count==1 的值必须内联"那条规格坑）：
`caps_declare_scaled_now`、`uncompressed_strips_are_addressable_and_direct_write_matches_library`
（1/3/4 通道整图直写与库路径**逐字节相同**）、`boxed_row_sampling_filter_is_exact`（取样腿口径）、
`averaged_mode_is_a_true_block_mean`（真平均腿 = 块均值）、`only_small_boxes_get_sampled`、
`scaled_plan_never_lets_the_host_upscale`（7 形状 × 5 框尺寸矩阵不变式）、
`scaled_query_reports_scaled_bytes_and_small_images_stay_plain`（查询按档位申报 + 小图不缩）、
`addressability_gates_reject_what_we_cannot_index`（压缩 / 反相 / 条带偏移越界 / RowsPerStrip=0）、
`multi_strip_files_are_addressable_too`（1/5/7 行每带跨带边界）。

m89 新增 `scaled_modes_handle_ragged_edges_for_every_layout`：1/3/4 通道 × 三组不整除尺寸，
取样腿与真平均腿都逐像素对照独立参考值（含边缘格按真实样本数取整、α 填充）。
通道特化改的是循环结构，这条用例就是"结果不许变"的锁。

样本 6 个，全在**全仓共享**的 `testdata/golden/`（不在本 crate 里）：`cmyk_8x8`、`golden_16x12_rgb`、
`grey16_4x4`、`grey_6x4`、`palette_8x8`、`rgba_8x8`，均 `include_bytes!` 编译期嵌入。
**未压缩样本不入库**，由 `raw_tiff` 在测试里现造——造出来的字节与真文件同构，且能随手改标签造畸形头。
（原 `src/testdata/red_1x1.tif` 零引用，2026-10-25 随夹具迁移一并删除。）

## 维护注意

* `unwrap_or(1)` 用在 PlanarConfiguration / Predictor 上（缺 tag 视为默认值）——安全但会改变拒绝面。
* 两趟解码交叉校验意味着：任何"只读一次头"的重构都要保留这个一致性检查。
* **查询与渲染两次调用各自算一遍 `plan`**，两次的输入（字节 + req）相同所以结果相同。
  任何引入随机 / 时间 / 全局状态的改动都会让宿主按查询申报值分配的缓冲被渲染写越界。
* 改 `put_px8` 同时影响两条腿——它就是"两条腿不许变色"的那道锁。
