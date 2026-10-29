# rawpx — 裸像素 / YUV 参数化解码器

一句话：**唯一一个 `iv_probe` 恒返 0、只能靠 `iv_render_raw` 工作的解码器**。
没有魔数、没有文件头，宽高与布局由宿主向用户收集后传进来。

## 身份（`src/lib.rs:42-49` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `rawpx` / rupix-rawpx / **0.1.0** |
| name | `Raw Pixel Data` |
| extensions | `raw;rgb;rgba;gray;gry;bgra;yuv;i420;yv12;nv12;yuy2;uyvy` |
| is_vector / has_alpha | `0` / `1` |
| 产物 | `rawpx_0.1.0.dll`（201 216 B） |

依赖只有 `rupix-abi`（契约 + kit，样板已并入）（`Cargo.toml:20-20`）：无第三方解码库、
无 `build.rs` / `csrc` / `.3rd` 依赖 / `examples` / `testdata`。`crate-type = ["cdylib"]`。
★ 它在 **workspace 默认 `opt-level = "z"`** 下编译，**没进例外清单**，而下面全是逐像素循环——
这是 33 个单库里唯一一个"性能相关却没有 O3 例外"的 crate。要提记得按仓库口径重测。

## 认领：故意恒 0

`iv_probe`（`lib.rs:68-77`）就是 `guard(|| 0)`——**给了 `ext` 也返回 0**。
理由写在 `:60-66`：`.raw` 既是相机 RAW 也是各种转储的尾巴，按后缀认领等于"双击任意二进制
就用上次记住的几何去解"。仲裁的另一半在宿主侧（宿主 `src/decoders/select.rs` 的 `select_candidate`，`require_caps = CAP_RAW_PARAM`）。

`iv_render`（`:81-110`）是桩：永远 `ERR_UNRECOGNIZED` +
`"裸数据需要宽/高/布局参数（请走 iv_render_raw）"`。

## 导出：`iv_render_raw`（规范 §十二，v4 可选导出，不 bump 版本）

`lib.rs:116-209`。两阶段与 `iv_render` 一致（`out == NULL` 只填 info，`lib.rs:196-199`），
然后先做**长度必须严格等于 `expected_len`** 的检查（`:178-183`）再碰像素。

布局表（`expected_len :170-197`）：

| 类别 | 布局 | 每像素字节 | 约束 |
|---|---|---|---|
| packed | `PF_GRAY8` / `RGB8` / `RGBA8` / `BGRA8` / `GRAY16` / `RGB16` / `RGBA16` | 1 / 3 / 4 / 4 / 2 / 6 / 8 | — |
| packed 4:2:2 | `YUY2` = `Y0 U Y1 V`、`UYVY` = `U Y0 V Y1` | 2 | **宽为偶数** |
| planar 4:2:0 | `I420` = Y+U+V、`YV12` = Y+V+U、`NV12` = Y + 交错 UV | 1.5 | **宽高皆偶** |

像素数学（`:245-342`）：16 bit 通道 `>> 8` 降位（±1 vs ÷257，`:273-274` 注释）；
4:2:2 奇数像素复用相邻色度组（`:292-316`）；4:2:0 取 2×2 块左上色度（`:319-338`）；
YUV→RGB 是**定点 BT.601 有限范围 10 bit、+512 舍入**：1192 / 1634 / 400 / 833 / 2066（`:344-359`）。

## 多帧 / 多页 / 元数据

都不存在：`frame_index`、`page_index`、`target_w/h`、`flags` 不读，只读 `budget_bytes`（`:185-189`）。
`fill_info`（`:362-375`）把 out 的每个字段都写满：`PF_RGBA8`、depth 8、`orientation = 1`、
`already_oriented = 1`、`frame_count = 1`、`frame_delay_ms = 0`、`page_count = 1`、`icc_len = 0`。
元数据/ICC 按构造就是 absent（根本没有头）。

## 错误码口径

1（空输入）、4（空 info、`out_cap` 不足）、2（`宽和高都不能为 0`、
奇偶数不符 `YUV 布局要求宽为偶数（4:2:0 还要求高为偶数）`、
长度不符 `文件 {len} 字节与 {width}x{height} {layout} 布局所需的 {src_len} 字节不符`）、
3（`该布局不支持`）、7（64 MP、预算）。

## 限制与已知空白

* `MAX_PIXELS = 64 MP`（`:53-54`）**两阶段都强制**（`:163`）；预算门在 `:190`；
  **完全不分配中间缓冲**，直写宿主 `out`。
* 未支持：大端 16 bit、10 bit / packed YUV、planar 4:2:2、全范围（JFIF）YUV 变体。
* `.yuv` 不带布局信息，必须由宿主对话框选（`:19-20`）。
* `has_alpha = 1` 只对 RGBA/BGRA/RGBA16 成立；灰度与 RGB 路径 α 写死 255。
* `.raw` 与 `raw` 解码器后缀重叠（仲裁靠本解码器 probe 恒 0）。

## 测试（18 个 `#[test]`，`lib.rs:378-705`）

`abi_layout_asserts_hold`、`bgra_to_rgba_swaps_channels`、`gray16_normalizes_to_8bit`、
`rgb8_fills_alpha`、`rgba8_is_passthrough`、`two_phase_ffi_roundtrip`、`size_mismatch_is_corrupt`、
`unknown_layout_is_unsupported`、`plain_render_is_unrecognized`、`probe_never_claims_without_magic`、
`budget_is_enforced`、`yuv_to_rgb_known_colors`、`i420_2x2_upsamples_chroma`、`yv12_swaps_chroma_planes`、
`nv12_interleaved_chroma`、`yuy2_packed_422`、`yuv_odd_dimensions_rejected`、`yuv_size_mismatch_is_corrupt`。
断的是**精确字节**（如 Y=235 / UV=128 → 255，`:595-598`；Y=81 / U=90 / V=240 的红条 `:601-606`），
并断言"probe 拒绝但 1×1 RGBA 的 `iv_render_raw` 仍要成功"（`:545-561`）。

## 维护注意

`unreachable!()`（`:340`）只有在 `expected_len` 与 match 表漂移时才会命中——加布局时两处一起改。
dst 切片按 `out_cap` 构造再收缩到 `n*4`（`:247`）。`#![allow(linker_messages)]` 的原因（MSVC
link.exe 噪声）在 `:25-28`。
