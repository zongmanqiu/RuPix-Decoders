//! RuPix pro 档成员：GIMP XCF（`.xcf`）。
//!
//! `xcf 0.4` 只解图层像素、不处理图层偏移/蒙版；这里取**面积最大**的图层作为
//! 可见内容（常见的单层/背景层场景正确；多图层工程图会少掉叠加）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-xcf",
    name: "GIMP XCF",
    extensions: "xcf",
    description: "GIMP XCF（取最大图层，不合成图层偏移/蒙版）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn parse(data: &[u8]) -> Result<xcf::Xcf, (i32, &'static str)> {
    xcf::Xcf::load(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "XCF 解析失败"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let x = parse(data).ok()?;
    let (w, h) = x.dimensions();
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let x = parse(data)?;
    let (w, h) = x.dimensions();
    support::check_dims(w, h, 0)?;
    let layer = x
        .layers
        .iter()
        .max_by_key(|l| l.width as u64 * l.height as u64)
        .ok_or((ERR_CORRUPT, "XCF 里没有图层"))?;
    if layer.width == 0 || layer.height == 0 {
        return Err((ERR_CORRUPT, "XCF 图层尺寸为 0"));
    }
    let px = layer.raw_rgba_buffer();
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    // 画布可能是透明底；图层从 (0,0) 起贴，越界部分裁掉。
    for y in 0..h {
        for xx in 0..w {
            if xx < layer.width && y < layer.height {
                let idx = (y as usize) * layer.width as usize + xx as usize;
                let p = px.get(idx).ok_or((ERR_CORRUPT, "XCF 图层像素被截断"))?.0;
                rgba.extend_from_slice(&p);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
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
        if data.is_null() || len < 9 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(b"gimp xcf ") {
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
            set_last_error("不是合法的 XCF 字节特征");
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
        let (_, _, rgba) = match decode(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if rgba.len() as u64 != bytes {
            set_last_error("XCF 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_xcf_magic() {
        assert_eq!(iv_probe(b"gimp xcf ".as_ptr(), 9, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
