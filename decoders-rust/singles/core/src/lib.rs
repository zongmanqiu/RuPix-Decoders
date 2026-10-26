//! RuPix 解码器：**常用位图格式**（core）。
//!
//! 一个解码器覆盖 APNG / GIF / WebP / BMP / ICO / TGA / DDS / PNM / Farbfeld，
//! 并在嗅探兜底里以**低置信度**补位任何专项 DLL 缺席时的常见格式（含 PNG / JPEG / TIFF）。
//! 解码库分工：`image` crate（内部即 zune-jpeg、png 0.18、gif 0.14、tiff 0.10）+
//! **WebP 走 libwebp**（image-webp 0.2 慢 44%，3MP 实测 90.6 → 50.3ms，见 Cargo.toml）。
//!
//! # 本解码器的定位：通用兜底手，不是冠军
//!
//! `image` 路线对每种格式都比"直接调该格式的冠军库"慢（G1 实测 JPEG 1.80× /
//! PNG 1.19× / GIF 1.18× / TIFF 1.24×），所以冠军格式各自有专项 DLL。core 的价值在
//! 两处：① 还没被拆出去的格式（BMP/ICO/TGA/DDS/PNM…）由它主认领；② 专项 DLL 被用户
//! 关掉或 DLL 缺失时，它以 50 置信度把图照样打开（宿主嗅探兜底按最高分派，见下）。
//!
//! # 为什么是一个解码器而不是十个
//!
//! DLL 有约 101 KB 的固定开销，且**依赖不跨 DLL 共享**（每个 DLL 各带一份 Rust 运行时、
//! 各带一份 flate2）。十个小 DLL = 1 MB 纯固定开销 + 重复依赖。常用位图格式使用频率极高、
//! 缺一不可，合成一个 `core` 只付一次开销，族内共享 `flate2` / `moxcms`。
//!
//! # 扩展名清单为什么这么长
//!
//! `extensions` 里既有真格式名，也有大量**别名**（.jpe .jfif .exif .dib .rle .icon .icb…）。
//! 宿主按扩展名找到本解码器后，**解码走内容嗅探**（`guess_format`），不看扩展名——
//! 所以别名只要进了清单就能开。清单漏一个，用户就翻不到那个后缀的图。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use std::ffi::c_char;
use std::io::Cursor;

use image::{ImageError, ImageReader};
use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};

#[cfg(feature = "standalone")]
export_decoder! {
    id: "core",
    name: "Core Raster Formats",
    // v3 起宿主把扩展名传进 probe/render，`.tga`（无魔数）由此按扩展名认领。
    // ★ JPEG / TIFF / PNG 后缀已整体交给对应冠军解码器（一库一 DLL + 唯一认领门禁）：G1 实测
    //   zune-jpeg 原生 RGBA 比 `image` 路线快 1.80×、tiff crate 直调快 1.24×、
    //   png crate 直调快 1.19×。
    //   本解码器仍能在嗅探兜底里以 50 置信度接住它们（冠军被关掉 / DLL 缺失 / 冠军主动让位）。
    //   `apng` 留在本解码器：PNG 冠军只做静帧，APNG 动画由这里的 `apng_anim` 路线服务。
    // GIF 已随 #58 冠军解码器迁出（gif crate 直调 1.18×）；`gif` 静态帧与动画帧 ≥1 的
    // 播放仍归 core（冠军对 frame_index != 0 主动让位，本解码器的会话缓存继续兜底）。
    extensions: "apng;webp;bmp;dib;rle;ico;icon;pnm;ppm;pgm;pbm;pam;dds;ff;tga",
    description: "常用位图兜底：APNG/WebP/BMP/ICO/TGA/DDS/PNM（JPEG/PNG/TIFF/GIF 已由冠军解码器认领）",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

/// 通用兜底手的置信度：低到让专项冠军 DLL（报 100）在宿主嗅探时胜出，
/// 高到冠军缺席（被关掉 / DLL 缺失）时本解码器仍能开图。
const CONFIDENCE_GENERALIST: i32 = 50;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。超出即判太大，不尝试分配。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render
// ---------------------------------------------------------------------------

/// 魔数嗅探；v3 起另收 `ext`（小写扩展名，可 NULL）用于认领无魔数格式（TGA）。
///
/// ★ core 对**内容认得的格式只报 [`CONFIDENCE_GENERALIST`]（50）**，不报 100。
/// 原因：本解码器走 `image` 通用路线，G1 实测对每种格式都比"直接调该格式的冠军库"慢
/// （JPEG 1.80× / PNG 1.19× / GIF 1.18× / TIFF 1.24×）。宿主嗅探兜底按最高置信度分派，
/// 专项 DLL 报 100 因此永远压过 core；而专项 DLL 被用户关掉或文件缺失时，core 的 50
/// 仍能把图打开——兜底手不该抢冠军的活，但必须在冠军缺席时补位。
/// 命中返回置信度，不认返回 0。
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_impl(data, len, ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if ext_is(ext, b"tga") {
            // TGA 没有魔数可验，扩展名即认领；坏文件让 render 报 ERR_CORRUPT。
            // 这条只在宿主按扩展名分派后传真实 ext 时触发（嗅探兜底传 NULL，不会误认）。
            return 100;
        }
        if data.is_null() || len < 8 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if is_webp(slice) {
            return CONFIDENCE_GENERALIST;
        }
        match image::guess_format(slice) {
            Ok(_) => CONFIDENCE_GENERALIST,
            Err(_) => 0,
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
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
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
    ext: *const c_char,
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
        let frame_index = if req.is_null() {
            0
        } else {
            unsafe { (*req).frame_index }
        };
        let is_gif = input.starts_with(b"GIF87a") || input.starts_with(b"GIF89a");
        let is_apng = !is_gif && is_apng(input);
        let is_tga = ext_is(ext, b"tga");

        // ── 阶段 1：只解析头，拿尺寸。查询与渲染两阶段共用。──────────────
        let (w, h) = if is_gif {
            // GIF：走会话缓存查逻辑屏幕尺寸。查询阶段顺带回传已知帧数即返回；
            // 渲染阶段继续往下走 serve 真解码。
            match gif_anim::query(input) {
                Ok(((w, h), count)) => {
                    if out.is_null() {
                        fill_info_gif_header(info, w, h, count);
                        return ERR_OK;
                    }
                    (w, h)
                }
                Err(_) => {
                    set_last_error("GIF 文件头损坏");
                    return ERR_CORRUPT;
                }
            }
        } else if is_apng {
            // APNG：与 GIF 同理，走会话缓存查尺寸；查询阶段帧数未知(0)。
            match apng_anim::query(input) {
                Ok(((w, h), count)) => {
                    if out.is_null() {
                        fill_info_gif_header(info, w, h, count);
                        return ERR_OK;
                    }
                    (w, h)
                }
                Err(_) => {
                    set_last_error("APNG 文件头损坏");
                    return ERR_CORRUPT;
                }
            }
        } else {
            match dimensions(input, is_tga) {
                Ok(v) => v,
                Err(e) => {
                    set_last_error(&format!("读文件头失败，库报错 \"{e}\""));
                    return map_err(&e);
                }
            }
        };

        let npixels = w as u64 * h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码器上限");
            return ERR_TOO_LARGE;
        }
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

        // ★ 方向：本解码器认领的格式里只有 WebP 有方向语义（RIFF 里的 `EXIF` chunk，
        //   kit 函数逐 chunk 走、微秒级）；GIF / BMP / ICO / TGA / PNM / DDS 没有这枚
        //   标签，报 1 就是如实。读到就照实报 + `already_oriented = 0` 交宿主补转（m48）。
        let orientation = if is_webp(input) {
            rupix_abi::webp_exif_orientation(input).unwrap_or(1)
        } else {
            1
        };
        // ICC：只有 WebP 的 RIFF 里有 `ICCP` chunk（GIF/APNG/BMP/ICO/TGA/PNM/DDS 没有）。
        // kit 函数逐 chunk 走、长度驱动；超限/缺块 ⇒ 空。
        let icc = if is_webp(input) {
            rupix_abi::webp_icc_profile(input).unwrap_or_default()
        } else {
            Vec::new()
        };
        let icc_len = icc.len() as u64;

        // 查询阶段：out 为空指针，只回报尺寸/格式，不解码。
        // （GIF 已在上方提前返回；count 未知时填 0，宿主以渲染阶段回填为准。）
        if out.is_null() {
            fill_info(info, w, h, bytes_needed, 1, 0, orientation, icc_len as u32);
            return ERR_OK;
        }

        if (out_cap as u64) < bytes_needed + icc_len {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // ── 阶段 2：真解码。─────────────────────────────────────────────
        //
        // `image` 没有内建的 DCT 缩放解码，一律解原生尺寸，由宿主按 target_w/h 缩放。
        let (rgba, frame_count, delay_ms) = if is_webp(input) {
            // WebP 专用路线：libwebp 直接解到 RGBA8。动图取首帧（本版明确不做动图 WebP）。
            match decode_webp(input) {
                Ok(v) => (v, 1, 0),
                Err(code) => {
                    set_last_error("WebP 数据损坏");
                    return code;
                }
            }
        } else if is_gif {
            match gif_anim::serve(input, frame_index) {
                Ok(((w2, h2), v, delay, count)) => {
                    debug_assert!((w2, h2) == (w, h), "GIF 帧尺寸与逻辑屏幕不符");
                    // 单帧 GIF 是静态图（delay=0）；多帧 0-delay 钳到 ≥10ms 防忙转。
                    match count {
                        Some(1) => (v, 1, 0),
                        Some(c) => (v, c, delay),
                        None => (v, 0, delay),
                    }
                }
                Err(code) => {
                    set_last_error("GIF 帧解码失败");
                    return code;
                }
            }
        } else if is_apng {
            match apng_anim::serve(input, frame_index) {
                Ok(((w2, h2), v, delay, count)) => {
                    debug_assert!((w2, h2) == (w, h), "APNG 帧尺寸与画布不符");
                    // 单帧 APNG 是静态图（delay=0）；多帧 0-delay 钳到 ≥10ms 防忙转。
                    match count {
                        Some(1) => (v, 1, 0),
                        Some(c) => (v, c, delay),
                        None => (v, 0, delay),
                    }
                }
                Err(code) => {
                    set_last_error("APNG 帧解码失败");
                    return code;
                }
            }
        } else {
            let r = if is_tga {
                decode_rgba_fmt(input, image::ImageFormat::Tga)
            } else {
                decode_rgba(input)
            };
            match r {
                Ok(v) => (v, 1, 0),
                Err(e) => {
                    set_last_error(&format!("解码失败，库报错 \"{e}\""));
                    return map_err(&e);
                }
            }
        };

        // 解码器报的尺寸与头解析一致（不一致说明文件撒谎），以实际缓冲区为准。
        if rgba.len() as u64 != bytes_needed {
            set_last_error("解出的尺寸与文件头不符");
            return ERR_CORRUPT;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
        }
        if !icc.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    icc.as_ptr(),
                    out.add(bytes_needed as usize),
                    icc.len(),
                );
            }
        }
        fill_info(
            info,
            w,
            h,
            bytes_needed,
            frame_count,
            delay_ms,
            orientation,
            icc_len as u32,
        );
        ERR_OK
    })
}
/// 扩展名比对（NUL 结尾小写串，大小写不敏感；NULL = 未知）。
fn ext_is(ext: *const c_char, want: &[u8]) -> bool {
    if ext.is_null() {
        return false;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(ext) };
    s.to_bytes().eq_ignore_ascii_case(want)
}

/// GIF 查询阶段的专用回填：尺寸 + 已知帧数（未知填 0，宿主以渲染阶段为准）。
/// bytes_needed 按画布算，各帧相同。
fn fill_info_gif_header(info: *mut IvRenderOut, w: u32, h: u32, count: u32) {
    let bytes_needed = w as u64 * h as u64 * 4;
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    // GIF 没有方向标签（规范里没有这个概念），报 1 就是如实；像素也从没摆正过。
    oi.orientation = 1;
    oi.already_oriented = 0;
    oi.frame_count = count;
    // v4：GIF 单文件单页；ICC 不透传（image 不出 ICC，语义先通）。
    oi.page_count = 1;
    oi.icc_len = 0;
    // delay 填 0：查询阶段还不知道首帧时长；宿主对 GIF 不看这个值，
    // 而是发 frame_index=0 的渲染请求拿真实 delay。
}

#[inline]
#[allow(clippy::too_many_arguments)] // 参数就是 IvRenderOut 的字段表，逐项写清比打包成结构更直白
fn fill_info(
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    bytes_needed: u64,
    frame_count: u32,
    frame_delay_ms: u32,
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
    // 本解码器从不摆正像素（`image` 与 libwebp 出的都是文件里的原始行序），所以老实报 0。
    // 旧注释"`image` 不解析方向，声明无需宿主再转以免双重旋转"站不住：二次旋转只在
    // "转了却不声明"时发生，如实回报才是契约给的路（`av` / `raw` 一直是这么写的）。
    oi.already_oriented = 0;
    oi.frame_count = frame_count;
    oi.frame_delay_ms = frame_delay_ms;
    // v4：单页；ICC 由 WebP 的 `ICCP` 原样透传，其它格式恒空。
    oi.page_count = 1;
    oi.icc_len = icc_len;
}

// ---------------------------------------------------------------------------
// image crate 的薄封装
// ---------------------------------------------------------------------------

/// WebP 魔数：`RIFF` + 4 字节长度 + `WEBP`（内容嗅探，与扩展名无关）。
fn is_webp(data: &[u8]) -> bool {
    data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP"
}

/// APNG 检测：PNG 签名 + 含 `acTL` 块（动画控制块，紧跟 IHDR）。静态 PNG 无此块。
/// 仅用于把 APNG 从普通 PNG 的「单帧」路径里分出来走动画会话。
fn is_apng(data: &[u8]) -> bool {
    const SIG: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if data.len() < 8 || &data[0..8] != SIG {
        return false;
    }
    // acTL 紧跟 IHDR，扫头部若干 KB 即可（动画块不可能跑到文件尾）。
    let head = &data[..data.len().min(1 << 16)];
    head.windows(4).any(|w| w == b"acTL")
}

/// WebP 头解析：`WebPGetInfo` 只读 VP8/VP8L 头，不解码，微秒级。
#[cfg(feature = "webp-c")]
fn webp_dimensions(data: &[u8]) -> Result<(u32, u32), i32> {
    let mut w = 0i32;
    let mut h = 0i32;
    let ok = unsafe { libwebp_sys::WebPGetInfo(data.as_ptr(), data.len(), &mut w, &mut h) };
    if ok == 0 || w <= 0 || h <= 0 {
        return Err(ERR_CORRUPT);
    }
    Ok((w as u32, h as u32))
}

/// WebP 解码：libwebp 直接输出 RGBA8（非预乘，行主序），与 ABI 契约一致。
/// 动图取首帧（与 image-webp 行为一致）。失败返回 ABI 错误码。
#[cfg(feature = "webp-c")]
fn decode_webp(data: &[u8]) -> Result<Vec<u8>, i32> {
    let mut w = 0i32;
    let mut h = 0i32;
    let ptr = unsafe { libwebp_sys::WebPDecodeRGBA(data.as_ptr(), data.len(), &mut w, &mut h) };
    if ptr.is_null() {
        return Err(ERR_CORRUPT);
    }
    let n = (w as usize) * (h as usize) * 4;
    let rgba = unsafe {
        let src = std::slice::from_raw_parts(ptr, n);
        src.to_vec()
    };
    unsafe { libwebp_sys::WebPFree(ptr.cast()) };
    Ok(rgba)
}

/// 纯 Rust 分支（`webp-rust`，不含 libwebp）：同一套语义，换成 `image-webp`。
///
/// 慢一些（3 MP 实测 90.6 ms vs 50.3 ms，-44%），但换来"这枚 DLL 一个 C 字符都不带"。
/// 合并档（`common`）用它；独立 `core` 解码器仍走 libwebp。
#[cfg(all(not(feature = "webp-c"), feature = "webp-rust"))]
fn webp_dimensions(data: &[u8]) -> Result<(u32, u32), i32> {
    let dec = image_webp::WebPDecoder::new(Cursor::new(data)).map_err(|_| ERR_CORRUPT)?;
    let (w, h) = dec.dimensions();
    if w == 0 || h == 0 {
        return Err(ERR_CORRUPT);
    }
    Ok((w, h))
}

#[cfg(all(not(feature = "webp-c"), feature = "webp-rust"))]
fn decode_webp(data: &[u8]) -> Result<Vec<u8>, i32> {
    let mut dec = image_webp::WebPDecoder::new(Cursor::new(data)).map_err(|_| ERR_CORRUPT)?;
    let (w, h) = dec.dimensions();
    let has_alpha = dec.has_alpha();
    let mut buf = vec![0u8; dec.output_buffer_size().unwrap_or(0)];
    dec.read_image(&mut buf).map_err(|_| ERR_CORRUPT)?;
    if has_alpha {
        return Ok(buf);
    }
    // 无 alpha 的 WebP 解出来是 RGB8（3 B/px），ABI 只认 RGBA8，这里展开并补满不透明。
    let npix = (w as usize) * (h as usize);
    let mut rgba = Vec::with_capacity(npix * 4);
    for px in buf.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 0xFF]);
    }
    Ok(rgba)
}

/// 两个 webp 后端都没开：明确不许编（静默留下一个"webp 打不开"的构建比编不过更糟）。
#[cfg(all(not(feature = "webp-c"), not(feature = "webp-rust")))]
compile_error!("core 必须至少选择一种 WebP 后端：feature `webp-c`（libwebp，默认）或 `webp-rust`（image-webp）");

/// `with_guessed_format()` 会真的去读缓冲区头，所以返回 `Result`。
fn reader(data: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, ImageError> {
    ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(ImageError::IoError)
}

fn dimensions(data: &[u8], is_tga: bool) -> Result<(u32, u32), ImageError> {
    if is_webp(data) {
        return webp_dimensions(data)
            .map_err(|_| ImageError::IoError(std::io::ErrorKind::InvalidData.into()));
    }
    if is_tga {
        // TGA 靠魔数猜不出来，必须显式指定格式（这正是 v3 要传 ext 的原因）。
        return ImageReader::with_format(Cursor::new(data), image::ImageFormat::Tga)
            .into_dimensions();
    }
    reader(data)?.into_dimensions()
}

/// 按显式格式解码为 RGBA8（无魔数格式专用）。
fn decode_rgba_fmt(data: &[u8], fmt: image::ImageFormat) -> Result<Vec<u8>, ImageError> {
    Ok(ImageReader::with_format(Cursor::new(data), fmt)
        .decode()?
        .into_rgba8()
        .into_raw())
}

/// 解码为 RGBA8（非预乘，行主序）。16-bit / 灰度 / 调色板统一转成 8-bit RGBA。
fn decode_rgba(data: &[u8]) -> Result<Vec<u8>, ImageError> {
    Ok(reader(data)?.decode()?.into_rgba8().into_raw())
}

// ---------------------------------------------------------------------------
// GIF / APNG 共用的"后退帧环"：快进途中顺手留住最近几帧
// ---------------------------------------------------------------------------

/// 环的字节预算与帧数上下限。**只在后退快进时长**（正常顺序播放一帧都不进环），
/// 所以播放路径零额外内存、零额外拷贝。
const RING_BUDGET_BYTES: usize = 16 * 1024 * 1024;
const RING_MIN_FRAMES: usize = 2;
const RING_MAX_FRAMES: usize = 16;

/// 快进跳过的帧按 `(帧号, RGBA, delay_ms)` 留住，帧号连续递增。
///
/// 为什么放在解码器侧而不是宿主：GIF/APNG 的 disposal 合成天然串行，要第 k 帧就得从 0
/// 顺解到 k。宿主那份"落地帧缓存"收不到这些中途帧——它们从没上过屏。于是**连续后退**
/// 每一步都把 0..k 重解一遍再丢掉，20 步就是 O(n²) 的活。快进本来就在解这些帧，
/// 顺手存下来，后退就从"每步 O(n)"摊薄成"每 K 步 O(n)"（K = 环里装得下的帧数）。
struct FrameRing {
    frames: std::collections::VecDeque<(u32, Vec<u8>, u32)>,
    bytes: usize,
}

impl FrameRing {
    fn new() -> Self {
        Self {
            frames: std::collections::VecDeque::new(),
            bytes: 0,
        }
    }

    /// 收一帧。单帧就超预算的大图直接不收（K 至少 2 帧意味着"装不下就干脆不缓存"，
    /// 而不是把 16 MiB 花在两帧上却一步都省不了）。
    fn put(&mut self, index: u32, rgba: &[u8], delay_ms: u32) {
        let cap = (RING_BUDGET_BYTES / rgba.len().max(1)).clamp(RING_MIN_FRAMES, RING_MAX_FRAMES);
        if rgba.len() * cap > RING_BUDGET_BYTES || self.frames.len() >= RING_MAX_FRAMES {
            return;
        }
        self.bytes += rgba.len();
        self.frames.push_back((index, rgba.to_vec(), delay_ms));
        while self.frames.len() > cap {
            if let Some((_i, old, _d)) = self.frames.pop_front() {
                self.bytes -= old.len();
            }
        }
    }

    /// 命中就返回像素的拷贝。环里存的是"已经合成完的整画布"，取出来即用，无需再合成。
    fn get(&self, index: u32) -> Option<(&[u8], u32)> {
        self.frames
            .iter()
            .find(|(i, _, _)| *i == index)
            .map(|(_, px, d)| (px.as_slice(), *d))
    }

    /// 游标被重建到更早的位置时，环里比它更靠后的帧仍然是"同一份文件解出来的正确像素"，
    /// 所以不随重建清空；但**换文件**（会话被替换）时整个环跟着会话一起没。
    /// 只有单测探针 `ring_frames` 读它，DLL 里没人用 → 别让它进产物。
    #[cfg(test)]
    fn len(&self) -> usize {
        self.frames.len()
    }
}

// ---------------------------------------------------------------------------
// GIF 多帧：解码会话缓存 + 表头数帧（第一帧就知道总共几帧）
// ---------------------------------------------------------------------------

mod gif_anim {
    use super::*;

    /// 会话槽数：播放中的图 + 预读邻居并存。
    const SESSION_SLOTS: usize = 2;
    /// 动画帧最小展示时长（毫秒）：0-delay 动图防忙转。单帧 GIF 走静态（delay=0）。
    pub const MIN_DELAY_MS: u32 = 10;

    type Decoder = image::codecs::gif::GifDecoder<Cursor<Vec<u8>>>;

    /// 一个 GIF 的解码会话：迭代器游标是唯一划算的缓存粒度——
    /// GIF 的 disposal 合成天然串行（第 N 帧必须顺序解完 0..N），
    /// 随机访问只能重建解码器快进。
    struct Session {
        len: u64,
        head: [u8; 32],
        tail: [u8; 32],
        data: Vec<u8>,
        canvas: (u32, u32),
        next_index: u32,
        /// 总帧数。建会话时由 [`count_frames`] 走一遍表头拿到（不解像素）；
        /// 表头读不出来的畸形文件回落到 EOF 惰性发现，代价同旧行为。
        count: Option<u32>,
        iter: Option<image::Frames<'static>>,
        /// 快进跳过的帧（见 [`FrameRing`]）——连续后退不再重解 0..k。
        ring: FrameRing,
    }

    // 安全性：Session 内部全是自持数据（owned 缓冲 + owned 解码器），`!Send` 只是
    // trait object 没标 Send 边界的名义问题，不含线程亲和状态（无 Rc/RefCell/裸指针别名）。
    // 宿主只在解码线程调用 iv_*，即便另有调用方也被 SESSIONS 的 Mutex 串行化访问。
    unsafe impl Send for Session {}

    impl Session {
        fn fingerprint(data: &[u8]) -> (u64, [u8; 32], [u8; 32]) {
            let n = data.len();
            let mut head = [0u8; 32];
            let mut tail = [0u8; 32];
            head.copy_from_slice(&data[..n.min(32)]);
            let t = &data[n.saturating_sub(32)..];
            tail[..t.len()].copy_from_slice(t);
            (n as u64, head, tail)
        }

        fn new(data: &[u8]) -> Result<Self, ImageError> {
            let dec = open(data)?;
            let canvas = image::ImageDecoder::dimensions(&dec);
            let iter = image::AnimationDecoder::into_frames(dec);
            let (len, head, tail) = Self::fingerprint(data);
            Ok(Self {
                len,
                head,
                tail,
                data: data.to_vec(),
                canvas,
                next_index: 0,
                count: count_frames(data),
                iter: Some(iter),
                ring: FrameRing::new(),
            })
        }

        fn matches(&self, data: &[u8]) -> bool {
            let fp = Self::fingerprint(data);
            self.len == fp.0 && self.head == fp.1 && self.tail == fp.2
        }
    }

    fn open(data: &[u8]) -> Result<Decoder, ImageError> {
        image::codecs::gif::GifDecoder::new(Cursor::new(data.to_vec()))
    }

    /// 走一遍 GIF 表头数出总帧数，**不解像素**：gif crate 的 `next_frame_info`
    /// 内部用 `OutputBuffer::None`，LZW 数据段只按块跳过、不进解码器。
    /// 于是「总帧数」在第一帧之前就已知，宿主不会再显示 `帧 1/?`。
    ///
    /// 代价是一趟 O(文件字节) 的扫块（实测数字见 `docs/core.md` 的多帧会话一节）。
    /// 读不出来的（畸形、超过 `FRAME_SCAN_CAP` 的怪文件）返回 `None`，
    /// 回落到旧的 EOF 惰性发现——最坏只是暂时不知道总数，不会报错。
    pub(super) fn count_frames(data: &[u8]) -> Option<u32> {
        // 正常 GIF 到不了这个量级；纯粹是给"每次迭代都不消费字节"的畸形输入兜底，
        // 免得数帧的循环自己变成挂死点。
        const FRAME_SCAN_CAP: u32 = 100_000;
        let mut dec = gif::Decoder::new(data).ok()?;
        let mut n = 0u32;
        loop {
            match dec.next_frame_info() {
                Ok(Some(_)) => n += 1,
                Ok(None) => break,
                Err(_) => return None,
            }
            if n >= FRAME_SCAN_CAP {
                return None;
            }
        }
        (n > 0).then_some(n)
    }

    /// 会话池。解码线程是唯一使用方，但宿主契约不保证这一点，用 Mutex 兜底。
    /// lock 中毒（理论上 guard/panic 已隔离，防御性起见）→ 拿回锁继续用。
    static SESSIONS: std::sync::Mutex<Vec<Session>> = std::sync::Mutex::new(Vec::new());

    fn sessions() -> std::sync::MutexGuard<'static, Vec<Session>> {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 查询阶段：逻辑屏幕尺寸 + 帧数（表头数出来的；读不出 = 0）。不解码、不动游标。
    /// 会话不存在时新建一个（头解析 + 首帧迭代器就绪），渲染阶段直接续用。
    pub fn query(data: &[u8]) -> Result<((u32, u32), u32), i32> {
        let mut pool = sessions();
        if let Some(s) = pool.iter_mut().find(|s| s.matches(data)) {
            return Ok((s.canvas, s.count.unwrap_or(0)));
        }
        let s = Session::new(data).map_err(|_| ERR_CORRUPT)?;
        let (canvas, count) = (s.canvas, s.count.unwrap_or(0));
        push_session(&mut pool, s);
        Ok((canvas, count))
    }

    type Served = ((u32, u32), Vec<u8>, u32, Option<u32>); // (画布宽高, RGBA, delay_ms, count)
    /// 服务第 `index` 帧（0 起，越界自动回绕）：返回 (画布宽高, RGBA, delay_ms, count)。
    pub fn serve(data: &[u8], index: u32) -> Result<Served, i32> {
        let mut pool = sessions();
        if let Some(pos) = pool.iter().position(|s| s.matches(data)) {
            // LRU 触摸：挪到队首。
            let s = pool.remove(pos);
            pool.insert(0, s);
        } else {
            let s = Session::new(data).map_err(|_| ERR_CORRUPT)?;
            push_session(&mut pool, s);
        }
        let s = &mut pool[0];
        let mut index = index;

        // 已知帧数 → 取模回绕（宿主在拿到确切 count 前会递增地要帧，第一轮回绕
        // 最多多付一遍快进）。
        if let Some(c) = s.count {
            index %= c;
        }

        // 后退命中环：一次解码都不付，直接给成品像素。
        if let Some((px, delay)) = s.ring.get(index) {
            return Ok((s.canvas, px.to_vec(), delay, s.count));
        }

        // 游标跑过头（回绕）或迭代器被丢弃 → 重建快进。
        if s.iter.is_none() || s.next_index > index {
            s.iter = Some(image::AnimationDecoder::into_frames(
                open(&s.data).map_err(|_| ERR_CORRUPT)?,
            ));
            s.next_index = 0;
        }

        // 快进到目标帧；途中撞上 EOF → 帧数落袋，取模重来一次。
        loop {
            let mut eof_at: Option<u32> = None;
            while s.next_index < index {
                match s.iter.as_mut().unwrap().next() {
                    // 快进跳过的帧正是稍后后退要的——留住它，别解第二遍。
                    Some(Ok(f)) => {
                        s.ring.put(
                            s.next_index,
                            f.buffer().as_raw(),
                            f.delay().numer_denom_ms().0.max(MIN_DELAY_MS),
                        );
                        s.next_index += 1;
                    }
                    // 解错的帧不进环：环里的像素必须是能直接上屏的成品。
                    Some(_) => s.next_index += 1,
                    None => {
                        eof_at = Some(s.next_index);
                        break;
                    }
                }
            }
            match eof_at {
                None => break,
                Some(c) => {
                    s.count = Some(c.max(1));
                    s.iter = None;
                    index %= s.count.unwrap();
                    s.iter = Some(image::AnimationDecoder::into_frames(
                        open(&s.data).map_err(|_| ERR_CORRUPT)?,
                    ));
                    s.next_index = 0;
                }
            }
        }

        // 取目标帧。
        let frame = match s.iter.as_mut().unwrap().next() {
            Some(Ok(f)) => {
                s.next_index += 1;
                f
            }
            Some(Err(_)) => return Err(ERR_CORRUPT),
            None => {
                // EOF 恰在目标帧上：帧数就是 index，回绕到 0 重取。
                let c = index.max(1);
                s.count = Some(c);
                index %= c;
                s.iter = Some(image::AnimationDecoder::into_frames(
                    open(&s.data).map_err(|_| ERR_CORRUPT)?,
                ));
                s.next_index = 0;
                while s.next_index < index {
                    match s.iter.as_mut().unwrap().next() {
                        Some(Ok(_)) => s.next_index += 1,
                        _ => return Err(ERR_CORRUPT),
                    }
                }
                match s.iter.as_mut().unwrap().next() {
                    Some(Ok(f)) => {
                        s.next_index += 1;
                        f
                    }
                    _ => return Err(ERR_CORRUPT),
                }
            }
        };

        let mut delay = frame.delay().numer_denom_ms().0;
        if delay < MIN_DELAY_MS {
            delay = MIN_DELAY_MS;
        }
        Ok((s.canvas, frame.buffer().to_vec(), delay, s.count))
    }

    fn push_session(pool: &mut Vec<Session>, s: Session) {
        if pool.len() >= SESSION_SLOTS {
            pool.pop();
        }
        pool.insert(0, s);
    }

    /// 测试探针：这份字节的会话环里现在有几帧；无会话 = `None`。
    #[cfg(test)]
    pub(super) fn ring_frames(data: &[u8]) -> Option<usize> {
        sessions()
            .iter()
            .find(|s| s.matches(data))
            .map(|s| s.ring.len())
    }
}

// ---------------------------------------------------------------------------
// APNG 多帧：复用 GIF 的解码会话缓存 + 表头数帧（总数直接读 `acTL`；image crate
// 的 PngDecoder 在识别到 acTL 时会按动画帧迭代，与 GifDecoder 的 `into_frames` 契约一致）。
// ---------------------------------------------------------------------------

mod apng_anim {
    use super::*;

    /// 会话槽数：播放中的图 + 预读邻居并存。
    const SESSION_SLOTS: usize = 2;
    /// 动画帧最小展示时长（毫秒）：0-delay 动图防忙转。单帧 APNG 走静态（delay=0）。
    pub const MIN_DELAY_MS: u32 = 10;

    type Decoder = image::codecs::png::ApngDecoder<Cursor<Vec<u8>>>;

    /// 一个 APNG 的解码会话：迭代器游标是唯一划算的缓存粒度，与 GIF 同理。
    struct Session {
        len: u64,
        head: [u8; 32],
        tail: [u8; 32],
        data: Vec<u8>,
        canvas: (u32, u32),
        next_index: u32,
        /// 总帧数。建会话时从 `acTL` 直接读出（见 [`png_frame_count`]）；
        /// 读不出的畸形文件回落到 EOF 惰性发现，代价同旧行为。
        count: Option<u32>,
        iter: Option<image::Frames<'static>>,
        /// 快进跳过的帧（见 [`FrameRing`]）——连续后退不再重解 0..k。
        ring: FrameRing,
    }

    // 安全性：同 gif_anim —— Session 内部全是自持数据，无线程亲和状态；
    // 宿主只在解码线程调用 iv_*，即便另有调用方也被 SESSIONS 的 Mutex 串行化。
    unsafe impl Send for Session {}

    impl Session {
        fn fingerprint(data: &[u8]) -> (u64, [u8; 32], [u8; 32]) {
            let n = data.len();
            let mut head = [0u8; 32];
            let mut tail = [0u8; 32];
            head.copy_from_slice(&data[..n.min(32)]);
            let t = &data[n.saturating_sub(32)..];
            tail[..t.len()].copy_from_slice(t);
            (n as u64, head, tail)
        }

        fn new(data: &[u8]) -> Result<Self, ImageError> {
            let dec = open(data)?;
            let canvas = png_dimensions(data)
                .ok_or_else(|| ImageError::IoError(std::io::ErrorKind::InvalidData.into()))?;
            let iter = image::AnimationDecoder::into_frames(dec);
            let (len, head, tail) = Self::fingerprint(data);
            Ok(Self {
                len,
                head,
                tail,
                data: data.to_vec(),
                canvas,
                next_index: 0,
                count: png_frame_count(data),
                iter: Some(iter),
                ring: FrameRing::new(),
            })
        }

        fn matches(&self, data: &[u8]) -> bool {
            let fp = Self::fingerprint(data);
            self.len == fp.0 && self.head == fp.1 && self.tail == fp.2
        }
    }

    fn open(data: &[u8]) -> Result<Decoder, ImageError> {
        image::codecs::png::PngDecoder::new(Cursor::new(data.to_vec()))?.apng()
    }

    /// PNG/APNG 尺寸：8 字节签名后第一块是 IHDR，宽高在固定偏移（大端）。
    /// `ApngDecoder` 不实现 `ImageDecoder`，直接读 IHDR 最稳。
    fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        if data.len() < 24 || &data[0..8] != SIG {
            return None;
        }
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        Some((w, h))
    }

    /// APNG 总帧数：`acTL` 数据段的前 4 字节（大端）。PNG 块是
    /// `长度 | 类型 | 数据 | CRC` 的定长框，每轮至少推进 12 字节，所以逐块跳
    /// 一定按文件长度收敛——畸形长度只会提前 `None`，不会原地打转。
    /// 读不到（静态 PNG、截断、长度撒谎）返回 `None`，回落到旧的 EOF 惰性发现。
    pub(super) fn png_frame_count(data: &[u8]) -> Option<u32> {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        if data.len() < 8 || &data[0..8] != SIG {
            return None;
        }
        let mut pos = 8usize;
        loop {
            // 块框至少要有 4 长度 + 4 类型才读得出名字（saturating：pos 可能已经跑出文件）。
            if data.len().saturating_sub(pos) < 8 {
                return None;
            }
            let len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
            let ty = &data[pos + 4..pos + 8];
            if ty == b"acTL" {
                let d = pos.checked_add(8)?;
                if data.len().saturating_sub(d) < 4 {
                    return None;
                }
                let n = u32::from_be_bytes(data[d..d + 4].try_into().ok()?);
                return (n > 0).then_some(n);
            }
            if ty == b"IEND" {
                return None;
            }
            // 12 = 长度域 + 类型域 + CRC；跳过数据段。溢出 = 长度撒谎 = 未知。
            pos = pos.checked_add(12)?.checked_add(len)?;
        }
    }

    /// 会话池。解码线程是唯一使用方，但宿主契约不保证这一点，用 Mutex 兜底。
    static SESSIONS: std::sync::Mutex<Vec<Session>> = std::sync::Mutex::new(Vec::new());

    fn sessions() -> std::sync::MutexGuard<'static, Vec<Session>> {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 查询阶段：画布尺寸 + 帧数（`acTL` 读出来的；读不出 = 0）。不解码、不动游标。
    pub fn query(data: &[u8]) -> Result<((u32, u32), u32), i32> {
        let mut pool = sessions();
        if let Some(s) = pool.iter_mut().find(|s| s.matches(data)) {
            return Ok((s.canvas, s.count.unwrap_or(0)));
        }
        let s = Session::new(data).map_err(|_| ERR_CORRUPT)?;
        let (canvas, count) = (s.canvas, s.count.unwrap_or(0));
        push_session(&mut pool, s);
        Ok((canvas, count))
    }

    type Served = ((u32, u32), Vec<u8>, u32, Option<u32>); // (画布宽高, RGBA, delay_ms, count)
    /// 服务第 `index` 帧（0 起，越界自动回绕）：返回 (画布宽高, RGBA, delay_ms, count)。
    pub fn serve(data: &[u8], index: u32) -> Result<Served, i32> {
        let mut pool = sessions();
        if let Some(pos) = pool.iter().position(|s| s.matches(data)) {
            let s = pool.remove(pos);
            pool.insert(0, s);
        } else {
            let s = Session::new(data).map_err(|_| ERR_CORRUPT)?;
            push_session(&mut pool, s);
        }
        let s = &mut pool[0];
        let mut index = index;

        if let Some(c) = s.count {
            index %= c;
        }

        // 后退命中环：一次解码都不付，直接给成品像素。
        if let Some((px, delay)) = s.ring.get(index) {
            return Ok((s.canvas, px.to_vec(), delay, s.count));
        }

        if s.iter.is_none() || s.next_index > index {
            s.iter = Some(image::AnimationDecoder::into_frames(
                open(&s.data).map_err(|_| ERR_CORRUPT)?,
            ));
            s.next_index = 0;
        }

        loop {
            let mut eof_at: Option<u32> = None;
            while s.next_index < index {
                match s.iter.as_mut().unwrap().next() {
                    // 快进跳过的帧正是稍后后退要的——留住它，别解第二遍。
                    Some(Ok(f)) => {
                        s.ring.put(
                            s.next_index,
                            f.buffer().as_raw(),
                            f.delay().numer_denom_ms().0.max(MIN_DELAY_MS),
                        );
                        s.next_index += 1;
                    }
                    // 解错的帧不进环：环里的像素必须是能直接上屏的成品。
                    Some(_) => s.next_index += 1,
                    None => {
                        eof_at = Some(s.next_index);
                        break;
                    }
                }
            }
            match eof_at {
                None => break,
                Some(c) => {
                    s.count = Some(c.max(1));
                    s.iter = None;
                    index %= s.count.unwrap();
                    s.iter = Some(image::AnimationDecoder::into_frames(
                        open(&s.data).map_err(|_| ERR_CORRUPT)?,
                    ));
                    s.next_index = 0;
                }
            }
        }

        let frame = match s.iter.as_mut().unwrap().next() {
            Some(Ok(f)) => {
                s.next_index += 1;
                f
            }
            Some(Err(_)) => return Err(ERR_CORRUPT),
            None => {
                let c = index.max(1);
                s.count = Some(c);
                index %= c;
                s.iter = Some(image::AnimationDecoder::into_frames(
                    open(&s.data).map_err(|_| ERR_CORRUPT)?,
                ));
                s.next_index = 0;
                while s.next_index < index {
                    match s.iter.as_mut().unwrap().next() {
                        Some(Ok(_)) => s.next_index += 1,
                        _ => return Err(ERR_CORRUPT),
                    }
                }
                match s.iter.as_mut().unwrap().next() {
                    Some(Ok(f)) => {
                        s.next_index += 1;
                        f
                    }
                    _ => return Err(ERR_CORRUPT),
                }
            }
        };

        let mut delay = frame.delay().numer_denom_ms().0;
        if delay < MIN_DELAY_MS {
            delay = MIN_DELAY_MS;
        }
        Ok((s.canvas, frame.buffer().to_vec(), delay, s.count))
    }

    fn push_session(pool: &mut Vec<Session>, s: Session) {
        if pool.len() >= SESSION_SLOTS {
            pool.pop();
        }
        pool.insert(0, s);
    }
}

/// `image` 的错误细分到 ABI 错误码——宿主 UI 每类给不同文案，不能整个吞成"打不开"。
///
/// 注：image 0.25 没有独立的内存不足变体，分配失败落到 `Limits` / `IoError`。
fn map_err(e: &ImageError) -> i32 {
    match e {
        ImageError::Decoding(_) => ERR_CORRUPT,
        ImageError::Encoding(_) => ERR_UNSUPPORTED,
        ImageError::Unsupported(_) => ERR_UNSUPPORTED,
        ImageError::Limits(_) => ERR_TOO_LARGE,
        ImageError::IoError(_) => ERR_UNRECOGNIZED,
        _ => ERR_CORRUPT,
    }
}

// ---------------------------------------------------------------------------
// 单元测试：每种格式都做一次「编码 → 解码」往返，证明库真能用
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    use rupix_abi as kit;

    /// 造一张 4x3 的图：左上角红、其余青，带一个已知像素便于逐字节核对。
    fn sample() -> image::RgbaImage {
        let mut img = image::RgbaImage::new(4, 3);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = if x == 0 && y == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 128, 128, 255])
            };
        }
        img
    }

    /// JPEG 编码器不接受带 alpha 的输入（`Rgba8`），要单独给一份 `Rgb8`。
    fn sample_rgb() -> image::RgbImage {
        let mut img = image::RgbImage::new(4, 3);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = if x == 0 && y == 0 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 128, 128])
            };
        }
        img
    }

    /// 用 image 自带的编码器生成样本字节。
    fn encode(fmt: image::ImageFormat) -> Vec<u8> {
        let mut buf = Vec::new();
        let r: Result<(), image::ImageError> = if fmt == image::ImageFormat::Jpeg {
            sample_rgb().write_to(&mut Cursor::new(&mut buf), fmt)
        } else {
            sample().write_to(&mut Cursor::new(&mut buf), fmt)
        };
        r.unwrap_or_else(|e| panic!("encode {fmt:?} failed: {e}"));
        buf
    }

    /// 单格式往返：probe 命中 → 查询尺寸 → 渲染 → 首像素正确。
    fn roundtrip(fmt: image::ImageFormat, name: &str) {
        let bytes = encode(fmt);

        assert_eq!(
            iv_probe(bytes.as_ptr(), bytes.len(), std::ptr::null()),
            CONFIDENCE_GENERALIST,
            "{name}: probe 未识别"
        );

        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK, "{name}: 查询阶段失败");
        assert_eq!((info.actual_w, info.actual_h), (4, 3), "{name}: 尺寸不符");
        assert_eq!(info.bytes_needed, 4 * 3 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        // 静态格式一律报单帧（GIF 表头数帧之后，查询阶段也没有"未知 0"这个余地了）。
        assert_eq!(info.frame_count, 1, "{name}: 静态格式必须报单帧");
        assert_eq!(info.frame_delay_ms, 0, "{name}: 静态格式必须报 0 时长");

        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK, "{name}: 渲染阶段失败");

        // JPEG 是有损的，且 4x3 这么小的图上 4:2:0 色度子采样会把邻近的青色
        // 渗进红块（实测首像素 240,10,100）—— 所以它只验"能解出正确尺寸"，
        // 像素级的正确性交给下面那些无损格式去保证。
        // GIF 的编码侧（image GifEncoder）同样有损（NeuQuant），像素正确性由
        // 下面用 gif crate 直写调色板的专用 GIF 测试保证。
        let (pr, pg, pb, pa) = (buf[0], buf[1], buf[2], buf[3]);
        assert_eq!(pa, 255, "{name}: alpha 应为不透明");
        if fmt != image::ImageFormat::Jpeg && fmt != image::ImageFormat::Gif {
            assert_eq!([pr, pg, pb, pa], [255, 0, 0, 255], "{name}: 首像素不符");
        }
    }

    #[test]
    fn png_roundtrip() {
        roundtrip(image::ImageFormat::Png, "png");
    }

    #[test]
    fn jpeg_roundtrip() {
        roundtrip(image::ImageFormat::Jpeg, "jpeg");
    }

    #[test]
    fn gif_roundtrip() {
        roundtrip(image::ImageFormat::Gif, "gif");
    }

    #[test]
    fn bmp_roundtrip() {
        roundtrip(image::ImageFormat::Bmp, "bmp");
    }

    #[test]
    fn tiff_roundtrip() {
        roundtrip(image::ImageFormat::Tiff, "tiff");
    }

    #[test]
    fn webp_roundtrip() {
        roundtrip(image::ImageFormat::WebP, "webp");
    }

    /// 在 RIFF/WEBP 容器**尾部**追加一枚 `EXIF` chunk（4CC + u32 **小端**长度 + 数据，
    /// 奇数长度再补一字节），并把 RIFF 自己的总长字段一起加上去——容器自称多大就得有
    /// 多大，不然测的是畸形解析器而不是方向。
    ///
    /// ★ 位置不是随手选的：实测把 EXIF 插在 `VP8L` 之前，`WebPGetInfo` 直接判坏
    ///   （`dims=Err(2)`）——非扩展名的 WebP 要求图像 chunk 打头。`webpmux -set exif`
    ///   也是追加在后，所以这里跟现实里的工具一致。
    fn webp_with_exif(webp: &[u8], orient: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 紧跟 8 字节头
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 一条标签
        tiff.extend_from_slice(&274u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        tiff.extend_from_slice(&orient.to_le_bytes()); // 值在偏移字段低半部
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut chunk = b"EXIF".to_vec();
        chunk.extend_from_slice(&(tiff.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&tiff);
        if tiff.len() % 2 == 1 {
            chunk.push(0);
        }
        let mut out = webp.to_vec();
        out.extend_from_slice(&chunk);
        let size = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) + chunk.len() as u32;
        out[4..8].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// m48「竖拍照片不再横躺」在 WebP 上的落点：RIFF 里的 `EXIF` 照实回报，转正交宿主。
    /// 查询与渲染两阶段都走——只在查询报对、渲染改回 1/1 是实现里最容易犯的错。
    #[test]
    fn webp_exif_orientation_is_reported_for_the_host_to_apply() {
        let data = webp_with_exif(&encode(image::ImageFormat::WebP), 6);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK,
            "带 EXIF chunk 的 WebP，查询阶段就该过"
        );
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(
            (info.orientation, info.already_oriented),
            (6, 0),
            "读到 6 就报 6，并声明自己没转"
        );

        let mut buf = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK,
            "libwebp 应照旧解得动这张图"
        );
        assert_eq!(
            (info.orientation, info.already_oriented),
            (6, 0),
            "渲染阶段不许改口"
        );
        assert_eq!(&buf[..4], &[255, 0, 0, 255], "插 chunk 不许动像素");

        // 对照组：没插 EXIF 的同一张 → 1/0，与历史行为逐字节相同。
        let plain = encode(image::ImageFormat::WebP);
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                plain.as_ptr(),
                plain.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!((info.orientation, info.already_oriented), (1, 0));
    }

    #[test]
    fn pnm_roundtrip() {
        roundtrip(image::ImageFormat::Pnm, "pnm");
    }

    /// 噪声数据必须被拒绝，且错误码是"不识别"而不是崩。
    #[test]
    fn garbage_is_rejected() {
        let garbage = vec![0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        // 诊断输出：噪声被误判成了什么格式（若不是 Err，说明魔数表过于宽松）。
        println!("guess(garbage) = {:?}", image::guess_format(&garbage));
        assert_eq!(
            iv_probe(garbage.as_ptr(), garbage.len(), std::ptr::null()),
            0
        );

        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            garbage.as_ptr(),
            garbage.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        // 拒绝即可：不识别 / 不支持 / 损坏 都算正确拒绝，唯独不能返回成功。
        assert!(
            c == ERR_UNRECOGNIZED || c == ERR_UNSUPPORTED || c == ERR_CORRUPT,
            "噪声数据被接受，返回 {c}"
        );
    }

    /// ★ v3 起宿主传扩展名，TGA（无魔数）按 ext 认领；魔数嗅探依旧不认识它。
    #[test]
    fn tga_is_claimed_by_extension() {
        let bytes = encode(image::ImageFormat::Tga);
        // 无 ext：魔数嗅探不认识。
        assert_eq!(iv_probe(bytes.as_ptr(), bytes.len(), std::ptr::null()), 0);
        // ext = "tga"：认领（大小写不敏感）。
        let ext_tga = c"tga".as_ptr();
        let ext_TGA = c"TGA".as_ptr();
        assert_eq!(iv_probe(bytes.as_ptr(), bytes.len(), ext_tga), 100);
        assert_eq!(iv_probe(bytes.as_ptr(), bytes.len(), ext_TGA), 100);

        // render 按 ext 路由：解出正确内容。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            ext_tga,
        );
        assert_eq!(c, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            ext_tga,
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(
            [buf[0], buf[1], buf[2], buf[3]],
            [255, 0, 0, 255],
            "首像素应为红"
        );

        // 坏 TGA（有扩展名认领、内容损坏）→ render 报损坏，probe 不崩。
        let bad = *b"tgajunk";
        assert_eq!(iv_probe(bad.as_ptr(), bad.len(), ext_tga), 100);
        let c = iv_render(
            bad.as_ptr(),
            bad.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            ext_tga,
        );
        assert_ne!(c, ERR_OK, "坏 TGA 不该解码成功");
    }

    /// GIF：逐帧解码、惰性数帧、回绕、delay 语义。
    #[test]
    fn gif_frames_play_and_wrap() {
        // 3 帧 4x3 动画：第 i 帧整帧颜色 = [i, 128, 128]。
        let bytes = encode_gif(&[[0u8, 128, 128], [1, 128, 128], [2, 128, 128]]);
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();

        // 查询：尺寸正确；count 由表头遍历在第一帧之前就给出（不再要等撞 EOF）。
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(info.frame_count, 3, "查询阶段就该知道总共 3 帧");

        // 逐帧渲染：像素随帧变；每一帧的 count 都是 3，宿主不会写出 `帧 1/?`。
        for i in 0..3u32 {
            let req = IvRenderReq {
                target_w: 0,
                target_h: 0,
                budget_bytes: 0,
                flags: 0,
                frame_index: i,
                page_index: 0,
            };
            let mut info = IvRenderOut::default();
            let q = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(q, ERR_OK);
            let mut buf = vec![0u8; info.bytes_needed as usize];
            let r = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(r, ERR_OK, "帧 {i} 渲染失败");
            assert_eq!(
                [buf[0], buf[1], buf[2]],
                [i as u8, 128, 128],
                "帧 {i} 像素不符"
            );
            assert_eq!(info.frame_delay_ms, 100, "帧 {i} delay 应为 100ms");
            assert_eq!(info.frame_count, 3, "帧 {i} 也该报得出总帧数");
        }

        // 宿主继续要帧 3：已知 3 帧 → 直接取模回绕到帧 0。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 3,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.frame_count, 3, "回绕后仍要报出总帧数");
        assert_eq!([buf[0], buf[1], buf[2]], [0, 128, 128], "帧 3 应回绕到帧 0");

        // 已知帧数后越界回绕：index=5 → 5 % 3 = 第 2 帧。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 5,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.frame_count, 3);
        assert_eq!([buf[0], buf[1], buf[2]], [2, 128, 128]);
    }

    /// 单帧 GIF 是静态图：表头数帧第一轮就报 count=1、delay=0，宿主不挂动画。
    #[test]
    fn gif_single_frame_is_static() {
        let bytes = encode_gif(&[[255u8, 0, 0]]);
        // 第一轮：count 未知（0），delay 钳到 ≥10ms（此时宿主还不知道它是静态的）。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.frame_count, 1, "表头数帧第一轮就报单帧");
        assert_eq!(info.frame_delay_ms, 0, "单帧 GIF 第一轮就该判静态");
        assert_eq!([buf[0], buf[1], buf[2]], [255, 0, 0]);

        // 第二轮：宿主多要一帧 → 取模回绕到帧 0，仍是静态。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 1,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.frame_count, 1, "回绕后仍报单帧");
        assert_eq!(info.frame_delay_ms, 0, "单帧 GIF 必须报静态");
        assert_eq!([buf[0], buf[1], buf[2]], [255, 0, 0], "帧 1 应回绕到帧 0");
    }

    /// 透明帧必须叠加在前一帧之上（而非裸帧/黑底）。用 image crate 自身的 `Frames`
    /// 迭代器作基准：我们的 `gif_anim::serve` 只是它的薄封装，契约就是「逐帧结果与
    /// 上游合成逐字节一致」——透明/disposal 语义由上游负责，我们只锁住「封装没改坏」。
    #[test]
    fn gif_transparent_frames_composite() {
        let bytes = encode_gif_transparent();

        // 基准：直接用 image crate 解同一份字节，逐帧收集 RGBA（已合成完整画布）。
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let reference: Vec<Vec<u8>> = image::AnimationDecoder::into_frames(dec)
            .map(|f| f.unwrap().buffer().to_vec())
            .collect();
        assert_eq!(reference.len(), 3, "基准应解出 3 帧");

        let mut served: Vec<Vec<u8>> = Vec::new();
        for i in 0..3u32 {
            let req = IvRenderReq {
                target_w: 0,
                target_h: 0,
                budget_bytes: 0,
                flags: 0,
                frame_index: i,
                page_index: 0,
            };
            let mut info = IvRenderOut::default();
            let q = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(q, ERR_OK, "帧 {i} 查询失败");
            let mut buf = vec![0u8; info.bytes_needed as usize];
            let r = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(r, ERR_OK, "帧 {i} 渲染失败");
            assert_eq!(
                buf, reference[i as usize],
                "帧 {i} 与上游合成结果不一致（透明叠加被改坏）"
            );
            served.push(buf);
        }

        // 像素级锚点：第 2 帧 = 红底 + 左上 2x2 绿块 + 右下 2x2 蓝块。
        let px = |p: usize| &served[2][p * 4..p * 4 + 4];
        assert_eq!(px(0), &[0, 255, 0, 255], "第2帧 (0,0) 应为绿");
        assert_eq!(px(3), &[255, 0, 0, 255], "第2帧 (3,0) 应为红（透出底图）");
        assert_eq!(px(11), &[0, 0, 255, 255], "第2帧 (3,2) 应为蓝");
    }

    /// 后退走环：连续后退不再重解 0..k。★ 这里锁的是**像素逐字节一致**——环按帧号
    /// 索引，错一位就是上一帧/下一帧盖在屏上，而那在屏幕上完全"像对的"，只有比对拦得住。
    #[test]
    fn gif_backward_steps_serve_from_ring_and_match() {
        let bytes = encode_gif(&[
            [1, 0, 0],
            [2, 0, 0],
            [3, 0, 0],
            [4, 0, 0],
            [5, 0, 0],
            [6, 0, 0],
        ]);
        let serve = |i: u32| -> Vec<u8> {
            let req = IvRenderReq {
                target_w: 0,
                target_h: 0,
                budget_bytes: 0,
                flags: 0,
                frame_index: i,
                page_index: 0,
            };
            let mut info = IvRenderOut::default();
            let q = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(q, ERR_OK, "帧 {i} 查询失败");
            let mut buf = vec![0u8; info.bytes_needed as usize];
            let r = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(r, ERR_OK, "帧 {i} 渲染失败");
            buf
        };

        // 基准：顺序播一遍。游标直进，一帧都不该进环——播放路径零额外内存是这条设计的
        // 前提，播完发现环里有东西就说明我们把开销塞回了热路径。
        let truth: Vec<Vec<u8>> = (0..6u32).map(serve).collect();
        assert_eq!(gif_anim::ring_frames(&bytes), Some(0), "顺序播放不该填环");

        // 后退第一跳必然 miss（这几帧从没被快进跳过），它顺手把 0..3 存进环。
        assert_eq!(serve(4), truth[4], "后退首帧像素不对");
        assert!(
            gif_anim::ring_frames(&bytes).unwrap_or(0) >= 3,
            "快进跳过的帧要留在环里"
        );

        // 之后的后退走命中路径，像素必须与正解逐字节相同。
        for i in [3u32, 2, 1] {
            assert_eq!(serve(i), truth[i as usize], "后退第 {i} 帧像素不对");
        }
    }

    /// 造一段带透明帧的多帧动画：0=红 1=绿 2=蓝 3=透明占位（标记透明索引）。
    /// 帧0 全红；帧1 左上 2x2 绿、其余透明；帧2 右下 2x2 蓝、其余透明。
    /// 预期合成：帧1 = 红+绿，帧2 = 红+绿+蓝。
    fn encode_gif_transparent() -> Vec<u8> {
        use std::borrow::Cow;
        let palette = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 4, 3, &palette).unwrap();
            let f0 = gif::Frame {
                width: 4,
                height: 3,
                delay: 10,
                buffer: Cow::Owned(vec![0u8; 12]),
                transparent: None,
                ..Default::default()
            };
            let mut b1 = vec![3u8; 12];
            b1[0] = 1;
            b1[1] = 1;
            b1[4] = 1;
            b1[5] = 1;
            let f1 = gif::Frame {
                width: 4,
                height: 3,
                delay: 10,
                buffer: Cow::Owned(b1),
                transparent: Some(3),
                ..Default::default()
            };
            let mut b2 = vec![3u8; 12];
            b2[6] = 2;
            b2[7] = 2;
            b2[10] = 2;
            b2[11] = 2;
            let f2 = gif::Frame {
                width: 4,
                height: 3,
                delay: 10,
                buffer: Cow::Owned(b2),
                transparent: Some(3),
                ..Default::default()
            };
            enc.write_frame(&f0).unwrap();
            enc.write_frame(&f1).unwrap();
            enc.write_frame(&f2).unwrap();
        }
        out
    }

    /// APNG：逐帧解码、惰性数帧、回绕、delay。镜像 gif_frames_play_and_wrap。
    /// 样本 3 帧全幅纯色：红→绿→蓝（APNG 每帧整幅覆盖，合成后各帧独立可见）。
    #[test]
    fn apng_frames_play_and_wrap() {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../testdata/golden/apng_test.png"
        ));

        // 查询：尺寸正确；count 直接来自 `acTL`，第一帧之前就已知。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(info.frame_count, 3, "APNG 查询阶段就该从 acTL 读出总帧数");

        let expect = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]];
        for i in 0..3u32 {
            let req = IvRenderReq {
                frame_index: i,
                ..req
            };
            let mut info = IvRenderOut::default();
            let q = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(q, ERR_OK, "APNG 帧 {i} 查询失败");
            let mut buf = vec![0u8; info.bytes_needed as usize];
            let r = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(r, ERR_OK, "APNG 帧 {i} 渲染失败");
            assert_eq!(&buf[0..3], &expect[i as usize][..], "APNG 帧 {i} 像素不符");
            assert!(info.frame_delay_ms >= 10, "APNG 帧 {i} delay 应钳到 ≥10ms");
            assert_eq!(info.frame_count, 3, "APNG 帧 {i} 也该报得出总帧数");
        }

        // 帧 3：已知 3 帧 → 取模回绕到帧 0。
        let req = IvRenderReq {
            frame_index: 3,
            ..req
        };
        let mut info = IvRenderOut::default();
        iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.frame_count, 3, "APNG 回绕后仍要报出总帧数");
        assert_eq!(&buf[0..3], &[255, 0, 0], "APNG 帧 3 应回绕到帧 0");
    }

    /// 表头数帧的两条路：真样本要数得准，畸形字节要**安静地**报不出（不 panic、不挂死）。
    /// 数不出来只是让宿主少个分母，不是错误。
    #[test]
    fn frame_counts_are_exact_and_survive_hostile_bytes() {
        // 真样本
        let gif = encode_gif(&[[0u8, 128, 128], [1, 128, 128], [2, 128, 128]]);
        assert_eq!(gif_anim::count_frames(&gif), Some(3));
        let apng = *include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../testdata/golden/apng_test.png"
        ));
        assert_eq!(apng_anim::png_frame_count(&apng), Some(3));

        // 空 / 只有签名：读不出，回 None。
        assert_eq!(gif_anim::count_frames(&[]), None);
        assert_eq!(gif_anim::count_frames(b"GIF89a"), None);
        assert_eq!(apng_anim::png_frame_count(&[]), None);
        assert_eq!(apng_anim::png_frame_count(b"\x89PNG\r\n\x1a\n"), None);

        // 任意前缀都不许 panic（越界切片 / 减法溢出会当场炸给宿主）。
        for c in 0..=apng.len() {
            let _ = apng_anim::png_frame_count(&apng[..c]);
        }
        for c in 0..=gif.len() {
            let _ = gif_anim::count_frames(&gif[..c]);
        }
        // APNG 的块框是定长的：签名 8 + IHDR(12+13) → `acTL` 块从 33 起，数据从 41 起。
        // 于是"读到第几字节算读得到帧数"是可精确断言的。
        assert_eq!(
            apng_anim::png_frame_count(&apng[..40]),
            None,
            "acTL 块名还没读全"
        );
        assert_eq!(
            apng_anim::png_frame_count(&apng[..44]),
            None,
            "acTL 数据不足 4 字节"
        );
        assert_eq!(
            apng_anim::png_frame_count(&apng[..45]),
            Some(3),
            "帧数那 4 字节读全就该报"
        );

        // 长度域撒谎的 PNG 块：逐块跳必须按文件长度收敛，不许原地打转、不许越界。
        let mut lying = b"\x89PNG\r\n\x1a\n".to_vec();
        lying.extend_from_slice(&u32::MAX.to_be_bytes());
        lying.extend_from_slice(b"IDAT");
        lying.resize(lying.len() + 8, 0);
        assert_eq!(apng_anim::png_frame_count(&lying), None);
    }

    /// 造一段多帧动画：gif crate 直写精确索引调色板（image 的 GifEncoder 走
    /// NeuQuant 有损量化，4x3 小图纯色会被毁色，不能用来当测试基准）。
    /// 每帧整帧纯色、无 disposal 纠缠，delay 统一 100ms。
    fn encode_gif(colors: &[[u8; 3]]) -> Vec<u8> {
        use std::borrow::Cow;
        // 全局调色板：颜色逐项放入 + 0 填充到 8 项（GIF 要求调色板是 2 的幂）。
        let mut palette = vec![0u8; 8 * 3];
        for (i, c) in colors.iter().enumerate() {
            palette[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 4, 3, &palette).unwrap();
            for (i, _) in colors.iter().enumerate() {
                let frame = gif::Frame {
                    width: 4,
                    height: 3,
                    delay: 10, // GIF delay 单位 10ms → 100ms
                    buffer: Cow::Owned(vec![i as u8; 4 * 3]),
                    ..Default::default()
                };
                enc.write_frame(&frame).unwrap();
            }
        }
        out
    }

    /// budget 生效：4x3 只要 48 字节，给 8 字节预算必须报太大。
    #[test]
    fn budget_is_enforced() {
        let bytes = encode(image::ImageFormat::Png);
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 8,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_TOO_LARGE);
    }

    /// 缓冲不足必须是明确的错误码，不能越界写。
    #[test]
    fn short_buffer_is_rejected() {
        let bytes = encode(image::ImageFormat::Png);
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let mut small = vec![0u8; 8];
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            small.as_mut_ptr(),
            small.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_BUFFER_TOO_SMALL);
    }

    /// 截断的文件在**渲染阶段**必须报错。
    ///
    /// 注意别在查询阶段断言：查询只读 PNG 的 IHDR（文件头），IDAT 压根没碰，
    /// 所以截断文件的查询会正常返回尺寸——这是两阶段设计的固有特性，不是 bug。
    #[test]
    fn truncated_png_fails_at_render_stage() {
        let bytes = encode(image::ImageFormat::Png);
        let half = &bytes[..bytes.len() / 2];
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();

        // 查询阶段：尺寸照常回报（头是完整的）。
        let q = iv_render(
            half.as_ptr(),
            half.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK, "截断 PNG 的头仍然完整，查询应成功");
        assert_eq!((info.actual_w, info.actual_h), (4, 3));

        // 渲染阶段：IDAT 被截断，必须失败。
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let c = iv_render(
            half.as_ptr(),
            half.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert!(
            c == ERR_CORRUPT || c == ERR_UNRECOGNIZED || c == ERR_UNSUPPORTED,
            "截断 PNG 渲染应失败，实为 {c}"
        );
    }

    /// 自述里声明的扩展名必须都被宿主看见（分号分隔、小写、不带点）。
    #[test]
    fn declares_a_broad_extension_list() {
        let info = unsafe { &*iv_decoder_info() };
        assert_eq!(info.abi_version, IV_ABI_VERSION);
        let exts = unsafe { std::ffi::CStr::from_ptr(info.extensions) }
            .to_str()
            .unwrap()
            .to_string();
        for must in ["apng", "webp", "bmp", "ico", "dds", "pnm", "tga"] {
            assert!(
                exts.split(';').any(|e| e == must),
                "扩展名清单缺 {must}：{exts}"
            );
        }
        // JPEG / TIFF / PNG / GIF 家族后缀必须由各自冠军 DLL 独占认领（G1 实测 zune-jpeg
        // 快 1.80×、tiff crate 直调快 1.24×、png crate 直调快 1.19×、gif crate 直调快 1.18×）。
        // core 只在嗅探兜底里以 50 置信度补位，绝不能再出现在按扩展名分派的候选里——
        // 否则 DLL 路径字典序会让 core 抢先，实测提速静默失效。
        // 注意 `apng` 不在让出之列：PNG 冠军对动画主动让位，动画路线是 core 自己的功能；
        // GIF 冠军只服务第 0 帧，frame ≥1 的播放也是 core 的会话缓存兜底。
        for gone in [
            "jpg", "jpeg", "jpe", "jfif", "jif", "jfi", "exif", "tif", "tiff", "png", "gif",
        ] {
            assert!(
                !exts.split(';').any(|e| e == gone),
                "core 不该再认领 {gone}：{exts}"
            );
        }
        assert_eq!(info.is_vector, 0);
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 5);
    }

    /// 渲染成功后 last-error 必须为空（宿主据此判断是否有附加信息）。
    #[test]
    fn success_clears_last_error() {
        let bytes = encode(image::ImageFormat::Png);
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let mut buf = vec![0u8; 48];
        assert_eq!(
            iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        let mut ebuf = [0i8; 16];
        assert_eq!(
            unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) },
            1
        );
    }

    /// 在 RIFF/WEBP 尾部追加一枚 `ICCP` chunk（与 EXIF 同一套长度/补位规则）。
    fn webp_with_icc(webp: &[u8], profile: &[u8]) -> Vec<u8> {
        let mut chunk = b"ICCP".to_vec();
        chunk.extend_from_slice(&(profile.len() as u32).to_le_bytes());
        chunk.extend_from_slice(profile);
        if profile.len() % 2 == 1 {
            chunk.push(0);
        }
        let mut out = webp.to_vec();
        out.extend_from_slice(&chunk);
        let size = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) + chunk.len() as u32;
        out[4..8].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// m53-J：WebP 的 `ICCP` 在查询/渲染两阶段都申报，且尾部字节与原配置相同。
    #[test]
    fn webp_icc_profile_is_declared_and_appended_after_pixels() {
        let profile: Vec<u8> = (0..256u32).map(|i| (i * 17 % 241) as u8).collect();
        let data = webp_with_icc(&encode(image::ImageFormat::WebP), &profile);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req,
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
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK,
            "追加 ICCP 不许让 libwebp 解不动"
        );
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        assert_eq!(&buf[info.bytes_needed as usize..], &profile[..]);

        // 没有 ICCP 的同一张：申报 0。
        let plain = encode(image::ImageFormat::WebP);
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                plain.as_ptr(),
                plain.len(),
                &req,
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
