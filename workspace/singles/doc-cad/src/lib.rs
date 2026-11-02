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
//!
//! ★ 2026-11-01 的三处修正（真机样本暴露）：
//! 1. **颜色**：`dxf_to_svg` 读的是 `EntityCommon::color_name`（DXF 组码 430，
//!    几乎永远为空），不是组码 62 的颜色索引 ⇒ 整张图都按 "black" 画。这里在
//!    交给它之前把 ACI 索引（含 BYLAYER 解析）折算成 CSS 颜色写进 `color_name`。
//! 2. **纸面**：DXF 只有描边、没有背景，浅色棋盘格上"只剩几条线"。补一张覆盖
//!    viewBox 的白纸（索引 7 的"白/黑随背景"在纸面上按黑画）。
//! 3. **文字与解析路径**：
//!    - `.dxf` 输入**直接**交给 `dxf` crate。原先一律走 `acadrust → DxfWriter →
//!      dxf` 的二次转换，DXF 本来就是 DXF，这一圈只会丢东西（字高、部分实体）。
//!      `acadrust` 现在只服务 DWG（它把 DWG 写成 DXF 是必由之路）。
//!    - `TEXT` 不交给 `dxf_to_svg`：它排出的 `<text>` 不带 `font-size`（落到 SVG
//!      默认 16 用户单位）也不带字体栈 ⇒ 中文要么巨大、要么落到没有汉字的字体上。
//!      这里按组码 40 的字高自己排字，字体栈带系统中文。
//!
//! ★ 区域渲染（ABI v6，规范 §十六）：`export_region_impl` 按密度把指定区域
//! **直接重画**（共用件 `pro_support::vector`），解析结果按字节指纹缓存。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
use std::collections::HashMap;
use std::ffi::c_char;
use std::io::Cursor;
use std::rc::Rc;
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

// ── ACI 颜色（AutoCAD 颜色索引）→ 纸面 CSS 颜色 ──────────────────────

fn hsl_to_css(h: f32, s: f32, l: f32) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let f = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", f(r), f(g), f(b))
}

/// ACI 1..=9 是标准色；10..=249 是色相环（24 色相 × 明暗），250..=255 是灰度。
///
/// 纸面（白底）取**深一档**的饱和度/明度：黄色、青色按原值在白纸上几乎看不见
/// （AI 的浅色棋盘格上同样看不清），索引 7 的"白/黑随背景"在纸面上按黑画。
fn aci_css(idx: u8) -> String {
    match idx {
        1 => "#e02020".into(),
        2 => "#8b8b00".into(),
        3 => "#008000".into(),
        4 => "#008b8b".into(),
        5 => "#0044cc".into(),
        6 => "#8b008b".into(),
        7 => "#000000".into(),
        8 => "#707070".into(),
        9 => "#a0a0a0".into(),
        250..=255 => {
            let v = 0x22 + (idx - 250) * 0x1d;
            format!("#{v:02x}{v:02x}{v:02x}")
        }
        n => {
            let hue = ((n as f32 - 10.0) * 15.0).rem_euclid(360.0);
            hsl_to_css(hue, 0.70, 0.38)
        }
    }
}

/// 把每个实体的颜色索引折算成 CSS 颜色塞进 `color_name`（`dxf_to_svg` 的取色口）。
///
/// BYLAYER / BYENTITY 解析到所在图层的颜色；BYBLOCK 与无索引的情况留给
/// `dxf_to_svg` 的 "black" 兜底（块参照的颜色由块内实体自己带）。
fn paint_colors(drawing: &dxf::Drawing) -> Vec<dxf::entities::Entity> {
    let layers: HashMap<String, u8> = drawing
        .layers()
        .filter_map(|l| l.color.index().map(|i| (l.name.to_ascii_lowercase(), i)))
        .collect();
    let mut entities: Vec<dxf::entities::Entity> = drawing.entities().cloned().collect();
    for e in &mut entities {
        let idx = e
            .common
            .color
            .index()
            .or_else(|| layers.get(&e.common.layer.to_ascii_lowercase()).copied());
        if let Some(i) = idx {
            e.common.color_name = aci_css(i);
        }
    }
    entities
}

/// 在根 `<svg …>` 之后插一张覆盖 viewBox 的白纸（DXF 本身没有背景）。
fn with_white_paper(mut svg: String) -> String {
    let Some(vp) = svg.find("viewBox=\"") else {
        return svg;
    };
    let rest = &svg[vp + "viewBox=\"".len()..];
    let Some(end) = rest.find('"') else {
        return svg;
    };
    let nums: Vec<f32> = rest[..end]
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() != 4 {
        return svg;
    }
    let Some(tag_end) = svg[vp..].find('>') else {
        return svg;
    };
    let rect = format!(
        r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#ffffff"/>"##,
        nums[0], nums[1], nums[2], nums[3]
    );
    svg.insert_str(vp + tag_end + 1, &rect);
    svg
}

/// DWG/DXF 字节 → 自包含 SVG 文本（白纸 + 实体真彩色 + 文字）。
fn cad_svg(slice: &[u8], ext: Option<&str>) -> Result<String, (i32, &'static str)> {
    let dwg = is_dwg_magic(slice) || is_dwg(ext);
    let drawing = if dwg {
        let doc = acadrust::DwgReader::from_stream(Cursor::new(slice.to_vec()))
            .read()
            .map_err(|_| (ERR_CORRUPT, "DWG 解析失败"))?;
        let dxf = acadrust::DxfWriter::new(&doc)
            .write_to_vec()
            .map_err(|_| (ERR_CORRUPT, "CAD 转 DXF 失败"))?;
        dxf::Drawing::load(&mut Cursor::new(dxf)).map_err(|_| (ERR_CORRUPT, "DXF 二次解析失败"))?
    } else {
        // DXF 直接读：再经一轮 DWG→DXF 写入器只会丢掉它不认识的字段。
        dxf::Drawing::load(&mut Cursor::new(slice.to_vec()))
            .map_err(|_| (ERR_CORRUPT, "DXF 解析失败"))?
    };
    let entities = paint_colors(&drawing);
    let texts = texts_svg(&entities);
    // 文字实体**留在**这遍里只为把插入点算进 viewBox（`dxf_to_svg` 自己排的
    // `<text>` 不带 font-size，随后整段剥掉，换成上面那份）。
    let refs: Vec<&dxf::entities::Entity> = entities.iter().collect();
    let mut svg = strip_fontless_text(dxf_to_svg::dxf_to_svg(refs, None));
    svg = tighten_viewbox(svg);
    svg = with_white_paper(svg);
    if !texts.is_empty() {
        if let Some(end) = svg.rfind("</svg>") {
            svg.insert_str(end, &texts);
        }
    }
    Ok(svg)
}

/// `dxf_to_svg` 的 padding 写死 1.0（每边各留 100%）⇒ viewBox 是内容框的 **3 倍**，
/// 图纸在窗口里只占中间九分之一。这里把它收回到内容框 + 4% 边距。
fn tighten_viewbox(svg: String) -> String {
    let Some(vp) = svg.find("viewBox=\"") else {
        return svg;
    };
    let rest = &svg[vp + "viewBox=\"".len()..];
    let Some(end) = rest.find('"') else {
        return svg;
    };
    let nums: Vec<f32> = rest[..end]
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() != 4 || !nums.iter().all(|v| v.is_finite()) || nums[2] <= 0.0 || nums[3] <= 0.0 {
        return svg;
    }
    let (mut x, mut y, mut w, mut h) = (nums[0], nums[1], nums[2], nums[3]);
    // 只在"确实是 1.0 padding 的形状"上收：内容框 = 1/3，边距各占 1/3。
    let (cw, ch) = (w / 3.0, h / 3.0);
    x += cw;
    y += ch;
    w = cw * 1.08;
    h = ch * 1.08;
    let replacement = format!("viewBox=\"{x:.3} {y:.3} {w:.3} {h:.3}\"");
    let mut out = String::with_capacity(svg.len());
    out.push_str(&svg[..vp]);
    out.push_str(&replacement);
    out.push_str(&svg[vp + "viewBox=\"".len() + end + 1..]);
    out
}

/// 剥掉 `dxf_to_svg` 排的 `<text>`（它们没有 `font-size`）。
fn strip_fontless_text(mut svg: String) -> String {
    let mut from = 0;
    while let Some(rel) = svg[from..].find("<text ") {
        let start = from + rel;
        let Some(rel_end) = svg[start..].find("</text>") else {
            break;
        };
        let end = start + rel_end + "</text>".len();
        if svg[start..end].contains("font-size") {
            from = end;
            continue;
        }
        svg.replace_range(start..end, "");
        from = start;
    }
    svg
}

/// DXF `TEXT` → SVG `<text>`：字高取组码 40，字体栈带系统中文。
///
/// 为什么不用 `dxf_to_svg` 的 `<text>`：它不写 `font-size`（SVG 默认 16 用户单位，
/// 在 420 单位宽的图纸里是"一个字占满半张纸"），也不写字体栈（中文可能落到
/// 无汉字的字体上）。DXF 的插入点就是基线左端，与 SVG 的 `x/y` 语义一致。
fn texts_svg(entities: &[dxf::entities::Entity]) -> String {
    let mut out = String::new();
    for e in entities {
        let dxf::entities::EntityType::Text(t) = &e.specific else {
            continue;
        };
        if t.value.trim().is_empty() {
            continue;
        }
        let color = if e.common.color_name.trim().is_empty() {
            "black"
        } else {
            e.common.color_name.trim()
        };
        // 组码 40 是**字高**（大写字母净高），SVG 的 font-size 是 em ⇒ 除 0.72。
        // NaN 单独落到下界：`clamp` 会把 NaN 原样传出去，而 `max().min()` 丢掉 NaN 后
        // 得到的是 0.5——畸形 DXF 的两种结果必须与旧口径逐字节相同。
        let raw = (t.text_height.abs() * t.relative_x_scale_factor.abs()) / 0.72;
        let size = if raw.is_nan() {
            0.5
        } else {
            raw.clamp(0.5, 1400.0)
        };
        let (x, y) = (t.location.x, t.location.y);
        let transform = if t.rotation.abs() > 1e-6 {
            format!(r#" transform="rotate({} {} {})""#, -t.rotation, x, y)
        } else {
            String::new()
        };
        out.push_str(&format!(
            r#"<text x="{x:.3}" y="{y:.3}" font-size="{size:.3}" font-family="Microsoft YaHei, Noto Sans CJK SC, PingFang SC, Segoe UI, sans-serif" fill="{color}"{transform}>{}</text>"#,
            escape_xml(&unescape_dxf(&t.value))
        ));
    }
    out
}

/// DXF 的 `\U+XXXX`（老版本写法）。R2007+ 的文件是 UTF-8、`dxf` crate 已按 UTF-8
/// 读，不会解这个转义；老文件里它又是标准写法。两种都见过，就地解一次。
fn unescape_dxf(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 6 < chars.len() && chars[i + 1] == 'U' && chars[i + 2] == '+' {
            let hex: String = chars[i + 3..i + 7].iter().collect();
            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                if let Some(ch) = char::from_u32(code) {
                    out.push(ch);
                    i += 7;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
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

thread_local! {
    /// 按文件字节指纹缓存已解析的场景（契约 §十六 第 4 条）。
    static SCENES: support::vector::DocCache<Rc<resvg::usvg::Tree>> =
        const { support::vector::DocCache::new(3) };
}

fn cad_tree(slice: &[u8], ext: Option<&str>) -> Result<Rc<resvg::usvg::Tree>, (i32, &'static str)> {
    let key = support::vector::doc_key(slice);
    if let Some(hit) = SCENES.with(|c| c.get(key)) {
        return Ok(hit);
    }
    let svg = cad_svg(slice, ext)?;
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &svg_options())
        .map_err(|_| (ERR_CORRUPT, "CAD 渲染结果不是有效 SVG"))?;
    let tree = Rc::new(tree);
    SCENES.with(|c| c.put(key, Rc::clone(&tree)));
    Ok(tree)
}

fn render_tree(
    tree: &resvg::usvg::Tree,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
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
        tree,
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
        let tree = match cad_tree(slice, ext.as_deref()) {
            Ok(t) => t,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (rgba, w, h) = match render_tree(&tree, req) {
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
        if !is_cad(slice, ext.as_deref()) {
            set_last_error("不是 DWG/DXF CAD 数据");
            return ERR_UNRECOGNIZED;
        }
        let tree = match cad_tree(slice, ext.as_deref()) {
            Ok(t) => t,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let r = unsafe { *req };
        let (cw, ch, bytes) = match support::vector::render_region(&tree, &r, out, out_cap) {
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

    const DXF: &[u8] = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n100.0\n21\n100.0\n0\nENDSEC\n0\nEOF\n";

    /// 一条红色竖线：组码 62 = 1。
    const RED_LINE: &[u8] = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n62\n1\n10\n50\n20\n0\n11\n50\n21\n100\n0\nENDSEC\n0\nEOF\n";

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
        // 白纸：画面里必须有大量不透明的白像素（DXF 自己不带背景）。
        assert!(out
            .chunks_exact(4)
            .any(|p| p[3] == 255 && p[0] > 240 && p[1] > 240 && p[2] > 240));
    }

    /// 组码 62 的颜色必须真的走到 SVG（`dxf_to_svg` 只认 `color_name`）。
    #[test]
    fn entity_color_index_becomes_a_real_css_color() {
        let svg = cad_svg(RED_LINE, None).expect("cad svg");
        assert!(
            svg.contains("#e02020"),
            "红色实体必须折算成 CSS 颜色，实际 SVG：{svg}"
        );
    }

    /// `TEXT` 由本成员自己排字：字高来自组码 40，字体栈带中文，且不再交给
    /// `dxf_to_svg`（它不写 font-size，中文会落成 16 用户单位的一颗大字或方框）。
    ///
    /// 文字值用 `\U+XXXX`：DXF 的字符串编码跟 `$DWGCODEPAGE` 走，写裸 UTF-8 会被
    /// 当 ANSI 解成乱码（`中文` → `ä¸æ–‡`）。
    #[test]
    fn text_entities_carry_height_and_a_cjk_font_stack() {
        let src: &[u8] = b"0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n62\n3\n10\n10.0\n20\n20.0\n40\n8.0\n1\n\\U+4E2D\\U+6587\n0\nENDSEC\n0\nEOF\n";
        let svg = cad_svg(src, Some("dxf")).expect("cad svg");
        assert!(
            svg.contains("font-size=\"11.111\""),
            "字高 8 ⇒ em 11.111：{svg}"
        );
        assert!(svg.contains("中文"), "{svg}");
        assert!(svg.contains("#008000"), "文字颜色也走 ACI：{svg}");
        assert_eq!(
            svg.matches("<text").count(),
            1,
            "文字只能出现一次（不许 dxf_to_svg 再排一遍）"
        );
    }

    /// `dxf_to_svg` 的 padding 写死 1.0（viewBox = 内容框的 3 倍）⇒ 图纸只占窗口
    /// 中间九分之一。收紧后 100×100 的线框应落在 (0,0) 起的 108×108 上。
    #[test]
    fn viewbox_is_tightened_to_the_content_box() {
        let svg = cad_svg(DXF, None).expect("cad svg");
        assert!(
            svg.contains("viewBox=\"0.000 0.000 108.000 108.000\""),
            "{svg}"
        );
    }

    /// 区域通道：画布 = 区域 × 密度；文档外透明、文档内是白纸；同一份字节连问
    /// 两块区域走的是同一份缓存场景（内容各自正确）。
    #[test]
    fn region_is_rasterized_at_density_with_transparent_overflow() {
        let mut info = IvRenderOut::default();
        let inside = IvRegionReq {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        assert_eq!(
            export_region_impl(
                DXF.as_ptr(),
                DXF.len(),
                &inside,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (80, 80));
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            export_region_impl(
                DXF.as_ptr(),
                DXF.len(),
                &inside,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert!(px.chunks_exact(4).any(|p| p[3] == 255));

        // 远在文档之外（min 是负数、max 也小于 0）：整块透明。
        let outside = IvRegionReq {
            x: -500.0,
            y: -500.0,
            w: 20.0,
            h: 20.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        let mut info2 = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                DXF.as_ptr(),
                DXF.len(),
                &outside,
                std::ptr::null_mut(),
                0,
                &mut info2,
                std::ptr::null(),
            ),
            ERR_OK
        );
        let mut px2 = vec![0u8; info2.bytes_needed as usize];
        export_region_impl(
            DXF.as_ptr(),
            DXF.len(),
            &outside,
            px2.as_mut_ptr(),
            px2.len(),
            &mut info2,
            std::ptr::null(),
        );
        assert!(px2.chunks_exact(4).all(|p| p[3] == 0));
    }
}
