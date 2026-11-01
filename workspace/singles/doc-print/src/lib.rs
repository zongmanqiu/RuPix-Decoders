//! RuPix doc member: PostScript, EPS and PS-compatible Illustrator files.
//!
//! `stet` is a pure-Rust PostScript Level 3 interpreter with embedded fonts
//! and resources. It produces a **display list** (device-space at a chosen
//! resolution) which can be rasterized at any viewport/zoom afterwards; this
//! member builds that list once at 72 dpi (the PS point grid) and renders
//! every request from it.
//!
//! ★ 2026-11-01 的两个修正（真机样本暴露）：
//! 1. **锯齿**：旧实现固定 72 dpi 渲染，再用 `FilterType::Nearest` 放大 ⇒
//!    放大后是硬像素块。现在按目标密度**重画**（`render_region_prepared`
//!    的 zoom 通道），线条始终锐利。
//! 2. **区域通道（ABI v6，规范 §十六）**：同一张 display list 换视口就渲另一块，
//!    解析（解释执行）结果按字节指纹缓存，平移/缩放不再重跑 PostScript。
//!    `render_region_prepared` 的输出恒定合成到白纸（PS 是纸面文档），整图路线
//!    也统一用 `PageBackground::White`，两条路线像素口径一致。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::ffi::c_char;
use std::rc::Rc;
use stet::{
    prepare_display_list, render_region_prepared, DisplayListPage, Interpreter, PreparedDisplayList,
};

const EXTS: &[&str] = &["ps", "ps1", "ps2", "ps3", "prn", "eps", "ai"];

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

/// 解释结果：72 dpi 的 display list + 预计算的视口元数据。
struct PsDoc {
    pages: Vec<DisplayListPage>,
    prepared: Vec<PreparedDisplayList>,
}

thread_local! {
    /// 按文件字节指纹缓存解释结果（契约 §十六 第 4 条）。
    static PS: support::DocCache<Rc<PsDoc>> = const { support::DocCache::new(3) };
}

fn ps_doc(data: &[u8]) -> Result<Rc<PsDoc>, (i32, &'static str)> {
    let key = support::doc_key(data);
    if let Some(hit) = PS.with(|c| c.get(key)) {
        return Ok(hit);
    }
    let mut interp = Interpreter::builder().suppress_output().build();
    let pages = interp
        .render_to_display_list(data, 72.0)
        .map_err(|_| (ERR_CORRUPT, "PostScript 解释失败"))?;
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "PostScript 没有页面"));
    }
    let prepared = pages
        .iter()
        .map(|p| prepare_display_list(&p.display_list))
        .collect();
    let doc = Rc::new(PsDoc { pages, prepared });
    PS.with(|c| c.put(key, Rc::clone(&doc)));
    Ok(doc)
}

fn page_index_of(req: *const IvRenderReq, len: usize) -> usize {
    let asked = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    if asked < len {
        asked
    } else {
        0
    }
}

type PageResult = Result<(Vec<u8>, u32, u32, u32), (i32, &'static str)>;

/// 整图渲染：把整页视口按目标密度重画（不是渲 72 dpi 再放大）。
fn render_whole(doc: &PsDoc, req: *const IvRenderReq) -> PageResult {
    let idx = page_index_of(req, doc.pages.len());
    let Some(page) = doc.pages.get(idx) else {
        return Err((ERR_CORRUPT, "PostScript 没有页面"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, _) = target_size(page.width, page.height, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let px = render_region_prepared(
        &page.display_list,
        &doc.prepared[idx],
        0.0,
        0.0,
        page.width as f64,
        page.height as f64,
        w,
        h,
        page.dpi,
        None,
        None,
        false,
    );
    if px.len() as u64 != bytes {
        return Err((ERR_CORRUPT, "PostScript 像素缓冲尺寸不一致"));
    }
    Ok((px, w, h, doc.pages.len() as u32))
}

/// 查询阶段：只看 72 dpi 的页面尺寸，不做光栅化。
fn page_dims(doc: &PsDoc, req: *const IvRenderReq) -> Result<(u32, u32, u32), (i32, &'static str)> {
    let idx = page_index_of(req, doc.pages.len());
    let Some(page) = doc.pages.get(idx) else {
        return Err((ERR_CORRUPT, "PostScript 没有页面"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, _) = target_size(page.width, page.height, (target_w, target_h));
    let (w, h, _) = support::check_dims(w, h, support::budget_of(req))?;
    Ok((w, h, doc.pages.len() as u32))
}

/// 区域渲染：视口（72 dpi 设备坐标 = 自然像素）→ 画布。
fn render_region(
    doc: &PsDoc,
    req: &IvRegionReq,
    out: *mut u8,
    out_cap: usize,
) -> Result<(u32, u32, u64), (i32, &'static str)> {
    let idx = if (req.page_index as usize) < doc.pages.len() {
        req.page_index as usize
    } else {
        0
    };
    let Some(page) = doc.pages.get(idx) else {
        return Err((ERR_CORRUPT, "PostScript 没有页面"));
    };
    if !(req.w > 0.0 && req.h > 0.0) {
        return Err((ERR_CORRUPT, "区域请求的宽或高不为正"));
    }
    let (cw, ch, _) = support::region_canvas(req);
    let (_, _, bytes) = support::check_dims(cw, ch, req.budget_bytes)?;
    if out.is_null() {
        return Ok((cw, ch, bytes));
    }
    if (out_cap as u64) < bytes {
        return Err((ERR_BUFFER_TOO_SMALL, "内部错误（宿主给的缓冲小于申报值）"));
    }
    let px = render_region_prepared(
        &page.display_list,
        &doc.prepared[idx],
        req.x as f64,
        req.y as f64,
        req.w as f64,
        req.h as f64,
        cw,
        ch,
        page.dpi,
        None,
        None,
        false,
    );
    if px.len() as u64 != bytes {
        return Err((ERR_CORRUPT, "PostScript 区域缓冲尺寸不一致"));
    }
    unsafe { std::ptr::copy_nonoverlapping(px.as_ptr(), out, bytes as usize) };
    Ok((cw, ch, bytes))
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
        let doc = match ps_doc(slice) {
            Ok(d) => d,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            return match page_dims(&doc, req) {
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
        let (rgba, w, h, pages) = match render_whole(&doc, req) {
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
        let doc = match ps_doc(slice) {
            Ok(d) => d,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let r = unsafe { *req };
        let (cw, ch, bytes) = match render_region(&doc, &r, out, out_cap) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        support::fill_info(info, cw, ch, bytes, 1, 0);
        unsafe { (*info).page_count = doc.pages.len() as u32 };
        ERR_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &[u8] = b"%!PS\n100 100 moveto 200 200 lineto stroke showpage\n";

    /// 100×100 白纸 + 文档坐标 x=50 处 1pt 宽的黑竖条。
    const BAR: &[u8] = b"%!PS\n<< /PageSize [100 100] >> setpagedevice\n\
1 setgray 0 0 100 100 rectfill\n0 setgray 50 0 1 100 rectfill\nshowpage\n";

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

    #[test]
    fn renders_a_target_sized_canvas() {
        let req = IvRenderReq {
            target_w: 400,
            target_h: 400,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                BAR.as_ptr(),
                BAR.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                c"ps".as_ptr(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (400, 400));
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            render_impl(
                BAR.as_ptr(),
                BAR.len(),
                &req,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                c"ps".as_ptr(),
            ),
            ERR_OK
        );
        // 竖条在 x=50..51pt → 画布 200..204：左白右黑，边是锐的。
        let luma = |x: usize| px[(10 * 400 + x) * 4];
        assert!(luma(190) > 200, "竖条左侧必须白（实际 {}）", luma(190));
        assert!(luma(202) < 60, "竖条必须落在 x=200（实际 {}）", luma(202));
    }

    /// 区域通道按密度重画：区域 (40,0,20,20) → 4 倍 ⇒ 画布 80×80，竖条在
    /// (50-40)*4=40，边界锐利（插值放大只会是一条灰坡）。
    #[test]
    fn region_is_rasterized_at_density() {
        let req = IvRegionReq {
            x: 40.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                BAR.as_ptr(),
                BAR.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                c"ps".as_ptr(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (80, 80));
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            export_region_impl(
                BAR.as_ptr(),
                BAR.len(),
                &req,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                c"ps".as_ptr(),
            ),
            ERR_OK
        );
        let luma = |x: usize, y: usize| px[(y * 80 + x) * 4];
        assert!(
            luma(39, 10) > 200,
            "黑条左侧必须白（实际 {}）",
            luma(39, 10)
        );
        assert!(
            luma(41, 10) < 60,
            "黑条必须锐利地落在 x=40（实际 {}）",
            luma(41, 10)
        );
    }
}
