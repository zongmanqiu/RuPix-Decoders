//! RuPix pro 档成员：JPEG XR（`.jxr/.wdp/.hdp`）。
//!
//! `jxr 0.2.1` 纯 Rust；输出请求 `U8(RGBA)`，由库完成色彩/方向/alpha 处理。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use jxr::{ChannelLayout, DecodeRequest, DecodedSamples, JxrView, PixelFormat};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-jxr",
    name: "JPEG XR",
    extensions: "jxr;wdp;hdp",
    description: "JPEG XR（纯 Rust）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

const JXR_MAGIC: [u8; 4] = [0x49, 0x49, 0xBC, 0x01];

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let view = JxrView::parse(data).ok()?;
    let (w, h) = view.info().dimensions();
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let view = JxrView::parse(data).map_err(|_| (ERR_CORRUPT, "JPEG XR 文件头解析失败"))?;
    let (w, h) = view.info().dimensions();
    // ABI 只接受 RGBA8。源图没有 alpha 时 `Rgba` 请求会被 jxr 拒绝，
    // 必须先要 RGB，再由宿主侧约定补齐 alpha；否则普通 24bpp JXR 全部解不开。
    let has_alpha = view.info().alpha.is_some();
    let layout = if has_alpha {
        ChannelLayout::Rgba
    } else {
        ChannelLayout::Rgb
    };
    let mut dec = view.decoder();
    let req = DecodeRequest::new(PixelFormat::U8(layout));
    let img = dec
        .decode(&req)
        .map_err(|_| (ERR_CORRUPT, "JPEG XR 解码失败"))?;
    match img.samples {
        DecodedSamples::U8(px) if has_alpha => Ok((w, h, px)),
        DecodedSamples::U8(px) => Ok((w, h, support::rgba_from_rgb(&px))),
        _ => Err((ERR_UNSUPPORTED, "JPEG XR 输出格式不是 8 位 RGB/RGBA")),
    }
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(&JXR_MAGIC) {
            100
        } else {
            0
        }
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
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

#[allow(clippy::not_unsafe_ptr_arg_deref)]
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
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let Some((w, h)) = dimensions(slice) else {
            set_last_error("不是合法的 JPEG XR 字节特征");
            return ERR_UNRECOGNIZED;
        };
        let (_, _, bytes) = match support::check_dims(w, h, support::budget_of(req)) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, bytes, 1, 0);
            return ERR_OK;
        }
        let (dw, dh, rgba) = match decode(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if (dw, dh) != (w, h) || rgba.len() as u64 != bytes {
            set_last_error("JPEG XR 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_RGB: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/jxr_rgb_64x64.jxr"
    ));

    #[test]
    fn probe_requires_jxr_magic() {
        assert_eq!(iv_probe(JXR_MAGIC.as_ptr(), 4, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }

    #[test]
    fn decodes_no_alpha_jxr_as_rgba() {
        let (w, h, px) = decode(GOLDEN_RGB).expect("64x64 RGB JPEG XR must decode");
        assert_eq!((w, h), (64, 64));
        assert_eq!(px.len(), 64 * 64 * 4);
        assert!(px.chunks_exact(4).all(|p| p[3] == 255));
    }
}
