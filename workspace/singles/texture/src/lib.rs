//! RuPix texture formats: DDS, KTX/KTX2, ASTC, PVR, Basis, BLP and VTF.
//!
//! The format-specific work is kept here; `workspace/decoders/texture` only
//! assembles the ABI member table.

#![allow(non_snake_case)]

use image::ImageDecoder;
use pro_support as support;
use rupix_abi::guard;
use rupix_abi::*;
use std::ffi::c_char;
use std::io::Cursor;

pub(crate) fn input<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() || len == 0 {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(data, len) })
    }
}

pub(crate) fn finish(
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
) -> i32 {
    if out.is_null() {
        support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
        return ERR_OK;
    }
    support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
}

pub(crate) fn decode_dynamic(
    decoder: impl ImageDecoder,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (w, h) = decoder.dimensions();
    support::check_dims(w, h, support::budget_of(req))?;
    let img =
        image::DynamicImage::from_decoder(decoder).map_err(|_| (ERR_CORRUPT, "纹理解码失败"))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba.into_raw(), w, h))
}

pub(crate) fn render_scan(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    decode: impl FnOnce(&[u8], *const IvRenderReq) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)>,
) -> i32 {
    guard(|| {
        let Some(bytes) = input(data, len) else {
            set_last_error("内部错误（空数据）");
            return ERR_UNRECOGNIZED;
        };
        match decode(bytes, req) {
            Ok((rgba, w, h)) => finish(rgba, w, h, out, out_cap, info),
            Err((code, msg)) => {
                set_last_error(msg);
                code
            }
        }
    })
}

fn rgba_from_u32(buf: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len() * 4);
    for &v in buf {
        let b = v.to_le_bytes();
        out.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
    }
    out
}

// ---- DDS ---------------------------------------------------------------

fn dds_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder = image_extras::dds::DdsDecoder::new(Cursor::new(data))
        .map_err(|_| (ERR_CORRUPT, "DDS 文件头解析失败"))?;
    decode_dynamic(decoder, req)
}

pub fn probe_dds(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b) if b.starts_with(b"DDS ") => 100,
        _ => 0,
    })
}

pub fn render_dds(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, dds_decode)
}

// ---- ASTC ---------------------------------------------------------------

const ASTC_MAGIC: [u8; 4] = [0x13, 0xAB, 0xA1, 0x5C];

fn u24(d: &[u8], at: usize) -> Option<usize> {
    let s = d.get(at..at + 3)?;
    Some(s[0] as usize | (s[1] as usize) << 8 | (s[2] as usize) << 16)
}

fn astc_header(data: &[u8]) -> Option<(usize, usize, usize, usize)> {
    if !data.starts_with(&ASTC_MAGIC) || data.len() < 16 {
        return None;
    }
    let bw = data[4] as usize;
    let bh = data[5] as usize;
    let bz = data[6] as usize;
    let w = u24(data, 7)?;
    let h = u24(data, 10)?;
    let depth = u24(data, 13)?;
    (bw > 0 && bh > 0 && bz == 1 && w > 0 && h > 0 && depth == 1).then_some((w, h, bw, bh))
}

fn astc_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (w, h, bw, bh) = astc_header(data).ok_or((ERR_CORRUPT, "ASTC 头无效"))?;
    support::check_dims(w as u32, h as u32, support::budget_of(req))?;
    let mut px = vec![0u32; w * h];
    texture2ddecoder::decode_astc(&data[16..], w, h, bw, bh, &mut px)
        .map_err(|_| (ERR_CORRUPT, "ASTC 块解码失败"))?;
    Ok((rgba_from_u32(&px), w as u32, h as u32))
}

pub fn probe_astc(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b) if astc_header(b).is_some() => 100,
        _ => 0,
    })
}

pub fn render_astc(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, astc_decode)
}

// ---- PVR ----------------------------------------------------------------

fn pvr_header(data: &[u8]) -> Option<(usize, usize, usize, bool)> {
    if data.len() < 52 || u32::from_le_bytes(data[0..4].try_into().ok()?) != 0x0352_5650 {
        return None;
    }
    let fmt = u64::from_le_bytes(data[8..16].try_into().ok()?) as u32;
    let h = u32::from_le_bytes(data[24..28].try_into().ok()?) as usize;
    let w = u32::from_le_bytes(data[28..32].try_into().ok()?) as usize;
    let meta = u32::from_le_bytes(data[48..52].try_into().ok()?) as usize;
    let start = 52usize.checked_add(meta)?;
    let is2bpp = match fmt {
        0 | 1 => true,
        2 | 3 => false,
        _ => return None,
    };
    (w > 0 && h > 0 && start <= data.len()).then_some((w, h, start, is2bpp))
}

fn pvr_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (w, h, start, is2bpp) =
        pvr_header(data).ok_or((ERR_CORRUPT, "PVR 头无效或不支持该像素格式"))?;
    support::check_dims(w as u32, h as u32, support::budget_of(req))?;
    let mut px = vec![0u32; w * h];
    texture2ddecoder::decode_pvrtc(&data[start..], w, h, &mut px, is2bpp)
        .map_err(|_| (ERR_CORRUPT, "PVR 块解码失败"))?;
    Ok((rgba_from_u32(&px), w as u32, h as u32))
}

pub fn probe_pvr(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b) if pvr_header(b).is_some() || b.starts_with(b"PVR\x03") => 100,
        _ => 0,
    })
}

pub fn render_pvr(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, pvr_decode)
}

// ---- Basis Universal ----------------------------------------------------

fn basis_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let transcoder =
        basisu::Transcoder::new(data).map_err(|_| (ERR_CORRUPT, "Basis 容器解析失败"))?;
    let (w, h) = transcoder.base_dimensions();
    support::check_dims(w, h, support::budget_of(req))?;
    let px = transcoder
        .transcode(0, basisu::TargetFormat::Rgba32, basisu::DecodeFlags::NONE)
        .map_err(|_| (ERR_CORRUPT, "Basis 转码失败"))?;
    if px.len() != w as usize * h as usize * 4 {
        return Err((ERR_CORRUPT, "Basis 输出长度不符"));
    }
    Ok((px, w, h))
}

pub fn probe_basis(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b)
            if (b.starts_with(&KTX2_MAGIC) || (b.len() >= 2 && b[0] == 0x73 && b[1] == 0x42))
                && basisu::Transcoder::new(b).is_ok() =>
        {
            100
        }
        _ => 0,
    })
}

pub fn render_basis(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, basis_decode)
}

const KTX2_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

// ---- BLP ----------------------------------------------------------------
//
// Self-written BLP1/BLP2 parser. Do not replace this with the `blp` crate:
// it pulls `self_cell` under GPL-2.0-only, which cannot be combined into this
// AGPL-3.0 DLL by adding a notice file.

struct BlpMeta {
    version: u8,
    encoding: u8,
    alpha_bits: u8,
    alpha_type: u8,
    width: usize,
    height: usize,
    offset: usize,
    size: usize,
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn blp_meta(data: &[u8]) -> Option<BlpMeta> {
    if data.len() < 148 {
        return None;
    }
    if data.starts_with(b"BLP2") {
        let encoding = data[8];
        let alpha_bits = data[9];
        let alpha_type = data[10];
        let width = le32(data, 12)? as usize;
        let height = le32(data, 16)? as usize;
        let offset = le32(data, 20)? as usize;
        let size = le32(data, 84)? as usize;
        if width == 0 || height == 0 || offset > data.len() || size == 0 {
            return None;
        }
        if offset.checked_add(size)? > data.len() {
            return None;
        }
        return Some(BlpMeta {
            version: 2,
            encoding,
            alpha_bits,
            alpha_type,
            width,
            height,
            offset,
            size,
        });
    }
    if data.starts_with(b"BLP1") && data.len() >= 156 {
        let encoding = le32(data, 4)? as u8;
        let alpha_bits = le32(data, 8)? as u8;
        let width = le32(data, 12)? as usize;
        let height = le32(data, 16)? as usize;
        let offset = le32(data, 28)? as usize;
        let size = le32(data, 92)? as usize;
        if width == 0 || height == 0 || offset > data.len() || size == 0 {
            return None;
        }
        if offset.checked_add(size)? > data.len() {
            return None;
        }
        return Some(BlpMeta {
            version: 1,
            encoding,
            alpha_bits,
            alpha_type: 0,
            width,
            height,
            offset,
            size,
        });
    }
    None
}

fn blp_alpha(raw: &[u8], i: usize, bits: u8) -> u8 {
    match bits {
        0 => 255,
        1 => {
            let byte = raw.get(i / 8).copied().unwrap_or(0);
            if (byte >> (i % 8)) & 1 == 1 {
                255
            } else {
                0
            }
        }
        4 => {
            let byte = raw.get(i / 2).copied().unwrap_or(0);
            let n = if i & 1 == 0 { byte & 0x0F } else { byte >> 4 };
            (n << 4) | n
        }
        _ => raw.get(i).copied().unwrap_or(255),
    }
}

fn blp_palette(
    data: &[u8],
    w: usize,
    h: usize,
    palette_at: usize,
    body: &[u8],
    bits: u8,
) -> Result<Vec<u8>, (i32, &'static str)> {
    let palette = data
        .get(palette_at..palette_at + 1024)
        .ok_or((ERR_CORRUPT, "BLP 调色板被截断"))?;
    let pixels = w * h;
    let indices = body.get(..pixels).ok_or((ERR_CORRUPT, "BLP 索引被截断"))?;
    let alpha_bytes = match bits {
        0 => 0,
        1 => pixels.div_ceil(8),
        4 => pixels.div_ceil(2),
        8 => pixels,
        _ => return Err((ERR_UNSUPPORTED, "BLP 调色板 alpha 位深不支持")),
    };
    let alpha = body
        .get(pixels..pixels + alpha_bytes)
        .ok_or((ERR_CORRUPT, "BLP alpha 被截断"))?;
    let mut out = Vec::with_capacity(pixels * 4);
    for (i, &idx) in indices.iter().enumerate() {
        let e = palette
            .get(idx as usize * 4..idx as usize * 4 + 4)
            .ok_or((ERR_CORRUPT, "BLP 调色板索引越界"))?;
        out.extend_from_slice(&[e[2], e[1], e[0], blp_alpha(alpha, i, bits)]);
    }
    Ok(out)
}

fn blp_dxt(data: &[u8], meta: &BlpMeta) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let body = data
        .get(meta.offset..meta.offset + meta.size)
        .ok_or((ERR_CORRUPT, "BLP DXT 数据被截断"))?;
    let mut px = vec![0u32; meta.width * meta.height];
    let res = match meta.alpha_type {
        0 => texture2ddecoder::decode_bc1(body, meta.width, meta.height, &mut px),
        1 => texture2ddecoder::decode_bc2(body, meta.width, meta.height, &mut px),
        7 => texture2ddecoder::decode_bc3(body, meta.width, meta.height, &mut px),
        _ => return Err((ERR_UNSUPPORTED, "BLP DXT alpha 类型不支持")),
    };
    res.map_err(|_| (ERR_CORRUPT, "BLP DXT 解码失败"))?;
    Ok((rgba_from_u32(&px), meta.width as u32, meta.height as u32))
}

fn blp_jpeg(
    data: &[u8],
    meta: &BlpMeta,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (len_at, start) = if meta.version == 2 {
        (148, 152)
    } else {
        (156, 160)
    };
    let hdr_len = le32(data, len_at).ok_or((ERR_CORRUPT, "BLP JPEG 头长度缺失"))? as usize;
    let hdr = data
        .get(start..start + hdr_len)
        .ok_or((ERR_CORRUPT, "BLP JPEG 头被截断"))?;
    let body = data
        .get(meta.offset..meta.offset + meta.size)
        .ok_or((ERR_CORRUPT, "BLP JPEG 数据被截断"))?;
    let mut jpeg = Vec::with_capacity(hdr.len() + body.len());
    jpeg.extend_from_slice(hdr);
    jpeg.extend_from_slice(body);
    // The embedded JPEG's own SOF dimensions are attacker-controlled and need
    // not match the BLP header, so gate the decoder itself: without Limits a
    // 1x1 header can hide a 64k x 64k JPEG and abort the host on OOM.
    let budget = support::budget_of(req) as u64;
    let max_alloc = if budget == 0 {
        support::MAX_PIXELS * 4
    } else {
        budget
    };
    let mut reader = image::ImageReader::new(Cursor::new(&jpeg))
        .with_guessed_format()
        .map_err(|_| (ERR_CORRUPT, "BLP JPEG 解码失败"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(0xFFFF);
    limits.max_image_height = Some(0xFFFF);
    limits.max_alloc = Some(max_alloc);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|_| (ERR_CORRUPT, "BLP JPEG 解码失败"))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    support::check_dims(w, h, support::budget_of(req))?;
    Ok((rgba.into_raw(), w, h))
}

fn blp_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let meta = blp_meta(data).ok_or((ERR_CORRUPT, "BLP1/BLP2 头无效"))?;
    support::check_dims(
        meta.width as u32,
        meta.height as u32,
        support::budget_of(req),
    )?;
    let body = data
        .get(meta.offset..meta.offset + meta.size)
        .ok_or((ERR_CORRUPT, "BLP 数据被截断"))?;
    if meta.encoding == 2 {
        return blp_dxt(data, &meta);
    }
    if meta.encoding == 3 && meta.version == 2 {
        if body.len() < meta.width * meta.height * 4 {
            return Err((ERR_CORRUPT, "BLP BGRA 数据被截断"));
        }
        let mut out = Vec::with_capacity(meta.width * meta.height * 4);
        for c in body.chunks_exact(4).take(meta.width * meta.height) {
            out.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
        }
        return Ok((out, meta.width as u32, meta.height as u32));
    }
    if meta.encoding == 1 {
        let palette_at = if meta.version == 2 { 148 } else { 156 };
        let out = blp_palette(
            data,
            meta.width,
            meta.height,
            palette_at,
            body,
            meta.alpha_bits,
        )?;
        return Ok((out, meta.width as u32, meta.height as u32));
    }
    if meta.encoding == 0 {
        return blp_jpeg(data, &meta, req);
    }
    Err((ERR_UNSUPPORTED, "BLP 编码类型不支持"))
}

pub fn probe_blp(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b) if b.starts_with(b"BLP1") || b.starts_with(b"BLP2") => 100,
        _ => 0,
    })
}

pub fn render_blp(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, blp_decode)
}
// ---- VTF ----------------------------------------------------------------

fn vtf_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let vtf = vtf::from_bytes(data).map_err(|_| (ERR_CORRUPT, "VTF 文件头解析失败"))?;
    // Gate on the header dimensions BEFORE decode(): the crate allocates
    // width*height*4 from them, and a u16 pair allows up to 4 GP.
    support::check_dims(
        vtf.highres_image.width as u32,
        vtf.highres_image.height as u32,
        support::budget_of(req),
    )?;
    let img = vtf
        .highres_image
        .decode(0)
        .map_err(|_| (ERR_CORRUPT, "VTF 像素解码失败"))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    support::check_dims(w, h, support::budget_of(req))?;
    Ok((rgba.into_raw(), w, h))
}

pub fn probe_vtf(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(b) if b.starts_with(b"VTF\0") => 100,
        _ => 0,
    })
}

pub fn render_vtf(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, vtf_decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_texture_magics() {
        assert_eq!(probe_dds(b"DDS ".as_ptr(), 4, std::ptr::null()), 100);
        assert_eq!(
            probe_astc(
                [0x13, 0xAB, 0xA1, 0x5C, 4, 4, 1, 4, 0, 0, 4, 0, 0, 1, 0, 0].as_ptr(),
                16,
                std::ptr::null()
            ),
            100
        );
        assert_eq!(probe_blp(b"BLP2".as_ptr(), 4, std::ptr::null()), 100);
        assert_eq!(
            probe_vtf(c"VTF".as_ptr() as *const u8, 4, std::ptr::null()),
            100
        );
    }

    #[test]
    fn vtf_round_trips() {
        use image::{DynamicImage, Rgba, RgbaImage};
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([12, 34, 56, 255])));
        let bytes = vtf::create(img, vtf::ImageFormat::Rgba8888).expect("encode VTF");
        let (px, w, h) = vtf_decode(&bytes, std::ptr::null()).expect("decode VTF");
        assert_eq!((w, h), (2, 2));
        assert_eq!(px, [12, 34, 56, 255].repeat(4));
    }

    #[test]
    fn blp2_bgra_decodes() {
        let mut b = vec![0u8; 148];
        b[..4].copy_from_slice(b"BLP2");
        b[8] = 3;
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b[20..24].copy_from_slice(&148u32.to_le_bytes());
        b[84..88].copy_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[10, 20, 30, 40]);
        let (px, w, h) = blp_decode(&b, std::ptr::null()).expect("decode BLP2 BGRA");
        assert_eq!((w, h), (1, 1));
        assert_eq!(px, [30, 20, 10, 40]);
    }

    #[test]
    fn blp1_palette_decodes() {
        let mut b = vec![0u8; 156];
        b[..4].copy_from_slice(b"BLP1");
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        b[8..12].copy_from_slice(&0u32.to_le_bytes());
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b[28..32].copy_from_slice(&1180u32.to_le_bytes());
        b[92..96].copy_from_slice(&1u32.to_le_bytes());
        b.resize(1180, 0);
        b[156..160].copy_from_slice(&[10, 20, 30, 0]);
        b.push(0);
        let (px, w, h) = blp_decode(&b, std::ptr::null()).expect("decode BLP1 palette");
        assert_eq!((w, h), (1, 1));
        assert_eq!(px, [30, 20, 10, 255]);
    }
}
