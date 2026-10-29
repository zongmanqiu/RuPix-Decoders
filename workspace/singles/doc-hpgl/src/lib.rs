//! RuPix doc member: HP-GL / HP-GL/2.
//!
//! `nom-hpgl` parses the command stream; this member turns the supported
//! vector commands into an SVG scene and rasterizes it with the shared resvg
//! stack. Text and advanced pen attributes are intentionally deferred: the
//! first version must render pen paths without inventing glyph shapes.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use nom_hpgl::{Command, Vector};
use pro_support as support;
use rupix_abi::*;
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

struct Scene {
    segments: Vec<((f32, f32), (f32, f32))>,
    circles: Vec<((f32, f32), f32)>,
    min: (f32, f32),
    max: (f32, f32),
}

impl Scene {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            circles: Vec::new(),
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

    fn line(&mut self, a: (f32, f32), b: (f32, f32)) {
        self.point(a);
        self.point(b);
        self.segments.push((a, b));
    }

    fn circle(&mut self, center: (f32, f32), radius: f32) {
        self.point((center.0 - radius, center.1 - radius));
        self.point((center.0 + radius, center.1 + radius));
        self.circles.push((center, radius));
    }

    fn svg(&self) -> String {
        let width = (self.max.0 - self.min.0).max(1.0);
        let height = (self.max.1 - self.min.1).max(1.0);
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {} {}\">",
            self.min.0, self.min.1, width, height
        );
        if !self.segments.is_empty() {
            out.push_str("<path fill=\"none\" stroke=\"black\" stroke-width=\"1\" d=\"");
            for (a, b) in &self.segments {
                out.push_str(&format!("M{} {}L{} {} ", a.0, a.1, b.0, b.1));
            }
            out.push_str("\"/>");
        }
        for ((x, y), r) in &self.circles {
            out.push_str(&format!(
                "<circle cx=\"{x}\" cy=\"{y}\" r=\"{r}\" fill=\"none\" stroke=\"black\" stroke-width=\"1\"/>"
            ));
        }
        out.push_str("</svg>");
        out
    }
}

fn add_arc(scene: &mut Scene, start: (f32, f32), center: (f32, f32), sweep_deg: f32) {
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
        scene.line(prev, next);
        prev = next;
    }
}

fn build_scene(commands: &[Command<'_>]) -> Scene {
    let mut scene = Scene::new();
    let mut pos = (0.0f32, 0.0f32);
    let mut pen_down = false;

    for command in commands {
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
                        scene.line(pos, p);
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
                        scene.line(pos, p);
                    }
                    pos = p;
                }
            }
            Vector::Circle { radius, .. } => scene.circle(pos, *radius),
            Vector::ArcAbsolute {
                center,
                sweep_angle,
                ..
            } => {
                add_arc(&mut scene, pos, *center, *sweep_angle);
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
                add_arc(&mut scene, pos, center, *sweep_angle);
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
                scene.line(pos, *intermediate);
                scene.line(*intermediate, *end);
                pos = *end;
            }
            Vector::RelativeArcThreePoint {
                intermediate, end, ..
            } => {
                let a = (pos.0 + intermediate.0, pos.1 + intermediate.1);
                let b = (pos.0 + end.0, pos.1 + end.1);
                scene.line(pos, a);
                scene.line(a, b);
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

fn rasterize(
    svg: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .map_err(|_| (ERR_CORRUPT, "HPGL 场景不是有效 SVG"))?;
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
        let Ok(text) = std::str::from_utf8(slice) else {
            set_last_error("HPGL 不是 UTF-8 文本");
            return ERR_UNRECOGNIZED;
        };
        let commands = match nom_hpgl::parse(text) {
            nom::IResult::Done(_rest, v) => v,
            _ => {
                set_last_error("HPGL 命令解析失败");
                return ERR_CORRUPT;
            }
        };
        let scene = build_scene(&commands);
        if scene.segments.is_empty() && scene.circles.is_empty() {
            set_last_error("HPGL 没有可绘制的矢量");
            return ERR_CORRUPT;
        }
        let (rgba, w, h) = match rasterize(scene.svg().as_bytes(), req) {
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
}
