# core — 常用位图兜底解码器（一个 DLL 装十种格式）

一句话：**唯一带 50 分置信度档的解码器**。冠军解码器（jpeg/png/tiff/gif）不吃、或吃不动的位图，
全在这里兜住；GIF / APNG 的第 1 帧之后**只有这里会播**。

## 身份（`src/lib.rs:41-56` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `core` / rupix-core / **0.3.0** |
| name | `Core Raster Formats` |
| extensions | `apng;webp;bmp;dib;rle;ico;icon;pnm;ppm;pgm;pbm;pam;dds;ff;tga` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `core_0.3.0.dll` |

`crate-type = ["cdylib", "rlib"]`（`Cargo.toml:11`）——`rlib` 是给 `examples/` 三个工装用的
（thumbdoc / qoi 只有 `cdylib`）。

依赖：`image = "0.25"`，`default-features = false`，features
`["png","jpeg","gif","bmp","ico","tiff","pnm","tga","dds","ff"]`（`:24-42`）；
`libwebp-sys2 = "0.1"`，features `["1_1"]`（`:46`）。dev：`image` 加 `webp`、`gif = "0.14"`（`:50-53`）。

**DDS 的能力边界**（依赖局限，非解码器缺陷）：image-rs 的 DDS 解码只接受 FourCC 压缩格式
（DXT1/DXT3/DXT5，以及 DX10 头里的 BC1/BC2/BC3），且宽高必须是 4 的倍数——未压缩 RGBA/RGB
DDS、DX10 BC7 等一律返回 `ERR_UNSUPPORTED`，奇数尺寸返回 `ERR_CORRUPT`。样本生成脚本
`testdata/make_samples.py` 据此写 DXT1 并自动把边长对齐到 4（见 ROUND 表）。

**WebP 的 C 集成方式**（很多人以为本仓库有 libwebp 源码，其实没有）：`libwebp-sys2` 0.1.11
来自 crates.io（`Cargo.lock:908-920`，依赖 `cc`/`cfg-if`/`libc`/`pkg-config`/`vcpkg`），
它把整棵 libwebp 树 vendored 在自己的 `c_src/` 下，由**它自己的 build.rs**按 Makefile 的
`libwebpdecode_la_SOURCES` / `libwebpencode_la_SOURCES` 编译（其 build.rs `:86-97`）。
`1_1` feature 只管 API 版本符号（其 crate 自己的 `Cargo.toml`，不在本仓库）；`mux`/`demux`/`static` **没开**，
cwebp/dwebp 命令行不构建。core 自己没有 `build.rs`。

## 为什么是"一个 DLL 装十种"而不是十个 DLL

`lib.rs:8-19` 记着实测与账：`image` 路线比冠军 DLL 慢（JPEG 1.80× / PNG 1.19× / GIF 1.18× /
TIFF 1.24×），所以 **`jpg/jpeg/jfif/png/gif/tif/tiff` 被刻意从扩展名表里删掉了**
（`declares_a_broad_extension_list`，`:2094-2121` 的 `gone` 断言钉住它们不在场）。留下的都是没有冠军的格式 + 冠军吃不动的（APNG 交给这里，
因为 PNG 冠军只管静图；GIF 第 1 帧之后也交给这里）。
另一个理由：每个 DLL 有约 101 KB 固定开销且不能跨 DLL 共享依赖——十个 DLL 就是十倍。

## 认领（`iv_probe`，`lib.rs:81-106`）

三档逻辑，`CONFIDENCE_GENERALIST = 50`（`:62`）：
1. `ext == "tga"` → **100**（TGA 没有魔数，只能靠后缀，`:90-93`）。
2. `data.is_null()` 或 `len < 8` → 0（`:95`）。
3. RIFF + WEBP 魔数（`:369-371`）→ **50**；否则 `image::guess_format` 成功 → **50**，失败 → 0。

50 这个数是设计出来的（`:73-78`）：低到能让报 100 的冠军在宿主仲裁里赢，
又高到冠军 DLL 缺席时图还能开。

## 解码路径

两阶段（`:111-304`）：

1. 第 1 阶段只读头拿尺寸（`:153-193`），按格式分四路：GIF 走会话查询；APNG 直读 IHDR；
   WebP 用 `WebPGetInfo`（`:412-422`）；其余 `ImageReader::with_guessed_format().into_dimensions()`。
   然后 64 MP 上限（`:65, 196-199`）、`budget_bytes`（`:202-210`）、`out == NULL` 只回 info（`:231-234`）、
   `out_cap` 检查（`:228-231`）。
2. 第 2 阶段路由（`:244-296`）：WebP → `WebPDecodeRGBA` + `WebPFree`（`:425-441`），
   **动图 WebP 明确不支持，只出首帧**（`:236-239`）；GIF/APNG → 会话 `serve(frame_index)`；
   TGA → `ImageReader::with_format`（`:501-506`）；其余 → `decode_rgba`（guess + decode + `into_rgba8`，
   `:509-515`）。解完长度必须等于头部的 `bytes_needed`，否则
   `ERR_CORRUPT "解出的尺寸与文件头不符"`（`:293-296`）。
   不做 DCT 缩放，缩放归宿主（`:243`）。

## 多帧会话（本解码器最重的部分）

`gif_anim` / `apng_anim` 两个近似重复的模块（`:552-795`、`:802-1044`）：
* **2 槽 LRU 会话池**，键 = `(len, head32, tail32)` 指纹（`:586-594`）——不是指针键。
* 串行帧游标推进；请求回退时**重建 + 快进**（`:697-703`），因为 GIF/APNG 的帧间有依赖。
* ★ **快进跳过的帧进 `FrameRing`（`:502-546`）**（填环在 `:734-749`）：环按帧号索引、
  16 MiB 预算、2–16 帧，**只在后退快进时填**——顺序播放一帧都不进环，热路径零额外拷贝。
  为什么放解码器侧：这些帧合成完就被丢掉，宿主从来没见过它们，宿主那份"落地帧缓存"救不了
  连续后退。实测（`bench-100f.gif` 640×480，配对同一把尺只换 DLL）：**连退 20 步
  3.82 s → 0.22 s**，后退一步 178 ms → 1.3 ms。仍然不是零成本——环每耗尽一次就要付
  一次线性重建（实测最慢一步 171–217 ms），这是串行合成的固有代价，不是漏掉的优化。
  回归锁：`gif_backward_steps_serve_from_ring_and_match`（逐字节比对后退帧像素 +
  断言"顺序播放不填环"）。
* ★ **总帧数第一帧之前就已知**：GIF 走表头数帧 `count_frames`（`:625-648`，`gif::Decoder::next_frame_info`
  内部是 `OutputBuffer::None`，LZW 段只跳块不解像素）；APNG 直接读 `acTL` 的头 4 字节（`:884-915`）。
  代价 = 每**建一次会话**多一趟 O(文件字节) 的扫块（会话池 2 槽，所以一张图只付一次，
  不在每帧路径上）。配对实测（`.plan/m22-逐帧浏览-GIF-2026-10-18.md` §十，只换 DLL）：
  顺序播放 ② 4.45/4.46 → 4.64/4.65 ms/帧（**+0.19 ms，单帧的 4%，我没能进一步归因**），
  ③ 循环回绕 与 ④ 后退一步与旧版同量级；同款循环的独立探针（opt 3 直调 gif crate）
  在 8.29 MB / 100 帧上是中位 0.33 ms。★ 同一段循环在 `gif` 冠军解码器里**没缓存时要 3–12 ms**
  （每次回到第 0 帧都重扫）——那个解码器因此必须配一槽缓存，见 `decoders/gif/README.md`。
  数不出来的畸形文件回落到 EOF 惰性发现（`:727-740`），最坏只是宿主不写分母，不报错。
  索引取模回绕（`:683-689`）→ 宿主可以一直播。
* 延时夹到 ≥ 10 ms（多帧时，`:545, 760-762`）；`count = Some(1)` 视为静图（delay = 0，`:250-254`）。
* 查询阶段就把帧数报给宿主（`oi.frame_count = count`，`:328`）——单帧 GIF 因此第一轮就报 1，
  不再需要"多要一帧撞 EOF"才被认成静图。
* `unsafe impl Send for Session` 的理由写在 `:577-583`；两个静态 Mutex 池在毒化后恢复（`:639, 905`）。
* APNG 画布尺寸从 IHDR 偏移 16/20 的 u32BE 直接读（`:874-882`），`PngDecoder::apng()` 判动画（`:867-870`）。

## 元数据 / 色彩

**方向只在 WebP 路线读**（m48）：`:212-220` 用 kit 的 `webp_exif_orientation` 逐 chunk 走 RIFF
找 `EXIF`，`fill_info` `:351-355` 照实报 `orientation` + `already_oriented = 0`，转正由宿主做。
GIF / BMP / ICO / TGA / PNM / DDS 这些容器**没有方向标签**，一律报 1——GIF 表头那条 `:326-327`
也改成 `1/0`：本解码器从不摆正像素，别拿 `already_oriented` 表达"这格式不需要"。
ICC **只有 WebP 路线透传**：`:222-226` 取 RIFF 里的 `ICCP`，`fill_info` `:388` 照实报 `icc_len`
（像素后附加段，见规范 §十一）。GIF 查询阶段的专用回填是唯一的零值出口：`:354-356`
（`image` 不出 ICC，`page_count` 恒为 1）。**别把这句读成"本解码器不做 ICC"**——m53 之前确实不做，现在是做的。
`req.page_index` 忽略，只用 `frame_index`（`:144-148`）。`iv_render_raw`：absent。

## 错误码口径

`map_err :968-977`：`Decoding`→2、`Encoding`/`Unsupported`→3、`Limits`→7、`IoError`→1、兜底→2。
消息：`内部错误（宿主传入空数据）`→1、`内部错误（宿主传入空输出指针）`→4、`读文件头失败，库报错 "{e}"`、
`图片超过 6400 万像素的解码器上限`→7、`图片太大，超出宿主给的内存预算`→7、`WebP 数据损坏`→2、
`GIF/APNG 文件头损坏`、`GIF/APNG 帧解码失败`→2、`内部错误（宿主给的缓冲小于申报值）`→4。

## 限制与 ★ 维护者须知

* 64 MP ≈ 256 MiB（`:65`）；下层由 image crate 的 `Limits` 转 TOO_LARGE。
* `is_apng` 在前 64 KiB 里扫**任意** 4 字节 `acTL`（`:407-409`）：早期 IDAT 里的伪 `acTL`
  会把静图 PNG 骗进 APNG 路（结果是按 corrupt 失败，不会出错图，但会误报）。
* 生产路径有约 10 处 `unwrap()`，集中在 `gif_anim` / `apng_anim` 的 `serve` 循环
  （`:695, 719, 729, 745, 750, 955, 979, 988, 1003, 1008`）——都有不变式支撑，
  但一旦破口就是 `ERR_PANIC`，喂进宿主三振黑名单。
* unsafe：裸切片（`:98, 143`）、`copy_nonoverlapping` 进宿主缓冲（`:306-316`）、`CStr::from_ptr`（`:336`）、
  libwebp FFI（`:376, 388, 397`）。

## 测试与工装

23 个 `#[test]`（`lib.rs:1197-2180`）：7 个格式往返（png/jpeg/gif/bmp/tiff/webp/pnm，断 probe=50、
4×3 查询、无损格式首像素逐字节一致，JPEG/GIF 因有损豁免 `:1185-1190`）、垃圾拒绝、
TGA 按后缀认领、`gif_frames_play_and_wrap`（表头数帧直接得 3 → 越界取模回绕 → 100 ms）、
`gif_single_frame_is_static`（第一轮就 count=1、delay=0）、
`gif_backward_steps_serve_from_ring_and_match`（倒着翻也命中环形会话，且字节与顺序播放相同）、
`frame_counts_are_exact_and_survive_hostile_bytes`（真样本数得准 + 任意前缀不 panic +
`acTL` 的 40/44/45 字节边界 + 长度域撒谎的块框必须收敛）、
`gif_transparent_frames_composite`（与上游 `Frames` 逐字节对拍）、
`apng_frames_play_and_wrap`、`webp_icc_profile_is_declared_and_appended_after_pixels`（ICC 走
`icc_len` 附加段、排在像素之后）、预算、短缓冲、截断 PNG 在渲染阶段失败（查询阶段按设计能过）、
扩展名表断言、布局断言、成功清 last-error。
样本只有一枚：`testdata/golden/apng_test.png`（3 帧 4×3，全仓共享目录，`include_bytes!` 编译期嵌入）。
examples：`bench_core.rs`（逐格式速度，3 MP 程序图）、`bench_webp.rs`（image-webp vs libwebp，
参数 = 路径）、`gen_samples.rs`（写 `bench.{jpg,png,webp,tif,bmp,gif,ppm,tga}` 2000×1500；
拒绝 `-` 开头的参数，`:22-26`；样本不入库）。
