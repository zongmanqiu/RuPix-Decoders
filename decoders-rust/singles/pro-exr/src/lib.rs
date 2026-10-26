//! RuPix pro 档成员：OpenEXR（`.exr`）。
//!
//! EXR 是线性 HDR：我们取第一层、最大分辨率、RGBA 通道，最后用 sRGB 传输函数
//! 编码到 8 位（与 HDR 解码器的“clamp + gamma”口径一致；不做色调映射）。
//! 查询阶段只读元数据，渲染阶段才真解。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use exr::prelude::{read, ReadChannels, ReadLayers};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-exr",
    name: "OpenEXR",
    extensions: "exr",
    description: "OpenEXR（线性 HDR → sRGB 8 位；取第一层/最大分辨率）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

const EXR_MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let meta = exr::meta::MetaData::read_from_buffered(Cursor::new(data), false).ok()?;
    let header = meta.headers.first()?;
    let size = header.shared_attributes.display_window.size;
    let (w, h) = (size.width() as u32, size.height() as u32);
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |resolution, _channels| {
                vec![vec![[0.0f32, 0.0, 0.0, 1.0]; resolution.width()]; resolution.height()]
            },
            |pixels, position, (r, g, b, a): (f32, f32, f32, f32)| {
                pixels[position.y()][position.x()] = [r, g, b, a];
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(Cursor::new(data))
        .map_err(|_| (ERR_CORRUPT, "EXR 解码失败"))?;

    let pixels = &image.layer_data.channel_data.pixels;
    let h = pixels.len();
    let w = pixels.first().map_or(0, |row| row.len());
    if w == 0 || h == 0 {
        return Err((ERR_CORRUPT, "EXR 没有像素"));
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in pixels {
        for p in row {
            rgba.push(srgb(p[0]));
            rgba.push(srgb(p[1]));
            rgba.push(srgb(p[2]));
            rgba.push((p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    Ok((w as u32, h as u32, rgba))
}

fn srgb(v: f32) -> u8 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let c = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0 + 0.5) as u8
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
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if slice.starts_with(&EXR_MAGIC) {
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
            set_last_error("不是合法的 EXR，或没有可解的图像层");
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
            set_last_error("EXR 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_exr_magic() {
        assert_eq!(iv_probe(EXR_MAGIC.as_ptr(), 4, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }

    #[test]
    fn srgb_endpoints() {
        assert_eq!(srgb(0.0), 0);
        assert_eq!(srgb(1.0), 255);
        assert_eq!(srgb(2.0), 255);
    }
}
