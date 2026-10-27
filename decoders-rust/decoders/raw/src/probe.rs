use std::ffi::{c_char, CStr};

use rupix_abi::guard;

/// 路由表（`MEMBERS`）直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if looks_like_raw(slice) {
            return 100;
        }
        // CRW/ORF/ORI/RW2/RWL/ARI/QTK/Leica RAW have no TIFF magic that the
        // host can recognise. With a claimed extension, let rawler's own
        // header parser decide; bogus bytes still fail (and rawpx dumps stay
        // unclaimed), while a real vendor file gets through.
        if ext_name(ext).is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str())) {
            let source = rawler::rawsource::RawSource::new_from_slice(slice);
            if rawler::decode_dummy(&source).is_ok() {
                return 100;
            }
        }
        0
    })
}

fn ext_name(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ext) }
        .to_str()
        .ok()?
        .trim_start_matches('.');
    Some(s.to_ascii_lowercase())
}

const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "cr2", "cr3", "crm", "crw", "dcr", "dcs", "dng", "erf", "fff", "iiq",
    "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "ori", "pef", "qtk", "raf", "raw", "rw2",
    "rwl", "srw", "x3f",
];

/// RAW 家族魔数：
/// - `FUJIFILM`（RAF）、`FOVb`（X3F/Sigma）、`\0MRM`（Minolta MRW）、
///   `IIII`（Phase One IIQ）、`3ASH`（Hasselblad 3FR/FFF）
/// - `ftyp` + `crx`（CR3）
/// - 其余（CR2/NEF/ARW/DNG/PEF/RW2/ORF…）都是 TIFF 容器：`II*\0` / `MM\0*`。
fn looks_like_raw(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    if &data[0..8] == b"FUJIFILM" {
        return true;
    }
    if &data[0..4] == b"FOVb" {
        return true;
    }
    if &data[0..4] == b"\0MRM" {
        return true;
    }
    if &data[0..4] == b"IIII" {
        return true;
    }
    if &data[0..4] == b"3ASH" {
        return true;
    }
    if &data[4..8] == b"ftyp" {
        let brand = &data[8..12];
        if &data[8..11] == b"crx" || &brand[0..3] == b"c" && brand[3].is_ascii_digit() {
            return true;
        }
    }
    // TIFF 头（II = 小端，MM = 大端）。
    &data[0..4] == b"II*\0" || &data[0..4] == b"MM\0*"
}
