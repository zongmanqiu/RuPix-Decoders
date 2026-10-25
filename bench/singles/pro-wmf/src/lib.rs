//! RuPix pro 档成员：WMF（`.wmf`）。
//!
//! `wmf-core` 把 WMF 转成 SVG，再由 `resvg` 光栅化；输出 **非预乘** RGBA8。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-wmf",
    name: "Windows Metafile (WMF)",
    extensions: "wmf",
    description: "WMF（转 SVG 后光栅化）",
    is_vector: 1,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn to_tree(data: &[u8]) -> Result<resvg::usvg::Tree, (i32, &'static str)> {
    let svg = wmf_core::converter::convert_to_svg(data)
        .map_err(|_| (ERR_CORRUPT, "WMF 解析/转换 SVG 失败"))?;
    let opt = resvg::usvg::Options::default();
    resvg::usvg::Tree::from_data(&svg, &opt).map_err(|_| (ERR_CORRUPT, "WMF 生成的 SVG 无效"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let tree = to_tree(data).ok()?;
    let s = tree.size();
    let (w, h) = (s.width().round() as u32, s.height().round() as u32);
    (w > 0 && h > 0).then_some((w, h))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let tree = to_tree(data)?;
    let s = tree.size();
    let (w, h) = (s.width().round() as u32, s.height().round() as u32);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "WMF 画布分配失败"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let src = pixmap.data();
    let mut rgba = Vec::with_capacity(src.len());
    for c in src.chunks_exact(4) {
        let a = c[3] as u16;
        if a == 0 || a == 255 {
            rgba.extend_from_slice(c);
        } else {
            // tiny-skia 是预乘；宿主 ABI 要非预乘。
            let un = |v: u8| ((v as u16 * 255 + a / 2) / a).min(255) as u8;
            rgba.extend_from_slice(&[un(c[0]), un(c[1]), un(c[2]), c[3]]);
        }
    }
    Ok((w, h, rgba))
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
        // 可放置 WMF：D7 CD C6 9A；标准 WMF：01 00 09 00 / 02 00 09 00。
        if slice.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
            || slice.starts_with(&[0x01, 0x00, 0x09, 0x00])
            || slice.starts_with(&[0x02, 0x00, 0x09, 0x00])
        {
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
            set_last_error("不是合法的 WMF 字节特征");
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
            set_last_error("WMF 画布尺寸与像素数不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_wmf_magic() {
        assert_eq!(
            iv_probe([0xD7, 0xCD, 0xC6, 0x9A].as_ptr(), 4, std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
