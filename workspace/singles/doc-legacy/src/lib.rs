//! RuPix doc member: CMX/WPG/ZMF/ZBR through the Document Liberation C++
//! libraries (`libcdr`, `libwpg`, `libzmf`) and a tiny `extern "C"` shim.
//!
//! The shim returns an SVG page; the shared resvg path rasterizes it so the
//! doc assembly layer does not need to know the C++ ABI. The C++ libraries are
//! static and link into doc.dll; no sidecar DLL is required.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::ffi::{c_char, CStr};
use std::sync::{Arc, OnceLock};

extern "C" {
    fn rupix_legacy_probe(data: *const u8, len: u32, ext: *const c_char) -> i32;
    fn rupix_legacy_svg(
        data: *const u8,
        len: u32,
        ext: *const c_char,
        out: *mut *mut c_char,
        out_len: *mut u32,
    ) -> i32;
    fn rupix_legacy_free(p: *mut c_char);
}

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn named_ext(ext: Option<&str>) -> bool {
    matches!(
        ext,
        Some("cmx") | Some("ccx") | Some("wpg") | Some("zmf") | Some("zbr")
    )
}

fn zbr_preview(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 168 + 5200 || data[0] != 0x9A || data[1] != 0x02 {
        return None;
    }
    let palette = &data[104..168];
    let dib = &data[168..168 + 5200];
    let mut rgba = vec![0u8; 100 * 100 * 4];
    for row in 0..100 {
        let src_row = 99 - row;
        let src = &dib[src_row * 52..src_row * 52 + 52];
        for x in 0..100 {
            let byte = src[x / 2];
            let idx = if x % 2 == 0 { byte >> 4 } else { byte & 0x0F } as usize;
            let p = idx * 4;
            let d = (row * 100 + x) * 4;
            rgba[d] = palette[p + 2];
            rgba[d + 1] = palette[p + 1];
            rgba[d + 2] = palette[p];
            rgba[d + 3] = 255;
        }
    }
    Some(rgba)
}

fn scale_preview(
    rgba: Vec<u8>,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, _) = target_size(100, 100, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    if w == 100 && h == 100 {
        return Ok((rgba, w, h));
    }
    let mut out = vec![0u8; bytes as usize];
    for y in 0..h {
        let sy = (y as u64 * 100 / h as u64) as usize;
        for x in 0..w {
            let sx = (x as u64 * 100 / w as u64) as usize;
            let si = (sy * 100 + sx) * 4;
            let di = (y as usize * w as usize + x as usize) * 4;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    Ok((out, w, h))
}

fn legacy_svg(data: &[u8], ext: &str) -> Result<Vec<u8>, (i32, &'static str)> {
    let ext_c = std::ffi::CString::new(ext).map_err(|_| (ERR_CORRUPT, "legacy 后缀无效"))?;
    let mut out: *mut c_char = std::ptr::null_mut();
    let mut len = 0u32;
    let code = unsafe {
        rupix_legacy_svg(
            data.as_ptr(),
            data.len() as u32,
            ext_c.as_ptr(),
            &mut out,
            &mut len,
        )
    };
    if code != 0 || out.is_null() {
        return Err((ERR_CORRUPT, "legacy 文档解析失败"));
    }
    let svg = unsafe { std::slice::from_raw_parts(out as *const u8, len as usize).to_vec() };
    unsafe { rupix_legacy_free(out) };
    Ok(svg)
}

fn font_db() -> &'static Arc<resvg::usvg::fontdb::Database> {
    static FONT_DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONT_DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
}

fn svg_options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        fontdb: font_db().clone(),
        ..Default::default()
    }
}

fn positive_size(w: f32, h: f32) -> Option<(u32, u32)> {
    let (w, h) = (w.round() as u32, h.round() as u32);
    (w > 0 && h > 0).then_some((w, h))
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

fn rasterize(
    svg: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let tree = resvg::usvg::Tree::from_data(svg, &svg_options())
        .map_err(|_| (ERR_CORRUPT, "legacy 场景不是有效 SVG"))?;
    let size = tree.size();
    let Some((nw, nh)) = positive_size(size.width(), size.height()) else {
        return Err((ERR_CORRUPT, "legacy 画布尺寸为 0"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, scale) = target_size(nw, nh, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "legacy 画布分配失败"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(bytes as usize);
    for c in pixmap.data().chunks_exact(4) {
        let a = c[3] as u16;
        if a == 0 || a == 255 {
            rgba.extend_from_slice(c);
        } else {
            let un = |v: u8| ((v as u16 * 255 + a / 2) / a).min(255) as u8;
            rgba.extend_from_slice(&[un(c[0]), un(c[1]), un(c[2]), c[3]]);
        }
    }
    Ok((rgba, w, h))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 {
            return 0;
        }
        let ext = ext_str(ext);
        if !named_ext(ext.as_deref()) || len > u32::MAX as usize {
            return 0;
        }
        let Some(ext) = ext else { return 0 };
        if ext == "zbr" {
            let slice = unsafe { std::slice::from_raw_parts(data, len) };
            return zbr_preview(slice).map(|_| 100).unwrap_or(0);
        }
        let Ok(ext_c) = std::ffi::CString::new(ext) else {
            return 0;
        };
        unsafe { rupix_legacy_probe(data, len as u32, ext_c.as_ptr()) }
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
            set_last_error("不是 CMX/WPG/ZMF/ZBR 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if ext.as_deref() == Some("zbr") {
            let Some(preview) = zbr_preview(slice) else {
                set_last_error("ZBR 预览位图无效");
                return ERR_CORRUPT;
            };
            let (rgba, w, h) = match scale_preview(preview, req) {
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
            return support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1);
        }
        let svg = match legacy_svg(slice, ext.as_deref().unwrap_or_default()) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (rgba, w, h) = match rasterize(&svg, req) {
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
    fn rejects_unknown_extension_and_empty_input() {
        assert_eq!(probe_impl(std::ptr::null(), 0, std::ptr::null()), 0);
        let data = b"x";
        assert_eq!(probe_impl(data.as_ptr(), data.len(), c"pdf".as_ptr()), 0);
    }
}
