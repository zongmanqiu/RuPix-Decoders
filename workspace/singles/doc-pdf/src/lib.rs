//! RuPix doc member: PDF through `hayro` (pure Rust).
//!
//! `hayro` renders a page into a vello pixmap; this member converts it to
//! RGBA8 and applies the shared size/budget gate. Encrypted PDFs and some
//! advanced blend/knockout features are outside hayro's current scope; those
//! failures are reported as corrupt rather than silently returning a blank.
//!
//! ★ 区域渲染（ABI v6，规范 §十六）：`render_into` 接受任意 `kurbo::Affine`，
//! 所以"只渲看得见的那一块"就是**换一个变换矩阵**——密度由画布/区域算出来，
//! 页面按那个密度重新光栅化，不是把整页放大后再裁。`Pdf` 按字节指纹缓存
//! （契约第 4 条：同一份字节会连着换区域请求）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::TRANSPARENT;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{render, render_into, PixmapSettings, RenderCache, RenderSettings};
use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;
use std::rc::Rc;

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

thread_local! {
    /// 按文件字节指纹缓存已解析的 PDF（契约 §十六 第 4 条）。
    static PDFS: support::DocCache<Rc<Pdf>> = const { support::DocCache::new(3) };
}

fn pdf_of(data: &[u8]) -> Result<Rc<Pdf>, (i32, &'static str)> {
    let key = support::doc_key(data);
    if let Some(hit) = PDFS.with(|c| c.get(key)) {
        return Ok(hit);
    }
    let pdf = Rc::new(Pdf::new(data.to_vec()).map_err(|_| (ERR_CORRUPT, "PDF 解析失败"))?);
    PDFS.with(|c| c.put(key, Rc::clone(&pdf)));
    Ok(pdf)
}

/// 查询阶段直接问页面尺寸（不渲染）：`render_dimensions` 是页面在 72 dpi 下的
/// 像素尺寸，与 `render` 的 `(w * scale) as u16` 同一口径。
fn page_dims(data: &[u8], req: *const IvRenderReq) -> Result<(u32, u32, u32), (i32, &'static str)> {
    let pdf = pdf_of(data)?;
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
    let w = (nw * scale) as u32;
    let h = (nh * scale) as u32;
    let (w, h, _) = support::check_dims(w, h, support::budget_of(req))?;
    Ok((w, h, pages.len() as u32))
}

/// 预乘 RGBA8 → 宿主口径（非预乘）直写。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
unsafe fn write_unpremultiplied(src: &[u8], out: *mut u8, len: usize) {
    let dst = std::slice::from_raw_parts_mut(out, len);
    for (d, c) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let a = c[3] as u16;
        if a == 0 {
            d.copy_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            d.copy_from_slice(c);
        } else {
            let un = |v: u8| ((v as u16 * 255 + a / 2) / a).min(255) as u8;
            d[0] = un(c[0]);
            d[1] = un(c[1]);
            d[2] = un(c[2]);
            d[3] = c[3];
        }
    }
}

type PageResult = Result<(Vec<u8>, u32, u32, u32), (i32, &'static str)>;

fn render_page(data: &[u8], req: *const IvRenderReq) -> PageResult {
    let pdf = pdf_of(data)?;
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
    if expect_w > u16::MAX as u32 || expect_h > u16::MAX as u32 {
        return Err((ERR_TOO_LARGE, "PDF 目标尺寸超出渲染上限"));
    }
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
            bg_color: TRANSPARENT,
        },
    );
    let (w, h) = (pixmap.width() as u32, pixmap.height() as u32);
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    let mut pixels = vec![0u8; expected as usize];
    // SAFETY: 缓冲区刚按 expected 分配，`write_unpremultiplied` 只写这段。
    unsafe {
        write_unpremultiplied(
            pixmap.data_as_u8_slice(),
            pixels.as_mut_ptr(),
            expected as usize,
        )
    };
    Ok((pixels, w, h, page_count))
}

/// 区域渲染：页面坐标系（= `render_dimensions` 的像素）→ 区域画布。
fn render_region_page(
    data: &[u8],
    req: &IvRegionReq,
    out: *mut u8,
    out_cap: usize,
) -> Result<(u32, u32, u64), (i32, &'static str)> {
    let pdf = pdf_of(data)?;
    let pages = pdf.pages();
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "PDF 没有页面"));
    }
    let page_index = req.page_index as usize;
    let Some(page) = pages.get(page_index).or_else(|| pages.first()) else {
        return Err((ERR_CORRUPT, "PDF 页码越界"));
    };
    if !(req.w > 0.0 && req.h > 0.0) {
        return Err((ERR_CORRUPT, "区域请求的宽或高不为正"));
    }
    let (cw, ch, density) = support::region_canvas(req);
    let (_, _, bytes) = support::check_dims(cw, ch, req.budget_bytes)?;
    if out.is_null() {
        return Ok((cw, ch, bytes));
    }
    if (out_cap as u64) < bytes {
        return Err((ERR_BUFFER_TOO_SMALL, "内部错误（宿主给的缓冲小于申报值）"));
    }
    if cw > u16::MAX as u32 || ch > u16::MAX as u32 {
        return Err((ERR_TOO_LARGE, "PDF 区域画布超出渲染上限"));
    }
    let d = density as f64;
    let transform = hayro::kurbo::Affine::translate((-req.x as f64 * d, -req.y as f64 * d))
        * hayro::kurbo::Affine::scale(d)
        * page.initial_transform(true).to_kurbo();
    let settings = InterpreterSettings::default();
    let cache = RenderCache::new();
    let render_settings = RenderSettings::default();
    let mut ctx = RenderContext::new(cw as u16, ch as u16);
    render_into(
        page,
        &cache,
        &settings,
        &render_settings,
        &mut ctx,
        transform,
    );
    ctx.flush();
    let mut pixmap = Pixmap::new(cw as u16, ch as u16);
    ctx.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(TRANSPARENT),
            ..Default::default()
        },
    );
    // SAFETY: `out` 由宿主按 `bytes` 分配（上面核过 out_cap）。
    unsafe { write_unpremultiplied(pixmap.data_as_u8_slice(), out, bytes as usize) };
    Ok((cw, ch, bytes))
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
        let code = support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1);
        if code == ERR_OK {
            unsafe { (*info).page_count = pages };
        }
        code
    })
}

/// `iv_render_region` 的业务实现（规范 §十六）。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn export_region_impl(
    data: *const u8,
    len: usize,
    req: *const IvRegionReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() || req.is_null() {
            set_last_error("内部错误（空数据或空请求指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        if !is_pdf(slice, ext.as_deref()) {
            set_last_error("不是 PDF 字节特征");
            return ERR_UNRECOGNIZED;
        }
        let r = unsafe { *req };
        let (cw, ch, bytes) = match render_region_page(slice, &r, out, out_cap) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        support::fill_info(info, cw, ch, bytes, 1, 0);
        ERR_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一页 200×200 的 PDF：白底 + 文档坐标 x=100 处 1pt 宽的黑竖条。
    const PDF: &[u8] = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>
endobj
4 0 obj
<< /Length 52 >>
stream
1 1 1 rg 0 0 200 200 re f
0 0 0 rg 100 0 1 200 re f
endstream
endobj
trailer
<< /Size 5 /Root 1 0 R >>
%%EOF
";

    #[test]
    fn probes_pdf_magic() {
        assert_eq!(probe_impl(b"%PDF-1.7".as_ptr(), 8, std::ptr::null()), 100);
        assert_eq!(probe_impl(b"not pdf".as_ptr(), 7, std::ptr::null()), 0);
    }

    #[test]
    fn renders_a_minimal_pdf() {
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                PDF.as_ptr(),
                PDF.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (200, 200));
        assert_eq!(info.page_count, 1);
    }

    /// 区域通道：按密度重画（文档坐标 x=100 的黑条落在画布 x=(100-90)*8=80），
    /// 不是把整页渲出来再插值。
    #[test]
    fn region_is_rasterized_at_density() {
        let req = IvRegionReq {
            x: 90.0,
            y: 90.0,
            w: 20.0,
            h: 20.0,
            target_w: 160,
            target_h: 160,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                PDF.as_ptr(),
                PDF.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (160, 160));
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            export_region_impl(
                PDF.as_ptr(),
                PDF.len(),
                &req,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        let luma = |x: usize, y: usize| px[(y * 160 + x) * 4];
        assert!(
            luma(70, 10) > 200,
            "黑条左侧必须白（实际 {}）",
            luma(70, 10)
        );
        assert!(
            luma(84, 10) < 60,
            "黑条必须落在 x=80..88（实际 {}）",
            luma(84, 10)
        );
    }

    /// 越出页面的区域是透明（页面本身不画到那一块）。
    #[test]
    fn region_outside_the_page_is_transparent() {
        let req = IvRegionReq {
            x: 250.0,
            y: 250.0,
            w: 20.0,
            h: 20.0,
            target_w: 40,
            target_h: 40,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                PDF.as_ptr(),
                PDF.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        let mut px = vec![0u8; info.bytes_needed as usize];
        export_region_impl(
            PDF.as_ptr(),
            PDF.len(),
            &req,
            px.as_mut_ptr(),
            px.len(),
            &mut info,
            std::ptr::null(),
        );
        assert!(px.chunks_exact(4).all(|p| p[3] == 0));
    }
}
