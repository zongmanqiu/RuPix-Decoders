//! End-to-end RAW smoke: real samples through probe -> query -> render.
//!
//! Run from `RuPix-Decoder/workspace`:
//!   cargo run --release -p rupix-raw --example smoke_raw
//!
//! Sample lookup:
//!   1. `RUPIX_RAW_SAMPLES_DIR`, if set;
//!   2. `<project>/.temp/raw-samples/` (recommended; local only);
//!   3. `<RuPix-Decoder>/.temp/raw-samples/` (legacy local path);
//!   4. `testdata/golden/*.cr2|cr3|dng|raf` (sniff-only fixtures).
//!
//! Output is one `key=value` record per sample so a run can be diffed.

#![allow(non_snake_case)]

use rupix_abi::write_last_error;
use rupix_abi::*;
use std::path::{Path, PathBuf};

fn probe(data: &[u8], ext: &str) -> i32 {
    let mut e = ext.as_bytes().to_vec();
    e.push(0);
    raw::iv_probe(data.as_ptr(), data.len(), e.as_ptr().cast())
}

fn render(data: &[u8], ext: &str) -> Result<(u32, u32, Vec<u8>, i32), String> {
    let mut e = ext.as_bytes().to_vec();
    e.push(0);
    let ext_ptr = e.as_ptr().cast();
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
        ext_ptr,
    );
    if c != ERR_OK {
        return Err(format!("query code={c} err={}", read_last_error()));
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
        ext_ptr,
    );
    if c != ERR_OK {
        return Err(format!("render code={c} err={}", read_last_error()));
    }
    Ok((w, h, buf, info.orientation))
}

fn read_last_error() -> String {
    let mut ebuf = [0i8; 512];
    let n = unsafe { write_last_error(ebuf.as_mut_ptr(), ebuf.len()) }.max(0) as usize;
    String::from_utf8_lossy(unsafe {
        std::slice::from_raw_parts(ebuf.as_ptr() as *const u8, n.min(511))
    })
    .to_string()
}

fn sample_dirs() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let decoder = manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap_or(manifest);
    let project = decoder.parent().unwrap_or(decoder);
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("RUPIX_RAW_SAMPLES_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(project.join(".temp").join("raw-samples"));
    dirs.push(decoder.join(".temp").join("raw-samples"));
    dirs
}

fn raw_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "3fr"
                            | "ari"
                            | "arw"
                            | "cr2"
                            | "cr3"
                            | "crm"
                            | "crw"
                            | "dcr"
                            | "dcs"
                            | "dng"
                            | "erf"
                            | "fff"
                            | "iiq"
                            | "kdc"
                            | "mef"
                            | "mos"
                            | "mrw"
                            | "nef"
                            | "nrw"
                            | "orf"
                            | "ori"
                            | "pef"
                            | "qtk"
                            | "raf"
                            | "raw"
                            | "rw2"
                            | "rwl"
                            | "srw"
                            | "x3f"
                    )
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

fn ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase()
}

fn run_case(path: &Path, strict: bool) -> Option<String> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => {
            return Some(format!(
                "format={} file={} probe=missing decode=missing",
                ext(path),
                path.display()
            ));
        }
    };
    let probe = probe(&data, &ext(path));
    let (w, h, buf, orient) = match render(&data, &ext(path)) {
        Ok(v) => v,
        Err(error) => {
            let line = format!(
                "format={} file={} probe={} decode=fail error={}",
                ext(path),
                path.display(),
                probe,
                error
            );
            if strict {
                return Some(line);
            }
            return Some(line);
        }
    };
    let nonzero = buf
        .chunks_exact(4)
        .filter(|px| px[0] | px[1] | px[2] != 0)
        .count();
    let total = buf.len() / 4;
    let ratio = nonzero as f64 / total.max(1) as f64;
    let decode = if w > 0 && ratio > 0.0001 {
        "ok"
    } else {
        "blank"
    };
    Some(format!(
        "format={} file={} probe={} decode={} dims={}x{} orient={} nonzero={:.1}%",
        ext(path),
        path.display(),
        probe,
        decode,
        w,
        h,
        orient,
        ratio * 100.0
    ))
}

fn main() {
    let mut ran = 0usize;
    let mut failed = 0usize;
    let mut seen_dir = false;
    for dir in sample_dirs() {
        if !dir.is_dir() {
            continue;
        }
        seen_dir = true;
        for path in raw_files(&dir) {
            if let Some(line) = run_case(&path, true) {
                if (line.contains("decode=fail") || line.contains("decode=blank"))
                    && !line.contains("fixture=sniff-only")
                {
                    failed += 1;
                }
                println!("{line}");
                ran += 1;
            }
        }
    }

    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testdata/golden");
    for name in ["CanonRaw.cr2", "CanonRaw.cr3", "DNG.dng", "FujiFilm.raf"] {
        let path = testdata.join(name);
        if path.is_file() {
            if let Some(line) = run_case(&path, false) {
                println!("{line} fixture=sniff-only");
                ran += 1;
            }
        }
    }

    if !seen_dir {
        println!("samples=missing hint=set RUPIX_RAW_SAMPLES_DIR or use .temp/raw-samples");
    }
    println!("summary=ran:{ran} failed:{failed}");
    if failed > 0 {
        std::process::exit(1);
    }
}
