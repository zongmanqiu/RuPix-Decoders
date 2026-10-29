//! RuPix farbfeld (`.ff`) decoder.
//!
//! farbfeld is a deliberately simple 16-bit RGBA format:
//! `"farbfeld"`, big-endian width/height, then width*height RGBA16 samples.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if !data.starts_with(b"farbfeld") {
        return None;
    }
    let w = be32(data, 8)?;
    let h = be32(data, 12)?;
    let expected = 16usize.checked_add((w as usize).checked_mul(h as usize)?.checked_mul(8)?)?;
    (w > 0 && h > 0 && data.len() == expected).then_some((w, h))
}

fn decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (w, h) = dimensions(data).ok_or((ERR_CORRUPT, "farbfeld 尺寸/长度无效"))?;
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let mut rgba = Vec::with_capacity(bytes as usize);
    for c in data[16..].chunks_exact(8) {
        rgba.extend_from_slice(&[
            (u16::from_be_bytes([c[0], c[1]]) >> 8) as u8,
            (u16::from_be_bytes([c[2], c[3]]) >> 8) as u8,
            (u16::from_be_bytes([c[4], c[5]]) >> 8) as u8,
            (u16::from_be_bytes([c[6], c[7]]) >> 8) as u8,
        ]);
    }
    Ok((rgba, w, h))
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 16 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if dimensions(slice).is_some() {
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
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
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
    fn decodes_1x1_farbfeld() {
        let mut data = b"farbfeld".to_vec();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        for c in [0xFFFFu16, 0, 0x8000, 0xFFFF] {
            data.extend_from_slice(&c.to_be_bytes());
        }
        assert_eq!(probe_impl(data.as_ptr(), data.len(), std::ptr::null()), 100);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (1, 1));
    }
}
