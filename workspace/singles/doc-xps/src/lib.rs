//! RuPix doc member: XPS / OpenXPS.
//!
//! An XPS file is an OPC (ZIP) package of XML parts; the visible content lives
//! in each page's `FixedPage` (Path / Canvas / Glyphs elements, brushes in
//! resources). This member reads the package chain (`_rels/.rels` →
//! FixedDocumentSequence → FixedDocument → FixedPage), translates each page's
//! vector content into SVG, and hands it to the shared resvg rasterizer.
//!
//! ★ 2026-11-01：此前用的是 `justpdf-formats` 的 XPS 支持，它的 `render_page`
//! 是**占位实现**（只返回一张纯白页，"v1: return white page"）——所以真机上
//! XPS/OXPS 永远是一张白纸。这里改成自己渲染：路径几何是 SVG 兼容的 mini-language，
//! 画刷（SolidColorBrush / LinearGradientBrush / 径向近似）与 RenderTransform /
//! Clip 都映射过去，文字用系统字体近似（XPS 的内嵌 ODTTF 字体暂不反混淆）。
//!
//! 已知限制（如实记录）：`Image` 元素暂不渲染；`VisualBrush`/`ImageBrush` 当成
//! 不画；`Glyphs` 的字体度量按系统字体近似（跨机像素可能不同——这是 ABI §十六
//! 已登记的字体例外）。
//!
//! 区域渲染（ABI v6）：页面就是一棵 usvg 场景，`export_region_impl` 与其它矢量
//! 成员走同一段共用件（`pro_support::vector`），按密度重画。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use roxmltree::{Document, Node};
use rupix_abi::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_char;
use std::io::{Cursor, Read};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use zip::ZipArchive;

const XAML_NS: &str = "http://schemas.microsoft.com/winfx/2006/xaml";

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

fn parse_f64(s: &str) -> Option<f64> {
    s.trim().trim_end_matches("px").parse().ok()
}

// ── OPC 包结构 ──────────────────────────────────────────────────────

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

/// 单个 XML 部件的大小上限：XPS 页面/关系文件都是小文本，16 MiB 之外的
/// 只可能是 zip 炸弹（解压比没有下界，不设闸就会先占满内存再报错）。
const MAX_PART_BYTES: u64 = 16 * 1024 * 1024;

fn read_text(zip: &mut Archive<'_>, name: &str) -> Option<String> {
    let name = name.trim_start_matches('/');
    let f = zip.by_name(name).ok()?;
    let mut s = String::new();
    let mut limited = f.take(MAX_PART_BYTES + 1);
    limited.read_to_string(&mut s).ok()?;
    (s.len() as u64 <= MAX_PART_BYTES).then_some(s)
}

fn resolve_path(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if let Some(stripped) = rel.strip_prefix('/') {
        return stripped.to_string();
    }
    match base.rfind('/') {
        Some(i) => format!("{}/{}", &base[..i], rel),
        None => rel.to_string(),
    }
}

fn sources_of(xml: &str, tag: &str) -> Vec<String> {
    let Ok(doc) = Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == tag)
        .filter_map(|n| n.attribute("Source").map(|s| s.to_string()))
        .collect()
}

fn find_fdseq(zip: &mut Archive<'_>) -> Option<String> {
    if let Some(rels) = read_text(zip, "_rels/.rels") {
        if let Ok(doc) = Document::parse(&rels) {
            for n in doc
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
            {
                let ty = n.attribute("Type").unwrap_or_default().to_ascii_lowercase();
                if ty.contains("fixeddocumentsequence") {
                    if let Some(target) = n.attribute("Target") {
                        return Some(target.trim_start_matches('/').to_string());
                    }
                }
            }
        }
    }
    for known in [
        "FixedDocumentSequence.fdseq",
        "FixedDocSeq.fdseq",
        "Documents/FixedDocumentSequence.fdseq",
    ] {
        if read_text(zip, known).is_some() {
            return Some(known.to_string());
        }
    }
    let mut found = None;
    for i in 0..zip.len() {
        if let Ok(f) = zip.by_index(i) {
            if f.name().ends_with(".fdseq") {
                found = Some(f.name().to_string());
                break;
            }
        }
    }
    found
}

fn page_paths(zip: &mut Archive<'_>) -> Result<Vec<String>, (i32, &'static str)> {
    let Some(fdseq) = find_fdseq(zip) else {
        return Err((ERR_CORRUPT, "XPS 包里找不到 FixedDocumentSequence"));
    };
    let Some(xml) = read_text(zip, &fdseq) else {
        return Err((ERR_CORRUPT, "XPS 的 FixedDocumentSequence 读不出来"));
    };
    let mut docs = Vec::new();
    for src in sources_of(&xml, "DocumentReference") {
        docs.push(resolve_path(&fdseq, &src));
    }
    if docs.is_empty() {
        return Err((ERR_CORRUPT, "XPS 的 FixedDocumentSequence 里没有文档引用"));
    }
    let mut pages = Vec::new();
    for doc_path in docs {
        let Some(xml) = read_text(zip, &doc_path) else {
            return Err((ERR_CORRUPT, "XPS 的 FixedDocument 读不出来"));
        };
        for src in sources_of(&xml, "PageContent") {
            pages.push(resolve_path(&doc_path, &src));
        }
    }
    if pages.is_empty() {
        return Err((ERR_CORRUPT, "XPS 没有页面"));
    }
    Ok(pages)
}

// ── 颜色 / 画刷 / 变换 ─────────────────────────────────────────────

fn css_color(value: &str) -> Option<String> {
    let v = value.trim();
    if let Some(h) = v.strip_prefix('#') {
        return match h.len() {
            // XPS 的 `#AARRGGBB`（alpha 暂不参与；不透明页面的内容都是 FF）。
            8 => Some(format!("#{}", &h[2..])),
            6 => Some(format!("#{h}")),
            3 => Some(format!("#{h}")),
            _ => None,
        };
    }
    if let Some(sc) = v.strip_prefix("sc#") {
        let parts: Vec<f32> = sc
            .split(',')
            .filter_map(|x| x.trim().parse::<f32>().ok())
            .collect();
        if parts.len() == 4 {
            // scRGB 是线性光：转回 sRGB 近似（1/2.2 幂）。
            let f = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
            return Some(format!(
                "#{:02x}{:02x}{:02x}",
                f(parts[1]),
                f(parts[2]),
                f(parts[3])
            ));
        }
        return None;
    }
    let lower = v.to_ascii_lowercase();
    let named = match lower.as_str() {
        "white" => "#ffffff",
        "black" => "#000000",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "cyan" | "aqua" => "#00ffff",
        "magenta" | "fuchsia" => "#ff00ff",
        "gray" | "grey" => "#808080",
        "orange" => "#ffa500",
        "purple" => "#800080",
        "transparent" => "none",
        _ => return None,
    };
    Some(named.to_string())
}

/// `{StaticResource key}` / `{DynamicResource key}` → key。
fn resource_key(value: &str) -> Option<&str> {
    let v = value.trim();
    let inner = v.strip_prefix('{')?.strip_suffix('}')?;
    let mut it = inner.splitn(2, char::is_whitespace);
    let kind = it.next()?;
    if kind != "StaticResource" && kind != "DynamicResource" {
        return None;
    }
    it.next().map(str::trim)
}

fn key_of(node: Node) -> Option<String> {
    node.attribute((XAML_NS, "Key"))
        .or_else(|| node.attribute("Key"))
        .map(|s| s.to_string())
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn strip_fill_rule(d: &str) -> (&'static str, &str) {
    let t = d.trim_start();
    if let Some(rest) = t.strip_prefix("F0") {
        ("nonzero", rest.trim_start())
    } else if let Some(rest) = t.strip_prefix("F1") {
        ("evenodd", rest.trim_start())
    } else {
        ("nonzero", t)
    }
}

fn transform_attr(value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    if v.is_empty() {
        return None;
    }
    if v.contains('(') {
        // WPF 的变换 mini-language 与 SVG 同名同参（rotate/scale/translate/matrix/...）。
        return Some(v.to_string());
    }
    let parts: Vec<&str> = v
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() == 6 {
        return Some(format!("matrix({})", parts.join(",")));
    }
    None
}

fn family_of(font_uri: &str) -> String {
    let name = font_uri
        .rsplit('/')
        .next()
        .unwrap_or(font_uri)
        .split('.')
        .next()
        .unwrap_or(font_uri);
    let lower = name.to_ascii_lowercase();
    let family = if lower.contains("arial") {
        "Arial"
    } else if lower.contains("times") {
        "Times New Roman"
    } else if lower.contains("courier") || lower.contains("consol") {
        "Courier New"
    } else if lower.contains("dengxian") {
        "DengXian"
    } else if lower.contains("simsun") || lower.contains("song") {
        "SimSun"
    } else if lower.contains("yahei") || lower.contains("heiti") {
        "Microsoft YaHei"
    } else {
        "sans-serif"
    };
    family.to_string()
}

struct Ctx {
    defs: String,
    brushes: HashMap<String, String>,
    next_id: usize,
}

impl Ctx {
    fn new() -> Self {
        Self {
            defs: String::new(),
            brushes: HashMap::new(),
            next_id: 0,
        }
    }

    fn id(&mut self) -> String {
        let id = format!("g{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn paint(&mut self, value: Option<&str>, default: &str) -> String {
        let Some(v) = value else {
            return default.to_string();
        };
        let v = v.trim();
        if v.is_empty() {
            return default.to_string();
        }
        if let Some(key) = resource_key(v) {
            return self
                .brushes
                .get(key)
                .cloned()
                .unwrap_or_else(|| default.to_string());
        }
        css_color(v).unwrap_or_else(|| default.to_string())
    }

    fn clip(&mut self, value: Option<&str>) -> Option<String> {
        let v = value?.trim();
        if v.is_empty() {
            return None;
        }
        let (rule, d) = strip_fill_rule(v);
        let id = self.id();
        self.defs.push_str(&format!(
            "<clipPath id=\"{id}\"><path d=\"{}\" clip-rule=\"{rule}\"/></clipPath>",
            escape_attr(d)
        ));
        Some(format!("url(#{id})"))
    }

    /// 收集页面里所有带 `x:Key` 的画刷（资源字典可以在任意层级）。
    fn collect_brushes(&mut self, root: Node) {
        for n in root.descendants().filter(|n| n.is_element()) {
            let Some(key) = key_of(n) else { continue };
            let paint = match n.tag_name().name() {
                "SolidColorBrush" => n
                    .attribute("Color")
                    .and_then(css_color)
                    .unwrap_or_else(|| "#000000".to_string()),
                "LinearGradientBrush" => self.linear_gradient(n),
                "RadialGradientBrush" => {
                    // 径向不做精确映射：取中间那个色标当纯色（比"整块消失"接近）。
                    let stops = gradient_stops(n);
                    stops
                        .get(stops.len() / 2)
                        .map(|(_, c)| c.clone())
                        .unwrap_or_else(|| "#000000".to_string())
                }
                "ImageBrush" | "VisualBrush" => "none".to_string(),
                _ => continue,
            };
            self.brushes.insert(key, paint);
        }
    }

    fn linear_gradient(&mut self, node: Node) -> String {
        let stops = gradient_stops(node);
        if stops.is_empty() {
            return "#000000".to_string();
        }
        let point = |attr: &str, dx: f64, dy: f64| -> (f64, f64) {
            node.attribute(attr)
                .map(|s| {
                    let mut it = s.split(|c: char| c == ',' || c.is_whitespace());
                    let x = it.next().and_then(parse_f64).unwrap_or(dx);
                    let y = it.next().and_then(parse_f64).unwrap_or(dy);
                    (x, y)
                })
                .unwrap_or((dx, dy))
        };
        let (x1, y1) = point("StartPoint", 0.0, 0.0);
        let (x2, y2) = point("EndPoint", 1.0, 0.0);
        let absolute = node
            .attribute("MappingMode")
            .map(|m| m.eq_ignore_ascii_case("Absolute"))
            .unwrap_or(false);
        let id = self.id();
        let units = if absolute {
            "userSpaceOnUse"
        } else {
            "objectBoundingBox"
        };
        let mut def = format!(
            "<linearGradient id=\"{id}\" gradientUnits=\"{units}\" x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\">"
        );
        for (offset, color) in &stops {
            def.push_str(&format!(
                "<stop offset=\"{}\" stop-color=\"{color}\"/>",
                offset.clamp(0.0, 1.0)
            ));
        }
        def.push_str("</linearGradient>");
        self.defs.push_str(&def);
        format!("url(#{id})")
    }
}

fn gradient_stops(node: Node) -> Vec<(f64, String)> {
    let mut stops: Vec<(f64, String)> = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "GradientStop")
        .filter_map(|n| {
            let offset = n.attribute("Offset").and_then(parse_f64).unwrap_or(0.0);
            let color = n.attribute("Color").and_then(css_color)?;
            Some((offset, color))
        })
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    stops
}

// ── FixedPage → SVG ────────────────────────────────────────────────

fn emit_path(node: Node, ctx: &mut Ctx, out: &mut String) {
    let Some(data) = node.attribute("Data") else {
        return;
    };
    let (rule, d) = strip_fill_rule(data);
    let fill = ctx.paint(node.attribute("Fill"), "none");
    let stroke = ctx.paint(node.attribute("Stroke"), "none");
    if fill == "none" && stroke == "none" {
        return;
    }
    let mut path = format!(
        "<path d=\"{}\" fill=\"{fill}\" fill-rule=\"{rule}\"",
        escape_attr(d)
    );
    if stroke != "none" {
        let width = node
            .attribute("StrokeThickness")
            .and_then(parse_f64)
            .unwrap_or(1.0);
        path.push_str(&format!(" stroke=\"{stroke}\" stroke-width=\"{width}\""));
        let linecap = |attr: &str| -> &'static str {
            match node.attribute(attr).unwrap_or("Flat") {
                "Round" => "round",
                "Square" => "square",
                _ => "butt",
            }
        };
        let start = linecap("StrokeStartLineCap");
        let end = linecap("StrokeEndLineCap");
        if start != "butt" {
            path.push_str(&format!(" stroke-linecap=\"{start}\""));
        } else if end != "butt" {
            path.push_str(&format!(" stroke-linecap=\"{end}\""));
        }
        let join = match node.attribute("StrokeLineJoin").unwrap_or("Miter") {
            "Round" => "round",
            "Bevel" => "bevel",
            _ => "miter",
        };
        path.push_str(&format!(" stroke-linejoin=\"{join}\""));
        if let Some(limit) = node.attribute("StrokeMiterLimit").and_then(parse_f64) {
            path.push_str(&format!(" stroke-miterlimit=\"{limit}\""));
        }
        if let Some(dash) = node.attribute("StrokeDashArray") {
            let joined: Vec<&str> = dash
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .collect();
            if !joined.is_empty() {
                path.push_str(&format!(" stroke-dasharray=\"{}\"", joined.join(",")));
            }
        }
    }
    path.push_str("/>");

    let transform = transform_attr(node.attribute("RenderTransform"));
    let clip = ctx.clip(node.attribute("Clip"));
    let opacity = node.attribute("Opacity").and_then(parse_f64);
    wrap(out, transform, clip, opacity, &path);
}

fn emit_canvas(node: Node, ctx: &mut Ctx, out: &mut String, depth: usize) {
    let mut inner = String::new();
    walk(node, ctx, &mut inner, depth + 1);
    if inner.is_empty() {
        return;
    }
    let transform = transform_attr(node.attribute("RenderTransform"));
    let clip = ctx.clip(node.attribute("Clip"));
    let opacity = node.attribute("Opacity").and_then(parse_f64);
    wrap(out, transform, clip, opacity, &inner);
}

fn emit_glyphs(node: Node, ctx: &mut Ctx, out: &mut String) {
    let Some(text) = node.attribute("UnicodeString") else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let x = node.attribute("OriginX").and_then(parse_f64).unwrap_or(0.0);
    let y = node.attribute("OriginY").and_then(parse_f64).unwrap_or(0.0);
    let size = node
        .attribute("FontRenderingEmSize")
        .and_then(parse_f64)
        .unwrap_or(12.0);
    let fill = ctx.paint(node.attribute("Fill"), "#000000");
    let family = node
        .attribute("FontUri")
        .map(family_of)
        .unwrap_or_else(|| "sans-serif".to_string());
    let inner = format!(
        "<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\" font-family=\"{}\" fill=\"{fill}\">{}</text>",
        escape_attr(&family),
        escape_text(text)
    );
    let transform = transform_attr(node.attribute("RenderTransform"));
    let clip = ctx.clip(node.attribute("Clip"));
    let opacity = node.attribute("Opacity").and_then(parse_f64);
    wrap(out, transform, clip, opacity, &inner);
}

fn wrap(
    out: &mut String,
    transform: Option<String>,
    clip: Option<String>,
    opacity: Option<f64>,
    inner: &str,
) {
    if transform.is_none() && clip.is_none() && opacity.is_none() {
        out.push_str(inner);
        return;
    }
    out.push_str("<g");
    if let Some(t) = transform {
        out.push_str(&format!(" transform=\"{}\"", escape_attr(&t)));
    }
    if let Some(c) = clip {
        out.push_str(&format!(" clip-path=\"{c}\""));
    }
    if let Some(o) = opacity {
        out.push_str(&format!(" opacity=\"{}\"", o.clamp(0.0, 1.0)));
    }
    out.push('>');
    out.push_str(inner);
    out.push_str("</g>");
}

/// 嵌套上限：Canvas 可以套 Canvas，输入驱动的递归必须有上界
/// （规范 §六 第 10 条：挂死/爆栈的源头是输入字节，不是作者）。
const MAX_NEST: usize = 64;

fn walk(parent: Node, ctx: &mut Ctx, out: &mut String, depth: usize) {
    if depth > MAX_NEST {
        return;
    }
    for child in parent.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "Path" => emit_path(child, ctx, out),
            "Canvas" => emit_canvas(child, ctx, out, depth),
            "Glyphs" => emit_glyphs(child, ctx, out),
            // `Image` / 属性元素（`Canvas.Resources` 等）暂不渲染。
            _ => {}
        }
    }
}

fn build_svg(xml: &str) -> Result<(u32, u32, String), (i32, &'static str)> {
    let doc = Document::parse(xml).map_err(|_| (ERR_CORRUPT, "XPS 页面 XML 解析失败"))?;
    let root = doc.root_element();
    let width = root.attribute("Width").and_then(parse_f64).unwrap_or(816.0);
    let height = root
        .attribute("Height")
        .and_then(parse_f64)
        .unwrap_or(1056.0);
    if !(width > 0.0 && height > 0.0) {
        return Err((ERR_CORRUPT, "XPS 页面尺寸为 0"));
    }
    let mut ctx = Ctx::new();
    ctx.collect_brushes(root);
    let mut body = String::new();
    walk(root, &mut ctx, &mut body, 0);
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
viewBox=\"0 0 {width} {height}\"><defs>{}</defs>\
<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>{body}</svg>",
        ctx.defs
    );
    Ok((
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
        svg,
    ))
}

struct XpsPage {
    width: u32,
    height: u32,
    svg: String,
    tree: RefCell<Option<Rc<resvg::usvg::Tree>>>,
}

struct XpsDoc {
    pages: Vec<XpsPage>,
}

fn open_doc(data: &[u8]) -> Result<XpsDoc, (i32, &'static str)> {
    let mut zip =
        ZipArchive::new(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "XPS 包解析失败"))?;
    let paths = page_paths(&mut zip)?;
    let mut pages = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(xml) = read_text(&mut zip, &path) else {
            return Err((ERR_CORRUPT, "XPS 页面文件读不出来"));
        };
        let (width, height, svg) = build_svg(&xml)?;
        pages.push(XpsPage {
            width,
            height,
            svg,
            tree: RefCell::new(None),
        });
    }
    Ok(XpsDoc { pages })
}

thread_local! {
    /// 按文件字节指纹缓存整份包（契约 §十六 第 4 条）。
    static DOCS: support::DocCache<Rc<XpsDoc>> = const { support::DocCache::new(2) };
}

fn doc_of(data: &[u8]) -> Result<Rc<XpsDoc>, (i32, &'static str)> {
    let key = support::doc_key(data);
    if let Some(hit) = DOCS.with(|c| c.get(key)) {
        return Ok(hit);
    }
    let doc = Rc::new(open_doc(data)?);
    DOCS.with(|c| c.put(key, Rc::clone(&doc)));
    Ok(doc)
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

fn page_tree(page: &XpsPage) -> Result<Rc<resvg::usvg::Tree>, (i32, &'static str)> {
    if let Some(hit) = page.tree.borrow().as_ref() {
        return Ok(Rc::clone(hit));
    }
    let tree = resvg::usvg::Tree::from_data(page.svg.as_bytes(), &svg_options())
        .map_err(|_| (ERR_CORRUPT, "XPS 页面场景不是有效 SVG"))?;
    let tree = Rc::new(tree);
    *page.tree.borrow_mut() = Some(Rc::clone(&tree));
    Ok(tree)
}

fn pick_page(doc: &XpsDoc, req: *const IvRenderReq) -> Result<usize, (i32, &'static str)> {
    let idx = if req.is_null() {
        0
    } else {
        unsafe { (*req).page_index as usize }
    };
    if idx < doc.pages.len() {
        Ok(idx)
    } else {
        Err((ERR_CORRUPT, "XPS 页码越界"))
    }
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

fn render_tree(
    tree: &resvg::usvg::Tree,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let size = tree.size();
    let nw = size.width().round().max(1.0) as u32;
    let nh = size.height().round().max(1.0) as u32;
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, scale) = target_size(nw, nh, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "XPS 画布分配失败"))?;
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
        let doc = match doc_of(slice) {
            Ok(d) => d,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let idx = match pick_page(&doc, req) {
            Ok(i) => i,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let page = &doc.pages[idx];
        let (target_w, target_h) = if req.is_null() {
            (0, 0)
        } else {
            unsafe { ((*req).target_w, (*req).target_h) }
        };
        let (w, h, _) = target_size(page.width, page.height, (target_w, target_h));
        if out.is_null() {
            if let Err((code, msg)) = support::check_dims(w, h, support::budget_of(req)) {
                set_last_error(msg);
                return code;
            }
            support::fill_info(info, w, h, w as u64 * h as u64 * 4, 1, 0);
            unsafe { (*info).page_count = doc.pages.len() as u32 };
            return ERR_OK;
        }
        let tree = match page_tree(page) {
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
        let code = support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1);
        if code == ERR_OK {
            unsafe { (*info).page_count = doc.pages.len() as u32 };
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
        if !is_xps_ext(ext.as_deref()) {
            set_last_error("不是 XPS/OXPS 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if !is_zip(slice) {
            set_last_error("不是 XPS 的 OPC/ZIP 字节特征");
            return ERR_UNRECOGNIZED;
        }
        let doc = match doc_of(slice) {
            Ok(d) => d,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let idx = if (unsafe { (*req).page_index } as usize) < doc.pages.len() {
            unsafe { (*req).page_index as usize }
        } else {
            0
        };
        let page = &doc.pages[idx];
        let tree = match page_tree(page) {
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
        unsafe { (*info).page_count = doc.pages.len() as u32 };
        ERR_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// 最小 XPS：一页 200×200，白底 + 一条红色竖条（x=100..120）。
    pub(crate) fn sample() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            let mut put = |name: &str, text: &str| {
                zip.start_file(name, opts).unwrap();
                zip.write_all(text.as_bytes()).unwrap();
            };
            put(
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.microsoft.com/xps/2005/06/fixedrepresentation" Target="/FixedDocSeq.fdseq" Id="R0"/></Relationships>"#,
            );
            put(
                "FixedDocSeq.fdseq",
                r#"<?xml version="1.0"?><FixedDocumentSequence xmlns="http://schemas.microsoft.com/xps/2005/06"><DocumentReference Source="/Documents/1/FixedDoc.fdoc"/></FixedDocumentSequence>"#,
            );
            put(
                "Documents/1/FixedDoc.fdoc",
                r#"<?xml version="1.0"?><FixedDocument xmlns="http://schemas.microsoft.com/xps/2005/06"><PageContent Source="Pages/1.fpage"/></FixedDocument>"#,
            );
            put(
                "Documents/1/Pages/1.fpage",
                r##"<?xml version="1.0"?><FixedPage xmlns="http://schemas.microsoft.com/xps/2005/06" Width="200" Height="200"><Path Fill="#FFE02020" Data="M 100,0 L 120,0 L 120,200 L 100,200 Z"/></FixedPage>"##,
            );
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn probes_named_xps_zip() {
        assert_eq!(probe_impl(b"PK\x03\x04".as_ptr(), 4, c"xps".as_ptr()), 100);
        assert_eq!(probe_impl(b"PK\x03\x04".as_ptr(), 4, c"pdf".as_ptr()), 0);
        assert_eq!(probe_impl(b"not zip".as_ptr(), 7, c"xps".as_ptr()), 0);
    }

    #[test]
    fn renders_page_content_instead_of_a_blank_sheet() {
        let data = sample();
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                c"xps".as_ptr(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (200, 200));
        assert_eq!(info.page_count, 1);
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            render_impl(
                data.as_ptr(),
                data.len(),
                &req,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                c"xps".as_ptr(),
            ),
            ERR_OK
        );
        let at = |x: usize, y: usize| {
            let i = (y * 200 + x) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        assert_eq!(at(10, 10), [255, 255, 255], "页底必须是白纸");
        let red = at(110, 100);
        assert!(
            red[0] > 180 && red[1] < 80 && red[2] < 80,
            "红条没画出来：{red:?}"
        );
    }

    /// 区域通道按密度重画：区域 (90,0,20,20) 4 倍 ⇒ 画布 80×80，红条在 x=40..80。
    #[test]
    fn region_is_rasterized_at_density() {
        let data = sample();
        let req = IvRegionReq {
            x: 90.0,
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
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                c"xps".as_ptr(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (80, 80));
        let mut px = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            export_region_impl(
                data.as_ptr(),
                data.len(),
                &req,
                px.as_mut_ptr(),
                px.len(),
                &mut info,
                c"xps".as_ptr(),
            ),
            ERR_OK
        );
        let at = |x: usize| {
            let i = (10 * 80 + x) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        assert_eq!(at(30), [255, 255, 255], "红条左侧必须白");
        let red = at(50);
        assert!(red[0] > 180 && red[1] < 80, "红条必须在 x=40..80：{red:?}");
    }
}
