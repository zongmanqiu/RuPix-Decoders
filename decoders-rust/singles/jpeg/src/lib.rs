//! RuPix 外挂解码器：JPEG → RGBA8（库：zune-jpeg）。
//!
//! # 为什么是这个库（G1 实测，不是"全网都说"）
//!
//! 2026-10-15 本机实测（`.plan/bench/`，run r1792004407 合成内容 / r1792004645 照片内容，
//! n=200 取 median，同一 run 内配对）：
//!
//! | 配置 | 合计 median ms | 相对第一名 |
//! |---|---|---|
//! | **zune-jpeg + 原生 RGBA 输出** | **56.50** | **1.00×** |
//! | libjpeg-turbo（fast DCT，最优配置） | 65.80 | 1.16× |
//! | `image`（core 旧路线） | 101.51 | 1.80× |
//!
//! libjpeg-turbo 落选**不是**桥接层造成的假慢：SIMD 已确认编进去
//! （`jconfig.h` 里 `WITH_SIMD 1`），fast DCT / fast upsampling 全开，直接吐 RGBA 免二次转换，
//! 仍然更慢。故本解码器用 zune-jpeg，并按它的冠军配置走：`ColorSpace::RGBA` 让库自己出 RGBA8，
//! 解码器侧不做任何逐像素转换（那是 §1「死法 3」）。
//!
//! # 两条腿：整解走 zune，缩放解走 turbo（同一枚 DLL 内分派）
//!
//! 2026-10-18 缩略图档实测（`.plan/bench/FINDINGS.md` §缩略图档，正序 `r1792312340` +
//! 倒序 `r1792312454`，n=200，同 run 内配对）：只要**小** target（240×180），turbo 的
//! DCT 1/8 缩放解码端到端快 **1.50–1.70×**（两序方向一致）；满尺寸整解它反而**慢 1.22×**。
//! 于是分派按**解码类型**走而不是按格式走——同后缀不许两枚 DLL 认领（§二），一库一 DLL
//! 在这条路上只能变成一 DLL 两腿。触发条件是宿主的 [`REQ_ALLOW_SCALED`] 位：**只看
//! `target_w/h` 不够**，主画面也传非零 target（视口尺寸），那儿要的是清晰不是快。
//!
//! ABI 契约来自 [`rupix_abi`]，panic gate / last-error / 固定导出由
//! [`rupix_abi::export_decoder!`] 生成。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "jpeg",
    name: "JPEG (zune-jpeg)",
    extensions: "jpg;jpeg;jpe;jfif;jif;jfi;exif",
    description: "解码 JPEG（baseline / progressive），zune-jpeg 直出 RGBA8",
    is_vector: 0,
    has_alpha: 0,
}

// 整解人人会（`iv_render` 的固有语义）；缩放解是这枚 DLL 里 turbo 那条腿**真实现**的
// 能力——位图只写真做到的，谎报的代价不是崩而是宿主把小图请求发过来、我们把整图解完。
// 纯 Rust 构建（`--no-default-features --features standalone`）没有那条腿，就不许声明。
#[cfg(all(feature = "standalone", feature = "turbo"))]
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_SCALED);
#[cfg(all(feature = "standalone", not(feature = "turbo")))]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。与 core 同口径，防畸形头把内存榨干。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// iv_probe / iv_render
// ---------------------------------------------------------------------------

/// JPEG 魔数：`FF D8`。SOI 之后必须是另一个 marker（`FF xx`，xx≠0），
/// 这条附加校验专门用来把"以 FF D8 开头的垃圾"挡在认领之外。
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
        let s = unsafe { std::slice::from_raw_parts(data, len) };
        if s[0] == 0xFF && s[1] == 0xD8 && s[2] == 0xFF && s[3] != 0x00 {
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
        if info.is_null() {
            set_last_error("内部错误（宿主传入空输出指针）");
            return ERR_BUFFER_TOO_SMALL;
        }
        let input = unsafe { std::slice::from_raw_parts(data, len) };

        // ── 阶段 1：只读头拿尺寸（zune 的 decode_headers 不做 IDCT）。──────
        let (w, h) = match header_dimensions(input) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };

        let npixels = w as u64 * h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码器上限");
            return ERR_TOO_LARGE;
        }
        // 预算与尺寸两道闸都按**整图**算：缩放腿省的是 IDCT/上采样和输出缓冲，熵解码照样
        // 跑完整份文件，"这张我解得起吗"的答案必须与今天一致（否则畸形头又绕过闸了）。
        let full_bytes = npixels * 4;
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };
        if budget != 0 && full_bytes > budget as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }

        // ── 分派：这一次请求走哪条腿（模块头「两条腿」）。None = 整解。──────
        let scaled = scaled_plan(req, input, w, h);
        let (dw, dh) = match &scaled {
            Some(p) => (p.w, p.h),
            None => (w, h),
        };
        let bytes_needed = dw as u64 * dh as u64 * 4;
        // ★ 方向：EXIF 标签在 APP1 里，**只扫段表不碰熵数据**（`rupix_abi` 的 kit 函数，
        //   微秒级）。读到就照实报 + `already_oriented = 0`，由宿主补转（m48）；
        //   读不到报 1，与历史行为逐字节相同。本解码器从不自己转像素。
        let orientation = rupix_abi::jpeg_exif_orientation(input).unwrap_or(1);
        // ICC：APP2 多分片按 seq/count 拼齐才透传（kit 函数只扫段表；缺片/超限 ⇒ 空）。
        let icc = rupix_abi::jpeg_icc_profile(input).unwrap_or_default();
        let icc_len = icc.len() as u64;

        if out.is_null() {
            fill_info(info, dw, dh, bytes_needed, orientation, icc_len as u32);
            return ERR_OK;
        }

        if (out_cap as u64) < bytes_needed + icc_len {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // ── 阶段 2：真解码，库直接输出 RGBA8（解码器侧零逐像素转换）。────────
        if let Some(p) = scaled {
            // 缩放腿直写宿主缓冲，连整图那次拷贝也一起省了。
            if let Err((code, msg)) = unsafe { decode_scaled_into(input, &p, out) } {
                set_last_error(msg);
                return code;
            }
            copy_icc(out, bytes_needed, &icc);
            fill_info(info, dw, dh, bytes_needed, orientation, icc_len as u32);
            return ERR_OK;
        }

        let rgba = match decode_rgba(input) {
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
        copy_icc(out, bytes_needed, &icc);
        fill_info(info, dw, dh, bytes_needed, orientation, icc_len as u32);
        ERR_OK
    })
}
/// 把 ICC 段写到像素段之后（宿主按 `bytes_needed + icc_len` 分配）。
fn copy_icc(out: *mut u8, bytes_needed: u64, icc: &[u8]) {
    if !icc.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(icc.as_ptr(), out.add(bytes_needed as usize), icc.len());
        }
    }
}

#[inline]
fn fill_info(
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    bytes_needed: u64,
    orientation: i32,
    icc_len: u32,
) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = orientation;
    // 本解码器**从不**自己转像素（zune 整解与 turbo 缩放腿出来的都是未摆正的原始像素），
    // 所以恒报 `already_oriented = 0`，把转正交给宿主那一条已有 4 份矩阵用例的路（m48）。
    // 早年这里写死 `1/1` 并注释"避免双重旋转"——双重旋转只在"解码器转了却不声明"时发生，
    // 如实回报才是契约给的路（`av` / `raw` 一直是这么写的）。
    oi.already_oriented = 0;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1;
    oi.icc_len = icc_len;
}

// ---------------------------------------------------------------------------
// zune-jpeg 薄封装
// ---------------------------------------------------------------------------

/// 只解头拿尺寸。zune 的 `decode_headers()` 不碰熵解码，微秒级。
///
/// ★ `ZCursor::new(input)` 借用语义，不复制输入字节——早先 bench 里写成
/// `data.to_vec()` 是多余的拷贝，解码器侧必须避免（否则测到的冠军还带着桥接税）。
fn header_dimensions(data: &[u8]) -> Result<(u32, u32), (i32, &'static str)> {
    use zune_core::bytestream::ZCursor;
    use zune_jpeg::JpegDecoder;
    let mut d = JpegDecoder::new(ZCursor::new(data));
    if d.decode_headers().is_err() {
        return Err((ERR_CORRUPT, "JPEG 文件头解析失败"));
    }
    let i = d.info().ok_or((ERR_CORRUPT, "缺少 SOF（帧开始）标记"))?;
    if i.width == 0 || i.height == 0 {
        return Err((ERR_CORRUPT, "文件头声明的宽或高为 0"));
    }
    Ok((i.width as u32, i.height as u32))
}

/// 解码为 RGBA8：把输出色彩空间设成 RGBA，让库自己填 alpha=255。
fn decode_rgba(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    use zune_jpeg::JpegDecoder;
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut d = JpegDecoder::new_with_options(ZCursor::new(data), opts);
    match d.decode() {
        Ok(px) => Ok(px),
        Err(e) => {
            // 分层归类：MCU 越界 → 太大；其余归 corrupt。
            let es = format!("{:?}", e);
            Err(classify(&es))
        }
    }
}

/// 把库的错误文本映射到 ABI 错误码。只依赖前缀，避免匹配未公开的枚举细节。
fn classify(err: &str) -> (i32, &'static str) {
    if err.starts_with("LargeDimensions") {
        (ERR_TOO_LARGE, "文件头声明的尺寸超出范围")
    } else if err.starts_with("Unsupported") {
        (ERR_UNSUPPORTED, "该 JPEG 子格式不支持")
    } else if err.starts_with("IllegalMagicBytes") {
        (ERR_UNRECOGNIZED, "JPEG 魔数不对")
    } else {
        (ERR_CORRUPT, "JPEG 解码失败")
    }
}

// ---------------------------------------------------------------------------
// 缩放腿：libjpeg-turbo 的 DCT 1/8、1/4、1/2 降采样解码
//
// 只在 `turbo` feature 打开时存在。关掉后下面的占位版本让**上层代码一字不改**：
// `scaled_plan` 恒返 None、`decode_scaled_into` 恒报 UNSUPPORTED（永不触达）。
// ---------------------------------------------------------------------------

#[cfg(feature = "turbo")]
/// 已选定的缩放方案：档位 + 该档下库真正会吐出的尺寸（宿主按它分配缓冲）。
struct ScaledPlan {
    factor: turbojpeg::ScalingFactor,
    w: u32,
    h: u32,
}

#[cfg(not(feature = "turbo"))]
/// 纯 Rust 构建的占位：永不构造，只为让共用代码的字段访问照样编得过。
struct ScaledPlan {
    w: u32,
    h: u32,
}

#[cfg(feature = "turbo")]
/// 档位阶梯，**按输出面积从小到大**排：第一个装得下 target 的档就是最省的档。
/// turbo 只有这三档能在 IDCT 里真省算力（没有 1/16；2/1 是放大，不在本用途）。
const SCALE_LADDER: [turbojpeg::ScalingFactor; 3] = [
    turbojpeg::ScalingFactor::ONE_EIGHTH,
    turbojpeg::ScalingFactor::ONE_QUARTER,
    turbojpeg::ScalingFactor::ONE_HALF,
];

/// 这次请求要不要走缩放腿、走哪档。任一条不成立都返回 `None`，那条路就是今天的路：
///
/// - 宿主没置 [`REQ_ALLOW_SCALED`]。**只看 `target_w/h` 不够**——主画面也传非零 target
///   （视口尺寸），那儿要的是清晰，把它解成 1/8 再放大是功能缺陷不是优化。
/// - target 为 0（要原图）或已不小于原生。
/// - 头读不出 / lossless / 算术熵编码：turbo 这条腿解不了（或解了也不省），
///   交回 zune 按它自己的口径归类。
/// - 三档都装不下 target（target 大于半尺寸）：缩放会让宿主**放大**补齐，不做。
#[cfg(feature = "turbo")]
fn scaled_plan(req: *const IvRenderReq, input: &[u8], nw: u32, nh: u32) -> Option<ScaledPlan> {
    if req.is_null() {
        return None;
    }
    let r = unsafe { *req };
    if r.flags & REQ_ALLOW_SCALED == 0 || r.target_w == 0 || r.target_h == 0 {
        return None;
    }
    if r.target_w >= nw && r.target_h >= nh {
        return None;
    }
    let header = turbojpeg::read_header(input).ok()?;
    if header.is_lossless || header.is_arithmetic {
        return None;
    }
    let (tw, th) = (r.target_w as usize, r.target_h as usize);
    let factor = SCALE_LADDER
        .iter()
        .copied()
        .find(|f| f.scale(header.width) >= tw && f.scale(header.height) >= th)?;
    // ★ 码率闸（m35 实测）：DCT 缩放只省掉 IDCT 与上采样，**省不掉 Huffman 熵解码**，
    //   于是收益随"每像素字节数"单调下滑；而能省多少又取决于**档位多小**——
    //   5120×3840 实测加速（视口 2560×1440 → 1/2 档 / 缩略图 240×180 → 1/8 档）：
    //   0.1 B/px（gradient，2 MB）**1.86× / 3.08×**；2.0 B/px（photo，39 MB）
    //   0.83×（1/2 档**倒亏**）；4.0 B/px（noise，79 MB）0.91×（1/2 档倒亏）。
    //   故阈值按档位给：档位越小省得越多，允许的码率越高。整数式写
    //   （`len * 8 <= px * 每像素字节数×8`）避免浮点与溢出。
    let bpp_x8: u64 = match factor {
        turbojpeg::ScalingFactor::ONE_EIGHTH => 64, // ≤ 8 B/px：基本照旧，几乎总走
        turbojpeg::ScalingFactor::ONE_QUARTER => 16, // ≤ 2 B/px
        _ => 4,                                     // 1/2 档最保守：≤ 0.5 B/px
    };
    // 小图不劳这条闸：解码本身就在微秒级，缩放的赚与亏都无所谓，而小 JPEG 的
    // "每像素字节数"天生偏高（头开销占比大），拿它判会把本该缩放的场景误挡掉。
    const GATE_MIN_PIXELS: u64 = 1_000_000;
    let px = nw as u64 * nh as u64;
    if px >= GATE_MIN_PIXELS && (input.len() as u64).saturating_mul(8) > px.saturating_mul(bpp_x8) {
        return None;
    }
    // `scale()` 是向上取整（ceil），出来的尺寸只会比 target 大、不会小 → 宿主只需缩小。
    let (sw, sh) = (
        factor.scale(header.width) as u32,
        factor.scale(header.height) as u32,
    );
    // 两库对同一份 SOF 读出的尺寸必须一致；对不上就回整解，别让缩放腿去猜谁的账错。
    if sw == 0 || sh == 0 || sw > nw || sh > nh || (sw, sh) == (nw, nh) {
        return None;
    }
    Some(ScaledPlan {
        factor,
        w: sw,
        h: sh,
    })
}

/// 纯 Rust 构建：没有缩放腿，任何请求都整解（宿主按 caps 也不会点 `REQ_ALLOW_SCALED`）。
#[cfg(not(feature = "turbo"))]
fn scaled_plan(_req: *const IvRenderReq, _input: &[u8], _nw: u32, _nh: u32) -> Option<ScaledPlan> {
    None
}

/// 缩放解码：直接写进宿主的缓冲（省掉整解那条 78 MB 中转 + 一次 memcpy）。
#[cfg(feature = "turbo")]
unsafe fn decode_scaled_into(
    input: &[u8],
    p: &ScaledPlan,
    out: *mut u8,
) -> Result<(), (i32, &'static str)> {
    use turbojpeg::{Decompressor, Image, PixelFormat};
    let (w, h) = (p.w as usize, p.h as usize);
    let mut d = Decompressor::new().map_err(|e| classify_tj(&e))?;
    d.set_scaling_factor(p.factor)
        .map_err(|e| classify_tj(&e))?;
    // out_cap 已在上游按 w*h*4 验过，这里按申报尺寸精确切片（pitch 无填充）。
    let buf = std::slice::from_raw_parts_mut(out, w * h * 4);
    let img = Image {
        pixels: buf,
        width: w,
        pitch: w * 4,
        height: h,
        format: PixelFormat::RGBA,
    };
    d.decompress(input, img).map_err(|e| classify_tj(&e))
}

/// 纯 Rust 构建的占位：`scaled_plan` 恒 None，这里永不触达。
#[cfg(not(feature = "turbo"))]
unsafe fn decode_scaled_into(
    _input: &[u8],
    _p: &ScaledPlan,
    _out: *mut u8,
) -> Result<(), (i32, &'static str)> {
    Err((ERR_UNSUPPORTED, "本构建未编入缩放解码"))
}

/// TurboJPEG 的错误文本 → ABI 错误码。只认关键词，不依赖 crate 的枚举细节
/// （`Error::TurboJpegError(String)` 里装的是 libjpeg-turbo 自己的话）。
#[cfg(feature = "turbo")]
fn classify_tj(err: &turbojpeg::Error) -> (i32, &'static str) {
    let s = format!("{err}").to_lowercase();
    if s.contains("out of memory") || s.contains("null pointer") {
        (ERR_OOM, "缩放解码内存不足")
    } else if s.contains("not supported") {
        (ERR_UNSUPPORTED, "该 JPEG 子格式不支持缩放解码")
    } else {
        // 其余（Corrupt / Invalid / Bogus / Premature / 参数越界）统一按坏文件报，
        // 宿主侧的表现与整解腿遇到同类文件一致。
        (ERR_CORRUPT, "JPEG 缩放解码失败")
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    const GOLDEN: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/golden_16x12.jpg"
    ));

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

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 5);
    }

    #[test]
    #[allow(clippy::manual_c_str_literals)] // 末尾 \x00 是被测数据的一部分，不是字符串结束符
    fn probe_accepts_jpeg_magic_and_rejects_lookalikes() {
        assert_eq!(
            iv_probe(GOLDEN.as_ptr(), GOLDEN.len(), std::ptr::null()),
            100
        );
        // FF D8 后面不是 marker → 不认领
        assert_eq!(
            iv_probe(b"\xFF\xD8\x00\x00".as_ptr(), 4, std::ptr::null()),
            0
        );
        // PNG / 垃圾
        assert_eq!(
            iv_probe(b"\x89PNG\r\n\x1a\n".as_ptr(), 8, std::ptr::null()),
            0
        );
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(iv_probe(b"\xFF\xD8".as_ptr(), 2, std::ptr::null()), 0);
    }

    #[test]
    fn query_phase_reports_size_without_decoding() {
        let mut info = IvRenderOut::default();
        let r = iv_render(
            GOLDEN.as_ptr(),
            GOLDEN.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (16, 12));
        assert_eq!(info.bytes_needed, 16 * 12 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        // 语料这张没有 EXIF 方向标签 → 如实报 1；`already_oriented=0` 是**契约值**：
        // 本解码器从不转像素，转正归宿主（那条路有 4 份矩阵用例）。旧断言写 (1,1) 把
        // "没做" 说成 "我做过所以不用做"——m48 把它改回诚实的一对。
        assert_eq!((info.orientation, info.already_oriented), (1, 0));
        assert_eq!(info.frame_count, 1);
        assert_eq!(info.page_count, 1);
    }

    /// 把一张「EXIF 方向 = 6」的 APP1 段插到 SOI 之后（语料本身不带方向标签）。
    /// APP1 的长度字段含自身两字节，所以写 `body.len() + 2`。
    fn jpeg_with_orientation(v: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 紧跟 8 字节头
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 一条标签
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        tiff.extend_from_slice(&v.to_le_bytes()); // 值落在 4 字节偏移字段的低半部
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // 没有下一个 IFD
        let mut body = b"Exif\0\0".to_vec();
        body.extend_from_slice(&tiff);
        let mut out = GOLDEN[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&GOLDEN[2..]);
        out
    }

    /// m48「竖拍照片不再横躺」的落点：标签读出来照实回报，转正由宿主那一条已有矩阵
    /// 用例的路完成。三处断言分别钉住整解申报、缩放腿申报、无标签时行为不变。
    #[test]
    fn exif_orientation_is_reported_for_the_host_to_apply() {
        let data = jpeg_with_orientation(6);
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(
            (info.actual_w, info.actual_h),
            (16, 12),
            "申报的是解码器看到的原始尺寸——转正是宿主的事"
        );
        assert_eq!(
            (info.orientation, info.already_oriented),
            (6, 0),
            "读到 6 就报 6，并声明自己没转"
        );

        // 缩放腿同样如实：否则同一张图的缩略图与主画面方向不一致。
        let (code, info, _) = render_scaled(&data, 4, 3);
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(
            (info.orientation, info.already_oriented),
            (6, 0),
            "缩放腿也要报方向"
        );

        // 对照组：同一份语料没有标签 → 报 1，与历史行为逐字节相同。
        let mut plain = IvRenderOut::default();
        assert_eq!(
            iv_render(
                GOLDEN.as_ptr(),
                GOLDEN.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut plain,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!((plain.orientation, plain.already_oriented), (1, 0));
    }

    #[test]
    fn render_phase_matches_query_and_fills_alpha() {
        let mut info = IvRenderOut::default();
        iv_render(
            GOLDEN.as_ptr(),
            GOLDEN.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            GOLDEN.as_ptr(),
            GOLDEN.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(buf.len(), 16 * 12 * 4);
        // 库直出 RGBA：alpha 必须恒 255（若不是，说明输出色彩空间没设对，回到了桥接老路）。
        assert!(
            buf.chunks_exact(4).all(|p| p[3] == 255),
            "alpha 未由库填满 255"
        );
        // 非全黑：真解码发生了。
        assert!(buf.iter().any(|&v| v > 0));
    }

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 16];
        let r = iv_render(
            GOLDEN.as_ptr(),
            GOLDEN.len(),
            &req(),
            tiny.as_mut_ptr(),
            tiny.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_BUFFER_TOO_SMALL);
    }

    #[test]
    fn byte_budget_is_honoured() {
        let mut info = IvRenderOut::default();
        let r = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 64, // 16x12x4 = 768 远超
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        assert_eq!(
            iv_render(
                GOLDEN.as_ptr(),
                GOLDEN.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_TOO_LARGE
        );
    }

    /// G5（输入线性）盲点：头里写巨大尺寸，必须在分配前被 MAX_PIXELS 挡住。
    #[test]
    fn huge_dimensions_rejected_before_alloc() {
        // 取真 JPEG，把 SOF 里的尺寸改成 65535x65535（≈ 4.3 GPixel）。
        let (sof, _) = find_sof(GOLDEN).expect("语料里应能找到 SOF 段");
        let mut bad = GOLDEN.to_vec();
        for k in [sof + 5, sof + 6, sof + 7, sof + 8] {
            bad[k] = 0xFF;
        }
        let mut info = IvRenderOut::default();
        let r = iv_render(
            bad.as_ptr(),
            bad.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        // 尺寸被改坏后可能头校验就失败（CORRUPT），也可能走到上限（TOO_LARGE）——
        // 两者都算挡住；关键绝不能是 OK 或 panic。
        assert!(
            r == ERR_TOO_LARGE || r == ERR_CORRUPT || r == ERR_UNSUPPORTED,
            "畸形尺寸应被挡下，实际 code={}",
            r
        );
        assert_ne!(r, ERR_OK);
    }

    /// 定位 SOF 段（跳过 DQT/DHT 等），返回其段起始下标与段长。测试用。
    fn find_sof(data: &[u8]) -> Option<(usize, usize)> {
        let mut i = 2;
        while i + 4 < data.len() {
            if data[i] == 0xFF {
                let m = data[i + 1];
                if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                    let seglen = (data[i + 2] as usize) << 8 | data[i + 3] as usize;
                    return Some((i, seglen));
                }
                if m != 0xFF && !(0xD0..=0xD9).contains(&m) && m != 0x01 {
                    let seglen = (data[i + 2] as usize) << 8 | data[i + 3] as usize;
                    i += 2 + seglen;
                    continue;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        None
    }

    /// 截断到"头完整、熵数据为空"：不得 panic、不得挂死，且必须报诚实的错误码。
    #[test]
    fn truncated_stream_is_corrupt_not_hung() {
        let (sof, seglen) = find_sof(GOLDEN).expect("语料里应能找到 SOF 段");
        let head = &GOLDEN[..(sof + 2 + seglen).min(GOLDEN.len() - 1)];
        let mut info = IvRenderOut::default();
        let q = iv_render(
            head.as_ptr(),
            head.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if std::env::var("JPEG_PROBE_CODES").is_ok() {
            println!(
                "sof@{} seglen={} headlen={} query={} err={}",
                sof,
                seglen,
                head.len(),
                q,
                last_err()
            );
        }
        // 只剩 SOF、没有 DHT/SOS：头不完整，查询阶段就该诚实失败，绝不能是 ERR_PANIC。
        assert!(
            matches!(q, ERR_CORRUPT | ERR_UNSUPPORTED | ERR_UNRECOGNIZED),
            "缺 DHT/SOS 的头应报诚实错误码，实际 {}",
            q
        );

        // 砍掉尾部 32 字节（头完整、scan 数据被切）：渲染阶段同样不得 panic/越界。
        let cut = &GOLDEN[..GOLDEN.len() - 32];
        let mut info2 = IvRenderOut::default();
        let q2 = iv_render(
            cut.as_ptr(),
            cut.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info2,
            std::ptr::null(),
        );
        assert_eq!(q2, ERR_OK, "头完整时查询应能拿到尺寸");
        assert_eq!((info2.actual_w, info2.actual_h), (16, 12));
        let mut buf = vec![0u8; info2.bytes_needed as usize];
        let r = iv_render(
            cut.as_ptr(),
            cut.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info2,
            std::ptr::null(),
        );
        if std::env::var("JPEG_PROBE_CODES").is_ok() {
            println!("cut render={} err={}", r, last_err());
        }
        assert_ne!(r, ERR_PANIC, "截断熵数据绝不能走到 panic 分支");
        assert_ne!(r, ERR_BUFFER_TOO_SMALL);
    }

    fn last_err() -> String {
        use rupix_abi as kit;
        let mut e = [0i8; 256];
        let need = unsafe { kit::write_last_error(e.as_mut_ptr(), e.len()) } as usize;
        let n = need.saturating_sub(1).min(e.len());
        String::from_utf8_lossy(&e[..n].iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }

    #[test]
    fn garbage_input_is_corrupt() {
        let junk = [0u8; 64];
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                junk.as_ptr(),
                junk.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_CORRUPT
        );
    }

    #[test]
    fn last_error_is_set_on_failure_and_empty_on_success() {
        use rupix_abi as kit;
        let mut info = IvRenderOut::default();
        let junk = [0u8; 64];
        iv_render(
            junk.as_ptr(),
            junk.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        let mut ebuf = [0i8; 128];
        let need = unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        assert!(need > 1, "失败路径应写入非空 last error");
        let s: Vec<u8> = ebuf[..(need as usize - 1)]
            .iter()
            .map(|&c| c as u8)
            .collect();
        let msg = String::from_utf8_lossy(&s).into_owned();
        assert!(
            !msg.starts_with("jpeg: "),
            "规范 §六 第 11 条：不得自带解码器名前缀（宿主补「（解码器 jpeg）」），实际「{msg}」"
        );
    }

    // ── 缩放腿（`iv_caps` 的 CAP_SCALED ↔ 请求的 REQ_ALLOW_SCALED）──────

    const BASE64: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/base_64x48.jpg"
    ));
    const PROG64: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/prog_64x48.jpg"
    ));
    const GREY8: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/grey_8x8.jpg"
    ));

    /// 带缩放意图的请求——宿主缩略图路径发的就是这一枚 flags。
    fn scaled_req(tw: u32, th: u32) -> IvRenderReq {
        IvRenderReq {
            target_w: tw,
            target_h: th,
            flags: REQ_ALLOW_SCALED,
            ..req()
        }
    }

    /// 查询 → 按申报尺寸分配 → 渲染，两步走一遍（与宿主真实调用顺序一致）。
    fn render_scaled(data: &[u8], tw: u32, th: u32) -> (i32, IvRenderOut, Vec<u8>) {
        let r = scaled_req(tw, th);
        let mut info = IvRenderOut::default();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            &r,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if q != ERR_OK {
            return (q, info, Vec::new());
        }
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let code = iv_render(
            data.as_ptr(),
            data.len(),
            &r,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        (code, info, buf)
    }

    /// **只认意图位，不认 target**：主画面也传非零 target（视口尺寸），那儿要的是清晰。
    /// 没置 `REQ_ALLOW_SCALED` 的路径必须与今天逐字节相同——这条是"加位不抢既有路"的钉子。
    #[test]
    fn target_alone_never_triggers_scaling() {
        let mut info = IvRenderOut::default();
        let r = IvRenderReq {
            target_w: 4,
            target_h: 3,
            ..req()
        };
        assert_eq!(
            iv_render(
                GOLDEN.as_ptr(),
                GOLDEN.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(
            (info.actual_w, info.actual_h),
            (16, 12),
            "target 单独摆在那儿不许触发缩放"
        );
    }

    /// 置了位才走缩放腿，档位取**装得下 target 的最小档**：16×12 要 4×3，
    /// 1/8 只有 2×2（装不下 → 会让宿主放大），1/4 正好 4×3。
    #[cfg(feature = "turbo")]
    #[test]
    fn flag_picks_ladder_rung_that_covers_target() {
        let (code, info, buf) = render_scaled(GOLDEN, 4, 3);
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(info.bytes_needed, 4 * 3 * 4);
        assert_eq!(buf.len(), 48);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(
            (info.orientation, info.already_oriented),
            (1, 0),
            "缩放腿出来的像素同样是未摆正的原始像素，报 1/0 = 没标签、我不转"
        );
        assert!(
            buf.chunks_exact(4).all(|p| p[3] == 255),
            "turbo 的 RGBA 输出必须自填 alpha=255"
        );
        assert!(buf.iter().any(|&v| v > 0), "真解码发生了");
    }

    /// target 大过半尺寸（40×30 > 32×24）→ 三档都装不下 → 回整解。
    /// 缩放腿绝不让宿主**放大**补齐：那是画质倒退，不是优化。
    #[test]
    fn target_bigger_than_half_falls_back_to_full() {
        let (code, info, _) = render_scaled(BASE64, 40, 30);
        assert_eq!(code, ERR_OK);
        assert_eq!(
            (info.actual_w, info.actual_h),
            (64, 48),
            "40×30 装不进 1/2 以外的任何档，必须整解"
        );
    }

    /// 灰度 JPEG 走缩放腿也要拿到 RGBA（三个通道同值、alpha 由库填满）。
    #[cfg(feature = "turbo")]
    #[test]
    fn grey_uses_scaled_leg_and_gets_alpha() {
        let (code, info, buf) = render_scaled(GREY8, 1, 1);
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (1, 1));
        assert_eq!(buf.len(), 4);
        assert_eq!(buf[3], 255);
        assert_eq!(buf[0], buf[1]);
        assert_eq!(buf[1], buf[2]);
    }

    /// 渐进式（SOF2）：turbo 能不能真缩放由它自己的实现说话——这里只钉"诚实"：
    /// 申报尺寸 == 写出的数据量，且和整解腿一样不 panic。探测模式打印实况。
    #[cfg(feature = "turbo")]
    #[test]
    fn progressive_scaled_leg_stays_honest() {
        let (code, info, buf) = render_scaled(PROG64, 8, 6);
        if std::env::var("JPEG_PROBE_CODES").is_ok() {
            println!(
                "progressive 8x6 -> code={} size={}x{} bytes={} err={}",
                code,
                info.actual_w,
                info.actual_h,
                info.bytes_needed,
                last_err()
            );
        }
        assert_eq!(code, ERR_OK, "渐进式不许把缩放请求打成错误：{}", code);
        assert_eq!(buf.len(), info.bytes_needed as usize);
        assert!(buf.chunks_exact(4).all(|p| p[3] == 255));
    }

    /// 截断的渐进式 + 缩放意图：不得 panic、不得越界写（缩放腿走的是另一条 C 边界）。
    #[cfg(feature = "turbo")]
    #[test]
    fn truncated_progressive_with_scale_intent_is_safe() {
        let cut = &PROG64[..PROG64.len() - 24];
        let (code, info, buf) = render_scaled(cut, 8, 6);
        assert_ne!(code, ERR_PANIC, "截断流绝不能走到 panic 分支");
        if code == ERR_OK {
            assert_eq!(buf.len(), info.bytes_needed as usize);
        }
    }

    /// 把一枚单分片 ICC APP2 插到 SOI 之后（语料本身不带 ICC）。
    fn jpeg_with_icc(profile: &[u8]) -> Vec<u8> {
        let mut body = c"ICC_PROFILE".to_bytes_with_nul().to_vec();
        body.push(1);
        body.push(1);
        body.extend_from_slice(profile);
        let mut out = GOLDEN[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE2]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&GOLDEN[2..]);
        out
    }

    /// m53-J：ICC 在查询阶段如实申报、在渲染阶段写到像素段之后（整解与缩放两腿）。
    #[test]
    fn icc_profile_is_declared_and_appended_after_pixels() {
        let profile: Vec<u8> = (0..256u32).map(|i| (i * 7 % 251) as u8).collect();
        let data = jpeg_with_icc(&profile);

        // 整解腿：查询 → 按 bytes_needed + icc_len 分配 → 渲染。
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(info.icc_len as usize, profile.len(), "查询就该申报 ICC");
        let mut buf = vec![0u8; info.bytes_needed as usize + info.icc_len as usize];
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(&buf[info.bytes_needed as usize..], &profile[..]);

        // 缩放腿：换一条写宿主缓冲的路径，ICC 尾巴同样不能丢。
        let r = scaled_req(8, 6);
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(info.icc_len as usize, profile.len());
        let mut buf = vec![0u8; info.bytes_needed as usize + info.icc_len as usize];
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &r,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(&buf[info.bytes_needed as usize..], &profile[..]);

        // 没 ICC 的同一张：申报 0，行为与今天逐字节相同。
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                GOLDEN.as_ptr(),
                GOLDEN.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(info.icc_len, 0);
    }
}
