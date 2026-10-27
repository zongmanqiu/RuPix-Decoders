# raw — 相机 RAW + 裸像素数据

一枚 DLL，认领 **40 个后缀**：29 种相机 RAW（含 `.raw`）+ 11 种裸像素 / YUV；
`.raw` 也走 rawpx 的参数化入口，按请求类型分派，唯一认领一次。
相机 RAW 的 demux 与传感器解压交给 `rawler`；**显影链（线性化 → 去马赛克 →
色彩矩阵 → gamma）在本档内自写**。裸像素部分整体转发给 `rawpx`，只走参数化
`iv_render_raw`。

## 身份

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `raw` / `rupix-raw` / **0.5.0**（已发布，2026-10-27，见 `decoders.json`） |
| name | `Camera RAW & Raw Pixel Data` |
| 后缀 | 29 种相机 RAW（含 `.raw`）+ `rgb rgba gray gry bgra yuv i420 yv12 nv12 yuy2 uyvy`（11） |
| is_vector / has_alpha | `0` / `0` |
| 产物 | `raw_0.5.0.dll` |
| 构建 | 纯 Rust；无 `build.rs`、无 C 源、无 `.3rd` 依赖 |
| 许可证 | `rawler 0.7.2`（LGPL-2.1）+ `flashraw 0.1.1`（MIT/Apache-2.0）静态链接；ZIP 附许可证全文与 `SOURCE-OFFER.md`（rawler 上游 commit + crate 校验值 + 源码/重链接书面 offer） |

后缀表由 `examples/dump_exts.rs` 对 `rawler::decoders::supported_extensions()` 校准。
`.raw` 同时是相机 RAW 与裸像素 dump 的后缀，归属由**请求类型**决定：常规打开走魔数，
参数化请求由宿主按 `CAP_RAW_PARAM` 筛到本档。

## 成员

| 成员 | 作用 |
|---|---|
| `rawler 0.7.2` | 相机 RAW 的容器分派、传感器解压（CFA u16） |
| `flashraw 0.1.1` | X3F 回退：rawler 的 X3F 压缩路径未实现，真实 X3F 走 flashraw Rgba8 |
| `rawpx` | 裸像素参数化解码；**不在 `MEMBERS` 让位链里**，唯一入口是 `iv_render_raw` |

## 认领

- `iv_probe` 先按魔数认领；对 CRW/ORF/ORI/RW2/RWL/ARI/QTK/Leica RAW 这类
  没有 TIFF 魔数的真实文件，带上认领后缀后让 rawler 的头部解析器判定。
  无头裸数据（`.rgb` / `.yuv` 等）仍不认领，只走 `iv_render_raw`。`len < 12` → 不认。
- 置信度只有 100 / 0；没有 50 档。
- **能力位**：`CAP_FULL | CAP_SCALED | CAP_RAW_PARAM`。`iv_render_raw` 是可选导出；
  声明 `CAP_RAW_PARAM` 与它绑定。
- 认领面与 `common` 的 TIFF 有重叠：真 TIFF 照片可能先进本档，这是 blanket 认领的
  已知副作用；本档按真 RAW 解，失败后宿主可回退。

## 渲染

- 查询（`out == NULL`）：`rawler::decode_dummy` 只读头拿尺寸/方向，再过 `tier_plan`
  决定报整图还是报档位。报的是**画幅**，不是整块传感器。
- 渲染：`rawler::decode` 永远 page 0；厂商/容器分派全在 rawler 内部，本档没有
  按厂商分支。
- 出画两条腿：点亮缩放意图且本档能分箱 → `develop_binned` 直接出低清档；
  否则 `develop` 全尺寸显影。
- 画幅：`crop_area` → `active_area` → 整块；越界钳进原样；不做亮度启发式。
- 色彩矩阵：读 `color_matrix`，优先 D65，缺则按 illuminant 枚举序取最小；
  `normalize(xyz2cam · SRGB_TO_XYZ_D65)` 行和归一；白平衡折进列；
  sRGB gamma 走单值桶表。
- 方向不应用，交给宿主；画幅矩形在未旋转的传感器坐标里。
- 缓存：thread-local 单槽，键 = 首 64 B + 尾 64 B + 长度 + 请求档位；
  与返回值共享 `Arc<Vec<u8>>`。

## 限制与已知空白

- `MAX_PIXELS = 64 MP` 只在查询阶段检查；宿主跳过查询直接渲染可绕过。
- 自写双线性 demosaic 比 Malvar / AMaZE 软，这是明写的取舍。
- 没有 sidecar、没有 X3F 专用解析、没有内嵌 JPEG 预览层选择；永远全传感器解码。
- 画幅只听元数据；没有 `crop_area` / `active_area` 的文件交整块原样。
- compressed RAF 的真实白电平没有有内容的真机样本可验。
- 超 64 MP 的 RAW 今天开不了，即使视口只要很小一档。

## 测试

30 个 `#[test]` 覆盖探针、方向、矩阵、gamma、白平衡、黑帧、分箱、画幅、
缓存键与两阶段尺寸。examples：`dump_exts`、`smoke_raw`、`bench_raw_stages`、
`bench_raw_bin`、`check_raw_tier`、`probe_raw_frame`。
细节见 [`DESIGN.md`](DESIGN.md)，对外变化见 [`HISTORY.md`](HISTORY.md)。
