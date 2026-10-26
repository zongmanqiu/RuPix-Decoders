use std::ffi::c_char;

use rupix_abi::guard;

/// 路由表（`MEMBERS`）直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if looks_like_raw(slice) {
            100
        } else {
            0
        }
    })
}

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
