//! 端到端冒烟：真实 RAW 样本走 probe → 查询 → 渲染全流程。
//!
//! 跑法（在 RuPix-Decoder/ 下）：
//!   cargo run -p rupix-raw --example smoke_raw
//!
//! 样本查找顺序：
//!   1. `<RuPix-Decoder>/.temp/raw-samples/`（完好原图：Canon 30D CR2、Nikon D80 NEF、
//!      Fujifilm X100V RAF，来自 immich-app/test-assets，首次需手动下载）
//!   2. `testdata/`（exiftool 测试图库的小样本——注意它们被 exiftool 改过元数据，
//!      rawler 可能报损坏，只用于嗅探测试，解码失败不算冒烟失败）

#![allow(non_snake_case)]

use rupix_abi::write_last_error;
use rupix_abi::*;

fn probe(data: &[u8]) -> i32 {
    raw::iv_probe(data.as_ptr(), data.len(), std::ptr::null())
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
    let c = raw::iv_render(
        data.as_ptr(),
        data.len(),
        &req,
        std::ptr::null_mut(),
        0,
        &mut info,
        std::ptr::null(),
    );
    if c != ERR_OK {
        return Err(format!("query failed: code={c} err={}", read_last_error()));
    }
    let (w, h) = (info.actual_w, info.actual_h);
    let mut buf = vec![0u8; info.bytes_needed as usize];
    let c = raw::iv_render(
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
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ext_root = manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap(); // view/
    let good = ext_root.join(".temp").join("raw-samples"); // 完好原图（.temp，不入库）
    let testdata = manifest.join("../../../testdata/golden"); // 全仓共享的小样本（仅嗅探验证）

    let mut cases: Vec<(String, std::path::PathBuf, bool)> = vec![
        (
            "CR2 (Canon 30D)".into(),
            good.join("Canon_EOS_30D.CR2"),
            true,
        ),
        ("NEF (Nikon D80)".into(), good.join("glarus.nef"), true),
        (
            "RAF (X100V compressed)".into(),
            good.join("X100V_compressed.RAF"),
            true,
        ),
    ];
    for name in ["CanonRaw.cr2", "CanonRaw.cr3", "DNG.dng", "FujiFilm.raf"] {
        cases.push((name.into(), testdata.join(name), false));
    }

    let mut fail = 0;
    let mut ran = 0;

    for (label, path, strict) in cases {
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => {
                if strict {
                    println!("SKIP {}（{} 不存在）", label, path.display());
                }
                continue;
            }
        };
        ran += 1;

        let p = probe(&data);
        let (w, h, buf, orient, _) = match render(&data) {
            Ok(v) => v,
            Err(e) => {
                // strict=false 的样本只验证嗅探：解码失败仅提示，不算失败。
                if strict {
                    println!("FAIL {} probe={} ({})", label, p, e);
                    fail += 1;
                } else {
                    println!(
                        "WARN {} probe={} 解码失败（exiftool 改过元数据，预期内）：{}",
                        label, p, e
                    );
                }
                continue;
            }
        };

        // 非零像素占比（显影出全黑/全白即坏）。
        let nonzero = buf
            .chunks_exact(4)
            .filter(|px| px[0] + px[1] + px[2] > 30)
            .count();
        let total = buf.len() / 4;
        let ratio = nonzero as f64 / total.max(1) as f64;

        // 缓存命中路径计时（查询+渲染已各跑一遍，线程本地缓存热）。
        let t0 = std::time::Instant::now();
        let _ = render(&data);
        let dt = t0.elapsed().as_millis() as f32;

        let ok = w > 0 && ratio > 0.01;
        if !ok && strict {
            fail += 1;
        }
        println!(
            "{} {}  {}x{} orient={} {}px  nonzero={:.1}%  cached-render={:.1}ms",
            if ok || !strict { "OK  " } else { "FAIL" },
            label,
            w,
            h,
            orient,
            total,
            ratio * 100.0,
            dt
        );
    }

    if fail > 0 {
        std::process::exit(1);
    }
    if ran == 0 {
        println!("（无样本可跑）");
    }
}
