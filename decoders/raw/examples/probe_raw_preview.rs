//! m85-A 量具：**首帧能不能不碰传感器**——把「现路（传感器熵解 + 分箱显影）」与
//! 「文件内嵌的全尺寸预览（`Decoder::full_image`）」放在**同一进程、同一份文件**上交替量。
//!
//! 立案前要先量清的两件事：
//! 1. rawler 没有"只读预览尺寸"的入口——`full_image` 直接把预览解成 `DynamicImage`。
//!    所以档位尺寸仍必须由**传感器元数据**定（m83 立的「申报＝产出」），预览腿只能去凑那个尺寸。
//! 2. 预览是相机自己渲染的（它自己的去马赛克/白平衡/gamma），与我们不是同一条链，
//!    画质判据只能在**呈现像素**上算（m82 立的尺子）。
//!
//! 三条臂（都在档位的呈现尺寸上比）：
//!   Ref = 全尺寸显影（画幅）→ 面积平均到档   ← 我方"正确答案"
//!   Cur = 现路：CFA 分箱直显到档             ← 今天上屏的那一份
//!   New = 内嵌预览 → 面积平均到档            ← 候选
//! 判读看 `MAD(Cur,Ref)`（今天已经接受的差）与 `MAD(New,Ref)`（候选要付的差）并排。
//!
//! 跑法：`cargo run --release -p rupix-raw --example probe_raw_preview`
//! （可选参数：样本目录 / 框宽 / 框高，默认 `.temp/raw-samples` 1920 1080）。

use rawler::decoders::RawDecodeParams;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// 面积平均（RGBA8）。目标尺寸 == 源尺寸时原样搬，不重采样。
fn box_down(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let y0 = y * sh / dh;
        let y1 = (((y + 1) * sh / dh).max(y0 + 1)).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = (((x + 1) * sw / dw).max(x0 + 1)).min(sw);
            let (mut n, mut acc) = (0u32, [0u32; 4]);
            for sy in y0..y1 {
                let row = sy * sw;
                for sx in x0..x1 {
                    let p = &src[(row + sx) * 4..(row + sx) * 4 + 4];
                    acc[0] += p[0] as u32;
                    acc[1] += p[1] as u32;
                    acc[2] += p[2] as u32;
                    acc[3] += p[3] as u32;
                    n += 1;
                }
            }
            let d = y * dw + x;
            for c in 0..4 {
                out[d * 4 + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

/// 呈现像素上的差：平均绝对差（三道）+ 三道均值偏移。
fn diff(a: &[u8], b: &[u8]) -> (f64, [f64; 3]) {
    if a.len() != b.len() {
        return (-1.0, [-1.0; 3]);
    }
    let px = a.len() / 4;
    let (mut mad, mut ma, mut mb) = (0.0f64, [0.0f64; 3], [0.0f64; 3]);
    for i in 0..px {
        for c in 0..3 {
            let (x, y) = (a[i * 4 + c] as f64, b[i * 4 + c] as f64);
            mad += (x - y).abs();
            ma[c] += x;
            mb[c] += y;
        }
    }
    (
        mad / (px as f64 * 3.0),
        [
            (0..3)
                .map(|c| (mb[c] - ma[c]) / px as f64)
                .next()
                .unwrap_or(0.0),
            (mb[1] - ma[1]) / px as f64,
            (mb[2] - ma[2]) / px as f64,
        ],
    )
}

fn med(v: &mut [f64]) -> f64 {
    v.sort_by(|x, y| x.partial_cmp(y).unwrap());
    v[v.len() / 2]
}

/// 把三条臂在**档位呈现尺寸**上的那张整图落成裸 RGBA8，供 `m85_cmp.py` 拼对照 PNG
/// （判据：观感只在上屏那份上判，不看中间态）。
fn dump(stem: &str, w: usize, h: usize, tag: &str, rgba: &[u8]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".temp")
        .join("m85-out");
    std::fs::create_dir_all(&dir).expect("建 m85-out");
    std::fs::write(dir.join(format!("{stem}_{tag}.rgba")), rgba).expect("落 rgba");
    let mut mf = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("manifest.txt"))
        .expect("manifest");
    writeln!(mf, "{stem}\t{tag}\t{w}x{h}\t{}B", rgba.len()).expect("写 manifest");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = samples_dir();
    let (tw, th) = (
        args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1920u32),
        args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1080u32),
    );
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("样本目录")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    ["cr2", "nef", "raf", "dng", "cr3", "rw2", "arw"]
                        .contains(&e.to_lowercase().as_str())
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let data = std::fs::read(&path).expect("读文件");
        let src = rawler::rawsource::RawSource::new_from_slice(&data);
        let params = RawDecodeParams::default();

        // ── 传感器那趟（两条腿都要它：现路要像素，档位尺寸要元数据）───────────
        let t = Instant::now();
        let Ok(img) = rawler::decode(&src, &params) else {
            println!("== {stem}: rawler 解不动，跳过");
            continue;
        };
        let sensor_ms = t.elapsed().as_secs_f64() * 1e3;
        let Some((step, ow, oh)) = raw::bin_plan(&img, tw, th) else {
            println!("== {stem}: 这一框算不出档位，预览腿无从对齐，跳过");
            continue;
        };
        let f = raw::frame(&img);
        println!(
            "== {stem}  整块={}x{} 画幅={}x{}  档=步长{step} → {ow}x{oh}",
            img.width, img.height, f.w, f.h,
        );

        // ── 内嵌预览：拿得到吗、多大、朝向对不对 ───────────────────────────
        let dec = match rawler::get_decoder(&src) {
            Ok(d) => d,
            Err(e) => {
                println!("   get_decoder 失败: {e}");
                continue;
            }
        };
        let t = Instant::now();
        let preview = match dec.full_image(&src, &params) {
            Ok(Some(p)) => p,
            Ok(None) => {
                println!("   full_image: 这个容器没有内嵌全尺寸预览 ⇒ 预览腿无从谈起");
                continue;
            }
            Err(e) => {
                println!("   full_image 报错: {e}");
                continue;
            }
        };
        let preview_head_ms = t.elapsed().as_secs_f64() * 1e3;
        let (pw, ph) = (preview.width() as usize, preview.height() as usize);
        println!(
            "   预览: {}x{} 首次取回 {:.1} ms｜够不够填 {ow}x{oh} 档：{}",
            pw,
            ph,
            preview_head_ms,
            if pw >= ow && ph >= oh {
                "尺寸够"
            } else {
                "尺寸不够（要回落传感器 ⇒ 两条腿都付）"
            },
        );
        // ★ 方向：传感器那条腿交的是**未旋转**像素 + EXIF orientation（宿主补转）。
        //   预览腿拿到的那张是相机自己存进去的位图，rawler 不碰它的方向（cr2/nef/raf 三处
        //   `full_image` 都只是把 JPEG 流交给 `image` 解码，没有 `apply_rotation`）。
        //   ⇒ 只要预览的横竖和画幅的横竖不同侧，两腿就站在不同的朝向上，宿主按同一个
        //     orientation 补转必然把其中一条转错。这条只能在**竖构图样本**上真判。
        let (pv, pf) = (pw < ph, f.w < f.h);
        println!(
            "   方向: 传感器 orientation={:?}｜画幅 {}x{}（{}）｜预览 {}x{}（{}）｜横竖{}同侧",
            img.orientation,
            f.w,
            f.h,
            if pf { "竖" } else { "横" },
            pw,
            ph,
            if pv { "竖" } else { "横" },
            if pv == pf { "" } else { "★ 不" },
        );

        // ── 三条臂，交替跑，取中位 ───────────────────────────────────────
        let (mut a_cur, mut a_new, mut a_ref) = (vec![], vec![], vec![]);
        let (mut cur, mut new): (Vec<u8>, Vec<u8>) = (vec![], vec![]);
        for i in 0..6 {
            if i % 2 == 0 {
                let t = Instant::now();
                cur = raw::develop_binned(&img, step).expect("分箱").0;
                a_cur.push(t.elapsed().as_secs_f64() * 1e3);
            } else {
                let t = Instant::now();
                let p = dec
                    .full_image(&src, &params)
                    .expect("预览解码失败")
                    .expect("无内嵌预览");
                let mut ms = t.elapsed().as_secs_f64() * 1e3;
                let buf = p.to_rgba8().into_raw();
                let t2 = Instant::now();
                new = box_down(&buf, p.width() as usize, p.height() as usize, ow, oh);
                ms += t2.elapsed().as_secs_f64() * 1e3;
                a_new.push(ms);
            }
        }
        let t = Instant::now();
        let full = raw::develop_in(&img, &f).expect("全尺寸显影").0;
        let ref_full_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let rfs = box_down(&full, f.w, f.h, ow, oh);
        a_ref.push(ref_full_ms + t.elapsed().as_secs_f64() * 1e3);

        let (mad_cur, off_cur) = diff(&rfs, &cur);
        let (mad_new, off_new) = diff(&rfs, &new);
        println!(
            "   毫秒｜传感器熵解 {:.1}｜现路分箱 {:.1}（不含熵解）｜预览腿 {:.1}（取回+缩到档）｜参照腿 {:.1}",
            sensor_ms,
            med(&mut a_cur),
            med(&mut a_new),
            med(&mut a_ref),
        );
        println!(
            "   端点口径（首帧总账）：现路 = 熵解 + 分箱 = {:.1} ms｜预览 = 预览腿 = {:.1} ms｜比值 {:.2}×",
            sensor_ms + med(&mut a_cur),
            med(&mut a_new),
            (sensor_ms + med(&mut a_cur)) / med(&mut a_new).max(0.1),
        );
        println!(
            "   呈现像素 MAD（离参照越近越好，满量程 255）：现路 {mad_cur:.3}（偏色 {off_cur:?}）｜预览 {mad_new:.3}（偏色 {off_new:?}）"
        );
        // 分配账（**按下标算的缓冲区字节，不是进程 RSS**）：预览腿全程不碰 CFA，
        // 但要把相机那张全尺寸位图整张解码；传感器腿要 CFA + 线性化后的 f32。
        let mb = |n: usize| n as f64 / (1024.0 * 1024.0);
        println!(
            "   分配账｜预览腿 {:.1} MB（预览 RGBA {}x{}）+ {:.1} MB（缩到档）＝ {:.1}｜\
             现路 {:.1} MB（CFA u16）+ {:.1} MB（线性化 f32，上下各扩一行整行宽）+ {:.1} MB（档位 RGBA）＝ {:.1}",
            mb(pw * ph * 4),
            pw,
            ph,
            mb(ow * oh * 4),
            mb(pw * ph * 4 + ow * oh * 4),
            mb(img.width * img.height * 2),
            mb(img.width * (f.h + 2) * 4),
            mb(ow * oh * 4),
            mb(img.width * img.height * 2 + img.width * (f.h + 2) * 4 + ow * oh * 4),
        );
        dump(&stem, ow, oh, "ref", &rfs);
        dump(&stem, ow, oh, "cur", &cur);
        dump(&stem, ow, oh, "prev", &new);
    }
}
