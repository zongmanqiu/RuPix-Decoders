//! RuPix doc member: EMF/EMZ through `emfsdk`'s portable raster renderer.
//!
//! `emfsdk` returns PNG bytes; this member decodes them to RGBA8 and enforces
//! the shared 64 MP / budget limits. EMZ is gzip-wrapped EMF and is accepted
//! only when the caller names `.emz` (gzip magic alone is too generic).

#![allow(non_snake_case)]
#![allow(linker_messages)]

use emfsdk::render::{decode_metafile_as_raster_with_options, DecodedMetafile, RenderOptions};
use pro_support as support;
use rupix_abi::*;
use std::borrow::Cow;
use std::ffi::c_char;
use std::io::Read;

const EMZ_MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_PIXELS: u32 = 64 * 1024 * 1024;

fn is_gzip(slice: &[u8]) -> bool {
    slice.starts_with(&[0x1F, 0x8B])
}

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let mut out = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(data).take(EMZ_MAX_BYTES as u64 + 1);
    decoder
        .read_to_end(&mut out)
        .map_err(|_| (ERR_CORRUPT, "EMZ 的 gzip 包装损坏"))?;
    if out.len() > EMZ_MAX_BYTES {
        return Err((ERR_TOO_LARGE, "EMZ 解包后超过 64 MiB 上限"));
    }
    Ok(out)
}

fn payload(data: &[u8]) -> Result<Cow<'_, [u8]>, (i32, &'static str)> {
    if is_gzip(data) {
        Ok(Cow::Owned(gunzip(data)?))
    } else {
        Ok(Cow::Borrowed(data))
    }
}

fn is_emf(bytes: &[u8]) -> bool {
    matches!(
        emfsdk::Metafile::from_bytes(bytes),
        Ok(emfsdk::Metafile::Emf(_))
    )
}

fn render_metafile(
    bytes: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    let options = RenderOptions {
        target_width_px: (target_w > 0).then_some(target_w),
        target_height_px: (target_h > 0).then_some(target_h),
        max_pixels: Some(MAX_PIXELS),
    };
    let DecodedMetafile { data, .. } =
        decode_metafile_as_raster_with_options(bytes, Some("image/emf"), options)
            .map_err(|_| (ERR_CORRUPT, "EMF 光栅化失败"))?
            .ok_or((ERR_CORRUPT, "EMF 头无效"))?;
    let image = image::load_from_memory(&data).map_err(|_| (ERR_CORRUPT, "EMF 光栅结果无效"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    let pixels = rgba.into_raw();
    if pixels.len() as u64 != expected {
        return Err((ERR_CORRUPT, "EMF 光栅尺寸与像素缓冲不一致"));
    }
    Ok((pixels, w, h))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 8 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let Some(payload) = payload(slice).ok() else {
            return 0;
        };
        if is_emf(&payload) {
            100
        } else {
            0
        }
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
#[allow(clippy::too_many_arguments)]
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        let payload = match payload(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if !is_emf(&payload) {
            set_last_error("不是 EMF/EMZ 字节特征");
            return ERR_UNRECOGNIZED;
        }
        if ext.as_deref() == Some("emz") && !is_gzip(slice) {
            set_last_error("EMZ 缺少 gzip 包装");
            return ERR_CORRUPT;
        }
        let (rgba, w, h) = match render_metafile(&payload, req) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
            return ERR_OK;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emfsdk::{EmfMetafile, EmfRecord, EMR_EOF, EMR_HEADER};

    fn minimal_emf() -> Vec<u8> {
        let mut data = vec![0; 100];
        data[8..12].copy_from_slice(&1i32.to_le_bytes());
        data[12..16].copy_from_slice(&1i32.to_le_bytes());
        data[32..36].copy_from_slice(&emfsdk::emf::EMF_SIGNATURE.to_le_bytes());
        let header = EmfRecord::new(EMR_HEADER, data);
        let eof = EmfRecord::new(EMR_EOF, vec![0; 12]);
        EmfMetafile {
            records: vec![header, eof],
            trailing_data: Vec::new(),
        }
        .to_bytes()
        .expect("minimal EMF")
    }

    #[test]
    fn rejects_non_emf_bytes() {
        assert_eq!(probe_impl(b"not emf".as_ptr(), 7, std::ptr::null()), 0);
    }

    #[test]
    fn claims_and_queries_minimal_emf() {
        let data = minimal_emf();
        assert_eq!(probe_impl(data.as_ptr(), data.len(), std::ptr::null()), 100);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert!(info.actual_w > 0 && info.actual_h > 0);
    }
}
