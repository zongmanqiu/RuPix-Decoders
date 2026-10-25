//! 生成基准样本（一张有内容、不可压缩性较差的 3MP 图 × 各格式）。
//!
//! 跑法：
//!   cargo run --release --example gen_samples -p rupix-core
//!   cargo run --release --example gen_samples -p rupix-core -- <输出目录>
//!
//! 不带参数时输出到**仓库根 `.temp/samples/`**（bench_decode 取样处）；带参数则用
//! 该目录。以 `-` 开头的参数直接拒收：`--help` 之类一旦被当成目录名，会在仓库根
//! 凭空建出一个 `--help/`（2026-10-10 实际发生过，别再重演）。
//!
//! 样本给 `main/examples/bench_decode.rs` 用：测的是"双击打开"真实路径
//! （宿主 → 插件 DLL → ABI），不是库函数直调。

use std::path::PathBuf;

use image::{ImageFormat, RgbImage};

const W: u32 = 2000;
const H: u32 = 1500;

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(arg) if arg.starts_with('-') => {
            eprintln!("error: 不接受选项 {arg:?}（用法见文件头注释；无参数即输出 .temp/samples）");
            std::process::exit(2);
        }
        Some(arg) => PathBuf::from(arg),
        None => {
            let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            root.pop();
            root.pop();
            root.pop();
            root.join(".temp").join("samples")
        }
    };
    std::fs::create_dir_all(&dir).expect("create out dir");

    let mut img = RgbImage::new(W, H);
    for y in 0..H {
        for x in 0..W {
            let px = img.get_pixel_mut(x, y);
            px[0] = ((x * 7 + y * 3) % 251) as u8;
            px[1] = ((x ^ y) % 253) as u8;
            px[2] = ((x * 13 - y * 5 + 97) % 257) as u8;
        }
    }

    for (name, fmt) in [
        ("bench.jpg", ImageFormat::Jpeg),
        ("bench.png", ImageFormat::Png),
        ("bench.webp", ImageFormat::WebP),
        ("bench.tif", ImageFormat::Tiff),
        ("bench.bmp", ImageFormat::Bmp),
        ("bench.gif", ImageFormat::Gif),
        ("bench.ppm", ImageFormat::Pnm),
        ("bench.tga", ImageFormat::Tga),
    ] {
        let path = dir.join(name);
        img.save_with_format(&path, fmt).expect("save");
        println!("{}", path.display());
    }
}
