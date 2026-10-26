//! RuPix 外挂解码器：**裸像素数据**（rawpx）——参数化解码。
//!
//! `.raw` / `.rgba` / `.gray` 这类文件**没有魔数、没有头**：宽、高、像素布局只能
//! 由用户提供（XnView 同样弹参数框）。因此本解码器不实现常规的 `iv_render` 语义
//! （恒返 `ERR_UNRECOGNIZED`），而是实现 ABI v4 的**可选导出**
//! `iv_render_raw(data, len, width, height, layout, req, out, out_cap, info)`：
//! 宿主探测到该导出、弹参数窗收集宽/高/布局后调用。见 `docs/解码器开发规范.md` §十二。
//!
//! # 布局
//!
//! `layout` 复用 `PF_*` 常量：紧排（GRAY8/RGB8/RGBA8/GRAY16/RGB16/RGBA16 +
//! 仅输入用的 PF_BGRA8，行紧排、小端）+ YUV（PF_YUY2/PF_UYVY packed 4:2:2、
//! PF_I420/PF_YV12/PF_NV12 planar 4:2:0）。统一约定：**BT.601、limited range**
//! （见 abi 头注释与规范 §十二），YUV 布局要求宽（4:2:2）/宽高（4:2:0）为偶数。
//! 解码器内完成 → RGBA8 的转换，输出传输格式恒为 PF_RGBA8（宿主 to_rgba8 零改动）。
//!
//! # 扩展名
//!
//! 与清单一致的对应：`raw;rgb;rgba;gray;gry;bgra`（紧排）+ `yuv;i420;yv12;nv12;
//! yuy2;uyvy`（YUV；`.yuv` 不标布局，布局仍由参数窗选择）。
//!
//! 所有导出函数都套在 panic gate 里：任何内部 panic 都会被 `catch_unwind`
//! 拦截并转成错误码，宿主因此可以安全地隔离坏解码器。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印“正在创建库 ... 和对象 ...”，rustc 默认把
// 链接器 stdout 当作 lint 报出。这里显式静音，以满足“零警告”要求。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;

// ---------------------------------------------------------------------------
// 固定导出（ABI 自述 + 三个样板函数）
// ---------------------------------------------------------------------------

#[cfg(feature = "standalone")]
export_decoder! {
    id: "rawpx",
    name: "Raw Pixel Data",
    extensions: "raw;rgb;rgba;gray;gry;bgra;yuv;i420;yv12;nv12;yuy2;uyvy",
    description: "裸像素数据：按用户给定的宽/高/布局解码（无魔数无头）",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB），与 core 解码器同一道闸。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render（常规语义不适用，见模块头）
// ---------------------------------------------------------------------------

/// **无魔数可嗅探 → 恒不认领。**
///
/// ★ 这是`.raw` 仲裁的另一半（宿主侧在 `decoders::select_candidate`）：`.raw` 同时
/// 是相机 RAW 的后缀与最常见的裸像素 dump 后缀，若这里"按扩展名抢认"，常规双击
/// 打开 `.raw` 就会被判给本解码器，然后按用户上一次的宽高把任意二进制解成一屏雪花
/// ——比老实报"未知格式"糟得多。所以本解码器只认**显式给了参数**的请求
/// （宿主 `candidate_for_ext(_, require_raw=true)` → 直接调 `iv_render_raw`）。
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(_data: *const u8, _len: usize, _ext: *const c_char) -> i32 {
    probe_impl(_data, _len, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(_data: *const u8, _len: usize, _ext: *const c_char) -> i32 {
    guard(|| 0)
}
/// 常规渲染语义不适用于无头文件：参数（宽/高/布局）只能走 iv_render_raw。
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render(
    _data: *const u8,
    _len: usize,
    _req: *const IvRenderReq,
    _out: *mut u8,
    _out_cap: usize,
    _info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_impl(_data, _len, _req, _out, _out_cap, _info, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn render_impl(
    _data: *const u8,
    _len: usize,
    _req: *const IvRenderReq,
    _out: *mut u8,
    _out_cap: usize,
    _info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    guard(|| {
        set_last_error("裸数据需要宽/高/布局参数（请走 iv_render_raw）");
        ERR_UNRECOGNIZED
    })
}
// ---------------------------------------------------------------------------
// 导出：iv_render_raw（ABI v4 内可选导出，见规范 §十二）
// ---------------------------------------------------------------------------

/// 裸像素参数化解码。`width`/`height`/`layout` 由宿主向用户收集后传入；
/// 其余参数与 `iv_render` 同义（两阶段：`out=NULL` 查询回填，非空渲染）。
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render_raw(
    data: *const u8,
    len: usize,
    width: u32,
    height: u32,
    layout: u32,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
) -> i32 {
    render_raw_impl(data, len, width, height, layout, req, out, out_cap, info)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
// 9 个参数是 ABI 契约本身定的形态（裸数据的宽/高/布局只能显式传），
// 减参数就得自己发明一个结构体——那等于给契约加第二份定义。
#[allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]
pub fn render_raw_impl(
    data: *const u8,
    len: usize,
    width: u32,
    height: u32,
    layout: u32,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
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
        if width == 0 || height == 0 {
            set_last_error("宽和高都不能为 0");
            return ERR_CORRUPT;
        }
        let npixels = width as u64 * height as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码器上限");
            return ERR_TOO_LARGE;
        }
        let src_len = match expected_len(width, height, layout) {
            Some(n) => n,
            None => {
                set_last_error("该布局不支持");
                return ERR_UNSUPPORTED;
            }
        };
        if src_len == 0 {
            set_last_error("YUV 布局要求宽为偶数（4:2:0 还要求高为偶数）");
            return ERR_CORRUPT;
        }
        if src_len != len as u64 {
            set_last_error(&format!(
                "文件 {len} 字节与 {width}x{height} {layout} 布局所需的 {src_len} 字节不符"
            ));
            return ERR_CORRUPT;
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

        // 查询阶段：out 为空指针，只回报尺寸/格式，不解码。
        if out.is_null() {
            fill_info(info, width, height, bytes_needed);
            return ERR_OK;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // 渲染阶段：layout → RGBA8（行紧排、非预乘、小端），与 ABI 契约一致。
        let src = unsafe { std::slice::from_raw_parts(data, len) };
        let dst = unsafe { std::slice::from_raw_parts_mut(out, out_cap) };
        convert_to_rgba8(src, width, height, layout, dst);
        fill_info(info, width, height, bytes_needed);
        ERR_OK
    })
}
/// 该布局下 w×h 的文件字节数。`None` = 不支持的布局；
/// `Some(0)` = 布局支持但宽（4:2:2）/宽高（4:2:0）非偶数。
fn expected_len(w: u32, h: u32, layout: u32) -> Option<u64> {
    let (w, h) = (w as u64, h as u64);
    match layout {
        PF_GRAY8 => Some(w * h),
        PF_RGB8 => Some(w * h * 3),
        PF_RGBA8 | PF_BGRA8 => Some(w * h * 4),
        PF_GRAY16 => Some(w * h * 2),
        PF_RGB16 => Some(w * h * 6),
        PF_RGBA16 => Some(w * h * 8),
        // packed 4:2:2：水平 2 像素共用一组 UV，每像素平均 2 字节。
        PF_YUY2 | PF_UYVY => {
            if w % 2 != 0 {
                Some(0)
            } else {
                Some(w * h * 2)
            }
        }
        // planar 4:2:0：2×2 像素共用一组 UV，共 3/2 字节每像素。
        PF_I420 | PF_YV12 | PF_NV12 => {
            if w % 2 != 0 || h % 2 != 0 {
                Some(0)
            } else {
                Some(w * h * 3 / 2)
            }
        }
        _ => None,
    }
}

/// layout → RGBA8。调用方已保证 `src.len() == w*h*bpp`、`dst.len() >= w*h*4`。
fn convert_to_rgba8(src: &[u8], w: u32, h: u32, layout: u32, dst: &mut [u8]) {
    let n = w as usize * h as usize;
    let d = &mut dst[..n * 4];
    match layout {
        PF_GRAY8 => {
            for (i, &v) in src[..n].iter().enumerate() {
                d[i * 4..i * 4 + 3].copy_from_slice(&[v, v, v]);
                d[i * 4 + 3] = 255;
            }
        }
        PF_RGB8 => {
            for (i, px) in src[..n * 3].chunks_exact(3).enumerate() {
                d[i * 4..i * 4 + 3].copy_from_slice(px);
                d[i * 4 + 3] = 255;
            }
        }
        PF_RGBA8 => d.copy_from_slice(&src[..n * 4]),
        PF_BGRA8 => {
            for (i, px) in src[..n * 4].chunks_exact(4).enumerate() {
                d[i * 4] = px[2];
                d[i * 4 + 1] = px[1];
                d[i * 4 + 2] = px[0];
                d[i * 4 + 3] = px[3];
            }
        }
        PF_GRAY16 => {
            for (i, px) in src[..n * 2].chunks_exact(2).enumerate() {
                let v = u16::from_le_bytes([px[0], px[1]]);
                // >>8 与 /257 等价到 ±1，肉眼不可分；取快的。
                let v8 = (v >> 8) as u8;
                d[i * 4..i * 4 + 3].copy_from_slice(&[v8, v8, v8]);
                d[i * 4 + 3] = 255;
            }
        }
        PF_RGB16 | PF_RGBA16 => {
            let stride = if layout == PF_RGB16 { 6 } else { 8 };
            let channels = if layout == PF_RGB16 { 3 } else { 4 };
            for (i, px) in src[..n * stride].chunks_exact(stride).enumerate() {
                for (c, ch) in px[..channels * 2].chunks_exact(2).enumerate() {
                    d[i * 4 + c] = (u16::from_le_bytes([ch[0], ch[1]]) >> 8) as u8;
                }
                if channels == 3 {
                    d[i * 4 + 3] = 255;
                }
            }
        }
        // packed 4:2:2：偶数像素用自己右侧的 UV，奇数像素用左侧的（同组共用）。
        PF_YUY2 | PF_UYVY => {
            for y in 0..h as usize {
                let row = &src[y * w as usize * 2..];
                for x in 0..w as usize {
                    let (u, v) = if layout == PF_YUY2 {
                        // Y0 U Y1 V：偶像素 U=2x+1、V=2x+3；奇像素 U=2x-1、V=2x+1。
                        if x % 2 == 0 {
                            (row[2 * x + 1], row[2 * x + 3])
                        } else {
                            (row[2 * x - 1], row[2 * x + 1])
                        }
                    } else {
                        // UYVY：U Y0 V Y1。同组两像素共 U/V：偶像素 U=2x、V=2x+2；
                        // 奇像素回到本组左端的 U=2x-2、V=2x。
                        if x % 2 == 0 {
                            (row[2 * x], row[2 * x + 2])
                        } else {
                            (row[2 * x - 2], row[2 * x])
                        }
                    };
                    // YUY2 的 Y 在偶数位（2x）；UYVY 的 Y 在奇数位（2x+1）。
                    let y0 = row[2 * x + if layout == PF_UYVY { 1 } else { 0 }];
                    put_yuv(d, (y * w as usize + x) * 4, y0, u, v);
                }
            }
        }
        // planar 4:2:0：Y 面 + 色度面（2×2 共用一组 UV，取块内左上的色度值）。
        PF_I420 | PF_YV12 | PF_NV12 => {
            let cw = w as usize / 2;
            let ch = h as usize / 2;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let (cx, cy) = (x / 2, y / 2);
                    let (u, v) = match layout {
                        // I420：U 面在前；YV12：V 面在前。
                        PF_I420 => (src[n + cy * cw + cx], src[n + cw * ch + cy * cw + cx]),
                        PF_YV12 => (src[n + cw * ch + cy * cw + cx], src[n + cy * cw + cx]),
                        // NV12：色度交错 UVUV…，每色度行 2*cw 字节。
                        _ => (
                            src[n + cy * cw * 2 + cx * 2],
                            src[n + cy * cw * 2 + cx * 2 + 1],
                        ),
                    };
                    let y0 = src[y * w as usize + x];
                    put_yuv(d, (y * w as usize + x) * 4, y0, u, v);
                }
            }
        }
        _ => unreachable!("expected_len 已滤掉未知布局"),
    }
}

/// BT.601 limited range（Y∈[16,235]、UV∈[16,240]）定点换算（10 位小数，+512 舍入）：
/// R = 1.164·(Y-16) + 1.596·(V-128)，G = … - 0.391·(U-128) - 0.813·(V-128)，
/// B = 1.164·(Y-16) + 2.018·(U-128)，饱和裁剪后写入 dst（Y=235/UV=128 → 精确 255）。
#[inline]
fn put_yuv(dst: &mut [u8], o: usize, y: u8, u: u8, v: u8) {
    let yy = (y as i32 - 16) * 1192; // 1.164 << 10
    let uu = u as i32 - 128;
    let vv = v as i32 - 128;
    let r = (yy + 1634 * vv + 512) >> 10; // 1.596 << 10
    let g = (yy - 400 * uu - 833 * vv + 512) >> 10; // 0.391 / 0.813 << 10
    let b = (yy + 2066 * uu + 512) >> 10; // 2.018 << 10
    dst[o] = r.clamp(0, 255) as u8;
    dst[o + 1] = g.clamp(0, 255) as u8;
    dst[o + 2] = b.clamp(0, 255) as u8;
    dst[o + 3] = 255;
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
    oi.already_oriented = 1;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1;
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// 测试：FFI 级 + 纯函数级
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 跨边界结构体长度必须与宿主冻结一致（照 qoi 的同款断言）。
    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(IV_ABI_VERSION, 5);
        assert_eq!(std::mem::size_of::<IvRenderReq>(), 24);
        assert_eq!(std::mem::size_of::<IvRenderOut>(), 48);
    }

    /// 渲染正确性：3×2 BGRA 缓冲 → RGBA 还原（R/B 交换）。
    #[test]
    fn bgra_to_rgba_swaps_channels() {
        // 像素按 (b,g,r,a) 排列。
        let src: Vec<u8> = vec![
            1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18,
            255,
        ];
        let mut out = vec![0u8; 3 * 2 * 4];
        convert_to_rgba8(&src, 3, 2, PF_BGRA8, &mut out);
        assert_eq!(&out[0..4], &[3, 2, 1, 255]);
        assert_eq!(&out[4..8], &[6, 5, 4, 255]);
    }

    /// GRAY16 归一：0xFFFF → 0xFF，0x8000 → 0x80。
    #[test]
    fn gray16_normalizes_to_8bit() {
        let src: Vec<u8> = [0xFF, 0xFF, 0x00, 0x80].to_vec();
        let mut out = vec![0u8; 2 * 4];
        convert_to_rgba8(&src, 2, 1, PF_GRAY16, &mut out);
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[128, 128, 128, 255]);
    }

    /// RGB8 → RGBA：alpha 补 255。
    #[test]
    fn rgb8_fills_alpha() {
        let src: Vec<u8> = [10, 20, 30].to_vec();
        let mut out = vec![0u8; 4];
        convert_to_rgba8(&src, 1, 1, PF_RGB8, &mut out);
        assert_eq!(out, [10, 20, 30, 255]);
    }

    /// RGBA8 直拷。
    #[test]
    fn rgba8_is_passthrough() {
        let src: Vec<u8> = [1, 2, 3, 4].to_vec();
        let mut out = vec![0u8; 4];
        convert_to_rgba8(&src, 1, 1, PF_RGBA8, &mut out);
        assert_eq!(out, src);
    }

    /// FFI 级：查询阶段（out=NULL）只回填 info；渲染阶段像素就位。
    #[test]
    fn two_phase_ffi_roundtrip() {
        let w = 2u32;
        let h = 2u32;
        let src: Vec<u8> = (10..=25).collect();
        let mut info = IvRenderOut::default();
        let req = IvRenderReq::default();
        // 查询。
        let q = iv_render_raw(
            src.as_ptr(),
            src.len(),
            w,
            h,
            PF_BGRA8,
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(q, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (w, h));
        assert_eq!(info.bytes_needed, (w * h * 4) as u64);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.page_count, 1);
        assert_eq!(info.icc_len, 0);
        // 渲染。
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render_raw(
            src.as_ptr(),
            src.len(),
            w,
            h,
            PF_BGRA8,
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(&buf[0..4], &[12, 11, 10, 13]);
        assert_eq!(&buf[12..16], &[24, 23, 22, 25]);
    }

    /// 文件字节数与 宽×高×bpp 不符 → ERR_CORRUPT。
    #[test]
    fn size_mismatch_is_corrupt() {
        let src = [0u8; 5];
        let mut info = IvRenderOut::default();
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            2,
            2,
            PF_RGBA8,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_CORRUPT);
    }

    /// 未知布局（planar YUV 没有布局号）→ ERR_UNSUPPORTED。
    #[test]
    fn unknown_layout_is_unsupported() {
        let src = [0u8; 4];
        let mut info = IvRenderOut::default();
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            1,
            1,
            99,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_UNSUPPORTED);
    }

    /// 常规 iv_render 恒不识别（引导宿主走参数化导出）。
    #[test]
    fn plain_render_is_unrecognized() {
        let src = [0u8; 4];
        let mut info = IvRenderOut::default();
        let code = iv_render(
            src.as_ptr(),
            src.len(),
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(code, ERR_UNRECOGNIZED);
    }

    /// 无魔数 → probe 恒 0：常规打开不认领（否则会把任意二进制按旧参数解成雪花），
    /// 只接受显式给了宽高布局的 `iv_render_raw` 请求。见 `iv_probe` 的注释。
    #[test]
    fn probe_never_claims_without_magic() {
        let data = [0u8; 4];
        let raw = b"raw\0";
        assert_eq!(
            iv_probe(data.as_ptr(), data.len(), raw.as_ptr() as *const c_char),
            0
        );
        assert_eq!(iv_probe(data.as_ptr(), data.len(), std::ptr::null()), 0);
        // 认领的入口仍在，只是不在 probe 上：挤出 iv_render_raw 必须成功。
        let mut info = IvRenderOut::default();
        let src = [0u8; 4];
        assert_eq!(
            iv_render_raw(
                src.as_ptr(),
                4,
                1,
                1,
                PF_RGBA8,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                &mut info
            ),
            ERR_OK
        );
    }

    /// 预算闸：bytes_needed 超过 budget_bytes → ERR_TOO_LARGE。
    #[test]
    fn budget_is_enforced() {
        let src = [0u8; 2 * 2 * 4];
        let mut info = IvRenderOut::default();
        let req = IvRenderReq {
            budget_bytes: 4,
            ..Default::default()
        };
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            2,
            2,
            PF_RGBA8,
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_TOO_LARGE);
    }

    // ── YUV：BT.601 limited range 换算与各 planar/packed 布局 ──────────

    /// 纯色验证基准：Y=235（limited 白）、U=V=128 → R=G=B=255（±1 舍入）。
    /// Y=16、UV=128 → 0；Y=81、U=90、V=240 → 标准红条（BT.601彩条）。
    #[test]
    fn yuv_to_rgb_known_colors() {
        let mut out = vec![0u8; 4];
        // limited 白。
        put_yuv(&mut out, 0, 235, 128, 128);
        assert_eq!(&out[..3], &[255, 255, 255]);
        // limited 黑。
        put_yuv(&mut out, 0, 16, 128, 128);
        assert_eq!(&out[..3], &[0, 0, 0]);
        // 彩条红：Y=81, U=90, V=240 → (254, 0, 0) ±1。
        put_yuv(&mut out, 0, 81, 90, 240);
        assert!(
            out[0] >= 253 && out[1] <= 1 && out[2] <= 1,
            "red got {:?}",
            &out[..3]
        );
    }

    /// I420 2×2：Y 面 4 字节 + U(1) + V(1)。纯 Y=235/UV=128 → 全白。
    #[test]
    fn i420_2x2_upsamples_chroma() {
        let src = vec![235, 235, 235, 235, 128, 128];
        let mut out = vec![0u8; 16];
        convert_to_rgba8(&src, 2, 2, PF_I420, &mut out);
        for px in out.chunks_exact(4) {
            assert_eq!(&px[..3], &[255, 255, 255]);
        }
    }

    /// YV12 与 I420 的 U/V 面顺序相反：给不同 U/V 验证不混。
    #[test]
    fn yv12_swaps_chroma_planes() {
        // Y=16（黑基准），U=90、V=240：I420 应偏红，YV12 应偏蓝。
        let make = |first: u8, second: u8| vec![16u8, 16, 16, 16, first, second];
        let mut out = vec![0u8; 16];
        convert_to_rgba8(&make(90, 240), 2, 2, PF_I420, &mut out);
        assert!(out[0] > 100 && out[2] < 20, "I420 should be reddish");
        convert_to_rgba8(&make(90, 240), 2, 2, PF_YV12, &mut out);
        assert!(out[2] > 100 && out[0] < 20, "YV12 should be bluish");
    }

    /// NV12 2×2：Y 面 + 交错 UV 面。Y=16、U=90/V=240 → 偏红。
    #[test]
    fn nv12_interleaved_chroma() {
        let src = vec![16, 16, 16, 16, 90, 240];
        let mut out = vec![0u8; 16];
        convert_to_rgba8(&src, 2, 2, PF_NV12, &mut out);
        assert!(out[0] > 100 && out[2] < 20, "NV12 red got {:?}", &out[..3]);
    }

    /// YUY2 2×1：Y0 U Y1 V。两像素共用同一组 UV（Y=235、U=128、V=160 → 偏红）。
    #[test]
    fn yuy2_packed_422() {
        let src = vec![235, 128, 235, 160];
        let mut out = vec![0u8; 8];
        convert_to_rgba8(&src, 2, 1, PF_YUY2, &mut out);
        // 同组两像素共 UV：结果一致；V=160 使 G 低于 R/B（偏品红）。
        assert_eq!(&out[0..3], &[255, 229, 255]);
        assert_eq!(&out[4..7], &[255, 229, 255]);
        // UYVY 2×1：U Y0 V Y1 —— 同样字节另一种排法，结果一致。
        let src = vec![128, 235, 160, 235];
        convert_to_rgba8(&src, 2, 1, PF_UYVY, &mut out);
        assert_eq!(&out[0..3], &[255, 229, 255]);
        assert_eq!(&out[4..7], &[255, 229, 255]);
    }

    /// YUV 奇数尺寸：4:2:2 奇宽 / 4:2:0 奇高 → ERR_CORRUPT（尺寸约定，非崩溃）。
    #[test]
    fn yuv_odd_dimensions_rejected() {
        let src = [0u8; 8];
        let mut info = IvRenderOut::default();
        // YUY2 宽为 3 → 拒绝。
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            3,
            1,
            PF_YUY2,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_CORRUPT);
        // I420 高为 3 → 拒绝（3×3×3/2 本身也不是整数，expected_len 已按约定拦）。
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            2,
            3,
            PF_I420,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_CORRUPT);
    }

    /// YUV 文件长度不符 → ERR_CORRUPT（I420 2×2 应为 6 字节）。
    #[test]
    fn yuv_size_mismatch_is_corrupt() {
        let src = [0u8; 5];
        let mut info = IvRenderOut::default();
        let code = iv_render_raw(
            src.as_ptr(),
            src.len(),
            2,
            2,
            PF_I420,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
        );
        assert_eq!(code, ERR_CORRUPT);
    }
}
