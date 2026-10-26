//! av 解码器端到端冒烟：真实 HEIC（iPhone 式 grid 拼图）+ 真实 AVIF（含 alpha）。
//!
//! 跑法（在 RuPix-Decoder/ 下）：
//!   cargo run -p rupix-av --example smoke_av
//!
//! 样本来自 .ex 参考项目（只读，不改动它们）：
//!   ..\..\..\.ex\oculante-master\tests\orange.heic
//!   ..\..\..\.ex\reveal-main\tests\fixtures\avif\color_grid_alpha_nogrid.avif

#![allow(non_snake_case)]

use rupix_abi::write_last_error;
use rupix_abi::*;

fn probe(data: &[u8]) -> i32 {
    av::iv_probe(data.as_ptr(), data.len(), std::ptr::null())
}

fn render(data: &[u8]) -> Result<(u32, u32, Vec<u8>, i32, i32), String> {
    let req = IvRenderReq {
        target_w: 0,
        target_h: 0,
        budget_bytes: 0,
        flags: 0,
        frame_index: 0,
        page_index: 0,
    };
    let mut info = IvRenderOut::default();
    let c = av::iv_render(
        data.as_ptr(),
        data.len(),
        &req,
        std::ptr::null_mut(),
        0,
        &mut info,
        std::ptr::null(),
    );
    if c != ERR_OK {
        let mut ebuf = [0i8; 512];
        let n = unsafe { write_last_error(ebuf.as_mut_ptr(), ebuf.len()) }.max(0) as usize;
        let msg = String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(ebuf.as_ptr() as *const u8, n.min(511))
        })
        .to_string();
        return Err(format!("query failed: code={c} err={msg}"));
    }
    let (w, h) = (info.actual_w, info.actual_h);
    let mut buf = vec![0u8; info.bytes_needed as usize];
    let c = av::iv_render(
        data.as_ptr(),
        data.len(),
        &req,
        buf.as_mut_ptr(),
        buf.len(),
        &mut info,
        std::ptr::null(),
    );
    if c != ERR_OK {
        return Err(format!("render failed: code={c} err={}", read_last_error()));
    }
    Ok((w, h, buf, info.orientation, info.already_oriented))
}

fn read_last_error() -> String {
    let mut ebuf = [0i8; 512];
    let n = unsafe { write_last_error(ebuf.as_mut_ptr(), ebuf.len()) }.max(0) as usize;
    String::from_utf8_lossy(unsafe {
        std::slice::from_raw_parts(ebuf.as_ptr() as *const u8, n.min(511))
    })
    .to_string()
}

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf(); // = view/（RuPix-Decoder 的上一级）
    let ex = |rel: &str| root.join(".ex").join(rel).to_string_lossy().into_owned();
    let cases: Vec<(String, String)> = vec![
        (
            "HEIC (iPhone grid)".into(),
            ex("oculante-master/tests/orange.heic"),
        ),
        (
            "AVIF (grid+alpha)".into(),
            ex("reveal-main/tests/fixtures/avif/color_grid_alpha_nogrid.avif"),
        ),
        (
            "AVIF (irot)".into(),
            ex("reveal-main/tests/fixtures/avif/abc_color_irot_alpha_irot.avif"),
        ),
    ];
    let cases = cases
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect::<Vec<_>>();
    let cases: &[(&str, &str)] = &cases;

    let mut fail = 0;
    for (name, path) in cases {
        let t0 = std::time::Instant::now();
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP {name}: read {path}: {e}");
                continue;
            }
        };
        let t_read = t0.elapsed();
        if probe(&data) != 100 {
            println!("FAIL {name}: probe=0");
            fail += 1;
            continue;
        }
        let (w, h, buf, orient, already) = match render(&data) {
            Ok(v) => v,
            Err(e) => {
                println!("FAIL {name}: {e}");
                fail += 1;
                continue;
            }
        };
        println!(
            "OK   {name}: {w}x{h} rgba={}B orient={orient} already_oriented={already} read={:?} decode={:?}",
            buf.len(),
            t_read,
            t0.elapsed() - t_read
        );
    }

    if fail > 0 {
        println!("smoke FAILED: {fail} case(s)");
        std::process::exit(1);
    }
    println!("smoke passed");
}
