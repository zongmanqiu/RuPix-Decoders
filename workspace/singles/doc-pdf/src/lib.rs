//! RuPix doc member: PDF through `hayro` (pure Rust).
//!
//! `hayro` renders a page into a vello pixmap; this member converts it to
//! RGBA8 and applies the shared size/budget gate. Encrypted PDFs and some
//! advanced blend/knockout features are outside hayro's current scope; those
//! failures are reported as corrupt rather than silently returning a blank.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
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

fn is_pdf(slice: &[u8], ext: Option<&str>) -> bool {
    slice.starts_with(b"%PDF-") || ext == Some("pdf")
}

fn target_scale(nw: f32, nh: f32, (tw, th): (u32, u32)) -> f32 {
    if tw == 0 || th == 0 {
        return 1.0;
    }
    (tw as f32 / nw.max(1.0))
        .min(th as f32 / nh.max(1.0))
        .max(0.01)
}

type PageResult = Result<(Vec<u8>, u32, u32, u32), (i32, &'static str)>;

/// Query-phase answer without rasterizing the page: hayro sizes the pixmap as
/// `(render_dimensions * scale) as u16`, so the same arithmetic gives the size
/// the render phase will produce. Keeping the query cheap matters: the router
/// asks once for dimensions and once more to render.
fn page_dims(data: &[u8], req: *const IvRenderReq) -> Result<(u32, u32, u32), (i32, &'static str)> {
    let pdf = Pdf::new(data.to_vec()).map_err(|_| (ERR_CORRUPT, "PDF 解析失败"))?;
    let pages = pdf.pages();
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "PDF 没有页面"));
    }
    let page_index = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    let Some(page) = pages.get(page_index).or_else(|| pages.first()) else {
        return Err((ERR_CORRUPT, "PDF 页码越界"));
    };
    let (nw, nh) = page.render_dimensions();
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let scale = target_scale(nw, nh, (target_w, target_h));
    let (w, h, _) = support::check_dims(
        (nw * scale) as u32,
        (nh * scale) as u32,
        support::budget_of(req),
    )?;
    Ok((w, h, pages.len() as u32))
}

fn render_page(data: &[u8], req: *const IvRenderReq) -> PageResult {
    let pdf = Pdf::new(data.to_vec()).map_err(|_| (ERR_CORRUPT, "PDF 解析失败"))?;
    let pages = pdf.pages();
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "PDF 没有页面"));
    }
    let page_index = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    let page_count = pages.len() as u32;
    let Some(page) = pages.get(page_index).or_else(|| pages.first()) else {
        return Err((ERR_CORRUPT, "PDF 页码越界"));
    };
    let (nw, nh) = page.render_dimensions();
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let scale = target_scale(nw, nh, (target_w, target_h));
    // Gate BEFORE rasterizing: hayro sizes the pixmap as `(w * scale) as u16`,
    // so checking afterwards lets a huge page/target allocate first (Rust aborts
    // on allocation failure - catch_unwind cannot contain it).
    let expect_w = (nw * scale) as u32;
    let expect_h = (nh * scale) as u32;
    support::check_dims(expect_w, expect_h, support::budget_of(req))?;
    let settings = InterpreterSettings::default();
    let cache = RenderCache::new();
    let render_settings = RenderSettings::default();
    let pixmap = render(
        page,
        &cache,
        &settings,
        &render_settings,
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::TRANSPARENT,
        },
    );
    let png = pixmap
        .into_png()
        .map_err(|_| (ERR_CORRUPT, "PDF 像素编码失败"))?;
    let image = image::load_from_memory(&png).map_err(|_| (ERR_CORRUPT, "PDF 像素解码失败"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    let pixels = rgba.into_raw();
    if pixels.len() as u64 != expected {
        return Err((ERR_CORRUPT, "PDF 像素缓冲尺寸不一致"));
    }
    Ok((pixels, w, h, page_count))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 5 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        if is_pdf(slice, ext.as_deref()) {
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
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        if !is_pdf(slice, ext.as_deref()) {
            set_last_error("不是 PDF 字节特征");
            return ERR_UNRECOGNIZED;
        }
        if out.is_null() {
            return match page_dims(slice, req) {
                Ok((w, h, pages)) => {
                    support::fill_info(info, w, h, w as u64 * h as u64 * 4, 1, 0);
                    unsafe { (*info).page_count = pages };
                    ERR_OK
                }
                Err((code, msg)) => {
                    set_last_error(msg);
                    code
                }
            };
        }
        let (rgba, w, h, pages) = match render_page(slice, req) {
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
    fn probes_pdf_magic() {
        assert_eq!(probe_impl(b"%PDF-1.7".as_ptr(), 8, std::ptr::null()), 100);
        assert_eq!(probe_impl(b"not pdf".as_ptr(), 7, std::ptr::null()), 0);
    }
}
