# classic（旧式位图档）

归档、安装包与旧工具链里仍会遇到的旧式位图格式，纯 Rust 实现。

`classic` 是一枚独立发布 DLL，不是别的档的子 crate。目录名、包名与 DLL id 都是
`classic`；以后要拆新档，按 `doc` 的同一条规则办（新 DLL 用新目录，不在本档里再列第二张表）。

## 格式

| 成员 | 后缀 | 实现 |
|---|---|---|
| `pnm` | `pnm ppm pgm pbm pam` | `image`（P1-P7） |
| `pcx` | `pcx pcc` | `image-extras` / `pcx` |
| `dcx` | `dcx` | DCX 容器里的第一页 PCX |
| `wbmp` | `wbmp` | `image-extras` / `wbmp` |
| `xbm` | `xbm` | `image-extras` / `xbm` |
| `xpm` | `xpm` | `image-extras` / `xpm` |
| `pict` | `pict pct pic` | `oxideav-pict` |
| `ilbm` | `ilbm lbm iff` | `oxideav-iff` |
| `sgi` | `sgi iris bw` | `image-extras` / `sgi` |

`rgb` / `rgba` 仍归 `raw`：它们是 SGI 的常见后缀，但已属于裸像素档。`classic` 不认领
`common`、`modern`、`pro`、`raw`、`doc`、`doc-legacy` 已声明的任何后缀。

## 认领规则

`iv_probe` 只对内容可识别的文件返回 100。二进制格式按魔数/文件头判断；文本格式要求
XBM/XPM 的声明形状。宿主传 `ext == NULL` 时走同一套内容判定，所以拖放打开不依赖文件名。

## 构建

```powershell
cd RuPix-Decoder\workspace\decoders\classic
cargo test --release -- --test-threads=1
cargo build --release
```

产物落 `.temp/classic-target/release/classic.dll`；`.cargo/config.toml` 让它不落进源码树。
统一构建脚本发布为 `classic_0.1.0.dll`。

## 限制

统一走 `pro-support::check_dims`：6400 万像素硬顶 + 宿主给的字节预算。PICT/IFF 也把
同一组限制传给解码库，恶意文件头在分配前就失败。所有第三方解码器都在 `guard` 内调用，
畸形样本只会变成 `ERR_CORRUPT` / `ERR_PANIC`，不会带崩宿主。

## 样本包

20 个后缀都有自制、CC0-1.0 的样本：10 个由 `testdata/make_classic_samples.py` 生成，
PICT/ILBM/SGI/XPM 共 10 个由 `workspace/singles/classic/examples/make_legacy.rs` 生成
（见 [`CLASSIC-SAMPLES.md`](../../../testdata/CLASSIC-SAMPLES.md)）。不打包任何第三方图片，
因此没有逐图许可要追。
