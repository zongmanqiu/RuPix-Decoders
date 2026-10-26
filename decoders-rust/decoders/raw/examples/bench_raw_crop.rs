//! m84 归因探针：**同一次进程里**交替量「整块原样显影」与「画幅显影」两条腿。
//!
//! 存在理由：端点双臂量到 1:1 那一腿**新臂慢了 21%~29%**（`.plan/m84` §三），而裁切只该少算
//! 1.7%~3.6% 的像素 ⇒ 变慢不来自"裁掉的那部分"，只能来自这轮改写的循环形状。要判它归谁，
//! 必须在同一进程、同一份 `RawImage` 上交替跑两臂（跨进程的绝对值被机器负载支配，不作判词）。
//!
//! 跑法（在 `RuPix-Decoder/` 下）：
//! ```text
//! cargo run --release -p rupix-raw --example bench_raw_crop -- ..\.temp\raw-samples\glarus.nef
//! ```

use std::path::Path;
use std::time::Instant;

fn fnv(buf: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in buf {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: bench_raw_crop <one-raw-file>");
        std::process::exit(2);
    }
    let data = std::fs::read(Path::new(&args[0])).expect("读文件");
    let src = rawler::rawsource::RawSource::new_from_slice(&data);
    let img = rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()).expect("解");
    let whole = raw::Frame {
        x: 0,
        y: 0,
        w: img.width,
        h: img.height,
    };
    let f = raw::frame(&img);
    println!(
        "{} 整块={}x{} 画幅={}x{}（保留 {:.2}%）",
        Path::new(&args[0]).file_name().unwrap().to_string_lossy(),
        whole.w,
        whole.h,
        f.w,
        f.h,
        100.0 * (f.w * f.h) as f64 / (whole.w * whole.h) as f64
    );
    let mut a: Vec<f64> = Vec::new();
    let mut b: Vec<f64> = Vec::new();
    let mut fa = 0u64;
    let mut fb = 0u64;
    for i in 0..6 {
        // 交替跑，避免把"哪一臂先跑"的热身差算进判词。
        if i % 2 == 0 {
            let t = Instant::now();
            let (o, w) = raw::develop_in(&img, &whole).unwrap();
            a.push(t.elapsed().as_secs_f64() * 1e3);
            fa = fnv(&o[..(w * 4).min(o.len())]);
        } else {
            let t = Instant::now();
            let (o, w) = raw::develop_in(&img, &f).unwrap();
            b.push(t.elapsed().as_secs_f64() * 1e3);
            fb = fnv(&o[..(w * 4).min(o.len())]);
        }
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(|x, y| x.partial_cmp(y).unwrap());
        v[v.len() / 2]
    };
    println!(
        "整块腿 {:.1} ms（{:?}）｜画幅腿 {:.1} ms（{:?}）｜差 {:+.1}%",
        med(&mut a),
        a.iter().map(|x| *x as i64).collect::<Vec<_>>(),
        med(&mut b),
        b.iter().map(|x| *x as i64).collect::<Vec<_>>(),
        100.0 * (med(&mut b) - med(&mut a)) / med(&mut a)
    );
    println!("首行指纹 整块={fa:016x} 画幅={fb:016x}（画幅腿的首行是画幅首行，与整块不同属正常）");
}
