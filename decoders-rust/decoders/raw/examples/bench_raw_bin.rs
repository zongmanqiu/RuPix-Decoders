//! m82-A 离线量测：同一张真机 RAW，**两条腿解到同一档**，比时间、比字节、比呈现像素。
//!
//! 存在理由：㉑「按视口档显影」的赌注是——首屏那一档不必先解整张。省多少毫秒、
//! 画面变不变，都必须在这条腿落地前用**呈现像素**量出来（正本 §二 的口径：画质只认
//! 呈现像素，不认中间态口径之争）。
//!
//! 两条腿（都从同一次 `rawler::decode` 起算，那一趟无论如何省不掉）：
//! - **A 现路**：`develop` 全尺寸显影 → 按同一分箱几何做面积平均到目标档。
//!   这就是今天宿主看到的那条链（宿主拿到整张 RGBA 再面积平均到视口）。
//!   平均几何刻意与 B **完全对齐**（同 step、同 `min(w-step)` 右边缘钳位），
//!   这样两条腿的逐像素差里只剩算法差（双线性去马赛克后平均 vs 分箱后平均、
//!   线性域平均 vs 编码域平均），不含取样点位移。
//! - **B 候选**：`bin_plan` + `develop_binned` 直接在 CFA 上分箱显影到目标档。
//!
//! 产出：两条腿各段的毫秒数、档尺寸与字节数、逐字节指纹（fnv1a64），
//! 以及两份**档尺寸**的裸 RGBA8（`*_A_*.rgba8` / `*_B_*.rgba8`），
//! 差值分布与 PNG 由 `.temp/m82_diff.py` 在呈现像素上算。
//!
//! 跑法（在 `RuPix-Decoder/` 下，一次一张）：
//! ```text
//! cargo run --release -p rupix-raw --example bench_raw_bin -- \
//!   ..\.temp\raw-samples\X100V_compressed.RAF 1920 1440 ..\.temp\m82-out
//! ```

use std::path::Path;
use std::time::Instant;

fn fnv(hex: &[u8]) -> u64 {
    let mut d: u64 = 0xcbf29ce484222325;
    for &b in hex {
        d ^= b as u64;
        d = d.wrapping_mul(0x100000001b3);
    }
    d
}

/// 面积平均到 `ow×oh`：与 `develop_binned` 同一套几何（块起点 `ox*step` 钳到 `w-step`）。
/// 输入是**已编码的 8 位像素**——这正是宿主今天实际做的事（拿到 RGBA8 再缩）。
///
/// `off` 把整套块起点平移 `off` 像素（钳边保证尺寸不变）。`off=0` 是与 B 对齐的那条；
/// `off=step/2` 是**另一份同样正当的取样**——它和 `off=0` 之间的差就是"纯取样几何差"，
/// 用来给"B 比 A 差多少算多"定一把不靠嘴说的尺子。
fn box_avg_u8(rgba: &[u8], w: usize, h: usize, step: usize, off: usize) -> (Vec<u8>, usize, usize) {
    let (ow, oh) = (w.div_ceil(step), h.div_ceil(step));
    let x0s: Vec<usize> = (0..ow).map(|ox| (ox * step + off).min(w - step)).collect();
    let mut out = vec![0u8; ow * oh * 4];
    for oy in 0..oh {
        let y0 = (oy * step + off).min(h - step);
        for ox in 0..ow {
            let x0 = x0s[ox];
            let mut acc = [0u32; 4];
            for j in 0..step {
                let row = &rgba[(y0 + j) * w * 4..];
                for k in 0..step {
                    let px = &row[(x0 + k) * 4..(x0 + k) * 4 + 4];
                    for (a, &v) in acc.iter_mut().zip(px) {
                        *a += v as u32;
                    }
                }
            }
            let n = (step * step) as u32;
            let o = &mut out[(oy * ow + ox) * 4..(oy * ow + ox) * 4 + 4];
            for c in 0..4 {
                o[c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    }
    (out, ow, oh)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!("usage: bench_raw_bin <raw-file> <target_w> <target_h> <out-dir>");
        std::process::exit(2);
    }
    let path = Path::new(&args[0]);
    let (tw, th): (u32, u32) = match (args[1].parse(), args[2].parse()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            eprintln!("target_w/target_h 要填整数");
            std::process::exit(2);
        }
    };
    let out_dir = Path::new(&args[3]);
    let _ = std::fs::create_dir_all(out_dir);
    let ms = |d: Instant| d.elapsed().as_secs_f64() * 1e3;

    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读文件失败 {path:?}: {e}");
            std::process::exit(1);
        }
    };

    // ── 公共前段：rawler 解 CFA 原始像素（两条腿共用，省不掉的那一趟）──────
    let t = Instant::now();
    let img = match rawler::decode(
        &rawler::rawsource::RawSource::new_from_slice(&data),
        &rawler::decoders::RawDecodeParams::default(),
    ) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("rawler::decode 失败: {e}");
            std::process::exit(1);
        }
    };
    let unpack = ms(t);

    // 诊断：CFA 原始数据的分布（min/max/均值 + 按 CFA 道分的均值）。用来分清
    // "rawler 解出来的原样就是垃圾"与"本仓显影链把它算成垃圾"——两者修法完全不同。
    if let rawler::RawImageData::Integer(p) = &img.data {
        let mut mn = u16::MAX;
        let mut mx = 0u16;
        let mut sum = 0u64;
        let mut ch_sum = [0u64; 3];
        let mut ch_n = [0u64; 3];
        let cfa = match &img.photometric {
            rawler::rawimage::RawPhotometricInterpretation::Cfa(c) => Some(&c.cfa),
            _ => None,
        };
        for (i, &v) in p.iter().enumerate() {
            mn = mn.min(v);
            mx = mx.max(v);
            sum += v as u64;
            if let Some(c) = &cfa {
                let k = c.color_at(i / img.width, i % img.width).min(2);
                ch_sum[k] += v as u64;
                ch_n[k] += 1;
            }
        }
        let n = p.len().max(1) as u64;
        println!(
            "RAW 数据 min={} max={} mean={:.1} 按道均值=[{:.1},{:.1},{:.1}]",
            mn,
            mx,
            sum as f64 / n as f64,
            ch_sum[0] as f64 / (ch_n[0].max(1)) as f64,
            ch_sum[1] as f64 / (ch_n[1].max(1)) as f64,
            ch_sum[2] as f64 / (ch_n[2].max(1)) as f64,
        );
    }

    // 诊断：显影链吃的所有元数据，原样打出来（白平衡/黑电平/白电平/色彩矩阵）。
    // 本轮逮到"两条腿颜色对不上"，判案得先看这些数到底长什么样，不能靠猜。
    println!(
        "META {} {} cpp={} bits={} wb={:?} black{{cpp={},w={},h={},v={:?}}} white={:?} crop={:?} active={:?}",
        img.make,
        img.model,
        img.cpp,
        img.bps,
        img.wb_coeffs,
        img.blacklevel.cpp,
        img.blacklevel.width,
        img.blacklevel.height,
        img.blacklevel.as_vec(),
        img.whitelevel.as_vec(),
        img.crop_area,
        img.active_area,
    );

    // 诊断：这台传感器的 CFA 真实周期与颜色排布（决定分箱步长的下限，也决定它是不是
    // 常规 R/G/B 齐备的图案——X-Trans 那类 6×6 图案的步长只能是 6 的倍数）。
    if let rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) = &img.photometric {
        let (ph, pw) = raw::cfa_period(&cfg.cfa);
        let mut rows = String::new();
        for r in 0..ph.min(6) {
            let line: String = (0..pw.min(6))
                .map(|c| match cfg.cfa.color_at(r, c) {
                    0 => 'R',
                    1 => 'G',
                    2 => 'B',
                    3 => 'C',
                    4 => 'M',
                    5 => 'Y',
                    v => {
                        let _ = v;
                        '?'
                    }
                })
                .collect();
            rows.push_str(&line);
            rows.push('/');
        }
        println!(
            "CFA 周期={}x{} cpp={} 图案={} 原图={}x{}",
            ph,
            pw,
            img.cpp,
            rows.trim_end_matches('/'),
            img.width,
            img.height
        );
    } else {
        println!(
            "CFA 非图案（photometric={:?} cpp={}）",
            img.photometric, img.cpp
        );
    }

    // ── 候选腿是否入围。A 臂**先算**：它不依赖分箱计划，就算这档走不了分箱，
    //    也要量出"全尺寸显影 + 面积平均到这一档"的价——那是这条路的现路成本。──
    let t = Instant::now();
    let (full, fw) = match raw::develop(&img) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("develop 失败: {e}");
            std::process::exit(1);
        }
    };
    let develop_full = ms(t);
    let fh = full.len() / 4 / fw;
    let full_len = full.len();
    let full_digest = fnv(&full);

    let plan = match raw::bin_plan(&img, tw, th) {
        Some(p) => p,
        None => {
            println!(
                "BIN-N/A file={:?} 原图={}x{} 目标档={}x{} → 这档不走分箱（回全尺寸）",
                path.file_name(),
                fw,
                fh,
                tw,
                th
            );
            std::process::exit(3);
        }
    };
    let (step, bow, boh) = plan;

    // ── A 现路收尾：按同一分箱几何面积平均到档 ──────────────────────────
    let t = Instant::now();
    let (a_tier, a_w, a_h) = box_avg_u8(&full, fw, fh, step, 0);
    let downscale = ms(t);
    let a_digest = fnv(&a_tier);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "img".into());
    let a_path = out_dir.join(format!("{stem}_A_{a_w}x{a_h}.rgba8"));
    if let Err(e) = std::fs::write(&a_path, &a_tier) {
        eprintln!("写 A 失败: {e}");
    }
    // ── A2 参照臂：同一条腿，整套块起点平移 step/2。它不是"另一种实现"，而是
    //    **同一份全尺寸像素的另一种同样正当的取样**——A 与 A2 之差 = 纯取样几何差，
    //    给"B 比 A 差多少才算多"提供一把自定的尺子（不靠嘴说"这点差看不见"）。
    let a2_tier = {
        let (v, _, _) = box_avg_u8(&full, fw, fh, step, step / 2);
        v
    };
    drop(full);
    drop(a_tier);
    let a2_digest = fnv(&a2_tier);
    let a2_path = out_dir.join(format!("{stem}_A2_{a_w}x{a_h}.rgba8"));
    if let Err(e) = std::fs::write(&a2_path, &a2_tier) {
        eprintln!("写 A2 失败: {e}");
    }
    drop(a2_tier);

    // ── B 候选：CFA 上直接分箱显影到档 ───────────────────────────────────
    let t = Instant::now();
    let (b_tier, bw, bh) = match raw::develop_binned(&img, step) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("develop_binned 失败: {e}");
            std::process::exit(1);
        }
    };
    let develop_bin = ms(t);
    assert_eq!(
        (bw, bh),
        (bow, boh),
        "bin_plan 预告的档尺寸与 develop_binned 实际产出的不一致"
    );
    let b_digest = fnv(&b_tier);
    let b_path = out_dir.join(format!("{stem}_B_{bw}x{bh}.rgba8"));
    if let Err(e) = std::fs::write(&b_path, &b_tier) {
        eprintln!("写 B 失败: {e}");
    }

    let mp = (fw * fh) as f64 / 1e6;
    println!(
        "file={:?} 原图={}x{}={:.2}MP 目标档={}x{} 步长={} 档尺寸={}x{}",
        path.file_name(),
        fw,
        fh,
        mp,
        tw,
        th,
        step,
        bw,
        bh
    );
    println!(
        "A 现路: unpack={:.1} develop_full={:.1} downscale={:.1} → 到档合计={:.1} ms  全尺寸字节={}  档字节={}",
        unpack,
        develop_full,
        downscale,
        unpack + develop_full + downscale,
        full_len,
        bw * bh * 4
    );
    println!(
        "B 候选: unpack={:.1} develop_binned={:.1} → 到档合计={:.1} ms  档字节={}",
        unpack,
        develop_bin,
        unpack + develop_bin,
        bw * bh * 4
    );
    println!(
        "省下: {:.1} ms（占 A 的 {:.1}%）  develop {:.1}→{:.1} ms  少搬 {} B",
        (unpack + develop_full + downscale) - (unpack + develop_bin),
        ((unpack + develop_full + downscale) - (unpack + develop_bin))
            / (unpack + develop_full + downscale)
            * 100.0,
        develop_full + downscale,
        develop_bin,
        (fw * fh * 4).saturating_sub(bw * bh * 4)
    );
    println!(
        "digest A=fnv1a64:{:016x} A2=fnv1a64:{:016x} B=fnv1a64:{:016x} full=fnv1a64:{:016x}",
        a_digest, a2_digest, b_digest, full_digest
    );
    println!(
        "dump A={:?} B={:?}（裸 RGBA8 {}x{}，行主序，逐字节可比；A2 为参照取样臂）",
        a_path, b_path, a_w, a_h
    );
}
