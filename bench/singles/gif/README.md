# gif — GIF 冠军插件（只出首帧）

一句话：`gif` 的选取景器，gif crate 直调解首帧画布；**第 1 帧之后一律 `ERR_UNSUPPORTED`
让位 `core` 的动画会话**。零拷贝快路是它的性能来源。

## 身份（`src/lib.rs:65-74` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `gif` / rupix-gif / **0.1.0** |
| name | `GIF (gif crate)` |
| extensions | `gif` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `gif_0.1.0.dll` |

依赖 `gif_crate = { package = "gif", version = "0.14" }`（锁 **0.14.2**，MIT / Apache-2.0）——
别名同样是因为 `[lib] name = "gif"` 撞名（`Cargo.toml:26-27`）。
dev-dependency `image = "0.25"`（只开 gif 特性）当对拍基准。
根 `Cargo.toml:69-70` 给 `gif` 库提 3，**插件本体没进例外清单**：首帧合成的逐像素循环其实
跑在 `opt-level = "z"` 上——要提性能先看这里（提完必须按仓库口径重测，不许只改不测）。
无 `build.rs`、无 C 源、无 `.3rd` 依赖。

## 认领（`iv_probe`，`lib.rs:100-121`）

**纯魔数**，`_ext` 忽略。`len < 13`（一个 LSD 都不全，`MIN_LEGAL_LEN :77`）→ 0；
前 6 字节必须 `GIF87a` 或 `GIF89a`；逻辑屏宽（LE u16 @6）或高（@8）为 0 → 0；否则 **100**。
没有第二档。

## 解码路径

1. 第 13 字节 LSD 自己解析出 `sw × sh`（`screen_size :224-235`）→ `npixels`；
   `> MAX_PIXELS(64 MP)` → 7，`bytes_needed = npixels*4` 超 `budget_bytes` → 7。
2. `out == NULL` → 只填 info（查询阶段还没探帧，`frame_count = 0`），ERR_OK（`:189-194`）。
3. `frame_index != 0` → **`ERR_UNSUPPORTED "动图播放由 core 插件负责，本冠军插件让位"`**
   （`:197-200`）。
4. `decode_first_frame`（`:318-422`）：`DecodeOptions` + `ColorOutput::RGBA`（`:243-247`）；
   库报的宽高必须等于自解析 LSD，否则 `ERR_CORRUPT "gif: screen ... disagrees with header ..."`；
   帧矩形 + 原点必须落在屏内，否则 CORRUPT。只有调色板索引 → RGBA 的展开，
   **没有 YUV 矩阵、没有 ICC 转换、没有预乘**。

两条合成路径：
* **整屏帧**（矩形 == 全屏，`:362`）：`read_into_buffer` **直接写宿主缓冲**；透明像素把 RGB
  清零（`:373-381`）。
* **局部帧**：先 `dst.fill(0)`，再逐行 blit；透明像素**保留画布值 `(0,0,0,0)`**，
  而不是 gif crate 的"调色板色 + α=0"。这不是美化：`image` 的 `into_frames` 就是这个字节结果，
  而第 1 帧之后由 `core` 播，两边字节必须一致，否则第一次播放跳变（`:382-413`；
  对接点在 `decoders/core/src/lib.rs:209-213, 423-467`）。

## 多帧语义

首帧之后不服务。总数由 **`next_frame_info()` 的表头跳读**得到：`count_frames`
（`:268-287`，只读块结构、LZW 段按块跳过不跑解码器，一趟 O(流长度)）+ 一槽缓存
`cached_count` / `COUNT_CACHE` / `fingerprint`（`:289-313`，键 = `(len, head32, tail32)`，
与 core 的会话键同法）。调用点在 `decode_first_frame` 尾部（`:412-420`）。
* 探不到第二帧 → `frame_count = 1`、`delay = 0`（静图）。
* 探到 → `frame_count = 真总数`（**旧行为在这里报 0=未知，于是宿主刚打开就暂停只能写
  `帧 1/?`**）、`frame_delay_ms = raw_centi × 10` 且**夹到 ≥ `MIN_DELAY_MS = 10`**（`:79-84`）。
* ★ **为什么非要另开一个解码器还要缓存**：本机配对实测（同一把尺，只换 DLL，
  `.plan/m22-逐帧浏览-GIF-2026-10-18.md` §十）——不缓存时"回到第 0 帧"从 2.0 ms 涨到 6.0–6.9 ms
  （循环回绕 ③ 与后退到头都走这条路，每次都要重扫 8 MB）；缓存一槽之后实测
  ③ 回到 2.36 / 2.52 ms，与旧版 2.53 / 2.88 ms 同量级，而第 0 帧就报得出 100 帧。
  另外：**接着已经解过第 0 帧的那个 `dec` 往下走**比新开一个更贵（实测 render 4.8 ms vs
  基线 2.0 ms），所以数帧必须是独立一趟、且必须缓存。
* 中途数不动 → 退回 `frame_count = 0`（未知）。★ **不许**因为"总数没数清"把一张首帧
  解得出来的图报成"文件已损坏"——这条由 `uncountable_tail_still_opens_and_never_fakes_a_total`
  钉住；宿主拿到 0 就只写 `帧 1`，不写分母、也不写问号。
* 迭代上限 `FRAME_SCAN_CAP = 100_000`（`:82-84`）只防"每次迭代都不消费字节"的畸形输入把
  数帧循环本身变成挂死点，与 core `gif_anim::count_frames` 同值。

**dispose 方法（2/3）完全不处理**：首帧画在透明画布上。

## 元数据 / 色彩

`PF_RGBA8`、`bit_depth = 8`、`orientation = 1`、`already_oriented = 1`、`page_count = 1`、
`icc_len = 0`（`fill_info :193-210`）。GIF 没有方向语义，声明已定向以免宿主二次旋转。

## 错误码口径

`dec_err :401-416`：`OutOfMemory | MemoryLimit` → 7；`Format | LzwError | Io | UnexpectedEof |
DecoderNotFound | EndCodeNotFound` → 2；`#[non_exhaustive]` 未知变体 → **3**（诚实交接给 core）。
消息前缀一律 `gif:`，另有通用的 1/4/7 入口与缓冲错误。

## 限制与已知空白

* `target_w/target_h` 从不读（缩放归宿主），`flags`、`page_index` 忽略。
* 动画播放能力 = 0，这是设计而非 bug；要加第二帧必须走 `core` 的 `gif_anim` 会话，
  否则首帧/后续帧字节口径一断，播放就花。
* 本 crate **没有样本文件**：测试样本都在内存里用 `gif::Encoder` 现生成（原 `src/testdata/red_1x1.gif`
  零引用，2026-10-25 已删）。

## 测试（13 个 `#[test]`，`lib.rs:447-855`）

覆盖：布局钉子、魔数与伪魔数（含零宽屏、null、长度不足）、查询阶段报屏尺寸、
静图与 `image` 逐字节一致、多图报**精确总数（3 帧报 3）+ 精确延时 + 第 2 帧 UNSUPPORTED**、
零延时夹到 10 ms、透明首帧字节对拍、局部矩形首帧对拍（屏外区域透明 + (1,1) 处 2×2 蓝）、
缓冲不足、65535² 巨屏在分配前拒绝（probe 仍给 100）、预算 8 字节拒绝、截断 → CORRUPT、
尾块非法但首帧可解 → 照开，总数只允许"数得出"或 0。

## 维护注意

* 模块文档记录了真实历史：初版发布配置**输给 `image` 2.2%**，是零拷贝快路把比值拉回 1.17×
  （`lib.rs:10-15`）——改缓冲策略前先看这段，别把快路改回去。
* gif 0.14.2 的类型叫 `Decoder` 不叫 `Reader`（`:89-91` 注释留了痕）。
* 生产路径零 panic 源；`expect` / `unwrap_err` 只在 `#[cfg(test)]` 里。
