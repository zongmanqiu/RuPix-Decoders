//! RuPix ICNS decoder.
//!
//! Picks the largest available icon family member and emits RGBA8. This is the
//! implementation formerly embedded in the `pro-small` grab bag.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;
use std::io::Cursor;

fn decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let family = icns::IconFamily::read(Cursor::new(data))
        .map_err(|_| (ERR_CORRUPT, "ICNS 容器解析失败"))?;
    let best = family
        .available_icons()
        .into_iter()
        .max_by_key(|t| t.pixel_width() as u64 * t.pixel_height() as u64)
        .ok_or((ERR_CORRUPT, "ICNS 里没有可用图标"))?;
    let img = family
        .get_icon_with_type(best)
        .map_err(|_| (ERR_CORRUPT, "ICNS 图标解码失败"))?;
    let rgba = img.convert_to(icns::PixelFormat::RGBA);
    let (w, h) = (rgba.width(), rgba.height());
    support::check_dims(w, h, support::budget_of(req))?;
    Ok((rgba.into_data().into_vec(), w, h))
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_impl(data, len, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(b"icns") {
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
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(clippy::too_many_arguments)]
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
        let (rgba, w, h) = match decode(slice, req) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
            return ERR_OK;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_icns_magic() {
        assert_eq!(
            probe_impl(b"icns\0\0\0\x08".as_ptr(), 8, std::ptr::null()),
            100
        );
        assert_eq!(probe_impl(b"not icns".as_ptr(), 8, std::ptr::null()), 0);
    }
}
