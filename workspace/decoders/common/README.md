# `common` — 常见位图合集（合并档 · 1 枚 DLL）

一枚 DLL 装 6 个「每格式冠军」，认领 **24 个后缀**。合并的动机只有一个：**省体积**。
当前成员：JPEG、PNG、GIF、TIFF、ICNS、core（APNG/WebP/BMP/ICO/TGA 等）。
PNM/PAM、QOI、FF、DDS 已移出：QOI/FF 去 modern，PNM/PAM 去 classic，DDS 去 texture。

每枚独立解码器都自带一份底座（空壳 + 一份 Rust 运行时，按 `qoi` 那枚 192 KB 反推约 150 KB/枚）。
六个成员单飞时底座重复 6 次，库本体却一分不少；合成一枚后底座只剩 1 份。

| 量法 | 体积 |
|---|---|
| 原 6 枚独立解码器合计（`core`+`jpeg`+`png`+`gif`+`tiff`+`qoi`） | 3884 KB |
| 合并档（0.2.4，2026-10-28，收口为 24 后缀并迁入 ICNS） | **2,241,536 B** |
| 差（对原 6 枚口径） | 约 **−50%** |

## 成员

| 成员 | 库 | 认领后缀 | 为什么是它 |
|---|---|---|---|
| `jpeg` | `zune-jpeg` 0.5 | jpg jpeg jpe jfif jif jfi exif jps mpo | 本机实测 208 Mpix/s，比 `image` 包装路线快 1.80× |
| `png` | `png` 0.18 | png | 本机实测 395 Mpix/s（`fdeflate` 后端） |
| `gif` | `gif` 0.14 | gif | 本机实测 223 Mpix/s，无对手 |
| `tiff` | `tiff` 0.11 | tif tiff | 本机实测 435 Mpix/s，全场第一 |
| `qoi` | 自研（零依赖） | qoi | 规范本身只有几百行，引库不划算 |
| `core` | `image` 0.25 + `image-webp` | apng webp bmp dib rle ico icon cur ani pnm ppm pgm pbm pam dds ff tga | 通用兜底：低成本格式 + 冠军让位时接住 |

**没有 `turbo` 腿**：`jpeg` 以 `default-features = false` 依赖，`turbo` feature 关闭
⇒ 只剩 `zune-jpeg` 整解，也不再声明 `CAP_SCALED`。缩放是基座的事
（`core::geometry::downscale`，AVX2 面积平均），解码器只负责"按真实尺寸解出一张位图"。
这一条同时把 1.25 MB 的 libjpeg-turbo 静态库挡在门外。

**WebP 走 `image-webp`**（`core` 的 `webp-rust` feature），不是 libwebp：慢 44%（3 MP 实测
90.6 vs 50.3 ms），但换来"这枚 DLL 一个 C 字符都不带"。依赖树全 Rust，
无 `-sys`、无 `cc`、无 `bindgen`。

## 路由规则（与宿主同口径，见 `src/lib.rs` 模块头）

1. **探针取最高分**：`iv_probe` 让所有成员各跑一遍 `probe_impl`，合并解码器对外报**最高的那个置信度**
   （冠军 100、`core` 的通用兜底 50、不认 0）——合并解码器只有一个身份，"我有多确定"
   必须取最确定的那个。
2. **查询阶段允许让位**：按置信度从高到低试，遇到 `ERR_UNSUPPORTED` / `ERR_UNRECOGNIZED`
   换下一家；坏文件 / 超预算 / OOM 这类硬错误立即返回（换一家也是同一个结论）。
   这一跳过去是三跳：冠军 DLL → 宿主候选循环 → `core` DLL。现在是**同一枚 DLL 内部的一次循环**。
3. **渲染阶段也让位，但换人前重问一次尺寸**：宿主是按上一个赢家申报的 `bytes_needed`
   分配缓冲的，换人后尺寸可能更大——所以每换一家都先做一次查询（`out = NULL`）核对
   装不装得下，装不下就不写、最后如实报 `ERR_BUFFER_TOO_SMALL`（本宿主不重试，但报错
   比静默失败可查）。

    **为什么必须让**：`gif` 只服务第 0 帧、`apng` 在查询阶段就让位，动图后续帧要靠
   `core` 的会话缓存。合并前那是"宿主换解码器"，合并后宿主只看得到 `common`——让位不在
   这里走完，动图从第 2 帧起每次都报"该文件用了此解码器不支持的特性"。

实测例证（`cargo test`，都是回归）：

- `fallback_chain_hands_over_to_the_generalist`：CMYK TIFF 被 `tiff` crate 拒收
  （`ERR_UNSUPPORTED`），`common` 在同一枚 DLL 内落到 `core` 用 `image` 解得开——
  而独立 `tiff` 解码器遇到同一个文件只能报错（查询阶段让位）。
- `animation_frames_fall_back_inside_the_dll`：两帧 GIF 的第 2 帧由 `gif` 软失败、
  `core` 接住（渲染阶段让位）。第一版合并只在查询阶段让位，实测 `sample-gif.gif`
  从第 2 帧起停住不动，这条就是补的回归。

错误文本会把**最后一家的原话**留在前面（`rupix_abi::last_error_text`），
尾巴才写"common 已试：tiff、core"。用户要看的是"为什么打不开"，不是"试过谁"。

## 构建（为什么是独立 workspace）

`common` 住在 `workspace/decoders/common/`，与 `modern` / `raw` 一起属于仓库根的**档 workspace**；
单库在 `workspace/singles/` 是**另一个 workspace**。

原因不是洁癖：单库同时是独立解码器（`standalone` feature 生成 `#[no_mangle] iv_*`），
而档以 `default-features = false` 只取 `*_impl`。同一个 cargo 调用里同时构建两种形态时
feature 会被**并集**，档就会链进多份 `iv_abi_version`，链接器直接报：

```
LNK2005: iv_abi_version 已经在 common.common.<hash>.rcgu.o 中定义
```

cargo 没有"按依赖方隔离 feature"的开关，只能切成两个构建图、两个 target 目录
（`.temp/decoder-target`（根 workspace）与 `.temp/singles-target`（singles））。`build.ps1` 从根/pro 取发布 DLL，
单库只跑测试不发布；发布产物统一落到 `.temp/decoder-dist`。

根 `Cargo.toml` 的 `[profile.release]` 用**全局 `opt-level = 3`**，不抄单库那份
`z + 逐包 3` 的清单——那份名单靠人一条条加（m78/m89 都是"漏了一条、热循环掉速"才发现），
而档的成员还会变。实测代价只有 +97 KB，换来"新增成员不必补 profile"。

## 扩张规则

加一个格式只有三步：① `Cargo.toml` 加依赖（`default-features = false`）；
② `src/lib.rs` 的 `MEMBERS` 加一行；③ 认领后缀写进 `export_decoder!` 的 `extensions`。
**这里不写解码逻辑**——要换库、要改算法，去那个成员的 crate（`workspace/singles/<id>/`）里改。

下一档：`modern`（heic + jxl）、`raw`（rawler + 裸像素）、`pro`（jp2/jxr/exr/…）。
分层与后缀路由表见本仓 `workspace/decoders/README.md` 与 `decoders.json`。

## 自测

```bash
cargo test --release -p rupix-common -- --test-threads=1   # 7 项
```

体积与像素一致性的对撞脚本在宿主项目 `.temp/` 下（`m138_smoke_daily.py`、
`m138_pixel_parity.py`）：16 份样本与独立解码器逐字节一致，5 组大图耗时 0.77–1.03×
（合并后**没有掉速**，部分样本更快，因为档用的是全局 `opt-level = 3`）。

## 历史

对外可见变化见 [`HISTORY.md`](HISTORY.md)。本页只保留当前状态、成员与限制。
