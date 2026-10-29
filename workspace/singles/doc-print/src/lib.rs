//! RuPix doc member: PostScript, EPS and PS-compatible Illustrator files.
//!
//! `stet` is a pure-Rust PostScript Level 3 interpreter with embedded fonts
//! and resources. It renders at a chosen DPI; this member renders at 72 DPI
//! (the PS point grid), then scales to the requested viewport with the shared
//! image path. PDF-compatible `.ai` files are handled by `doc-pdf`.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use image::imageops::FilterType;
use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;
use stet::{Interpreter, PageBackground};

const EXTS: &[&str] = &["ps", "ps1", "ps2", "ps3", "prn", "eps", "ai"];

type PageResult = Result<(Vec<u8>, u32, u32, u32), (i32, &'static str)>;

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn named_ext(ext: Option<&str>) -> bool {
    ext.is_some_and(|e| EXTS.contains(&e))
}

fn is_ps(slice: &[u8]) -> bool {
    slice.starts_with(b"%!PS") || slice.starts_with(b"%!")
}

fn target_size(nw: u32, nh: u32, (tw, th): (u32, u32)) -> (u32, u32, f32) {
    if tw == 0 || th == 0 {
        return (nw, nh, 1.0);
    }
    let scale = (tw as f32 / nw as f32).min(th as f32 / nh as f32);
    (
        ((nw as f32 * scale).round() as u32).max(1),
        ((nh as f32 * scale).round() as u32).max(1),
        scale,
    )
}

fn render_ps(data: &[u8], req: *const IvRenderReq) -> PageResult {
    let mut interp = Interpreter::builder()
        .page_background(PageBackground::Transparent)
        .suppress_output()
        .build();
    let pages = interp
        .render(data, 72.0)
        .map_err(|_| (ERR_CORRUPT, "PostScript 解释失败"))?;
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "PostScript 没有页面"));
    }
    let page_index = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    let page_count = pages.len() as u32;
    let Some(page) = pages.get(page_index).or_else(|| pages.first()) else {
        return Err((ERR_CORRUPT, "PostScript 页码越界"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, scale) = target_size(page.width, page.height, (target_w, target_h));
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    if (w, h) == (page.width, page.height) {
        if page.rgba.len() as u64 != expected {
            return Err((ERR_CORRUPT, "PostScript 像素缓冲尺寸不一致"));
        }
        return Ok((page.rgba.clone(), w, h, page_count));
    }
    let Some(image) = image::RgbaImage::from_raw(page.width, page.height, page.rgba.clone()) else {
        return Err((ERR_CORRUPT, "PostScript 像素缓冲不是 RGBA8"));
    };
    let resized = image::imageops::resize(
        &image,
        w,
        h,
        if scale < 1.0 {
            FilterType::Lanczos3
        } else {
            FilterType::Nearest
        },
    );
    let pixels = resized.into_raw();
    if pixels.len() as u64 != expected {
        return Err((ERR_CORRUPT, "PostScript 缩放尺寸不一致"));
    }
    Ok((pixels, w, h, page_count))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 3 {
            return 0;
        }
        let ext = ext_str(ext);
        if !named_ext(ext.as_deref()) {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if is_ps(slice) {
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
        if !named_ext(ext.as_deref()) {
            set_last_error("不是 PostScript/EPS/AI 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if !is_ps(slice) {
            set_last_error("不是 PostScript 字节特征");
            return ERR_UNRECOGNIZED;
        }
        let (rgba, w, h, pages) = match render_ps(slice, req) {
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

    const PS: &[u8] = b"%!PS\n100 100 moveto 200 200 lineto stroke showpage\n";

    #[test]
    fn probes_named_postscript() {
        assert_eq!(probe_impl(PS.as_ptr(), PS.len(), c"ps".as_ptr()), 100);
        assert_eq!(probe_impl(PS.as_ptr(), PS.len(), std::ptr::null()), 0);
    }

    #[test]
    fn renders_basic_postscript() {
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        let code = render_impl(
            PS.as_ptr(),
            PS.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            c"ps".as_ptr(),
        );
        assert_eq!(code, ERR_OK);
        assert!(info.actual_w > 0 && info.actual_h > 0);
    }
}
