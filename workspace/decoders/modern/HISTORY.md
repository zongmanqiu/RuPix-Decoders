# modern 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.3.5 — 2026-11-25 — abi 6

- **可并发调用**：新增能力位 `CAP_PARALLEL`（v6 内加法，`1 << 8`）。宿主据此对
  modern 跳过"同一枚 DLL 串行锁"——多窗口同后缀、同窗多车道（m349）不再排队。
- **审计依据**：五个成员（av/jng/jxl/qoi/ff）全部纯 Rust、零 C 库；av 的
  `DECODE_CACHE` 与 jng 的 `CACHE` 都是 `thread_local`（每线程 1 槽，跨线程无共享）；
  qoi/ff 无任何全局量；heif-oxide / zenavif / jxl-oxide 声明零 unsafe、无静态可变量。
- **并发压力验收**：4 线程 × 多格式（avif/jxl/jng/heic）× 200 次，像素与单线程
  逐字节一致，无 panic / 挂死 / 错误文案串味。
- **未变**：后缀集合、ABI 版本（v6 内加法）、单线程解码路径与像素输出。

## 0.3.4 — 2026-11-04 — abi 6

- **修复（挂死）**：AVIF/AVIFS 一律**单线程**解码（`zenavif::DecoderConfig::new().threads(1)`，
  走 `decode_with` / `decode_animation_with`）。
  病因：`rav1d-safe 0.5.7` 的多线程路径存在真实的别名竞争——`rav1d-disjoint-mut` 的
  `DisjointMut` 校验器会在 `rav1d-worker-N` 上 panic（`overlapping DisjointMut:
  current &mut _[40448..40512] ... existing &mut _[40500..40504]`）。**panic 发生在 worker
  线程，而死等它的解码调用者收不到任何信号**，于是 `iv_render` 永不返回：宿主侧表现为
  "这张图永远转圈、后面的图全堵住"，`catch_unwind` / SEH 都看不见（本线程没抛异常）。
  单线程把 worker 整条路去掉，竞争与"等死掉的 worker"一起消失。
- **代价**：AVIF 解码慢一些（1 MP 量级仍在几十毫秒；4K AVIF 从并行档降到单线程档）。
  与"偶发永久挂死"比，这个取向沿用宪章 §5.1「跨机行为一致 > 快」。
- **证据**：`noise.avif` 400×300 档重复解码——修前 3 轮×15 笔出现 1 次
  `rav1d-worker-2 panicked` + 挂死（不可稳定复现，race 决定成败）；修后 100 笔
  （4×15 + 2×10×2 文件）**0 panic、0 超时**。
- 未变：后缀集合、能力位、ABI、orientation 口径、其余成员（JXL/JNG/QOI/Farbfeld）行为。

## 0.3.3 — 2026-11-02 — abi 6

- **动图**：JXL 从"只出首帧"改为**逐帧服务**。`num_loaded_keyframes()` 数帧、
  `render_frame(i)` 逐帧渲染；`FrameHeader.duration`（tick）按
  `tps_denominator * 1000 / tps_numerator` 换算成毫秒。帧数上限 512（超限当静态图出首帧，
  避免一次解几百帧吃满内存）。静态 JXL 行为不变。
- **动图**：AVIF/AVIFS 逐帧服务（`decode_animation` 优先于 `decode`——后者对动图
  也返回 `Ok` 且静默只给首帧）。每帧独立时长取自动画头。
- 未变：后缀集合、能力位、ABI、orientation 口径（仍在解码时应用并报 `already_oriented=1`）。

## 0.3.2 — 2026-10-29 — abi 6

- 收编 `qoi` 与 `ff`：QOI 复用 `rupix-qoi`，Farbfeld 使用新 `rupix-ff` 单库。
- 后缀从 7 增到 9；原有 HEIC/AVIF/JXL/JNG 行为不变。

## 0.3.1 — 2026-10-28 — abi 6

- **变更**：ABI 代次前移到 **6**。v6 是**纯加法**（新增可选导出 `iv_render_region` 与新结构体 `IvRegionReq`），既有导出签名与结构体布局**逐字节未动** ⇒ 本档的解码行为与上一版完全一致。
- **未变**：后缀集合、像素格式、识别与容错逻辑均不变；本档不提供任何矢量能力。

## 0.3.0 — 2026-10-28 — abi 5

- **变更**：新增 JNG（JPEG Network Graphics）成员；JPEG 图像 + PNG 灰度 alpha。
- **认领**：7 个后缀（heic/heif/hif/avif/avifs/jxl/jng）。
- **证据**：`renders_jpeg_with_png_alpha`、`claims_only_its_own_seven_suffixes`、CC0 `heic_640x480.heic` golden。

## 0.2.0 — 2026-10-27 — abi 5

- **变更**：DLL 自述增加可扩展 `iv_metadata()`；包格式改为 ZIP + 同版本许可证目录。
- **认领**：6 个后缀、成员与能力位不变。

## 0.1.0 — 2026-10-26 — abi 5

- **认领**：6 个后缀（heic/heif/hif/avif/avifs/jxl）。
- **成员**：`av`（HEIC/HEIF + AVIF，`heic` 开 `av1` feature）与 `jxl`（jxl-oxide）。
- **崩溃域**：与现代格式的年轻解码库隔离，坏 HEIC/AVIF/JXL 不拖累 common 的 33 个后缀。
- **能力位**：只声明 `CAP_FULL`；预测编码没有解码期降采样入口，缩放归宿主基座。
- **让位**：查询阶段按置信度换成员；渲染阶段换人前重问尺寸。
- **证据**：`claims_only_its_own_six_suffixes`、
  `both_members_render_through_one_entry_point`、
  `unknown_bytes_are_not_claimed_and_not_rendered`。
- **已知空白**：`heic/heif/hif` 当前无 golden 夹具；AVIF/JXL 有样本。
- **升级建议**：首个发布版本；无更早版本可升级。
