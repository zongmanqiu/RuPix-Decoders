//! RuPix doc member: XPS/OXPS.
//!
//! `justpdf-formats::xps` parses the OPC package and exposes RGBA page
//! rendering. The member maps the host's target box to a DPI request so the
//! page is rasterized at a useful density instead of being upscaled later.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use justpdf_formats::xps::XpsDocument;
use justpdf_formats::FormatDocument;
use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn is_xps_ext(ext: Option<&str>) -> bool {
    matches!(ext, Some("xps") | Some("oxps"))
}

fn is_zip(slice: &[u8]) -> bool {
    slice.starts_with(b"PK\x03\x04")
}

fn target_dpi(width_pt: f64, height_pt: f64, req: *const IvRenderReq) -> f64 {
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    if target_w == 0 && target_h == 0 {
        return 72.0;
    }
    let by_w = if target_w > 0 {
        target_w as f64 / width_pt.max(1.0) * 72.0
    } else {
        f64::INFINITY
    };
    let by_h = if target_h > 0 {
        target_h as f64 / height_pt.max(1.0) * 72.0
    } else {
        f64::INFINITY
    };
    by_w.min(by_h).clamp(24.0, 600.0)
}

type PageResult = Result<(Vec<u8>, u32, u32, u32), (i32, &'static str)>;

fn render_xps(data: &[u8], req: *const IvRenderReq) -> PageResult {
    let doc = XpsDocument::from_bytes(data).map_err(|_| (ERR_CORRUPT, "XPS 包解析失败"))?;
    let pages = doc.page_count();
    if pages == 0 {
        return Err((ERR_CORRUPT, "XPS 没有页面"));
    }
    let page_index = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    let Some(page_info) = doc.page(page_index).ok() else {
        return Err((ERR_CORRUPT, "XPS 页码越界"));
    };
    let dpi = target_dpi(page_info.width_pt, page_info.height_pt, req);
    // Gate before render_page(): the backend allocates ceil(pt * dpi / 72) up
    // front, and allocation failure aborts the process (guard cannot catch it).
    let expect_w = (page_info.width_pt * dpi / 72.0).ceil() as u32;
    let expect_h = (page_info.height_pt * dpi / 72.0).ceil() as u32;
    support::check_dims(expect_w, expect_h, support::budget_of(req))?;
    let rendered = doc
        .render_page(page_index, dpi)
        .map_err(|_| (ERR_CORRUPT, "XPS 页面渲染失败"))?;
    let (w, h) = (rendered.width, rendered.height);
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    if rendered.data.len() as u64 != expected {
        return Err((ERR_CORRUPT, "XPS 像素缓冲尺寸不一致"));
    }
    Ok((rendered.data, w, h, pages as u32))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let ext = ext_str(ext);
        if !is_xps_ext(ext.as_deref()) {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if is_zip(slice) {
            100
        } else {
            0
        }
    })
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
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let ext = ext_str(ext);
        if !is_xps_ext(ext.as_deref()) {
            set_last_error("不是 XPS/OXPS 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if !is_zip(slice) {
            set_last_error("不是 XPS 的 OPC/ZIP 字节特征");
            return ERR_UNRECOGNIZED;
        }
        let (rgba, w, h, pages) = match render_xps(slice, req) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
            unsafe { (*info).page_count = pages };
            return ERR_OK;
        }
        let code = support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1);
        if code == ERR_OK {
            unsafe { (*info).page_count = pages };
        }
        code
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_named_xps_zip() {
        assert_eq!(probe_impl(b"PK\x03\x04".as_ptr(), 4, c"xps".as_ptr()), 100);
        assert_eq!(probe_impl(b"PK\x03\x04".as_ptr(), 4, c"pdf".as_ptr()), 0);
    }
}
