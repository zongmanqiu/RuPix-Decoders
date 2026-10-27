# common 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.2.1 — 2026-10-27 — abi 5

- **状态**：待上传（ZIP 由发布流程生成）。
- **变更**：支持真正的 headerless DIB（补合成 `BITMAPFILEHEADER`）；`.dib` 不再依赖“带 BM 头的 BMP”冒充。
- **认领**：29 个后缀、成员与能力位不变。
- **基线**：仍为纯 Rust；无 C 编译依赖。

## 0.2.0 — 2026-10-27 — abi 5

- **状态**：发布包已生成；等待 Gitee/GitHub 上传。
- **变更**：DLL 自述增加可扩展 `iv_metadata()`（version/author/license/released）；包格式改为 ZIP + 同版本许可证目录。
- **认领**：29 个后缀、成员与能力位不变。
- **基座**：仍为纯 Rust；无 C 编译依赖。

## 0.1.0 — 2026-10-26 — abi 5

- **状态**：已发布。release tag `common_0.1.0`，资产 `common_0.1.0.dll`。
- **认领**：29 个后缀（jpg/jpeg/jpe/jfif/jif/jfi/exif/png/apng/gif/tif/tiff/qoi/
  webp/bmp/dib/rle/ico/icon/pnm/ppm/pgm/pbm/pam/dds/ff/tga/psd/hdr）。
- **成员**：8 个（jpeg、png、gif、tiff、qoi、psd、hdr、core）。
- **合并**：8 个独立解码器合并为一枚 DLL，底座只留一份；体积
  1,967,104 B（对原 6 枚独立解码器口径约 −50%）。
- **纯 Rust**：无 `-sys`、无 `cc`、无 `bindgen`；JPEG 不带 turbo，WebP 走
  `image-webp`，PSD 自解像素，HDR 走 float 路径。
- **让位**：查询阶段软失败换成员；渲染阶段换人前重问尺寸，装不下报
  `ERR_BUFFER_TOO_SMALL`。动图第 2 帧起由 `core` 会话缓存接住。
- **能力位**：只声明整解；不声明 `CAP_SCALED`（缩放归宿主基座）。
- **证据**：`fallback_chain_hands_over_to_the_generalist`、
  `animation_frames_fall_back_inside_the_dll`、
  `psd_and_hdr_render_through_the_merged_entry_point`。
- **升级建议**：首个发布版本；无更早版本可升级。
