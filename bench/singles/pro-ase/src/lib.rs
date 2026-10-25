//! RuPix pro 档成员：Aseprite（`.ase/.aseprite`）。
//!
//! 取第 0 帧、可见图层；动画帧选择不在本档（`iv_render` 的 `frame_index` 暂忽略）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use aseprite_loader::loader::{AsepriteFile, LayerSelection};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "pro-ase",
    name: "Aseprite",
    extensions: "ase;aseprite",
    description: "Aseprite（第 0 帧可见图层）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn parse(data: &[u8]) -> Result<AsepriteFile<'_>, (i32, &'static str)> {
    AsepriteFile::load(data).map_err(|_| (ERR_CORRUPT, "Aseprite 文件解析失败"))
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let f = parse(data).ok()?;
    let (w, h) = f.size();
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    let f = parse(data)?;
    let (w, h) = f.size();
    let mut px = vec![0u8; w as usize * h as usize * 4];
    f.render_frame(0, &mut px, &LayerSelection::Visible)
        .map_err(|_| (ERR_CORRUPT, "Aseprite 渲染失败"))?;
    Ok((w as u32, h as u32, px))
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
        if data.is_null() || len < 6 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        // ASE：前 4 字节是文件长度，随后是 magic 0xA5E0 的小端字节序 0xE0 0xA5。
        if slice[4..6] == [0xE0, 0xA5] {
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
            set_last_error("不是合法的 Aseprite 字节特征");
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
        let (_, _, rgba) = match decode(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if rgba.len() as u64 != bytes {
            set_last_error("Aseprite 头与像素尺寸不一致");
            return ERR_CORRUPT;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_ase_magic() {
        let mut v = [0u8; 8];
        v[4] = 0xE0;
        v[5] = 0xA5;
        assert_eq!(iv_probe(v.as_ptr(), v.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }
}
