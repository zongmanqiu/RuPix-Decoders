//! RuPix 外挂解码插件：Photoshop PSD → RGBA8（库：zune-psd 0.5.1，只用它的头解析）。
//!
//! # 为什么是这个库（2026-10-25 晚，stb 单头 C 库的纯 Rust 替身）
//!
//! PSD 在 crates.io 上只有一枚纯 Rust 实现：`zune-psd`（zune-image 家族，
//! 与 `zune-jpeg` 同一个 `zune-core` 底座）。本仓为解 JPEG 已经带着
//! `zune-core`，所以换掉 stb 之后**净增依赖只有 zune-psd 一个 crate**
//! （约 12 KB 源码，无 C 源、无 build.rs），全仓至此零 C 编译。
//!
//! # 能力边界
//!
//! | 特性 | 本插件 |
//! |---|---|
//! | version | 只认 1（PSB v2 拒） |
//! | 位深 | 8 / 16 |
//! | 色彩模式 | RGB / 灰度（CMYK 要转色，stb 时代也不转） |
//! | 通道数 | ≤ 4 |
//! | 压缩 | 无压缩 + RLE，各支持 8/16 位（四格全解） |
//! | 图层 / 蒙版 | 全跳过，只出拼合图 |
//!
//! ## 为什么像素解码是插件自己写的
//!
//! 2026-10-25 逐字节对 Adobe 规范实测，`zune-psd 0.5.1` 的像素路径有三处硬缺陷
//! （头解析没有问题，所以只借它那一段）：
//!
//! 1. **8 位无压缩少读且写乱**：`decode_raw` 的循环上界写成 `pixel_count`
//!    （不是 `pixel_count * channel_count`），每个通道只读到 `ceil(px/ch)` 个样本，
//!    其余位置留 0 且写入下标交错——4 个红像素解成 `白/红/黑/黑`；
//! 2. **16 位无压缩字节反转**：先 `to_ne_bytes()` 写缓冲、`decode()` 又按 BE 解析，
//!    小端机上一律反（`0x8040 → 0x4080`）；
//! 3. **16 位 RLE 未实现**：`Generic("Not implemented or Unknown")`，而它是
//!    Photoshop 存 16 位文档的默认压缩。
//!
//! 因此 `decode_pixels` 只接受 zune 解出的尺寸/色彩空间，像素一律走下面两条
//! 自写路径（`decode_planar` / `decode_rle`）——两者都按行/平面带错读，
//! EOF 即 `CORRUPT`，绝不静默补零。
//!
//! # 两条实现级硬约束
//!
//! 1. **probe 拒识的形态不许进 render**：认了却解不动 = 把"不支持"演变成
//!    "文件损坏"（插件规范 §六 第 10 条）。
//! 2. **截断必须报 CORRUPT**：RLE 走行计数表预检、无压缩走最小长度预检；
//!    两者都只读头部，不解像素。
//!
//! # 已知口径说明（不是 bug，是继承 stb 的定义行为）
//!
//! - 16 位按**高字节**降位（`>> 8`），与 stb 的 `stbi__get16be >> 8` 一致；
//!   core 的 tiff 路线是 ÷257 四舍五入，两者差 ≤ 1 LSB。
//! - 4 通道时做"去白杂边" un-premultiply（PSD 存的是预乘值），算法与
//!   stb / zune 同源：`a ∈ (0, max)` 才处理，`c' = c/a + max*(1 - 1/a)`。
//! - 灰度模式一律按 1 通道读（zune 的定义行为）：多声明的通道直接丢，
//!   所以灰度 + alpha 的 PSD 解出来没有 alpha。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立插件形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::cmp::Ordering;
use std::ffi::c_char;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_psd::errors::PSDDecodeErrors;
use zune_psd::PSDDecoder;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "psd",
    name: "Photoshop (zune-psd)",
    extensions: "psd",
    description: "解码 Photoshop PSD（8/16 位，RGB/RGBA/灰度，RLE 或无压缩，只出拼合图）",
    is_vector: 0,
    has_alpha: 1,
}

// 只有整解：PSD 没有"原生降采样"这一说（zune-psd 不出缩略图腿），也没有
// 多帧 / 多页 / 矢量语义。谎报 CAP_SCALED 的代价不是崩，而是宿主把缩放请求
// 发过来、我们照样整解，白付一次全图解码——这种错测不出来也不报错。
#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

// ---------------------------------------------------------------------------
// 常量（全部对着 zune-psd 0.5.1 / Adobe PSD 规范核过，别凭记忆改）
// ---------------------------------------------------------------------------

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。与 qoi / 原 stb 插件同口径，
/// 防畸形头把内存榨干。**必须在任何分配之前**检查。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
/// zune 自己的尺寸闸（默认 16384）放到与总像素闸同宽：单维放行上限 =
/// 整张图的像素上限，于是"头撒谎"一律由 MAX_PIXELS 那条判据收口，
/// 不会出现 zune 先按 16384 拒一份本该由我们报"超过 6400 万像素"的文件。
const MAX_DIM: usize = MAX_PIXELS as usize;

/// 定头 26 字节布局：magic(4) version(2) reserved(6) channels(2)
/// height(4) width(4) depth(2) mode(2)。
const PSD_HEADER_LEN: usize = 26;
const PSD_VERSION: u16 = 1;
const PSD_DEPTH_8: u16 = 8;
const PSD_DEPTH_16: u16 = 16;
const PSD_MODE_GRAY: u16 = 1;
const PSD_MODE_RGB: u16 = 3;
const PSD_COMPRESSION_RAW: u16 = 0;
const PSD_COMPRESSION_RLE: u16 = 1;

#[inline]
fn be16(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

#[inline]
fn be32(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

// ---------------------------------------------------------------------------
// 头解析（probe 与 render 共用；返回 None = "这家不认领"）
// ---------------------------------------------------------------------------

/// 认领时一并带回的头字段。`pixel_off` 是压缩方式字段之后、平面像素数据的起点。
#[derive(Clone, Copy, Debug)]
struct PsdHeader {
    w: u32,
    h: u32,
    channels: u16,
    depth: u16,
    mode: u16,
    compression: u16,
    pixel_off: usize,
}

/// 判断字节流是否属于本插件**能解**的 PSD 形态。
///
/// 比 zune-psd 的 `decode_headers` 多做的事：把三个长度字段也走一遍
/// （不然够不着 compression），任何一段越界就不认领——不能猜。
/// 截在 26..30 之间的头连 compression 的位置都够不着，必须先设门：
/// 直接 `be32(data, 26)` 会越界 panic，被 gate 转成 ERR_PANIC 喂宿主三振
/// 黑名单，等于一个坏样本拉黑整家插件。
fn psd_header(data: &[u8]) -> Option<PsdHeader> {
    if data.len() < PSD_HEADER_LEN || &data[0..4] != b"8BPS" {
        return None;
    }
    if be16(data, 4)? != PSD_VERSION {
        return None; // PSB v2 等
    }
    let channels = be16(data, 12)?;
    if !(1..=4).contains(&channels) {
        return None; // zune-psd 上限 4；0 通道同样拒
    }
    let h = be32(data, 14)?;
    let w = be32(data, 18)?;
    if h == 0 || w == 0 {
        return None;
    }
    let depth = be16(data, 22)?;
    if depth != PSD_DEPTH_8 && depth != PSD_DEPTH_16 {
        return None;
    }
    let mode = be16(data, 24)?;
    if mode != PSD_MODE_GRAY && mode != PSD_MODE_RGB {
        return None; // CMYK / 索引 / 双色调 / Lab 一概不认
    }
    // color-mode / image-resources / layer-mask 三个长度字段。起点是定头末尾
    // （26），不是 26+4——多跳 4 字节会把第二段当成第一段、再越界读第三段，
    // 结果所有合法 PSD 都在 probe 阶段被拒（2026-10-25 实测：18/23 用例红）。
    let mut p = PSD_HEADER_LEN;
    let cmlen = be32(data, p)? as usize;
    p = p.checked_add(cmlen)?.checked_add(4)?;
    let irlen = be32(data, p)? as usize;
    p = p.checked_add(irlen)?.checked_add(4)?;
    let lmlen = be32(data, p)? as usize;
    p = p.checked_add(lmlen)?.checked_add(4)?;
    let compression = be16(data, p)?;
    if compression != PSD_COMPRESSION_RAW && compression != PSD_COMPRESSION_RLE {
        return None; // ZIP 之类 zune-psd 不解
    }
    Some(PsdHeader {
        w,
        h,
        channels,
        depth,
        mode,
        compression,
        pixel_off: p + 2,
    })
}

/// 有效通道数：zune-psd 对灰度模式一律按 1 通道读（多声明的通道直接丢）。
/// 截断推算必须按**有效**通道数算，否则合法的"灰度 + alpha"文件会被误判截断。
fn effective_channels(ph: &PsdHeader) -> u64 {
    if ph.mode == PSD_MODE_GRAY {
        1
    } else {
        ph.channels as u64
    }
}

/// 截断预检：只读头部，不解像素。
///
/// 为什么必须自己算：zune-psd 的 RLE 分支用 `read_u8()`（EOF 静默返回 0）
/// 喂数据，被砍尾的 8 位 RLE 流会"成功"解成一张局部全零的图——那正是
/// 「文件坏了」被显示成「正常图片」的事故形态。无压缩分支虽然会报错，
/// 但走的是库的通用 I/O 错，说不清"缺多少"。
fn check_truncation(data: &[u8], ph: &PsdHeader) -> Result<(), &'static str> {
    if ph.compression == PSD_COMPRESSION_RAW {
        // 饱和乘法：w、h 各是 u32，乘积再乘通道数会超过 u64 —— 调用方虽然已经
        // 用 MAX_PIXELS 封了顶，这里仍不许靠"上游一定挡了"来保证不溢出。
        let px = (ph.w as u64)
            .saturating_mul(ph.h as u64)
            .saturating_mul(effective_channels(ph))
            .saturating_mul(ph.depth as u64 / 8);
        let need = (ph.pixel_off as u64).saturating_add(px);
        if need > data.len() as u64 {
            return Err("PSD 被截断：平面像素数据的字节数不够");
        }
        return Ok(());
    }
    // RLE：行计数表（每行每通道一条 u16 BE）必须完整，且各行声明的压缩
    // 字节总数不能超过文件剩余字节。这条同时兜住 8 位（zune 静默补零）
    // 与 16 位（插件自己的解码器虽然带错读，但提前报更清楚）。
    let rows = ph.h as usize * ph.channels as usize;
    let table = rows.checked_mul(2).ok_or("PSD 行计数表长度溢出")?;
    let data_start = ph.pixel_off.checked_add(table).ok_or("PSD 长度字段溢出")?;
    let mut sum: usize = 0;
    for k in 0..rows {
        let n = be16(data, ph.pixel_off + k * 2).ok_or("PSD 行计数表被截断")? as usize;
        sum = sum.checked_add(n).ok_or("PSD 行计数溢出")?;
    }
    let rest = data
        .len()
        .checked_sub(data_start)
        .ok_or("PSD 行计数表被截断")?;
    if sum > rest {
        return Err("PSD RLE 数据被截断：行计数声明的字节数超过文件剩余长度");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render
// ---------------------------------------------------------------------------

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        // psd 在全链只有这一家认领 → 认了就是冠军置信度，没有 50 兜底分。
        if psd_header(slice).is_some() {
            100
        } else {
            0
        }
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 {
            set_last_error("内部错误（宿主传入空数据）");
            return ERR_UNRECOGNIZED;
        }
        let input = unsafe { std::slice::from_raw_parts(data, len) };

        let Some(ph) = psd_header(input) else {
            set_last_error("不是本插件能解的 PSD 字节特征");
            return ERR_UNRECOGNIZED;
        };
        // 尺寸闸排在截断预检**之前**：头撒谎的巨大尺寸要先按"太大"报（那条
        // 消息可操作），而不是先按"被截断"报；同时它给下面所有按 w*h 推算的
        // 算术封了顶（w*h ≤ 64 MP ⇒ 平面字节数 ≤ 256 M，不可能溢出）。
        // 库那边还有一道同宽的闸（psd_decoder 的 MAX_DIM），以及随后的
        // "两处解析必须一致"交叉核对——三道下来撒谎的头进不了解码。
        let npixels = ph.w as u64 * ph.h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码上限");
            return ERR_TOO_LARGE;
        }
        if let Err(msg) = check_truncation(input, &ph) {
            set_last_error(msg);
            return ERR_CORRUPT;
        }

        // ── 阶段 1：只读头拿尺寸（zune 的 decode_headers 不解像素数据）。──
        let mut dec = psd_decoder(input);
        if let Err(e) = dec.decode_headers() {
            let (code, msg) = classify_psd(&e);
            set_last_error(msg);
            return code;
        }
        let (Some((zw, zh)), Some(cs)) = (dec.dimensions(), dec.colorspace()) else {
            set_last_error("PSD 文件头不完整");
            return ERR_CORRUPT;
        };
        // 我方解析与库的解析必须一致；对不上就是有撒谎的头字段（或库行为变了）。
        if zw as u32 != ph.w || zh as u32 != ph.h {
            set_last_error("PSD 头字段前后不一致");
            return ERR_CORRUPT;
        }
        let (w, h) = (ph.w, ph.h);

        let bytes_needed = npixels * 4;
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };
        if budget != 0 && bytes_needed > budget as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }
        if info.is_null() {
            set_last_error("内部错误（宿主传入空输出指针）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // 查询阶段：out 为空指针，只回报尺寸/格式。
        if out.is_null() {
            fill_info(info, w, h, bytes_needed);
            return ERR_OK;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // ── 阶段 2：真解码。────────────────────────────────────────────
        let rgba = match decode_pixels(input, &ph, cs) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if rgba.len() as u64 != bytes_needed {
            // 头尺寸与像素数不符 = 畸形文件（或库行为变了）。宁可报错也不越界写。
            set_last_error("像素数与文件头不符");
            return ERR_CORRUPT;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
        }
        fill_info(info, w, h, bytes_needed);
        ERR_OK
    })
}

#[inline]
fn fill_info(info: *mut IvRenderOut, w: u32, h: u32, bytes_needed: u64) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = 1;
    oi.already_oriented = 1; // PSD 没有方向语义，宿主不必再转
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1;
    oi.icc_len = 0; // PSD 的色彩配置在 image-resources 段里，stb 时代也不透传
}

// ---------------------------------------------------------------------------
// zune-psd 薄封装
// ---------------------------------------------------------------------------

fn psd_decoder(input: &[u8]) -> PSDDecoder<ZCursor<&[u8]>> {
    // `ZCursor::new(input)` 借用语义，不复制输入字节。
    PSDDecoder::new_with_options(
        ZCursor::new(input),
        DecoderOptions::default()
            .set_max_width(MAX_DIM)
            .set_max_height(MAX_DIM),
    )
}

/// 把库的错误枚举映射到 ABI 错误码。
///
/// **逐一枚举、不按字符串前缀猜**：`PSDDecodeErrors` 是公开枚举，逐条对应
/// 比 jpeg 那边 `format!("{:?}", e)` 的前缀匹配稳（那边是因为 zune-jpeg 的
/// 错误类型不公开）。加一条 `Generic` 兜底：probe 已拦掉它覆盖的全部形态
/// （未知色彩模式、16 位 RLE 由插件自己解），真走到就是坏文件。
fn classify_psd(e: &PSDDecodeErrors) -> (i32, &'static str) {
    match e {
        PSDDecodeErrors::WrongMagicBytes(_) => (ERR_UNRECOGNIZED, "PSD 魔数不对"),
        PSDDecodeErrors::UnsupportedFileType(_) => {
            (ERR_UNSUPPORTED, "PSD 版本不支持（只认 version 1）")
        }
        PSDDecodeErrors::UnsupportedChannelCount(_) => {
            (ERR_UNSUPPORTED, "PSD 通道数超过 4，本插件不解")
        }
        PSDDecodeErrors::UnsupportedBitDepth(_) => {
            (ERR_UNSUPPORTED, "PSD 位深不支持（只认 8/16 位）")
        }
        PSDDecodeErrors::UnsupportedColorFormat(_) => {
            (ERR_UNSUPPORTED, "PSD 色彩模式不支持（只认灰度与 RGB）")
        }
        PSDDecodeErrors::LargeDimensions(_, _) => (ERR_TOO_LARGE, "文件头声明的尺寸超出范围"),
        PSDDecodeErrors::ZeroDimensions => (ERR_CORRUPT, "PSD 宽、高或通道数为 0"),
        PSDDecodeErrors::UnknownCompression => {
            (ERR_UNSUPPORTED, "PSD 压缩方式不支持（只认无压缩与 RLE）")
        }
        PSDDecodeErrors::BadRLE => (ERR_CORRUPT, "PSD RLE 数据损坏"),
        PSDDecodeErrors::Generic(reason) => (ERR_CORRUPT, reason),
        PSDDecodeErrors::IoErrors(_) => (ERR_CORRUPT, "PSD 数据不完整或已损坏"),
    }
}

/// 真解码：两种压缩 × 两种位深统一走插件自己的解码器（见下方
/// 「像素解码」一节的实测缺陷清单），zune 只用来解头。
fn decode_pixels(
    input: &[u8],
    ph: &PsdHeader,
    cs: ColorSpace,
) -> Result<Vec<u8>, (i32, &'static str)> {
    let ch = effective_channels(ph) as usize;
    let unit = ph.depth as usize / 8;
    let mut samples = if ph.compression == PSD_COMPRESSION_RLE {
        decode_rle(input, ph)
    } else {
        decode_planar(input, ph)
    }
    .map_err(|m| (ERR_CORRUPT, m))?;
    remove_white_matte(&mut samples, ch, unit);
    if ph.depth == PSD_DEPTH_8 {
        return rgba8_u8(&samples, ph, cs);
    }
    let px: Vec<u16> = samples
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    rgba8_u16(&px, ph, cs)
}

/// 色彩模式 → 通道数。`ColorSpace` 是 `#[non_exhaustive]`，多出来的分支
/// 一律按"不支持"报——猜通道数会把数据读窜。
fn channels_of(cs: ColorSpace) -> Result<usize, (i32, &'static str)> {
    match cs {
        ColorSpace::Luma => Ok(1),
        ColorSpace::RGB => Ok(3),
        ColorSpace::RGBA => Ok(4),
        _ => Err((ERR_UNSUPPORTED, "PSD 色彩模式本插件不解")),
    }
}

/// 8 位平面交错像素 → RGBA8。通道数由 colorspace 决定，与实际解出的字节数
/// 对不上 → 判坏文件（宁可报错，不按错误的通道数猜）。
fn rgba8_u8(px: &[u8], ph: &PsdHeader, cs: ColorSpace) -> Result<Vec<u8>, (i32, &'static str)> {
    let ch = channels_of(cs)?;
    let n = ph.w as usize * ph.h as usize;
    if px.len() != n * ch {
        return Err((ERR_CORRUPT, "PSD 像素数与文件头不符"));
    }
    let mut out = vec![0u8; n * 4];
    match ch {
        1 => {
            for (d, &s) in out.chunks_exact_mut(4).zip(px) {
                d[0] = s;
                d[1] = s;
                d[2] = s;
                d[3] = 255;
            }
        }
        3 => {
            for (d, s) in out.chunks_exact_mut(4).zip(px.chunks_exact(3)) {
                d[..3].copy_from_slice(s);
                d[3] = 255;
            }
        }
        _ => out.copy_from_slice(px),
    }
    Ok(out)
}

/// 16 位平面交错像素 → RGBA8。降位口径 = 高字节 `>> 8`，与 stb 一致
/// （`stbi__get16be >> 8`）；core 的 tiff 路线是 ÷257 四舍五入，差 ≤ 1 LSB。
fn rgba8_u16(px: &[u16], ph: &PsdHeader, cs: ColorSpace) -> Result<Vec<u8>, (i32, &'static str)> {
    let ch = channels_of(cs)?;
    let n = ph.w as usize * ph.h as usize;
    if px.len() != n * ch {
        return Err((ERR_CORRUPT, "PSD 像素数与文件头不符"));
    }
    let mut out = vec![0u8; n * 4];
    match ch {
        1 => {
            for (d, &s) in out.chunks_exact_mut(4).zip(px) {
                let v = (s >> 8) as u8;
                d[0] = v;
                d[1] = v;
                d[2] = v;
                d[3] = 255;
            }
        }
        3 => {
            for (d, s) in out.chunks_exact_mut(4).zip(px.chunks_exact(3)) {
                d[0] = (s[0] >> 8) as u8;
                d[1] = (s[1] >> 8) as u8;
                d[2] = (s[2] >> 8) as u8;
                d[3] = 255;
            }
        }
        _ => {
            for (d, s) in out.chunks_exact_mut(4).zip(px.chunks_exact(4)) {
                d[0] = (s[0] >> 8) as u8;
                d[1] = (s[1] >> 8) as u8;
                d[2] = (s[2] >> 8) as u8;
                d[3] = (s[3] >> 8) as u8;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 像素解码：插件自足（zune-psd 0.5.1 的像素路径有三处硬缺陷）
// ---------------------------------------------------------------------------
//
// 实测（2026-10-25，本机 x86_64 MSVC，逐字节对着 Adobe 规范核过）：
//   1. 8 位无压缩：`decode_raw` 的循环上界写成 `pixel_count`（不是
//      `pixel_count * channel_count`），每个通道只读到 `ceil(px/ch)` 个样本，
//      剩下的位置留 0，而且写入下标是交错的 —— 4 个红像素会解成
//      `白/红/黑/黑`；
//   2. 16 位无压缩：先 `to_ne_bytes()` 写进缓冲，`decode()` 又按 BE 解析，
//      小端机上一律字节反转（0x8040 → 0x4080）；
//   3. 16 位 RLE：`Generic("Not implemented or Unknown")`，而它是 Photoshop
//      存 16 位文档的默认压缩。
//
// 所以这里只借 zune 的头解析（`decode_headers` + dimensions/colorspace），
// 像素一律自己解：两种压缩 × 两种位深 = 四格，输出统一为**交错样本**
// （`out[(pixel*ch + channel)*unit]`），unit = 1（8 位）/ 2（16 位，BE 字节）。

/// 无压缩：按**平面序**读文件，写成交错样本。
fn decode_planar(data: &[u8], ph: &PsdHeader) -> Result<Vec<u8>, &'static str> {
    let ch = effective_channels(ph) as usize;
    let n = (ph.w as usize)
        .checked_mul(ph.h as usize)
        .ok_or("PSD 像素数溢出")?;
    let unit = ph.depth as usize / 8;
    let total = n
        .checked_mul(ch)
        .and_then(|v| v.checked_mul(unit))
        .ok_or("PSD 像素缓冲长度溢出")?;
    let mut out = vec![0u8; total];
    let mut pos = ph.pixel_off;
    for channel in 0..ch {
        for i in 0..n {
            let end = pos.checked_add(unit).ok_or("PSD 像素偏移溢出")?;
            let src = data.get(pos..end).ok_or("PSD 平面像素数据被截断")?;
            let at = (i * ch + channel) * unit;
            out[at..at + unit].copy_from_slice(src);
            pos = end;
        }
    }
    Ok(out)
}

/// RLE：行计数表（`h * declared_ch` 条 u16 BE）+ 按通道/行分段的行程流。
///
/// 与 zune 的两处差别都是硬要求：
/// - **按行解**，每条行程不许跨行（规范如此；zune 在整个通道上连续解，
///   撒谎的计数表会被它"解成功"）；
/// - **每字节带错读**，EOF 即 `CORRUPT`（zune 的 `read_u8()` EOF 返回 0，
///   截断文件会静默补零成一张"正常图片"）。
fn decode_rle(data: &[u8], ph: &PsdHeader) -> Result<Vec<u8>, &'static str> {
    let ch = effective_channels(ph) as usize;
    let declared = ph.channels as usize;
    let w = ph.w as usize;
    let h = ph.h as usize;
    let n = w.checked_mul(h).ok_or("PSD 像素数溢出")?;
    let unit = ph.depth as usize / 8;
    let total = n
        .checked_mul(ch)
        .and_then(|v| v.checked_mul(unit))
        .ok_or("PSD 像素缓冲长度溢出")?;
    let mut out = vec![0u8; total];
    // 计数表按**声明**通道数排（多余通道虽然不输出，但它们的表项占位置）。
    let table = declared
        .checked_mul(h)
        .and_then(|v| v.checked_mul(2))
        .ok_or("PSD 行计数表长度溢出")?;
    let mut pos = ph.pixel_off.checked_add(table).ok_or("PSD 长度字段溢出")?;
    for channel in 0..ch {
        for row in 0..h {
            let entry = ph.pixel_off + (channel * h + row) * 2;
            let row_len = be16(data, entry).ok_or("PSD 行计数表被截断")? as usize;
            let row_end = pos.checked_add(row_len).ok_or("PSD RLE 行长度溢出")?;
            if row_end > data.len() {
                return Err("PSD RLE 数据被截断");
            }
            let mut filled = 0usize;
            while filled < w {
                if pos >= row_end {
                    return Err("PSD RLE 行程未填满这一行");
                }
                let cnt = data[pos];
                pos += 1;
                let (len, literal) = match cnt.cmp(&128) {
                    Ordering::Less => (cnt as usize + 1, true),
                    Ordering::Equal => continue,
                    Ordering::Greater => (257 - cnt as usize, false),
                };
                if filled + len > w {
                    return Err("PSD RLE 行程越过行宽");
                }
                let at = |i: usize| ((row * w + i) * ch + channel) * unit;
                if literal {
                    for _ in 0..len {
                        let end = pos.checked_add(unit).ok_or("PSD RLE 偏移溢出")?;
                        if end > row_end {
                            return Err("PSD RLE 字面量越过行字节");
                        }
                        let d = at(filled);
                        out[d..d + unit].copy_from_slice(&data[pos..end]);
                        pos = end;
                        filled += 1;
                    }
                } else {
                    let end = pos.checked_add(unit).ok_or("PSD RLE 偏移溢出")?;
                    if end > row_end {
                        return Err("PSD RLE 重复行程缺少样本");
                    }
                    let sample = &data[pos..end];
                    pos = end;
                    for _ in 0..len {
                        let d = at(filled);
                        out[d..d + unit].copy_from_slice(sample);
                        filled += 1;
                    }
                }
            }
            if pos != row_end {
                return Err("PSD RLE 行字节数与计数表不符");
            }
        }
    }
    Ok(out)
}

/// 去白杂边（un-premultiply）：PSD 把 alpha 预乘后的值存进文件，4 通道时
/// 必须反推，否则半透明像素会发白。算法与 stb / zune 同源，逐行照抄语义：
/// `a ∈ (0, max)` 才处理，`c' = c/a + max*(1 - 1/a)`，负值靠 Rust 的饱和
/// 转换钳到 0（与 stb 的 `(int)` 截断再钳位同结果）。
fn remove_white_matte(buf: &mut [u8], ch: usize, unit: usize) {
    if ch < 4 {
        return;
    }
    if unit == 1 {
        for p in buf.chunks_exact_mut(4) {
            let a = p[3];
            if a != 0 && a != 255 {
                let af = f32::from(a) / 255.0;
                let ra = 1.0 / af;
                let inv = 255.0 * (1.0 - ra);
                for c in &mut p[..3] {
                    *c = (f32::from(*c) * ra + inv) as u8;
                }
            }
        }
    } else {
        for p in buf.chunks_exact_mut(8) {
            let a = u16::from_be_bytes([p[6], p[7]]);
            if a != 0 && a != 65535 {
                let af = f32::from(a) / 65535.0;
                let ra = 1.0 / af;
                let inv = 65535.0 * (1.0 - ra);
                for k in 0..3 {
                    let v = u16::from_be_bytes([p[k * 2], p[k * 2 + 1]]);
                    let v = (f32::from(v) * ra + inv) as u16;
                    p[k * 2..k * 2 + 2].copy_from_slice(&v.to_be_bytes());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    use rupix_abi as kit;

    // ---- 夹具构造（手工搭最小合法 PSD，字节全部对着规范写） ----------------

    /// 写 26 字节定头 + 三个长度字段（全 0）+ compression。
    fn psd_head(w: u32, h: u32, depth: u16, mode: u16, channels: u16, compression: u16) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"8BPS");
        v.extend_from_slice(&PSD_VERSION.to_be_bytes());
        v.extend_from_slice(&[0u8; 6]); // reserved
        v.extend_from_slice(&channels.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&depth.to_be_bytes());
        v.extend_from_slice(&mode.to_be_bytes());
        v.extend_from_slice(&0u32.to_be_bytes()); // color mode data len
        v.extend_from_slice(&0u32.to_be_bytes()); // image resources len
        v.extend_from_slice(&0u32.to_be_bytes()); // layer/mask len
        v.extend_from_slice(&compression.to_be_bytes());
        v
    }

    /// 无压缩 PSD：`planes` 是每通道的平面数据（长度 = w*h*unit）。
    fn psd_raw(w: u32, h: u32, depth: u16, mode: u16, channels: u16, planes: &[&[u8]]) -> Vec<u8> {
        assert_eq!(planes.len(), channels as usize);
        let mut v = psd_head(w, h, depth, mode, channels, PSD_COMPRESSION_RAW);
        for p in planes {
            assert_eq!(p.len(), (w * h) as usize * (depth as usize / 8));
            v.extend_from_slice(p);
        }
        v
    }

    /// RLE PSD：每行单独压成"全字面量"（每个样本前一条计数字节 0，
    /// 0 → 照抄后面 1 个单元），行前是 h*ch 条 u16 BE 计数。
    /// 这是最朴素的合法编码，专门给测试用——真实文件不会这么浪费。
    fn psd_rle(w: u32, h: u32, depth: u16, mode: u16, channels: u16, planes: &[&[u8]]) -> Vec<u8> {
        assert_eq!(planes.len(), channels as usize);
        let unit = depth as usize / 8;
        let per_row = w as usize * unit;
        // 每行字节数 = 每个样本 1 个计数字节 + unit 个样本字节。
        // 8 位恰好等于 per_row*2；16 位是 3w（不是 4w）——旧夹具按 4w 写，
        // 以前不按行解所以看不出来，改成按行校验后必须修准。
        let row_bytes = (w as usize * (unit + 1)) as u16;
        let mut v = psd_head(w, h, depth, mode, channels, PSD_COMPRESSION_RLE);
        // 行计数表：通道平面序，每行一条。
        for _ in 0..channels {
            for _ in 0..h {
                v.extend_from_slice(&row_bytes.to_be_bytes());
            }
        }
        for p in planes {
            for row in p.chunks_exact(per_row) {
                for s in row.chunks_exact(unit) {
                    v.push(0); // 计数字节 0 → 照抄 1 个单元
                    v.extend_from_slice(s);
                }
            }
        }
        v
    }

    /// 2x2 纯红 RGB8（平面序：R 面全 255，G/B 面全 0）。
    fn psd_red_2x2() -> Vec<u8> {
        psd_raw(
            2,
            2,
            PSD_DEPTH_8,
            PSD_MODE_RGB,
            3,
            &[&[255, 255, 255, 255], &[0, 0, 0, 0], &[0, 0, 0, 0]],
        )
    }

    fn req() -> IvRenderReq {
        IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        }
    }

    fn render_to_buf(data: &[u8]) -> (Vec<u8>, u32, u32) {
        let mut info = IvRenderOut::default();
        let c = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK, "查询阶段失败");
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK, "渲染阶段失败");
        (buf, info.actual_w, info.actual_h)
    }

    fn render_code(data: &[u8]) -> i32 {
        let mut info = IvRenderOut::default();
        // 必须给真缓冲：out 为空指针时 render 在查询阶段就返回，
        // "probe 认领但数据坏"的用例会永远拿不到 CORRUPT。
        let mut buf = vec![0u8; 1 << 20];
        iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        )
    }

    fn last_err() -> String {
        let mut e = [0i8; 256];
        let need = unsafe { kit::write_last_error(e.as_mut_ptr(), e.len()) } as usize;
        let n = need.saturating_sub(1).min(e.len());
        String::from_utf8_lossy(&e[..n].iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }

    // ---- 布局与认领 -------------------------------------------------------

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 4);
    }

    #[test]
    fn probe_claims_psd_and_rejects_lookalikes() {
        let psd = psd_red_2x2();
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 100);
        // 非本家字节 / 空 / 太短
        assert_eq!(iv_probe(b"nope!!!".as_ptr(), 8, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(iv_probe(b"8BP".as_ptr(), 3, std::ptr::null()), 0);
        // PNG / HDR 的字节
        assert_eq!(
            iv_probe(b"\x89PNG\r\n\x1a\n".as_ptr(), 8, std::ptr::null()),
            0
        );
        assert_eq!(iv_probe(b"#?RADIANCE\n".as_ptr(), 11, std::ptr::null()), 0);
    }

    #[test]
    fn probe_rejects_forms_zune_cannot_decode() {
        let mut psd = psd_red_2x2();
        psd[5] = 2; // PSB v2
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[25] = 4; // CMYK mode
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[22] = 0;
        psd[23] = 32; // depth = 32
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[13] = 5; // channels = 5（zune 上限 4）
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[12] = 0;
        psd[13] = 0; // channels = 0
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[39] = 2; // compression = 2（ZIP 之类）
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);

        let mut psd = psd_red_2x2();
        psd[29] = 0xff; // color-mode 长度字段撒谎
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 0);
    }

    /// 截在第一个长度字段中间的头：定头 26 字节看着齐全，但 compression 要到
    /// 偏移 30 之后才够得着。这类头必须**不认领**，而不是读越界——越界 panic
    /// 会被 gate 转成 ERR_PANIC，宿主据此把整家插件拉黑（一个坏样本的代价）。
    #[test]
    fn probe_survives_header_cut_inside_first_length_field() {
        let psd = psd_red_2x2();
        for cut in PSD_HEADER_LEN..PSD_HEADER_LEN + 4 {
            let head = &psd[..cut];
            assert_eq!(
                iv_probe(head.as_ptr(), head.len(), std::ptr::null()),
                0,
                "截到 {cut} 字节时 compression 还读不到，应不认领而不是 panic"
            );
        }
    }

    /// 灰度模式是本次移植**新增**的认领面（stb 只认 mode=3）。
    #[test]
    fn probe_claims_grayscale_and_16bit() {
        let gray = psd_raw(2, 2, PSD_DEPTH_8, PSD_MODE_GRAY, 1, &[&[10, 20, 30, 40]]);
        assert_eq!(iv_probe(gray.as_ptr(), gray.len(), std::ptr::null()), 100);

        let mut psd = psd_red_2x2();
        psd[22] = 0;
        psd[23] = 16; // depth = 16
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 100);

        let mut psd = psd_red_2x2();
        psd[39] = 1; // compression = RLE
        assert_eq!(iv_probe(psd.as_ptr(), psd.len(), std::ptr::null()), 100);
    }

    // ---- 解码 -------------------------------------------------------------

    #[test]
    fn decodes_8bit_rgb_psd() {
        let (buf, w, h) = render_to_buf(&psd_red_2x2());
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            &buf[..],
            &[
                255, 0, 0, 255, //
                255, 0, 0, 255, //
                255, 0, 0, 255, //
                255, 0, 0, 255,
            ]
        );
    }

    #[test]
    fn decodes_8bit_rle_psd() {
        let rle = psd_rle(
            2,
            2,
            PSD_DEPTH_8,
            PSD_MODE_RGB,
            3,
            &[&[255, 255, 255, 255], &[0, 0, 0, 0], &[0, 0, 0, 0]],
        );
        let (buf, w, h) = render_to_buf(&rle);
        assert_eq!((w, h), (2, 2));
        assert!(buf.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    /// 灰度：zune-psd 对灰度一律按 1 通道读，输出三通道同值。
    #[test]
    fn decodes_grayscale_psd() {
        let gray = psd_raw(2, 2, PSD_DEPTH_8, PSD_MODE_GRAY, 1, &[&[10, 20, 30, 40]]);
        let (buf, _, _) = render_to_buf(&gray);
        assert_eq!(
            &buf[..],
            &[
                10, 10, 10, 255, //
                20, 20, 20, 255, //
                30, 30, 30, 255, //
                40, 40, 40, 255,
            ]
        );
    }

    /// 灰度 + alpha：多声明的通道被丢掉（zune 的定义行为），alpha 恒 255。
    /// 这条同时钉住"截断推算按有效通道数算"——文件里只有 1 个通道的数据。
    #[test]
    fn grayscale_with_extra_channels_keeps_only_luma() {
        let gray = psd_raw(
            2,
            2,
            PSD_DEPTH_8,
            PSD_MODE_GRAY,
            2,
            &[&[10, 20, 30, 40], &[200, 200, 200, 200]],
        );
        let (buf, _, _) = render_to_buf(&gray);
        assert_eq!(
            &buf[..],
            &[
                10, 10, 10, 255, //
                20, 20, 20, 255, //
                30, 30, 30, 255, //
                40, 40, 40, 255,
            ]
        );
    }

    /// RGBA：alpha 照实透出；半透明像素会被 zune 做"去白杂边"
    /// （PSD 把预乘后的值存进文件）。这里把 zune 的那步数学钉住：
    /// (255,0,0,128) 存的是 255*0.502+255*0.498 ≈ 255 → 反推回 255/0/0。
    #[test]
    fn decodes_rgba_psd_with_white_matte_removed() {
        let rgba = psd_raw(
            2,
            1,
            PSD_DEPTH_8,
            PSD_MODE_RGB,
            4,
            &[
                &[255, 255],
                &[0, 0],
                &[0, 0],
                &[128, 255], // 左半透明、右不透明
            ],
        );
        let (buf, _, _) = render_to_buf(&rgba);
        assert_eq!(&buf[..4], &[255, 0, 0, 128], "去白杂边后应回到原色");
        assert_eq!(&buf[4..8], &[255, 0, 0, 255], "alpha=255 时不动像素");
    }

    /// 16 位无压缩：按高字节降位（stb 同口径）。
    #[test]
    fn decodes_16bit_uncompressed_psd() {
        let planes: [&[u8]; 3] = [
            &[0x80, 0x40, 0x00, 0x00, 0xFF, 0xFF, 0x12, 0x34],
            &[0x40, 0x00, 0x80, 0x00, 0x01, 0x02, 0xF0, 0xF0],
            &[0x20, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00, 0xFF],
        ];
        let raw = psd_raw(2, 2, PSD_DEPTH_16, PSD_MODE_RGB, 3, &planes);
        let (buf, _, _) = render_to_buf(&raw);
        assert_eq!(&buf[..4], &[0x80, 0x40, 0x20, 255]);
        assert_eq!(&buf[4..8], &[0x00, 0x80, 0x10, 255]);
        assert_eq!(&buf[8..12], &[0xFF, 0x01, 0x00, 255]);
        assert_eq!(&buf[12..16], &[0x12, 0xF0, 0x00, 255]);
    }

    /// **16 位 RLE 缺口自检**：同一张图一条走无压缩（zune 的路）、一条走
    /// RLE（插件自己的解码器），两边必须逐字节相同。这一条同时钉住
    /// "缺口补上了"与"补得对"——换成另一组样本重跑也成立。
    #[test]
    fn sixteen_bit_rle_matches_the_uncompressed_path() {
        let planes: [&[u8]; 3] = [
            &[0x80, 0x40, 0x00, 0x00, 0xFF, 0xFF, 0x12, 0x34],
            &[0x40, 0x00, 0x80, 0x00, 0x01, 0x02, 0xF0, 0xF0],
            &[0x20, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00, 0xFF],
        ];
        let raw = psd_raw(2, 2, PSD_DEPTH_16, PSD_MODE_RGB, 3, &planes);
        let rle = psd_rle(2, 2, PSD_DEPTH_16, PSD_MODE_RGB, 3, &planes);
        let (a, _, _) = render_to_buf(&raw);
        let (b, _, _) = render_to_buf(&rle);
        assert_eq!(a, b, "16 位 RLE 必须与无压缩路逐字节一致");
        assert_eq!(&a[..4], &[0x80, 0x40, 0x20, 255]);
        assert_eq!(&a[12..16], &[0x12, 0xF0, 0x00, 255]);
    }

    /// 重复行程（`n > 128`）：同一份 16 位数据换成"整行重复"的编码，
    /// 结果必须与字面量编码相同——行程分支不是装饰。
    #[test]
    fn sixteen_bit_rle_repeat_runs_decode_the_same() {
        let planes: [&[u8]; 1] = [&[0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34]];
        // 灰度 1 通道 2x2 16 位：四个样本同值。每行用 255（→ 重复下一样本
        // 2 次）+ 2 字节样本 = 3 字节，两行共 6 字节。
        let mut rle = psd_head(2, 2, PSD_DEPTH_16, PSD_MODE_GRAY, 1, PSD_COMPRESSION_RLE);
        rle.extend_from_slice(&(3u16).to_be_bytes()); // 行 0 计数：3 字节
        rle.extend_from_slice(&(3u16).to_be_bytes()); // 行 1 计数：3 字节
        for _ in 0..2 {
            rle.push(255); // 255 → 重复下一个样本 2 次
            rle.extend_from_slice(&[0x12, 0x34]);
        }
        let raw = psd_raw(2, 2, PSD_DEPTH_16, PSD_MODE_GRAY, 1, &planes);
        let (a, _, _) = render_to_buf(&raw);
        let (b, _, _) = render_to_buf(&rle);
        assert_eq!(a, b);
        assert!(a.chunks_exact(4).all(|p| p == [0x12, 0x12, 0x12, 255]));
    }

    /// 行程越过通道像素数 → CORRUPT，不 panic、不越界写。
    ///
    /// 夹具要点：计数表必须是**完整且自洽**的（声明的总量正好等于剩余字节），
    /// 这样截断预检放行，撒谎的行程才真的走到 `decode_rle_16` 里被抓住——
    /// 否则这条用例会变成"预检兜住了"，测不到想测的那一行。
    #[test]
    fn sixteen_bit_rle_run_past_channel_is_corrupt() {
        let mut rle = psd_head(2, 2, PSD_DEPTH_16, PSD_MODE_RGB, 3, PSD_COMPRESSION_RLE);
        // 行计数表：6 条。通道 0 的两行各声明 3 字节（合计 6），其余 0。
        rle.extend_from_slice(&(3u16).to_be_bytes());
        rle.extend_from_slice(&(3u16).to_be_bytes());
        for _ in 0..4 {
            rle.extend_from_slice(&(0u16).to_be_bytes());
        }
        // 通道 0 的数据：两条 0xFE（→ 重复 3 个样本）= 3+3 个样本，
        // 而通道 0 只有 2x2 = 4 个样本 —— 第二条就越界。
        for _ in 0..2 {
            rle.push(0xFE);
            rle.extend_from_slice(&[0x12, 0x34]);
        }
        assert_eq!(iv_probe(rle.as_ptr(), rle.len(), std::ptr::null()), 100);
        assert_eq!(render_code(&rle), ERR_CORRUPT);
    }

    // ---- 截断与坏文件 -----------------------------------------------------

    #[test]
    fn truncated_uncompressed_psd_is_corrupt() {
        let psd = psd_red_2x2();
        let cut = &psd[..psd.len() - 6];
        assert_eq!(
            iv_probe(cut.as_ptr(), cut.len(), std::ptr::null()),
            100,
            "头完好的截断文件照样认领"
        );
        assert_eq!(
            render_code(cut),
            ERR_CORRUPT,
            "但必须报损坏而不是解出半张图"
        );
    }

    #[test]
    fn truncated_rle_count_table_is_corrupt() {
        let rle = psd_rle(
            2,
            2,
            PSD_DEPTH_8,
            PSD_MODE_RGB,
            3,
            &[&[255, 255, 255, 255], &[0, 0, 0, 0], &[0, 0, 0, 0]],
        );
        // 砍掉最后 4 字节：计数表还在，但通道数据不够。
        let cut = &rle[..rle.len() - 4];
        assert_eq!(iv_probe(cut.as_ptr(), cut.len(), std::ptr::null()), 100);
        assert_eq!(render_code(cut), ERR_CORRUPT);
    }

    #[test]
    fn rle_row_counts_larger_than_file_is_corrupt() {
        let mut rle = psd_rle(
            2,
            2,
            PSD_DEPTH_8,
            PSD_MODE_RGB,
            3,
            &[&[255, 255, 255, 255], &[0, 0, 0, 0], &[0, 0, 0, 0]],
        );
        // 把第一条行计数改成 0xFFFF：表还在，但声明的总量远超文件剩余。
        rle[26 + 4 + 4 + 4 + 2] = 0xFF;
        rle[26 + 4 + 4 + 4 + 2 + 1] = 0xFF;
        assert_eq!(iv_probe(rle.as_ptr(), rle.len(), std::ptr::null()), 100);
        assert_eq!(render_code(&rle), ERR_CORRUPT);
    }

    /// 头撒谎的巨大尺寸必须在**分配之前**被 MAX_PIXELS 挡住。
    #[test]
    fn huge_dimensions_rejected_before_alloc() {
        let mut psd = psd_red_2x2();
        // height/width 都是 u32 BE：把 2x2 改成 40000x40000（1.6 GPixel）。
        psd[14..18].copy_from_slice(&40000u32.to_be_bytes());
        psd[18..22].copy_from_slice(&40000u32.to_be_bytes());
        let mut info = IvRenderOut::default();
        let r = iv_render(
            psd.as_ptr(),
            psd.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_TOO_LARGE, "畸形尺寸必须被上限挡住，实际 {}", r);
    }

    #[test]
    fn unrecognized_bytes_never_reach_decode() {
        let bad = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b";
        assert_eq!(iv_probe(bad.as_ptr(), bad.len(), std::ptr::null()), 0);
        assert_eq!(render_code(bad), ERR_UNRECOGNIZED);
    }

    // ---- 两阶段协议 -------------------------------------------------------

    #[test]
    fn two_phase_is_consistent() {
        let psd = psd_red_2x2();
        let mut info = IvRenderOut::default();
        let c = iv_render(
            psd.as_ptr(),
            psd.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (2, 2));
        assert_eq!(info.bytes_needed, 2 * 2 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        assert_eq!((info.orientation, info.already_oriented), (1, 1));
        assert_eq!(info.frame_count, 1);
        assert_eq!(info.page_count, 1);
        assert_eq!(info.icc_len, 0);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            psd.as_ptr(),
            psd.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        // 渲染成功后 last-error 必须为空（首次为空串）。
        let mut ebuf = [0i8; 16];
        let need = unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        assert_eq!(need, 1, "last error 应为空串");
    }

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let psd = psd_red_2x2();
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 8];
        let r = iv_render(
            psd.as_ptr(),
            psd.len(),
            &req(),
            tiny.as_mut_ptr(),
            tiny.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_BUFFER_TOO_SMALL);
    }

    #[test]
    fn budget_exceeded_is_too_large() {
        let psd = psd_red_2x2();
        let r = IvRenderReq {
            budget_bytes: 8, // 2x2x4 = 16 > 8
            ..req()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                psd.as_ptr(),
                psd.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_TOO_LARGE
        );
    }

    #[test]
    fn last_error_is_set_on_failure() {
        let code = render_code(b"\x00\x01\x02\x03");
        assert_eq!(code, ERR_UNRECOGNIZED);
        let msg = last_err();
        assert!(!msg.is_empty(), "失败路径应写入非空 last error");
        assert!(
            !msg.starts_with("psd: "),
            "规范 §六 第 11 条：不得自带插件名前缀，实际「{msg}」"
        );
    }
}
