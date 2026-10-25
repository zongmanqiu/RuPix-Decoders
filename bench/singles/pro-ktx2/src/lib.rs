//! RuPix pro 档成员：KTX 纹理（`.ktx/.ktx2`）。
//!
//! KTX1 只解未压缩 8 位 RGBA/BGRA/RGB/R8；KTX2 额外支持 BC1/BC3/BC7。
//! Basis Universal 等 `VK_FORMAT_UNDEFINED` 的超压缩纹理不在本档（返回 UNSUPPORTED）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use ktx2::{Format, Reader};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-ktx2",
    name: "KTX Texture",
    extensions: "ktx;ktx2",
    description: "KTX1 未压缩 8 位 + KTX2（未压缩 RGBA/BGRA/RGB/R8、BC1/BC3/BC7）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

const KTX2_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

const KTX1_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x31, 0x31, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

fn le32(d: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(off..off + 4)?.try_into().ok()?))
}

/// KTX1 header fields used by the uncompressed path: (w, h, glFormat, key-value bytes).
fn ktx1_header(d: &[u8]) -> Option<(u32, u32, u32, usize)> {
    if !d.starts_with(&KTX1_MAGIC) || le32(d, 12)? != 0x0403_0201 {
        return None;
    }
    let gl_type = le32(d, 16)?;
    let type_size = le32(d, 20)?;
    let format = le32(d, 24)?;
    let w = le32(d, 36)?;
    let h = le32(d, 40)?;
    let depth = le32(d, 44)?;
    let array = le32(d, 48)?;
    let faces = le32(d, 52)?;
    let kv = le32(d, 60)? as usize;
    (gl_type == 0x1401
        && type_size == 1
        && w > 0
        && h > 0
        && depth <= 1
        && array == 0
        && faces == 1)
        .then_some((w, h, format, kv))
}

fn ktx1_decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let (w, h, fmt, kv) = ktx1_header(data).ok_or((ERR_CORRUPT, "KTX1 头不完整或非未压缩 8 位"))?;
    let channels = match fmt {
        0x1908 | 0x80E1 => 4, // RGBA / BGRA
        0x1907 => 3,          // RGB
        0x1903 | 0x1909 => 1, // RED / LUMINANCE
        _ => return Err((ERR_UNSUPPORTED, "KTX1 像素格式本档不解")),
    };
    let off = 64usize
        .checked_add(kv)
        .ok_or((ERR_CORRUPT, "KTX1 头长度溢出"))?;
    let image_size = le32(data, off).ok_or((ERR_CORRUPT, "KTX1 缺少 imageSize"))? as usize;
    let start = off.checked_add(4).ok_or((ERR_CORRUPT, "KTX1 头长度溢出"))?;
    let stride = (w as usize * channels + 3) & !3;
    let need = stride
        .checked_mul(h as usize)
        .ok_or((ERR_CORRUPT, "KTX1 像素长度溢出"))?;
    if image_size < need || start.saturating_add(need) > data.len() {
        return Err((ERR_CORRUPT, "KTX1 像素数据被截断"));
    }
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let p = start + y * stride + x * channels;
            match channels {
                1 => {
                    let g = data[p];
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
                3 => rgba.extend_from_slice(&[data[p], data[p + 1], data[p + 2], 255]),
                _ if fmt == 0x80E1 => {
                    rgba.extend_from_slice(&[data[p + 2], data[p + 1], data[p], data[p + 3]])
                }
                _ => rgba.extend_from_slice(&data[p..p + 4]),
            }
        }
    }
    Ok((w, h, rgba))
}

fn reader(data: &[u8]) -> Result<Reader<&[u8]>, (i32, &'static str)> {
    Reader::new(data).map_err(|_| (ERR_CORRUPT, "KTX2 容器解析失败"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if let Some((w, h, _, _)) = ktx1_header(data) {
        return Some((w, h));
    }
    let r = reader(data).ok()?;
    let h = r.header();
    (h.pixel_width > 0 && h.pixel_height > 0).then_some((h.pixel_width, h.pixel_height))
}

fn rgba_from_bgra(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len());
    for c in px.chunks_exact(4) {
        out.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
    }
    out
}

fn rgba_from_u32(buf: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len() * 4);
    for &v in buf {
        let b = v.to_le_bytes();
        out.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
    }
    out
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    if data.starts_with(&KTX1_MAGIC) {
        return ktx1_decode(data);
    }
    let r = reader(data)?;
    let h = r.header();
    let (w, hgt) = (h.pixel_width, h.pixel_height);
    if h.supercompression_scheme.is_some() {
        return Err((ERR_UNSUPPORTED, "KTX2 超压缩纹理本档不解"));
    }
    let level = r
        .levels()
        .next()
        .ok_or((ERR_CORRUPT, "KTX2 没有 mip level"))?;
    let d = level.data;
    let Some(fmt) = h.format else {
        return Err((ERR_UNSUPPORTED, "KTX2 未声明纹素格式"));
    };
    let n = w as usize * hgt as usize;

    let (bytes, channels) = match fmt {
        Format::R8G8B8A8_UNORM | Format::R8G8B8A8_SRGB => (d, 4),
        Format::B8G8R8A8_UNORM | Format::B8G8R8A8_SRGB => (d, 4),
        Format::R8G8B8_UNORM | Format::R8G8B8_SRGB => (d, 3),
        Format::R8_UNORM | Format::R8_SRGB => (d, 1),
        _ => (d, 0),
    };
    if channels != 0 {
        let need = n * channels;
        let px = bytes
            .get(..need)
            .ok_or((ERR_CORRUPT, "KTX2 像素数据被截断"))?;
        let rgba = match channels {
            1 => support::rgba_from_gray(px),
            3 => support::rgba_from_rgb(px),
            _ if fmt == Format::B8G8R8A8_UNORM || fmt == Format::B8G8R8A8_SRGB => {
                rgba_from_bgra(px)
            }
            _ => support::rgba_passthrough(px),
        };
        return Ok((w, hgt, rgba));
    }

    // 块压缩：BC1/BC3/BC7 都按 4×4 解码到 u32 缓冲。
    let mut buf = vec![0u32; n];
    let res = if fmt == Format::BC1_RGB_UNORM_BLOCK
        || fmt == Format::BC1_RGB_SRGB_BLOCK
        || fmt == Format::BC1_RGBA_UNORM_BLOCK
        || fmt == Format::BC1_RGBA_SRGB_BLOCK
    {
        texture2ddecoder::decode_bc1(d, w as usize, hgt as usize, &mut buf)
    } else if fmt == Format::BC3_UNORM_BLOCK || fmt == Format::BC3_SRGB_BLOCK {
        texture2ddecoder::decode_bc3(d, w as usize, hgt as usize, &mut buf)
    } else if fmt == Format::BC7_UNORM_BLOCK || fmt == Format::BC7_SRGB_BLOCK {
        texture2ddecoder::decode_bc7(d, w as usize, hgt as usize, &mut buf)
    } else {
        return Err((ERR_UNSUPPORTED, "KTX2 纹素格式本档不解"));
    };
    res.map_err(|_| (ERR_CORRUPT, "KTX2 块解码失败"))?;
    Ok((w, hgt, rgba_from_u32(&buf)))
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 12 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(&KTX2_MAGIC) || ktx1_header(slice).is_some() {
            100
        } else {
            0
        }
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, _ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let Some((w, h)) = dimensions(slice) else {
            set_last_error("不是合法的 KTX 字节特征");
            return ERR_UNRECOGNIZED;
        };
        let (_, _, bytes) = match support::check_dims(w, h, support::budget_of(req)) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, bytes, 1, 0);
            return ERR_OK;
        }
        let (dw, dh, rgba) = match decode(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if (dw, dh) != (w, h) || rgba.len() as u64 != bytes {
            set_last_error("KTX 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_ktx2_magic() {
        assert_eq!(iv_probe(KTX2_MAGIC.as_ptr(), 12, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue() {
        assert_eq!(rgba_from_bgra(&[1, 2, 3, 4]), vec![3, 2, 1, 4]);
    }

    fn ktx1(w: u32, h: u32, fmt: u32, pixels: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; 64];
        d[0..12].copy_from_slice(&KTX1_MAGIC);
        for (off, v) in [
            (12, 0x0403_0201u32),
            (16, 0x1401),
            (20, 1),
            (24, fmt),
            (28, fmt),
            (32, fmt),
            (36, w),
            (40, h),
            (44, 0),
            (48, 0),
            (52, 1),
            (56, 1),
            (60, 0),
        ] {
            d[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        d.extend_from_slice(pixels);
        d
    }

    #[test]
    fn decodes_uncompressed_ktx1_rgba_and_rgb() {
        let rgba = ktx1(1, 1, 0x1908, &[1, 2, 3, 4]);
        assert_eq!(probe_impl(rgba.as_ptr(), rgba.len(), std::ptr::null()), 100);
        assert_eq!(dimensions(&rgba), Some((1, 1)));
        assert_eq!(decode(&rgba).expect("rgba").2, [1, 2, 3, 4]);

        // RGB rows are padded to 4 bytes: one row = 3 bytes + 1 pad.
        let rgb = ktx1(1, 1, 0x1907, &[10, 20, 30, 0]);
        assert_eq!(decode(&rgb).expect("rgb").2, [10, 20, 30, 255]);
    }
}
