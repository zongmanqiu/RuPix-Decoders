//! m84-A 量具：**画幅边缘**。把三件事放在一起对撞——
//! ① rawler 元数据报的 `active_area` / `crop_area` / `blackareas`（dummy 与真解各一次，
//!    验两阶段一致性），② 现路显影出来的**呈现像素**上实测的黑边宽度，
//! ③ 按元数据裁之后还剩多少画面、宽高比变成什么。
//!
//! 跑法：`cargo run --release -p rupix-raw --example probe_raw_frame`（可选参数：样本目录）。

use std::path::{Path, PathBuf};

fn samples_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let arg = std::env::args().nth(1);
    match arg {
        Some(p) => PathBuf::from(p),
        None => manifest
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".temp")
            .join("raw-samples"),
    }
}

fn rect_str(r: &Option<rawler::imgop::Rect>) -> String {
    match r {
        None => "None".to_string(),
        Some(v) => format!(
            "x0={} y0={} {}x{} (LTRB [{}, {}, {}, {}])",
            v.x(),
            v.y(),
            v.width(),
            v.height(),
            v.p.x,
            v.p.y,
            v.p.x + v.d.w,
            v.p.y + v.d.h
        ),
    }
}

/// 元数据里那圈遮光区（LTRB 像素数），由画幅矩形反推。
fn borders(r: &rawler::imgop::Rect, w: usize, h: usize) -> [usize; 4] {
    [
        r.x(),
        r.y(),
        w.saturating_sub(r.x() + r.width()),
        h.saturating_sub(r.y() + r.height()),
    ]
}

fn describe(tag: &str, img: &rawler::RawImage) -> String {
    let (pw, ph) = match &img.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) => raw::cfa_period(&cfg.cfa),
        _ => (0, 0),
    };
    let kind = match &img.data {
        rawler::RawImageData::Integer(p) => format!("Integer[{}]", p.len()),
        rawler::RawImageData::Float(p) => format!("Float[{}]", p.len()),
    };
    let ba: Vec<String> = img.blackareas.iter().map(|r| rect_str(&Some(*r))).collect();
    format!(
        "{tag:>5}: {w}x{h} cpp={cpp} {kind} period=({ph},{pw}) orient={o:?}\n       active   = {act}\n       crop     = {crop}\n       blackarea= {ba}",
        tag = tag,
        w = img.width,
        h = img.height,
        cpp = img.cpp,
        kind = kind,
        ph = ph,
        pw = pw,
        o = img.orientation,
        act = rect_str(&img.active_area),
        crop = rect_str(&img.crop_area),
        ba = if ba.is_empty() { "[]".to_string() } else { ba.join(" | ") },
    )
}

/// 元数据给的画幅矩形（与 rawler 自己的 `CropDefault` 同一优先级：crop 优先，回落 active）。
/// 只取与整块原样的交集；任何一边算出非正尺寸 ⇒ `None`（本轮的口径：撒谎的元数据不如没有）。
fn frame_rect(img: &rawler::RawImage) -> Option<rawler::imgop::Rect> {
    let src = img.crop_area.or(img.active_area)?;
    let (w, h) = (img.width, img.height);
    let x0 = src.x().min(w);
    let y0 = src.y().min(h);
    let x1 = (src.x() + src.width()).min(w);
    let y1 = (src.y() + src.height()).min(h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(rawler::imgop::Rect::new(
        rawler::imgop::Point::new(x0, y0),
        rawler::imgop::Dim2::new(x1 - x0, y1 - y0),
    ))
}

/// 一行（或一列）呈现像素的平均亮度。`stride` = 每行像素数（用于列采样）。
fn line_luma(rgba: &[u8], start: usize, count: usize, stride: usize) -> (f64, f64) {
    let mut sum = 0f64;
    let mut max = 0f64;
    for i in 0..count {
        let p = &rgba[(start + i * stride) * 4..(start + i * stride) * 4 + 3];
        let v = (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0;
        sum += v;
        if v > max {
            max = v;
        }
    }
    (sum / count as f64, max)
}

/// 从某条边起，连续多少条线"是黑的"（整线平均亮度 < 阈值）。`horiz` = 逐行扫（量上下边）。
fn black_run(rgba: &[u8], horiz: bool, forward: bool, w: usize, h: usize, thr: f64) -> usize {
    let (count, stride, total) = if horiz { (w, 1, h) } else { (h, w, w) };
    let mut n = 0usize;
    for i in 0..total {
        let k = if forward { i } else { total - 1 - i };
        let start = if horiz { k * w } else { k };
        let (mean, _max) = line_luma(rgba, start, count, stride);
        if mean < thr {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// 落一张左上角 320×256 的裸 RGBA8 补丁到 `.temp/m84-out/`，并往 `manifest.txt` 追加一行
/// （供 `.temp/m84_diff.py` 在呈现像素上比边缘、拼对照图）。
fn dump(path: &Path, tag: &str, w: usize, h: usize, rgba: &[u8]) {
    const PW: usize = 320;
    const PH: usize = 256;
    let ext_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = ext_root.join(".temp").join("m84-out");
    std::fs::create_dir_all(&dir).unwrap();
    let (pw, ph) = (PW.min(w), PH.min(h));
    let mut buf = Vec::with_capacity(pw * ph * 4);
    for ly in 0..ph {
        buf.extend_from_slice(&rgba[(ly * w) * 4..(ly * w + pw) * 4]);
    }
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    std::fs::write(dir.join(format!("{stem}_{tag}.rgba")), &buf).unwrap();
    use std::io::Write;
    let mut mf = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("manifest.txt"))
        .unwrap();
    writeln!(mf, "{stem}\t{tag}\tfull={w}x{h}\tpatch={pw}x{ph}").unwrap();
}

fn raw_col_mean(pix: &[u16], w: usize, h: usize, x0: usize, x1: usize) -> f64 {
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in 0..h {
        for x in x0..x1 {
            sum += pix[y * w + x] as u64;
            n += 1;
        }
    }
    sum as f64 / n.max(1) as f64
}

fn raw_row_mean(pix: &[u16], w: usize, y0: usize, y1: usize) -> f64 {
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in 0..w {
            sum += pix[y * w + x] as u64;
            n += 1;
        }
    }
    sum as f64 / n.max(1) as f64
}

fn probe(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    println!(
        "== {} ({} B)",
        path.file_name().unwrap().to_string_lossy(),
        bytes.len()
    );
    let src = rawler::rawsource::RawSource::new_from_slice(&bytes);
    let dummy = match rawler::decode_dummy(&src) {
        Ok(v) => v,
        Err(e) => {
            println!("   dummy 解码失败：{}", e);
            return;
        }
    };
    let real = match rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()) {
        Ok(v) => v,
        Err(e) => {
            println!("   真解码失败：{}", e);
            return;
        }
    };
    println!("{}", describe("dummy", &dummy));
    println!("{}", describe("real", &real));
    let same = dummy.width == real.width
        && dummy.height == real.height
        && dummy.cpp == real.cpp
        && dummy.active_area == real.active_area
        && dummy.crop_area == real.crop_area
        && dummy.blackareas == real.blackareas;
    println!("   两阶段元数据一致: {}", if same { "YES" } else { "NO ★" });

    let (w, h) = (real.width, real.height);
    match frame_rect(&real) {
        None => println!("   画幅: 无元数据 ⇒ 全画幅"),
        Some(f) => {
            let b = borders(&f, w, h);
            let kept = f.width() * f.height();
            println!(
                "   画幅: L 取 {} → {}x{}，遮光区 LTRB = [{}, {}, {}, {}]，保留 {:.2}% 面积",
                if real.crop_area.is_some() {
                    "crop_area"
                } else {
                    "active_area"
                },
                f.width(),
                f.height(),
                b[0],
                b[1],
                b[2],
                b[3],
                100.0 * kept as f64 / (w * h) as f64
            );
            println!(
                "   宽高比: 全画幅 {:.4} → 画幅 {:.4}",
                w as f64 / h as f64,
                f.width() as f64 / f.height() as f64
            );
            if let rawler::RawImageData::Integer(p) = &real.data {
                println!(
                    "   原样均值: 左带 {} 列 = {:.1}｜上带 {} 行 = {:.1}｜带内一圈(画幅外缘 8px) = {:.1}｜画幅内 = {:.1}",
                    b[0],
                    raw_col_mean(p, w, h, 0, b[0]),
                    b[1],
                    raw_row_mean(p, w, 0, b[1]),
                    raw_row_mean(p, w, b[1], b[1] + 8),
                    raw_col_mean(p, w, h, b[0], b[0] + 8),
                );
                println!(
                    "   画幅内部均值 = {:.1}｜blacklevel={:?} whitelevel={:?}",
                    raw_col_mean(p, w, f.height(), b[0], b[0] + 64),
                    real.blacklevel,
                    real.whitelevel
                );
            }
        }
    }

    let t0 = std::time::Instant::now();
    let whole = raw::Frame {
        x: 0,
        y: 0,
        w: real.width,
        h: real.height,
    };
    let (rgba, dw) = raw::develop_in(&real, &whole).unwrap();
    let dev_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let dh = rgba.len() / 4 / dw;
    let f = frame_rect(&real);
    // 本探针自己按 rawler 的文档读出来的画幅矩形，必须与生产函数算的同一个（否则上面那批
    // 元数据表就只是自说自话）。
    if let Some(r) = f {
        let want = raw::Frame {
            x: r.x(),
            y: r.y(),
            w: r.width(),
            h: r.height(),
        };
        assert_eq!(raw::frame(&real), want, "生产 `frame` 与探针读的画幅不一致");
        assert_eq!(
            raw::frame(&dummy),
            want,
            "dummy 与真解的画幅不一致（判据②）"
        );
    }
    let (bx, by) = f.map(|r| (r.x(), r.y())).unwrap_or((0, 0));
    println!(
        "   整块显影 {}x{} {:.0} ms（对照臂：含遮光区）",
        dw, dh, dev_ms
    );
    for thr in [4.0f64, 8.0, 16.0] {
        let top = black_run(&rgba, true, true, dw, dh, thr);
        let bot = black_run(&rgba, true, false, dw, dh, thr);
        let left = black_run(&rgba, false, true, dw, dh, thr);
        let right = black_run(&rgba, false, false, dw, dh, thr);
        println!(
            "   实测黑边(整线均值<{:.0}) 上 {:>4} 下 {:>4} 左 {:>4} 右 {:>4}｜元数据裁切线 上 {} 左 {}",
            thr, top, bot, left, right, by, bx
        );
    }
    if let Some(fr) = f {
        let band = line_luma(&rgba, 0, dw, 1).0;
        let inside = line_luma(&rgba, fr.y() * dw + fr.x(), fr.width(), 1).0;
        println!(
            "   带内首行均值 = {:.1}｜画幅首行均值 = {:.1}",
            band, inside
        );
        // ★ 画幅那条腿真出图：与"整块显影再裁出"逐字节对撞（真机上的判据③），
        //   并落两份裸 RGBA8 给 `.temp/m84_diff.py` 在呈现像素上比边缘、出对照图。
        let t1 = std::time::Instant::now();
        let (cut, cw) = raw::develop(&real).unwrap();
        let ch = cut.len() / 4 / cw;
        let mut same =
            cut.len() == fr.width() * fr.height() * 4 && cw == fr.width() && ch == fr.height();
        if same {
            for ly in 0..fr.height() {
                let from = ((fr.y() + ly) * dw + fr.x()) * 4;
                if rgba[from..from + fr.width() * 4] != cut[ly * cw * 4..(ly + 1) * cw * 4] {
                    same = false;
                    break;
                }
            }
        }
        println!(
            "   画幅显影 {cw}x{ch} {:.0} ms｜与整块裁出逐字节一致：{}",
            t1.elapsed().as_secs_f64() * 1000.0,
            if same { "YES" } else { "NO ★" }
        );
        let newtop = line_luma(&cut, 0, cw, 1).0;
        let newleft = line_luma(&cut, 0, ch, cw).0;
        println!(
            "   裁后首行均值 = {:.1}（裁前带内 {:.1}）｜裁后首列均值 = {:.1}",
            newtop,
            line_luma(&rgba, 0, dw, 1).0,
            newleft
        );
        dump(path, "whole", dw, dh, &rgba);
        dump(path, "frame", cw, ch, &cut);
    }
    for (tw, th) in [(1920u32, 1080u32), (240u32, 180u32)] {
        let full = raw::bin_plan(&real, tw, th);
        println!(
            "   bin_plan(全画幅 {}x{}, 框 {}x{}) = {:?}",
            w, h, tw, th, full
        );
        if let Some(fr) = f {
            let (fw, fh) = (fr.width(), fr.height());
            let cap = (fw / tw as usize)
                .max(fh / th as usize)
                .min((fw / th as usize).max(fh / tw as usize));
            let cap = cap.min(fw).min(fh);
            println!(
                "      画幅后 {}x{} ⇒ 步长上限 cap={}（全画幅那档步长 {:?}）",
                fw,
                fh,
                cap,
                full.map(|v| v.0)
            );
        }
    }
    println!();
}

fn main() {
    let dir = samples_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .map(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    ["cr2", "nef", "raf", "dng", "arw", "cr3"].contains(&e.as_str())
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    for p in &files {
        probe(p);
    }
}
