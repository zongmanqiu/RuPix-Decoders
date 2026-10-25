//! RuPix pro 档成员：科学体数据首层 —— NIfTI-1（`.nii`/`.nii.gz`）、
//! NRRD（`.nrrd`，只支持内嵌数据；分离 `.nhdr` 需要外部文件，不认领）、
//! MRC/CCP4（`.mrc/.mrcs/.ccp4/.map`）。
//!
//! 只取第一帧/第一层的 2D 切片，按该切片的 min/max 拉伸到 8 位灰度。
//! 这类格式没有“显示窗口”元数据时，拉伸是唯一不让画面全黑/全白的办法；
//! 查询阶段只读头，渲染阶段才读像素。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use flate2::read::GzDecoder;
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;
use std::io::Read;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-sci",
    name: "Scientific Volumes (NIfTI/NRRD/MRC)",
    extensions: "nii;nrrd;mrc;mrcs;ccp4;map",
    description: "科学体数据首层：NIfTI / NRRD / MRC（首层 2D，min/max 拉伸）",
    is_vector: 0,
    has_alpha: 0,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

#[derive(Clone, Copy)]
enum Sample {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    F32,
    F64,
}

impl Sample {
    fn size(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

fn read_u16(d: &[u8], off: usize, e: Endian) -> Option<u16> {
    let b = d.get(off..off + 2)?;
    Some(match e {
        Endian::Little => u16::from_le_bytes([b[0], b[1]]),
        Endian::Big => u16::from_be_bytes([b[0], b[1]]),
    })
}

fn read_u32(d: &[u8], off: usize, e: Endian) -> Option<u32> {
    let b = d.get(off..off + 4)?;
    let a = [b[0], b[1], b[2], b[3]];
    Some(match e {
        Endian::Little => u32::from_le_bytes(a),
        Endian::Big => u32::from_be_bytes(a),
    })
}

fn read_f32(d: &[u8], off: usize, e: Endian) -> Option<f32> {
    read_u32(d, off, e).map(f32::from_bits)
}

fn read_f64(d: &[u8], off: usize, e: Endian) -> Option<f64> {
    let b = d.get(off..off + 8)?;
    let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
    Some(match e {
        Endian::Little => f64::from_bits(u64::from_le_bytes(a)),
        Endian::Big => f64::from_bits(u64::from_be_bytes(a)),
    })
}

fn read_sample(d: &[u8], off: usize, s: Sample, e: Endian) -> Option<f64> {
    Some(match s {
        Sample::U8 => *d.get(off)? as f64,
        Sample::I8 => *d.get(off)? as i8 as f64,
        Sample::U16 => read_u16(d, off, e)? as f64,
        Sample::I16 => read_u16(d, off, e)? as i16 as f64,
        Sample::U32 => read_u32(d, off, e)? as f64,
        Sample::I32 => read_u32(d, off, e)? as i32 as f64,
        Sample::F32 => read_f32(d, off, e)? as f64,
        Sample::F64 => read_f64(d, off, e)?,
    })
}

fn normalize(vals: &[f64]) -> Vec<u8> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in vals {
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || !hi.is_finite() || hi - lo <= f64::EPSILON {
        return vec![0u8; vals.len()];
    }
    let span = hi - lo;
    vals.iter()
        .map(|&v| {
            if !v.is_finite() {
                0
            } else {
                (((v - lo) / span) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)] // 科学影像的“源 + 偏移 + 尺寸 + 类型 + 校准”本来就是一整组参数
fn read_slice(
    data: &[u8],
    off: usize,
    w: u32,
    h: u32,
    s: Sample,
    e: Endian,
    slope: f64,
    inter: f64,
) -> Result<Vec<u8>, (i32, &'static str)> {
    let n = w as usize * h as usize;
    let need = n
        .checked_mul(s.size())
        .and_then(|x| x.checked_add(off))
        .ok_or((ERR_CORRUPT, "科学影像长度溢出"))?;
    if need > data.len() {
        return Err((ERR_CORRUPT, "科学影像像素数据被截断"));
    }
    let mut vals = Vec::with_capacity(n);
    for i in 0..n {
        let v = read_sample(data, off + i * s.size(), s, e)
            .ok_or((ERR_CORRUPT, "科学影像像素数据被截断"))?;
        vals.push(v * slope + inter);
    }
    Ok(normalize(&vals))
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|_| (ERR_CORRUPT, "gzip 解压失败"))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// NIfTI-1
// ---------------------------------------------------------------------------

fn nii_endian(d: &[u8]) -> Option<Endian> {
    if read_u32(d, 0, Endian::Little) == Some(348) {
        Some(Endian::Little)
    } else if read_u32(d, 0, Endian::Big) == Some(348) {
        Some(Endian::Big)
    } else {
        None
    }
}

fn nii_dims_in(d: &[u8]) -> Option<(u32, u32)> {
    let e = nii_endian(d)?;
    if &d.get(344..348)? != b"n+1\0" && &d.get(344..348)? != b"ni1\0" {
        return None;
    }
    let ndim = read_u16(d, 40, e)? as i16;
    if ndim < 2 {
        return None;
    }
    let w = read_u16(d, 42, e)? as i16 as u32;
    let h = read_u16(d, 44, e)? as i16 as u32;
    (w > 0 && h > 0).then_some((w, h))
}

fn nii_decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let e = nii_endian(data).ok_or((ERR_CORRUPT, "不是 NIfTI-1"))?;
    let ndim = read_u16(data, 40, e).ok_or((ERR_CORRUPT, "NIfTI 头截断"))? as i16;
    if ndim < 2 {
        return Err((ERR_UNSUPPORTED, "NIfTI 少于二维"));
    }
    let w = read_u16(data, 42, e).ok_or((ERR_CORRUPT, "NIfTI 头截断"))? as i16 as u32;
    let h = read_u16(data, 44, e).ok_or((ERR_CORRUPT, "NIfTI 头截断"))? as i16 as u32;
    let dtype = read_u16(data, 70, e).ok_or((ERR_CORRUPT, "NIfTI 头截断"))? as i16;
    let off = read_f32(data, 108, e).unwrap_or(0.0) as usize;
    let off = if off < 348 { 352 } else { off };
    let slope = read_f32(data, 112, e).unwrap_or(1.0) as f64;
    let slope = if slope.is_finite() && slope != 0.0 {
        slope
    } else {
        1.0
    };
    let inter = read_f32(data, 116, e).unwrap_or(0.0) as f64;
    let s = match dtype {
        2 => Sample::U8,
        4 => Sample::I16,
        8 => Sample::I32,
        16 => Sample::F32,
        64 => Sample::F64,
        256 => Sample::I8,
        512 => Sample::U16,
        768 => Sample::U32,
        _ => return Err((ERR_UNSUPPORTED, "NIfTI 数据类型不支持")),
    };
    let px = read_slice(data, off, w, h, s, e, slope, inter)?;
    Ok((w, h, support::rgba_from_gray(&px)))
}

// ---------------------------------------------------------------------------
// NRRD
// ---------------------------------------------------------------------------

struct NrrdMeta {
    w: u32,
    h: u32,
    data_off: usize,
    sample: Sample,
    endian: Endian,
    gzip: bool,
}

fn nrrd_meta(data: &[u8]) -> Option<NrrdMeta> {
    if !data.starts_with(b"NRRD000") {
        return None;
    }
    let pos = data.windows(2).position(|w| w == b"\n\n")?;
    let head = std::str::from_utf8(&data[..pos]).ok()?;
    let mut dims = Vec::new();
    let mut sizes = Vec::new();
    let mut sample = Sample::U8;
    let mut endian = Endian::Little;
    let mut gzip = false;
    for line in head.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim() {
            "dimension" => {
                dims = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect()
            }
            "sizes" => {
                sizes = v
                    .split_whitespace()
                    .filter_map(|x| x.parse::<usize>().ok())
                    .collect()
            }
            "type" => sample = nrrd_sample(v.trim())?,
            "endian" => {
                endian = match v.trim() {
                    "big" => Endian::Big,
                    _ => Endian::Little,
                }
            }
            "encoding" => gzip = v.trim() == "gzip",
            _ => {}
        }
    }
    if dims.first().copied().unwrap_or(0) < 2 || sizes.len() < 2 {
        return None;
    }
    Some(NrrdMeta {
        w: sizes[0] as u32,
        h: sizes[1] as u32,
        data_off: pos + 2,
        sample,
        endian,
        gzip,
    })
}

fn nrrd_sample(s: &str) -> Option<Sample> {
    Some(match s {
        "uchar" | "uint8" | "unsigned char" => Sample::U8,
        "signed char" | "int8" => Sample::I8,
        "ushort" | "uint16" | "unsigned short" => Sample::U16,
        "short" | "int16" => Sample::I16,
        "uint" | "uint32" | "unsigned int" => Sample::U32,
        "int" | "int32" => Sample::I32,
        "float" => Sample::F32,
        "double" => Sample::F64,
        _ => return None,
    })
}

fn nrrd_decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let m = nrrd_meta(data).ok_or((ERR_CORRUPT, "NRRD 头不完整"))?;
    let payload = data
        .get(m.data_off..)
        .ok_or((ERR_CORRUPT, "NRRD 没有像素数据"))?;
    let owned;
    let payload = if m.gzip {
        owned = gunzip(payload)?;
        &owned[..]
    } else {
        payload
    };
    let px = read_slice(payload, 0, m.w, m.h, m.sample, m.endian, 1.0, 0.0)?;
    Ok((m.w, m.h, support::rgba_from_gray(&px)))
}

// ---------------------------------------------------------------------------
// MRC / CCP4
// ---------------------------------------------------------------------------

fn mrc_endian(d: &[u8]) -> Option<Endian> {
    let ok = |e: Endian| -> Option<bool> {
        let nx = read_u32(d, 0, e)? as usize;
        let ny = read_u32(d, 4, e)? as usize;
        let nz = read_u32(d, 8, e)? as usize;
        Some(
            (1..=500_000).contains(&nx)
                && (1..=500_000).contains(&ny)
                && (1..=500_000).contains(&nz)
                && nx.checked_mul(ny)?.checked_mul(nz)? <= 64_000_000,
        )
    };
    if ok(Endian::Little).unwrap_or(false) {
        Some(Endian::Little)
    } else if ok(Endian::Big).unwrap_or(false) {
        Some(Endian::Big)
    } else {
        None
    }
}

fn mrc_meta(data: &[u8]) -> Option<(u32, u32, Sample, Endian)> {
    if data.len() < 1024 {
        return None;
    }
    let e = mrc_endian(data)?;
    let w = read_u32(data, 0, e)?;
    let h = read_u32(data, 4, e)?;
    let mode = read_u32(data, 12, e)?;
    let s = match mode {
        0 => Sample::I8,
        1 => Sample::I16,
        2 => Sample::F32,
        6 => Sample::U16,
        _ => return None,
    };
    Some((w, h, s, e))
}

fn mrc_decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let (w, h, s, e) = mrc_meta(data).ok_or((ERR_CORRUPT, "MRC 头不完整或模式不支持"))?;
    let px = read_slice(data, 1024, w, h, s, e, 1.0, 0.0)?;
    Ok((w, h, support::rgba_from_gray(&px)))
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let owned;
    let d = if data.starts_with(&[0x1F, 0x8B]) {
        owned = gunzip(data).ok()?;
        &owned[..]
    } else {
        data
    };
    nii_dims_in(d)
        .or_else(|| nrrd_meta(d).map(|m| (m.w, m.h)))
        .or_else(|| mrc_meta(d).map(|m| (m.0, m.1)))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let owned;
    let d = if data.starts_with(&[0x1F, 0x8B]) {
        owned = gunzip(data)?;
        &owned[..]
    } else {
        data
    };
    if nii_dims_in(d).is_some() {
        nii_decode(d)
    } else if nrrd_meta(d).is_some() {
        nrrd_decode(d)
    } else if mrc_meta(d).is_some() {
        mrc_decode(d)
    } else {
        Err((ERR_UNRECOGNIZED, "不是本插件能解的科学影像字节特征"))
    }
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_impl(data, len, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 16 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = unsafe { rupix_abi::ext_str(ext) };
        // 解压前的 NIfTI gzip 也认；其余按头结构认。
        if slice.starts_with(&[0x1F, 0x8B]) {
            let Ok(plain) = gunzip(slice) else { return 0 };
            if nii_dims_in(&plain).is_some() {
                return 100;
            }
            return 0;
        }
        if nii_dims_in(slice).is_some() || nrrd_meta(slice).is_some() || mrc_meta(slice).is_some() {
            return 100;
        }
        // `.map` 太泛，只有 ext 点名且 1024 字节头结构成立才认。
        if ext.is_some_and(|e| e.eq_ignore_ascii_case("map")) && mrc_meta(slice).is_some() {
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
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
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
            set_last_error("不是合法的科学影像字节特征");
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
            set_last_error("科学影像头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_known_headers() {
        let nrrd =
            b"NRRD0004\ntype: uchar\ndimension: 2\nsizes: 2 2\nencoding: raw\n\n\x00\x00\x00\x00";
        assert_eq!(iv_probe(nrrd.as_ptr(), nrrd.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
    }

    #[test]
    fn normalize_endpoints() {
        assert_eq!(normalize(&[0.0, 255.0]), vec![0, 255]);
        assert_eq!(normalize(&[5.0, 5.0]), vec![0, 0]);
    }

    fn nii_fixture() -> Vec<u8> {
        let mut h = vec![0u8; 348];
        h[0..4].copy_from_slice(&348i32.to_le_bytes());
        for (off, v) in [(40, 2i16), (42, 2), (44, 2), (46, 1), (70, 2), (72, 8)] {
            h[off..off + 2].copy_from_slice(&v.to_le_bytes());
        }
        h[76..80].copy_from_slice(&1.0f32.to_le_bytes());
        h[108..112].copy_from_slice(&352.0f32.to_le_bytes());
        h[112..116].copy_from_slice(&1.0f32.to_le_bytes());
        h[344..348].copy_from_slice(b"n+1\0");
        h.extend_from_slice(&[0u8; 4]);
        h.extend_from_slice(&[255, 0, 0, 0]);
        h
    }

    fn mrc_fixture() -> Vec<u8> {
        let mut h = vec![0u8; 1024];
        h[0..4].copy_from_slice(&2i32.to_le_bytes());
        h[4..8].copy_from_slice(&2i32.to_le_bytes());
        h[8..12].copy_from_slice(&1i32.to_le_bytes());
        h[208..212].copy_from_slice(b"MAP ");
        h.extend_from_slice(&[255, 0, 0, 0]);
        h
    }

    #[test]
    fn decodes_minimal_nii_nrrd_mrc() {
        let nrrd =
            b"NRRD0004\ntype: uchar\ndimension: 2\nsizes: 2 2\nencoding: raw\n\n\xff\x00\x00\x00";
        for (name, bytes) in [("nii", nii_fixture()), ("nrrd", nrrd.to_vec())] {
            let (w, h, rgba) = decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!((w, h), (2, 2), "{name}");
            assert_eq!(&rgba[0..4], &[255, 255, 255, 255], "{name} first pixel");
            assert_eq!(&rgba[4..8], &[0, 0, 0, 255], "{name} second pixel");
        }
        // MRC mode 0 is signed int8: 0xFF is -1, so min/max maps -1..0 to 0..255.
        let (w, h, rgba) = decode(&mrc_fixture()).expect("mrc");
        assert_eq!((w, h), (2, 2), "mrc");
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255], "mrc first pixel");
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255], "mrc second pixel");
    }
}
