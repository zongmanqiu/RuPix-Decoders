//! RuPix pro 档成员：JPEG 2000（`.jp2/.j2k/.j2c/.jpc/.jpx/.jpf/.jpt`）。
//!
//! 库是 `hayro-jpeg2000`：纯 Rust、无 unsafe，支持 JP2 容器与裸 codestream。
//! 输出统一转 RGBA8；CMYK 按 Adobe 口径转 RGB。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use hayro_jpeg2000::{ColorSpace, DecodeSettings, DecoderContext, Image};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-jp2",
    name: "JPEG 2000",
    extensions: "jp2;j2k;j2c;jpc;jpx;jpf;jpt",
    description: "JPEG 2000（JP2 容器与裸 codestream）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let image = Image::new(data, &DecodeSettings::default()).ok()?;
    let (w, h) = (image.width(), image.height());
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let image = Image::new(data, &DecodeSettings::default())
        .map_err(|_| (ERR_CORRUPT, "JPEG 2000 文件头解析失败"))?;
    let (w, h) = (image.width(), image.height());
    let cs = image.color_space().clone();
    let alpha = image.has_alpha();
    let mut ctx = DecoderContext::default();
    let decoded = image
        .decode(&mut ctx)
        .map_err(|_| (ERR_CORRUPT, "JPEG 2000 解码失败"))?;
    let raw = decoded.data_u8();
    let px = w as usize * h as usize;
    if px == 0 || raw.len() % px != 0 {
        return Err((ERR_CORRUPT, "JPEG 2000 像素长度与尺寸不符"));
    }
    let comps = raw.len() / px;
    let rgba = match (comps, &cs, alpha) {
        (1, _, _) => support::rgba_from_gray(&raw),
        (2, _, _) => {
            let mut out = Vec::with_capacity(px * 4);
            for c in raw.chunks_exact(2) {
                out.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
            out
        }
        (3, _, _) => support::rgba_from_rgb(&raw),
        (4, ColorSpace::CMYK, _) => support::rgba_from_cmyk(&raw),
        (4, _, true) => support::rgba_passthrough(&raw),
        // 4 通道但库说没有 alpha：按 CMYK 兜底（JP2 里这是最常见的 4 通道语义）。
        (4, _, false) => support::rgba_from_cmyk(&raw),
        _ => return Err((ERR_UNSUPPORTED, "JPEG 2000 的通道组合本解码器不解")),
    };
    Ok((w, h, rgba))
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
        if (len >= 8 && slice.starts_with(b"\x00\x00\x00\x0CjP  "))
            || slice.starts_with(b"\xFF\x4F\xFF\x51")
        {
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
            set_last_error("不是合法的 JPEG 2000 字节特征");
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
            set_last_error("JPEG 2000 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_jp2_or_codestream_magic() {
        assert_eq!(
            iv_probe(b"\x00\x00\x00\x0CjP  ".as_ptr(), 8, std::ptr::null()),
            100
        );
        assert_eq!(
            iv_probe(b"\xFF\x4F\xFF\x51".as_ptr(), 4, std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
