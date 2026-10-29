//! Generate the four classic fixtures that have no Python encoder:
//! PICT, ILBM, SGI and XPM. The rest are produced by
//! `testdata/make_classic_samples.py`.
//!
//! Usage: cargo run --release --example make_legacy -- <out-dir>

use std::fs;
use std::path::PathBuf;

const SIDE: u32 = 16;

fn rgba() -> Vec<u8> {
    let mut out = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let r = (x * 15) as u8;
            let g = (y * 15) as u8;
            let b = if ((x / 4) + (y / 4)) % 2 == 0 {
                220
            } else {
                40
            };
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    out
}

fn sgi_bytes() -> Vec<u8> {
    let px = rgba();
    let mut out = Vec::with_capacity(512 + 3 * 256);
    out.extend_from_slice(&474u16.to_be_bytes()); // magic
    out.push(0); // storage: uncompressed
    out.push(1); // bytes per channel
    out.extend_from_slice(&3u16.to_be_bytes()); // dimension
    out.extend_from_slice(&(SIDE as u16).to_be_bytes());
    out.extend_from_slice(&(SIDE as u16).to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes()); // zsize
    out.extend_from_slice(&0u32.to_be_bytes()); // pixmin
    out.extend_from_slice(&255u32.to_be_bytes()); // pixmax
    out.extend_from_slice(&0u32.to_be_bytes()); // dummy
    let mut name = [0u8; 80];
    name[..13].copy_from_slice(b"rupix-classic");
    out.extend_from_slice(&name);
    out.extend_from_slice(&0u32.to_be_bytes()); // colormap NORMAL
    out.resize(512, 0);

    // SGI stores rows bottom-to-top; plane-major R, G, B.
    for channel in 0..3 {
        for y in (0..SIDE).rev() {
            for x in 0..SIDE {
                let i = ((y * SIDE + x) * 4) as usize + channel;
                out.push(px[i]);
            }
        }
    }
    out
}

fn xpm_bytes() -> Vec<u8> {
    let mut s = String::from("/* XPM */\nstatic char *rupix_classic[] = {\n");
    s.push_str("\"16 16 4 1\",\n");
    s.push_str("\"a c #FF0000\",\n");
    s.push_str("\"b c #00FF00\",\n");
    s.push_str("\"c c #0000FF\",\n");
    s.push_str("\"d c #FFFFFF\",\n");
    for y in 0..SIDE {
        s.push('"');
        for x in 0..SIDE {
            let ch = if (x / 4 + y / 4) % 2 == 0 {
                if x < 8 {
                    'a'
                } else {
                    'b'
                }
            } else if y < 8 {
                'c'
            } else {
                'd'
            };
            s.push(ch);
        }
        s.push_str("\",\n");
    }
    s.push_str("};\n");
    s.into_bytes()
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".temp/classic-samples"));
    fs::create_dir_all(&out).expect("create output directory");

    let px = rgba();
    let pict = oxideav_pict::encode_rgba8(SIDE, SIDE, &px, &oxideav_pict::EncodeOptions::default())
        .expect("encode PICT");
    fs::write(out.join("sample.pict"), pict).expect("write PICT");

    let ilbm = oxideav_iff::encode_rgba8(SIDE, SIDE, &px, &oxideav_iff::EncodeOptions::default())
        .expect("encode ILBM");
    fs::write(out.join("sample.ilbm"), ilbm).expect("write ILBM");

    fs::write(out.join("sample.sgi"), sgi_bytes()).expect("write SGI");
    fs::write(out.join("sample.xpm"), xpm_bytes()).expect("write XPM");
    println!("wrote PICT/ILBM/SGI/XPM to {}", out.display());
}
