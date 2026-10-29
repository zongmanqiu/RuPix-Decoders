//! RuPix doc member: DWG/DXF through pure-Rust `acadrust` parsing and the
//! `dxf_to_svg` raster path.
//!
//! The first slice deliberately avoids a libclang/bindgen build dependency:
//! `acadrust` reads native DWG and converts it to DXF in memory, then the
//! `dxf`/`dxf_to_svg` pair emits an SVG for the shared resvg path.
//!
//! License: `acadrust` is MPL-2.0; `dxf` and `dxf_to_svg` are MIT; `resvg`
//! is MPL-2.0. The member is part of the AGPL-3.0 doc DLL; release notices
//! must include these components and the source offer.
//!
//! LibreDWG/`uncad` remains the target renderer when a libclang toolchain is
//! available; it is not a build dependency of this first slice.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

fn is_dwg_magic(slice: &[u8]) -> bool {
    slice.starts_with(b"AC10")
}

fn is_dxf_text(slice: &[u8]) -> bool {
    slice.starts_with(b"0\r\nSECTION")
        || slice.starts_with(b"0\nSECTION")
        || slice.starts_with(b"AutoCAD Binary DXF")
}

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn is_dwg(ext: Option<&str>) -> bool {
    matches!(ext, Some("dwg") | Some("dwt"))
}

fn is_cad(slice: &[u8], ext: Option<&str>) -> bool {
    is_dwg_magic(slice) || is_dxf_text(slice) || is_dwg(ext) || ext == Some("dxf")
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

fn cad_svg(slice: &[u8], ext: Option<&str>) -> Result<String, (i32, &'static str)> {
    let dwg = is_dwg_magic(slice) || is_dwg(ext);
    let doc = if dwg {
        acadrust::DwgReader::from_stream(Cursor::new(slice.to_vec()))
            .read()
            .map_err(|_| (ERR_CORRUPT, "DWG 解析失败"))?
    } else {
        acadrust::DxfReader::from_reader(Cursor::new(slice.to_vec()))
            .and_then(|reader| reader.read())
            .map_err(|_| (ERR_CORRUPT, "DXF 解析失败"))?
    };
    let dxf = acadrust::DxfWriter::new(&doc)
        .write_to_vec()
        .map_err(|_| (ERR_CORRUPT, "CAD 转 DXF 失败"))?;
    let mut cursor = Cursor::new(dxf);
    let drawing = dxf::Drawing::load(&mut cursor).map_err(|_| (ERR_CORRUPT, "DXF 二次解析失败"))?;
    Ok(dxf_to_svg::dxf_to_svg(drawing.entities().collect(), None))
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

fn rgb_from_svg(
    svg: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let tree = resvg::usvg::Tree::from_data(svg, &svg_options())
        .map_err(|_| (ERR_CORRUPT, "CAD 渲染结果不是有效 SVG"))?;
    let size = tree.size();
    let Some((nw, nh)) = positive_size(size.width(), size.height()) else {
        return Err((ERR_CORRUPT, "CAD 画布尺寸为 0"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, scale) = target_size(nw, nh, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "CAD 画布分配失败"))?;
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
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        if is_cad(slice, ext.as_deref()) {
            return 100;
        }
        0
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
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
        if !is_cad(slice, ext.as_deref()) {
            set_last_error("不是 DWG/DXF CAD 数据");
            return ERR_UNRECOGNIZED;
        }
        let svg = match cad_svg(slice, ext.as_deref()) {
            Ok(svg) => svg,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (rgba, w, h) = match rgb_from_svg(svg.as_bytes(), req) {
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

    const DXF: &[u8] = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n100.0\n21\n100.0\n0\nENDSEC\n0\nEOF\n";

    #[test]
    fn probes_dxf_text_and_dwg_magic() {
        assert_eq!(probe_impl(DXF.as_ptr(), DXF.len(), std::ptr::null()), 100);
        assert_eq!(probe_impl(b"AC1027".as_ptr(), 6, std::ptr::null()), 100);
        assert_eq!(probe_impl(b"not a cad".as_ptr(), 9, std::ptr::null()), 0);
    }

    #[test]
    fn renders_a_minimal_dxf() {
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        let q = render_impl(
            DXF.as_ptr(),
            DXF.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        assert!(info.actual_w > 0 && info.actual_h > 0);
        let mut out = vec![0u8; info.bytes_needed as usize];
        let code = render_impl(
            DXF.as_ptr(),
            DXF.len(),
            &req,
            out.as_mut_ptr(),
            out.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(code, ERR_OK);
        assert!(out.chunks_exact(4).any(|p| p[3] != 0));
    }
}
