//! RuPix doc member: HP-GL / HP-GL/2.
//!
//! `nom-hpgl` parses the command stream; this member turns the supported
//! vector commands into an SVG scene and rasterizes it with the shared resvg
//! stack.
//!
//! ★ 笔色（2026-11-01）：HP-GL 的 `PC` 只出现在**设备无关**的那一半——喷墨/笔绘
//! 机器把它写成 `PC r,g,b;`（HP-GL/2）或 `PC pen,r,g,b;`（HP-GL）。`nom-hpgl`
//! 不解析 `PC`，而它的顶层解析是 `separated_list_complete`：**遇到不认识的命令
//! 就停在那里**，于是带 `PC` 的真实文件会被截断（后面的几何全丢）。所以这里先
//! 按 `;` 切一遍命令流，把 `PC` 取成"笔号 → 颜色"表并**从流里删掉**，几何再交给
//! `nom-hpgl`；场景按笔色分组输出。没有 `PC` 的文件按默认黑笔（绘图仪的默认笔色
//! 是设备相关的，不臆造颜色）。
//!
//! ★ 文字（2026-11-01 补）：`LB`（标签，`<ETX>` 结束）/ `SI`（字高）/ `DI`（方向）
//! 由 `nom-hpgl` 解析，本模块把它们排成 SVG `<text>` 交给 resvg——字体用系统字体
//! 栈，中文可用。`SI` 的第二个数是**字符高度**（按大写净高理解），`font-size`
//! 取 `高度 ÷ 0.72`，与 DXF `TEXT` 的组码 40 同一口径。标签正文里可以有 `;`，
//! 所以预处理**不能**在里面切分：只有 `<ETX>` 才结束标签。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use nom_hpgl::{Character, Command, LineFillAttribute, Vector};
use pro_support as support;
use rupix_abi::*;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::ffi::c_char;

const EXTS: &[&str] = &[
    "hp", "hpg", "plo", "hg", "hgl", "plt", "hp1", "hp2", "hpgl", "hpgl2", "gl2",
];

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

fn looks_like_hpgl(slice: &[u8]) -> bool {
    let head = &slice[..slice.len().min(256)];
    let Ok(text) = std::str::from_utf8(head) else {
        return false;
    };
    let upper = text.to_ascii_uppercase();
    upper.contains(';')
        && (upper.starts_with("IN;")
            || upper.starts_with("SP")
            || upper.contains("PU")
            || upper.contains("PD")
            || upper.contains("PA"))
}

/// 一条笔迹：段或圆，带当时的笔色。
struct Segment {
    a: (f32, f32),
    b: (f32, f32),
    color: String,
}

struct Circle {
    center: (f32, f32),
    radius: f32,
    color: String,
}

/// 一段 `LB` 文本：位置 = 当前笔位（基线左端），尺寸按 `SI`，角度按 `DI`。
struct TextItem {
    x: f32,
    y: f32,
    size: f32,
    angle: f32,
    text: String,
    color: String,
}

struct Scene {
    segments: Vec<Segment>,
    circles: Vec<Circle>,
    texts: Vec<TextItem>,
    min: (f32, f32),
    max: (f32, f32),
}

impl Scene {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            circles: Vec::new(),
            texts: Vec::new(),
            min: (f32::INFINITY, f32::INFINITY),
            max: (f32::NEG_INFINITY, f32::NEG_INFINITY),
        }
    }

    fn point(&mut self, p: (f32, f32)) {
        self.min.0 = self.min.0.min(p.0);
        self.min.1 = self.min.1.min(p.1);
        self.max.0 = self.max.0.max(p.0);
        self.max.1 = self.max.1.max(p.1);
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), color: &str) {
        self.point(a);
        self.point(b);
        self.segments.push(Segment {
            a,
            b,
            color: color.to_owned(),
        });
    }

    fn circle(&mut self, center: (f32, f32), radius: f32, color: &str) {
        self.point((center.0 - radius, center.1 - radius));
        self.point((center.0 + radius, center.1 + radius));
        self.circles.push(Circle {
            center,
            radius,
            color: color.to_owned(),
        });
    }

    fn text(&mut self, at: (f32, f32), text: &str, size: f32, angle: f32, color: &str) {
        let w = estimate_width(text, size);
        self.point((at.0, at.1 - size));
        self.point((at.0 + w, at.1 + size * 0.25));
        self.texts.push(TextItem {
            x: at.0,
            y: at.1,
            size,
            angle,
            text: text.to_owned(),
            color: color.to_owned(),
        });
    }

    fn svg(&self) -> String {
        let width = (self.max.0 - self.min.0).max(1.0);
        let height = (self.max.1 - self.min.1).max(1.0);
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {} {}\">",
            self.min.0, self.min.1, width, height
        );
        // 绘图仪 = 纸上的笔迹：补一张白纸（否则浅色棋盘格上"只剩几条线"）。
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>",
            self.min.0, self.min.1
        ));
        // 同一支笔的线段合成一条 path：颜色是 SVG 的呈现属性，不能挂在子路径上。
        let mut order: Vec<&str> = Vec::new();
        let mut paths: HashMap<&str, String> = HashMap::new();
        for seg in &self.segments {
            let entry = paths.entry(seg.color.as_str()).or_insert_with(|| {
                order.push(seg.color.as_str());
                String::new()
            });
            entry.push_str(&format!(
                "M{} {}L{} {} ",
                seg.a.0, seg.a.1, seg.b.0, seg.b.1
            ));
        }
        // 1 个用户单位在 1000+ 单位的画布上只有亚像素宽，缩到窗口就淡得看不见；
        // 按画布尺寸给一个下限（HP-GL 的 PW 没有解析，这里就是"默认笔宽"）。
        let stroke = (width.min(height) * 0.0018).max(1.0);
        for color in order {
            if let Some(d) = paths.get(color) {
                out.push_str(&format!(
                    "<path fill=\"none\" stroke=\"{color}\" stroke-width=\"{stroke:.2}\" d=\"{d}\"/>"
                ));
            }
        }
        for c in &self.circles {
            out.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke:.2}\"/>",
                c.center.0, c.center.1, c.radius, c.color
            ));
        }
        for t in &self.texts {
            let transform = if t.angle.abs() > 0.01 {
                format!(r#" transform="rotate({} {} {})""#, -t.angle, t.x, t.y)
            } else {
                String::new()
            };
            out.push_str(&format!(
                r#"<text x="{:.3}" y="{:.3}" font-size="{:.3}" fill="{}" font-family="Microsoft YaHei, Noto Sans CJK SC, PingFang SC, Segoe UI, sans-serif"{transform}>{}</text>"#,
                t.x,
                t.y,
                t.size,
                t.color,
                escape_xml(&t.text)
            ));
        }
        out.push_str("</svg>");
        out
    }
}

/// 标签宽度估算（HP-GL 只给了字高）：ASCII 约 0.58 em，CJK 约 1 em。
/// 只用来推进笔位——`LB` 之后的下一条命令从标签末尾起画。
fn estimate_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { 0.58 } else { 1.0 })
        .sum::<f32>()
        * size
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

/// HP-GL/2 的 `PC r,g,b` 与 HP-GL 的 `PC pen,r,g,b` 都用 0–100 的百分比；
/// 也有大量导出器直接写 0–255。按最大值判一次，不猜第二遍。
fn pen_color(r: f32, g: f32, b: f32) -> String {
    let scale = if r.max(g).max(b) > 100.5 { 1.0 } else { 2.55 };
    let f = |v: f32| (v * scale).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", f(r), f(g), f(b))
}

/// 按 `;` 切命令流，摘走 `PC`（`nom-hpgl` 不认它，留着会把后面的几何全截断），
/// 返回"可解析的命令流"与"笔号 → 颜色"表。
///
/// ★ 摘掉命令必须连分隔符一起摘：把 `PC…` 改成空串会留下 `;;`，而
/// `separated_list_complete` 在空命令处就会停——几何照样丢，只是换了个位置。
/// ★ `LB` 正文里可以有 `;`，只有 `<ETX>`（0x03）才结束标签，所以不能无脑切。
fn split_pen_colors(text: &str) -> (String, HashMap<i32, String>) {
    let mut kept: Vec<String> = Vec::new();
    let mut pens: HashMap<i32, String> = HashMap::new();
    let mut current = 1i32;
    let mut buf = String::new();
    let mut in_label = false;

    let flush = |buf: &mut String,
                 kept: &mut Vec<String>,
                 pens: &mut HashMap<i32, String>,
                 current: &mut i32| {
        let cmd = buf.trim().to_owned();
        buf.clear();
        if cmd.is_empty() {
            return;
        }
        let upper: String = cmd.chars().take(2).collect::<String>().to_ascii_uppercase();
        match upper.as_str() {
            "SP" => {
                if let Ok(n) = cmd[2..].trim().parse::<i32>() {
                    *current = n;
                }
                kept.push(cmd);
            }
            "PC" => {
                let nums: Vec<f32> = cmd[2..]
                    .split(',')
                    .filter_map(|t| t.trim().parse::<f32>().ok())
                    .collect();
                match nums.len() {
                    3 => {
                        pens.insert(*current, pen_color(nums[0], nums[1], nums[2]));
                    }
                    n if n >= 4 => {
                        pens.insert(nums[0] as i32, pen_color(nums[1], nums[2], nums[3]));
                    }
                    _ => {}
                }
            }
            _ => kept.push(cmd),
        }
    };

    for ch in text.chars() {
        if in_label {
            buf.push(ch);
            if ch == '\u{3}' {
                kept.push(std::mem::take(&mut buf));
                in_label = false;
            }
            continue;
        }
        if ch == ';' || ch == '\u{3}' {
            flush(&mut buf, &mut kept, &mut pens, &mut current);
            continue;
        }
        buf.push(ch);
        let head = buf.trim_start();
        if head.len() >= 2 && head[..2].eq_ignore_ascii_case("LB") && head.chars().next().is_some()
        {
            in_label = true;
        }
    }
    flush(&mut buf, &mut kept, &mut pens, &mut current);

    // ★ 拼回流时不能一律 `join(";")`：`LB…<ETX>` 已经用 ETX 当了分隔符，
    //   再补一个 `;` 就会产生**空命令**，`separated_list_complete` 在那里就停。
    let mut stream = String::new();
    for cmd in &kept {
        stream.push_str(cmd);
        if !cmd.ends_with('\u{3}') {
            stream.push(';');
        }
    }
    (stream, pens)
}

const DEFAULT_PEN: &str = "black";

fn add_arc(scene: &mut Scene, start: (f32, f32), center: (f32, f32), sweep_deg: f32, color: &str) {
    let start_angle = (start.1 - center.1).atan2(start.0 - center.0);
    let sweep = sweep_deg * PI / 180.0;
    let steps = ((sweep.abs() / (PI / 12.0)).ceil() as usize).clamp(2, 48);
    let radius = ((start.0 - center.0).powi(2) + (start.1 - center.1).powi(2)).sqrt();
    let mut prev = start;
    for i in 1..=steps {
        let angle = start_angle + sweep * (i as f32 / steps as f32);
        let next = (
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin(),
        );
        scene.line(prev, next, color);
        prev = next;
    }
}

fn build_scene(commands: &[Command<'_>], pens: &HashMap<i32, String>) -> Scene {
    let mut scene = Scene::new();
    let mut pos = (0.0f32, 0.0f32);
    let mut pen_down = false;
    let mut color = DEFAULT_PEN.to_owned();
    // `SI` 的第二个数（字符高度）与 `DI` 的角度；没给过就用 12 单位 / 水平。
    let mut char_h = 12.0f32;
    let mut angle = 0.0f32;

    for command in commands {
        match command {
            Command::LineFillAttribute(LineFillAttribute::SelectPen(pen)) => {
                color = pens
                    .get(pen)
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_PEN.to_owned());
                continue;
            }
            Command::Character(Character::AbsoluteCharacterSize(size)) => {
                if let Some((_w, h)) = size {
                    if *h > 0.0 {
                        char_h = *h;
                    }
                }
                continue;
            }
            Command::Character(Character::AbsoluteDirection(dir)) => {
                angle = dir.1.atan2(dir.0).to_degrees();
                continue;
            }
            Command::Character(Character::Label(raw)) => {
                let label = raw.trim_start_matches(' ').trim_end();
                if !label.is_empty() {
                    // SI 是字符高度（按大写净高理解）⇒ font-size = 高度 / 0.72。
                    scene.text(pos, label, char_h / 0.72, angle, &color);
                    // HP-GL 语义：`LB` 之后笔停在标签末尾。宽度按 **em**（= font-size）
                    // 估：按 SI 的字符高估会短 28%（CJK 一个字就是 1 em）。
                    let w = estimate_width(label, char_h / 0.72);
                    let rad = angle * PI / 180.0;
                    pos = (pos.0 + w * rad.cos(), pos.1 + w * rad.sin());
                }
                continue;
            }
            _ => {}
        }
        let Command::Vector(vector) = command else {
            continue;
        };
        match vector {
            Vector::PlotAbsolute(points) => {
                for &p in points {
                    pos = p;
                }
            }
            Vector::PlotRelative(points) => {
                for &p in points {
                    pos = (pos.0 + p.0, pos.1 + p.1);
                }
            }
            Vector::PenDown(points) => {
                pen_down = true;
                for &p in points {
                    if pen_down {
                        scene.line(pos, p, &color);
                    }
                    pos = p;
                }
            }
            Vector::PenUp(points) => {
                pen_down = false;
                for &p in points {
                    pos = p;
                }
            }
            Vector::PolylineEncoded(points) => {
                for &p in points {
                    if pen_down {
                        scene.line(pos, p, &color);
                    }
                    pos = p;
                }
            }
            Vector::Circle { radius, .. } => scene.circle(pos, *radius, &color),
            Vector::ArcAbsolute {
                center,
                sweep_angle,
                ..
            } => {
                add_arc(&mut scene, pos, *center, *sweep_angle, &color);
                let angle = (pos.1 - center.1).atan2(pos.0 - center.0) + sweep_angle * PI / 180.0;
                let radius = ((pos.0 - center.0).powi(2) + (pos.1 - center.1).powi(2)).sqrt();
                pos = (
                    center.0 + radius * angle.cos(),
                    center.1 + radius * angle.sin(),
                );
            }
            Vector::ArcRelative {
                increments,
                sweep_angle,
                ..
            } => {
                let center = (pos.0 + increments.0, pos.1 + increments.1);
                add_arc(&mut scene, pos, center, *sweep_angle, &color);
                let angle = (pos.1 - center.1).atan2(pos.0 - center.0) + sweep_angle * PI / 180.0;
                let radius = ((pos.0 - center.0).powi(2) + (pos.1 - center.1).powi(2)).sqrt();
                pos = (
                    center.0 + radius * angle.cos(),
                    center.1 + radius * angle.sin(),
                );
            }
            Vector::AbsoluteArcThreePoint {
                intermediate, end, ..
            } => {
                scene.line(pos, *intermediate, &color);
                scene.line(*intermediate, *end, &color);
                pos = *end;
            }
            Vector::RelativeArcThreePoint {
                intermediate, end, ..
            } => {
                let a = (pos.0 + intermediate.0, pos.1 + intermediate.1);
                let b = (pos.0 + end.0, pos.1 + end.1);
                scene.line(pos, a, &color);
                scene.line(a, b, &color);
                pos = b;
            }
        }
    }
    scene
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

/// HPGL 命令流 → SVG 文本（只认一遍命令，供缓存与两条渲染路线共用）。
fn scene_svg(slice: &[u8]) -> Result<String, (i32, &'static str)> {
    // 绘图仪年代的 PLT 里中文多半是 GBK：UTF-8 解不通就按 GBK 再试一次。
    let (text, _, _) = match std::str::from_utf8(slice) {
        Ok(t) => (std::borrow::Cow::Borrowed(t), false, false),
        Err(_) => {
            let (cow, _, _) = encoding_rs::GBK.decode(slice);
            (cow, false, false)
        }
    };
    let (stream, pens) = split_pen_colors(&text);
    let commands = match nom_hpgl::parse(&stream) {
        nom::IResult::Done(_rest, v) => v,
        _ => return Err((ERR_CORRUPT, "HPGL 命令解析失败")),
    };
    let scene = build_scene(&commands, &pens);
    if scene.segments.is_empty() && scene.circles.is_empty() && scene.texts.is_empty() {
        return Err((ERR_CORRUPT, "HPGL 没有可绘制的矢量"));
    }
    Ok(scene.svg())
}

thread_local! {
    /// 按文件字节指纹缓存已解析的场景（契约 §十六 第 4 条）。
    static SCENES: support::vector::DocCache<std::rc::Rc<resvg::usvg::Tree>> =
        const { support::vector::DocCache::new(3) };
}

/// ★ `usvg::Options::default()` 的字体库是**空的**：场景里的 `<text>` 一个字形也拿不到，
/// 直接被丢掉（`LB` 标签"解析对了却看不见"就是这么来的）。和其它矢量成员一样，
/// 装一份系统字体库、进程内只扫一次，`Arc` 克隆给每次解析。
fn font_db() -> &'static std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONT_DB: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONT_DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        std::sync::Arc::new(db)
    })
}

fn svg_options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        fontdb: font_db().clone(),
        ..Default::default()
    }
}

fn scene_tree(slice: &[u8]) -> Result<std::rc::Rc<resvg::usvg::Tree>, (i32, &'static str)> {
    let key = support::vector::doc_key(slice);
    if let Some(hit) = SCENES.with(|c| c.get(key)) {
        return Ok(hit);
    }
    let svg = scene_svg(slice)?;
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &svg_options())
        .map_err(|_| (ERR_CORRUPT, "HPGL 场景不是有效 SVG"))?;
    let tree = std::rc::Rc::new(tree);
    SCENES.with(|c| c.put(key, std::rc::Rc::clone(&tree)));
    Ok(tree)
}

fn rasterize(
    tree: &resvg::usvg::Tree,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let size = tree.size();
    let Some((nw, nh)) = positive_size(size.width(), size.height()) else {
        return Err((ERR_CORRUPT, "HPGL 画布尺寸为 0"));
    };
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let (w, h, scale) = target_size(nw, nh, (target_w, target_h));
    let (_, _, bytes) = support::check_dims(w, h, support::budget_of(req))?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "HPGL 画布分配失败"))?;
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
        if data.is_null() || len == 0 {
            return 0;
        }
        let ext = ext_str(ext);
        if !named_ext(ext.as_deref()) {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if looks_like_hpgl(slice) {
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
            set_last_error("不是 HPGL 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let tree = match scene_tree(slice) {
            Ok(t) => t,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (rgba, w, h) = match rasterize(&tree, req) {
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

/// `iv_render_region` 的业务实现（规范 §十六）：按密度重画指定区域。
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
            set_last_error("不是 HPGL 后缀");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let tree = match scene_tree(slice) {
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

    const SAMPLE: &[u8] = b"IN;SP1;PU0,0;PD100,0;PD100,100;PD0,100;PD0,0;";

    #[test]
    fn probes_named_hpgl() {
        assert_eq!(
            probe_impl(SAMPLE.as_ptr(), SAMPLE.len(), c"plt".as_ptr()),
            100
        );
        assert_eq!(
            probe_impl(SAMPLE.as_ptr(), SAMPLE.len(), std::ptr::null()),
            0
        );
    }

    #[test]
    fn renders_basic_pen_path() {
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        let code = render_impl(
            SAMPLE.as_ptr(),
            SAMPLE.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            c"plt".as_ptr(),
        );
        assert_eq!(code, ERR_OK);
        assert!(info.actual_w > 0 && info.actual_h > 0);
    }

    /// `PC` 是 nom-hpgl 不认识的命令：它既要把笔色带进场景，又**不许**把后面的
    /// 几何截断（`separated_list_complete` 遇到不认识的命令就停）。
    #[test]
    fn pen_colors_survive_without_truncating_geometry() {
        let src = b"IN;SP1;PC 0,0,100;PU0,0;PD100,0;SP2;PC 100,0,0;PU0,10;PD100,10;";
        let svg = scene_svg(src).expect("scene");
        assert!(svg.contains("stroke=\"#0000ff\""), "{svg}");
        assert!(svg.contains("stroke=\"#ff0000\""), "{svg}");
        // 两条线都在（第二段在 PC 之后，被截断就会只剩一条）。
        assert_eq!(svg.matches("L100 0 ").count(), 1, "{svg}");
        assert_eq!(svg.matches("L100 10 ").count(), 1, "{svg}");
    }

    /// HP-GL 的四参数写法 `PC pen,r,g,b`：给**指定**笔号上色。
    #[test]
    fn four_argument_pen_color_targets_a_named_pen() {
        let src = b"IN;SP1;PC 3,0,100,0;SP3;PU0,0;PD50,0;";
        let svg = scene_svg(src).expect("scene");
        assert!(svg.contains("stroke=\"#00ff00\""), "{svg}");
    }

    /// 没有 `PC` 的普通文件仍按默认黑笔（不臆造笔色）。
    #[test]
    fn files_without_pen_colors_stay_black() {
        let svg = scene_svg(SAMPLE).expect("scene");
        assert!(svg.contains("stroke=\"black\""), "{svg}");
    }

    /// `LB` 标签排成真 `<text>`：字号来自 `SI`（高度 ÷ 0.72），颜色来自当前笔。
    #[test]
    fn labels_render_as_real_text() {
        let src = b"IN;SP1;PC 0,0,100;SI 10,10;PU100,100;LBHello\x03;";
        let svg = scene_svg(src).expect("scene");
        assert!(svg.contains("<text"), "{svg}");
        assert!(svg.contains("Hello"), "{svg}");
        assert!(svg.contains("font-size=\"13.889\""), "{svg}");
        assert!(svg.contains("#0000ff"), "标签用当前笔色：{svg}");
    }

    /// ★ "SVG 里有 `<text>`"不等于"屏上有字"：`usvg::Options::default()` 的字体库是空的，
    /// 文字会被静默丢掉（m192 真机报障）。这条判据落在**像素**上。
    #[test]
    fn label_text_reaches_the_pixels() {
        let src = b"IN;SP1;SI 40,40;PU20,80;LBHi\x03;";
        let tree = scene_tree(src).expect("scene");
        let (rgba, w, h) = rasterize(&tree, std::ptr::null()).expect("raster");
        let text_box = (0..h.min(90))
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .any(|(x, y)| {
                let at = ((y * w + x) * 4) as usize;
                rgba[at] < 128 && rgba[at + 1] < 128 && rgba[at + 2] < 128
            });
        assert!(text_box, "画布右上（标签位置）一个深色像素都没有：{w}x{h}");
    }

    /// `LB` 正文里可以出现 `;`——只有 `<ETX>` 才结束标签，不能按 `;` 切。
    #[test]
    fn label_text_may_contain_semicolons() {
        let src = b"IN;SP1;PU0,0;LBa;b;c\x03;PU10,10;PD20,20;";
        let svg = scene_svg(src).expect("scene");
        assert!(svg.contains("a;b;c"), "{svg}");
        // 标签之后那条线必须还在（切分错了就会丢）。
        assert!(svg.contains("L20 20"), "{svg}");
    }

    /// 绘图仪年代的 PLT 中文多半是 GBK：UTF-8 解不通时按 GBK 再解一次。
    #[test]
    fn gbk_labels_are_decoded() {
        // ★ 必须用**真 GBK 字节**：`"中文".as_bytes()` 是 UTF-8，整份文件合法 UTF-8，
        //   第一跳就成功，GBK 分支根本没被走到（夹具不在判据管的那条分支上）。
        let mut gbk: Vec<u8> = Vec::new();
        gbk.extend_from_slice(&[0xd6, 0xd0, 0xce, 0xc4]);
        assert!(std::str::from_utf8(&gbk).is_err(), "夹具必须是非法 UTF-8");
        let mut src = b"IN;SP1;SI 10,10;PU10,10;LB".to_vec();
        src.extend_from_slice(&gbk);
        src.push(0x03);
        src.extend_from_slice(b";");
        let svg = scene_svg(&src).expect("scene");
        assert!(svg.contains("中文"), "{svg}");
    }

    /// 区域通道：画布跟目标走；文档外透明、文档内是不透明纸面。
    #[test]
    fn region_canvas_follows_density_and_overflow_is_transparent() {
        let inside = IvRegionReq {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                SAMPLE.as_ptr(),
                SAMPLE.len(),
                &inside,
                std::ptr::null_mut(),
                0,
                &mut info,
                c"plt".as_ptr(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (80, 80));

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
                SAMPLE.as_ptr(),
                SAMPLE.len(),
                &outside,
                std::ptr::null_mut(),
                0,
                &mut info2,
                c"plt".as_ptr(),
            ),
            ERR_OK
        );
        let mut px = vec![0u8; info2.bytes_needed as usize];
        assert_eq!(
            export_region_impl(
                SAMPLE.as_ptr(),
                SAMPLE.len(),
                &outside,
                px.as_mut_ptr(),
                px.len(),
                &mut info2,
                c"plt".as_ptr(),
            ),
            ERR_OK
        );
        assert!(px.chunks_exact(4).all(|p| p[3] == 0));
    }
}
