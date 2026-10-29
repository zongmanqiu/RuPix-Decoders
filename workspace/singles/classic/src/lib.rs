//! RuPix classic raster implementation. The published DLL is
//! `workspace/decoders/classic`; this single crate owns the format logic so it
//! can be unit-tested and, later, assembled differently without touching the
//! decoder export table.

#![allow(non_snake_case)]

pub mod iff;
pub mod image_family;
pub mod pict;

use rupix_abi::guard;
use rupix_abi::*;

pub(crate) fn input<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() || len == 0 {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(data, len) })
    }
}

pub(crate) fn finish(
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
) -> i32 {
    if out.is_null() {
        pro_support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
        return ERR_OK;
    }
    pro_support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
}

pub(crate) fn decode_dynamic(
    decoder: impl image::ImageDecoder,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (w, h) = decoder.dimensions();
    pro_support::check_dims(w, h, pro_support::budget_of(req))?;
    let img = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| (ERR_CORRUPT, "经典位图解码失败"))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba.into_raw(), w, h))
}

pub(crate) fn render_scan(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    decode: impl FnOnce(&[u8], *const IvRenderReq) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)>,
) -> i32 {
    guard(|| {
        let Some(bytes) = input(data, len) else {
            set_last_error("内部错误（空数据）");
            return ERR_UNRECOGNIZED;
        };
        match decode(bytes, req) {
            Ok((rgba, w, h)) => finish(rgba, w, h, out, out_cap, info),
            Err((code, msg)) => {
                set_last_error(msg);
                code
            }
        }
    })
}
