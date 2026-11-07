//! m257 量具：**首帧（分箱档）与升级档（全尺寸）亮度差**到底有多大。
//!
//! 存在理由：2026-11-08 用户实报「打开 CR2 已出图，停一会儿会变亮」。两条腿原来各自对
//! **自己那份线性图**调 `auto_gain`：分箱后噪点被面积平均抹平 ⇒ 99.9 分位掉到阈值以下
//! ⇒ 判成「已经够亮」给 1.0，而全尺寸腿给 1.39，于是升级落屏时整张图跳 15.7 个灰阶。
//! `scene_gain` 把统计源换成**未分箱的传感器样本**后，两条腿必须给出同一个数。
//!
//! 跑法（在 `RuPix-Decoder/workspace/` 下）：
//! ```text
//! cargo run --release -p rupix-raw --example probe_tier_brightness -- ..\..\.build\1M\noise.cr2 672 504
//! ```

use rawler::decoders::RawDecodeParams;

fn mean_rgb(buf: &[u8]) -> f64 {
    let n = (buf.len() / 4).max(1);
    let mut acc = 0u64;
    for px in buf.chunks_exact(4) {
        acc += px[0] as u64 + px[1] as u64 + px[2] as u64;
    }
    acc as f64 / (n as f64 * 3.0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("用法：probe_tier_brightness <raw 文件> [目标宽] [目标高]");
        return;
    }
    let tw = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(672u32);
    let th = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(504u32);

    let data = std::fs::read(&args[0]).expect("读文件失败");
    let src = rawler::rawsource::RawSource::new_from_slice(&data);
    let img = rawler::decode(&src, &RawDecodeParams::default()).expect("rawler 解失败");
    println!(
        "raw     : {}x{} cpp={} make={:?} model={:?}",
        img.width, img.height, img.cpp, img.make, img.model
    );

    // 臂 1：全尺寸（升级档走的就是这条）
    let t = std::time::Instant::now();
    let (full, fw) = raw::develop(&img).expect("全尺寸显影失败");
    let fh = full.len() / 4 / fw;
    let mf = mean_rgb(&full);
    println!(
        "full    : {fw}x{fh} mean={mf:.2} ({:.1} ms)",
        t.elapsed().as_secs_f64() * 1000.0
    );

    // 臂 2：分箱档（首帧）
    match raw::bin_plan(&img, tw, th) {
        Some((step, ow, oh)) => {
            let t = std::time::Instant::now();
            let (b, bw, bh) = raw::develop_binned(&img, step).expect("分箱显影失败");
            let mb = mean_rgb(&b);
            println!(
                "binned  : step={step} 申报={ow}x{oh} 实际={bw}x{bh} mean={mb:.2} ({:.1} ms)",
                t.elapsed().as_secs_f64() * 1000.0
            );
            println!("delta   : 两腿均值差 = {:.2} 个灰阶（目标 < 1）", mf - mb);
        }
        None => println!("binned  : bin_plan 返回 None（这一档不走分箱）"),
    }
}
