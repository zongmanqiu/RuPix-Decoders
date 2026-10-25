//! RuPix pro 档成员：DICOM（`.dcm/.dicom`）。
//!
//! `dicom-pixeldata` 解首帧，`ConvertOptions` 做 VOI normalize + 8 位输出
//! （医学影像的原始窗宽窗位差异太大，直接截断会全黑/全白）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use dicom_pixeldata::{ConvertOptions, PixelDecoder, VoiLutOption};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;
use std::io::Cursor;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-dicom",
    name: "DICOM",
    extensions: "dcm;dicom",
    description: "DICOM 首帧（VOI normalize → 8 位）",
    is_vector: 0,
    has_alpha: 0,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn load(data: &[u8]) -> Result<dicom_object::DefaultDicomObject, (i32, &'static str)> {
    dicom_object::from_reader(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "DICOM 对象解析失败"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let obj = load(data).ok()?;
    let px = obj.decode_pixel_data().ok()?;
    let (w, h) = (px.columns(), px.rows());
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let obj = load(data)?;
    let px = obj
        .decode_pixel_data()
        .map_err(|_| (ERR_CORRUPT, "DICOM 像素数据解码失败"))?;
    let opts = ConvertOptions::new()
        .with_voi_lut(VoiLutOption::Normalize)
        .force_8bit();
    let img = px
        .to_dynamic_image_with_options(0, &opts)
        .map_err(|_| (ERR_CORRUPT, "DICOM 图像转换失败"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok((w, h, rgba.into_raw()))
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
        if data.is_null() || len < 132 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if &slice[128..132] == b"DICM" {
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
            set_last_error("不是合法的 DICOM 字节特征");
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
            set_last_error("DICOM 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_dicm_marker() {
        let mut v = [0u8; 132];
        v[128..132].copy_from_slice(b"DICM");
        assert_eq!(iv_probe(v.as_ptr(), v.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
