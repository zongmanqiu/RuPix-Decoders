//! P2 实验：WebP 解码库对比 —— image-webp（纯 Rust）vs libwebp（C 库）。
//!
//! 跑法：`cargo run --release --example bench_webp -p rupix-core -- ..\..\..\.temp\samples\bench.webp`
//! 两边都解到 RGBA8（与真实路径一致），1 次预热 + 5 轮取平均/最快。

use std::time::Instant;

const ROUNDS: u32 = 5;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: bench_webp <webp文件>");
    let data = std::fs::read(&path).expect("读样本");

    // ① image-webp（现状）
    let first = decode_image_webp(&data);
    let (w, h) = (first.0, first.1);
    let mut total = std::time::Duration::ZERO;
    let mut best = std::time::Duration::MAX;
    for _ in 0..ROUNDS {
        let t0 = Instant::now();
        let _ = decode_image_webp(&data);
        total += t0.elapsed();
        best = best.min(t0.elapsed());
    }
    println!(
        "image-webp  {}x{}  avg {:.1} ms  min {:.1} ms",
        w,
        h,
        total.as_secs_f64() * 1000.0 / ROUNDS as f64,
        best.as_secs_f64() * 1000.0
    );

    // ② libwebp（C 库）
    let first = decode_libwebp(&data);
    let mut total = std::time::Duration::ZERO;
    let mut best = std::time::Duration::MAX;
    for _ in 0..ROUNDS {
        let t0 = Instant::now();
        let _ = decode_libwebp(&data);
        total += t0.elapsed();
        best = best.min(t0.elapsed());
    }
    println!(
        "libwebp     {}x{}  avg {:.1} ms  min {:.1} ms",
        first.0,
        first.1,
        total.as_secs_f64() * 1000.0 / ROUNDS as f64,
        best.as_secs_f64() * 1000.0
    );
}

fn decode_image_webp(data: &[u8]) -> (u32, u32) {
    let img = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap();
    let img = img.into_rgba8();
    (img.width(), img.height())
}

fn decode_libwebp(data: &[u8]) -> (u32, u32) {
    let mut w = 0i32;
    let mut h = 0i32;
    let ptr = unsafe { libwebp_sys::WebPDecodeRGBA(data.as_ptr(), data.len(), &mut w, &mut h) };
    assert!(!ptr.is_null(), "libwebp 解码失败");
    let len = (w * h * 4) as usize;
    // 拷走（模拟真实路径的归属转移），再释放 libwebp 的缓冲。
    let rgba = unsafe { std::slice::from_raw_parts(ptr, len) };
    let sum = rgba.len(); // 防优化：碰一下
    let _ = std::hint::black_box(sum);
    unsafe { libwebp_sys::WebPFree(ptr.cast()) };
    (w as u32, h as u32)
}
