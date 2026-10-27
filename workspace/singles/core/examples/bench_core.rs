//! 常用格式的解码速度实测。
//!
//! 跑法（必须 --release，否则测的是未优化代码）：
//!   cargo run --release --example bench_core -p rupix-core
//!
//! 用途：回答"这个库够不够快"。数字随机器与编译配置变化，
//! 重点是**同一台机器上 z 优化与 speed 优化的对比**，以及各格式的数量级。

use std::io::Cursor;
use std::time::Instant;

use image::{ImageFormat, RgbImage};

const W: u32 = 2000;
const H: u32 = 1500;
const ROUNDS: u32 = 3;

fn main() {
    // 造一张有内容、不可压缩性较差的图（纯色图会让编码器"作弊"）。
    let mut img = RgbImage::new(W, H);
    for y in 0..H {
        for x in 0..W {
            let px = img.get_pixel_mut(x, y);
            px[0] = ((x * 7 + y * 3) % 251) as u8;
            px[1] = ((x ^ y) % 253) as u8;
            px[2] = ((x * 13 - y * 5 + 97) % 257) as u8;
        }
    }
    println!(
        "sample {}x{} = {} MPixel",
        W,
        H,
        (W as f64 * H as f64) / 1e6
    );
    println!();
    println!(
        "{:<6} {:>10} {:>12} {:>14}",
        "format", "bytes", "decode(ms)", "MPix/s"
    );

    for fmt in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::WebP,
        ImageFormat::Tiff,
        ImageFormat::Bmp,
        ImageFormat::Pnm,
        ImageFormat::Gif,
    ] {
        let mut buf = Vec::new();
        match img.write_to(&mut Cursor::new(&mut buf), fmt) {
            Ok(()) => {}
            Err(e) => {
                println!("{:<6} encode failed: {e}", format!("{fmt:?}"));
                continue;
            }
        }

        // 预热一次，把页错误/惰性初始化排除掉。
        let warm = decode(&buf);
        if warm.is_err() {
            println!("{:<6} decode failed: {:?}", format!("{fmt:?}"), warm.err());
            continue;
        }

        let t0 = Instant::now();
        for _ in 0..ROUNDS {
            let _ = decode(&buf).expect("decode");
        }
        let per = t0.elapsed() / ROUNDS;
        let ms = per.as_secs_f64() * 1000.0;
        let mpix = (W as f64 * H as f64) / 1e6 / per.as_secs_f64();
        println!(
            "{:<6} {:>10} {:>12.1} {:>14.1}",
            format!("{fmt:?}"),
            buf.len(),
            ms,
            mpix
        );
    }
}

fn decode(data: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    Ok(image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?
        .decode()?
        .into_rgba8()
        .into_raw())
}
