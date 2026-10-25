//! RuPix pro 档成员：DjVu（`.djvu/.djv`）。
//!
//! 用 `djvu-rs`（MIT，纯 Rust）渲染第 0 页；页面尺寸/DPI 由库给出。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use djvu_rs::Document;
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-djvu",
    name: "DjVu",
    extensions: "djvu;djv",
    description: "DjVu 第 0 页（纯 Rust）",
    is_vector: 0,
    has_alpha: 0,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn open(data: &[u8]) -> Result<Document, (i32, &'static str)> {
    Document::from_bytes(data.to_vec()).map_err(|_| (ERR_CORRUPT, "DjVu 文档解析失败"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let doc = open(data).ok()?;
    let page = doc.page(0).ok()?;
    let (w, h) = (page.width(), page.height());
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let doc = open(data)?;
    let page = doc.page(0).map_err(|_| (ERR_CORRUPT, "DjVu 没有第 0 页"))?;
    let pix = page.render().map_err(|_| (ERR_CORRUPT, "DjVu 渲染失败"))?;
    let (w, h) = (pix.width, pix.height);
    Ok((w, h, pix.data))
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
        if data.is_null() || len < 8 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(b"AT&TFORM") {
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
            set_last_error("不是合法的 DjVu 字节特征");
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
            set_last_error("DjVu 页面尺寸与像素数不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_djvu_magic() {
        assert_eq!(iv_probe(b"AT&TFORM".as_ptr(), 8, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
