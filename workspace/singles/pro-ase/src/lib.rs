//! RuPix pro 档成员：Aseprite（`.ase/.aseprite`）。
//!
//! 逐帧服务：查询阶段报真实帧数与首帧时长，渲染阶段按 `frame_index` 取帧
//! （越界取模回绕），每帧报自己的 `duration`（ms）。合成口径固定为可见图层。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use aseprite_loader::loader::{AsepriteFile, LayerSelection};
use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-ase",
    name: "Aseprite",
    extensions: "ase;aseprite",
    description: "Aseprite（逐帧，可见图层合成）",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

fn parse(data: &[u8]) -> Result<AsepriteFile<'_>, (i32, &'static str)> {
    AsepriteFile::load(data).map_err(|_| (ERR_CORRUPT, "Aseprite 文件解析失败"))
}

/// 文件头信息：尺寸、帧数、每帧时长（ms）。
struct Meta {
    w: u32,
    h: u32,
    /// 每帧时长。`frames()` 的顺序就是 `render_frame` 的索引顺序。
    durations: Vec<u32>,
}

fn meta(data: &[u8]) -> Result<Meta, (i32, &'static str)> {
    let f = parse(data)?;
    let (w, h) = f.size();
    if w == 0 || h == 0 {
        return Err((ERR_CORRUPT, "Aseprite 头声明的宽或高为 0"));
    }
    let frames = f.frames();
    if frames.is_empty() {
        return Err((ERR_CORRUPT, "Aseprite 文件没有任何帧"));
    }
    // duration 是 u16 毫秒；0 会被宿主当"静态图"，钳到 ≥10ms 防忙转。
    let durations: Vec<u32> = frames
        .iter()
        .map(|fr| (fr.duration as u32).max(10))
        .collect();
    Ok(Meta {
        w: w as u32,
        h: h as u32,
        durations,
    })
}

/// 渲染第 `frame_index` 帧（越界取模回绕）。
fn decode_frame(data: &[u8], frame_index: u32) -> Result<(Vec<u8>, u32), (i32, &'static str)> {
    let f = parse(data)?;
    let (w, h) = f.size();
    let n = f.frames().len();
    if n == 0 {
        return Err((ERR_CORRUPT, "Aseprite 文件没有任何帧"));
    }
    let idx = (frame_index as usize) % n;
    let delay = (f.frames()[idx].duration as u32).max(10);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    f.render_frame(idx, &mut px, &LayerSelection::Visible)
        .map_err(|_| (ERR_CORRUPT, "Aseprite 渲染失败"))?;
    Ok((px, delay))
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
        let frame_index = if req.is_null() {
            0
        } else {
            unsafe { (*req).frame_index }
        };
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let m = match meta(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (_, _, bytes) = match support::check_dims(m.w, m.h, support::budget_of(req)) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let n = m.durations.len() as u32;
        if out.is_null() {
            // 查询阶段：报真实帧数 + 首帧时长（宿主据此知道要不要挂调度）。
            support::fill_info_frames(info, m.w, m.h, bytes, 1, 0, n, m.durations[0]);
            return ERR_OK;
        }
        let (rgba, delay) = match decode_frame(slice, frame_index) {
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
        // 写像素 + 填带帧语义的 info（不能用 write_rgba：它会把 frame_count 重置成 1）。
        if out_cap < rgba.len() {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, rgba.len());
        }
        support::fill_info_frames(info, m.w, m.h, bytes, 1, 0, n, delay);
        ERR_OK
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

    /// 6 帧 64×64 索引色样本（aseprite-loader 自带测试样本，MIT/Apache 双许可）：
    /// 查询报真实帧数、逐帧像素各不相同、越界取模回绕。
    const ANIM: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/multiframe_64x64_6f.aseprite"
    ));

    fn render_frame_bytes(bytes: &[u8], frame_index: u32) -> (i32, IvRenderOut, Vec<u8>) {
        let req = IvRenderReq {
            frame_index,
            ..IvRenderReq::default()
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if q != ERR_OK {
            return (q, info, Vec::new());
        }
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        (r, info, buf)
    }

    #[test]
    fn aseprite_serves_each_frame_and_reports_count() {
        // 查询：尺寸与帧数来自文件头。
        let (c, info, _) = render_frame_bytes(ANIM, 0);
        assert_eq!(c, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (64, 64));
        assert_eq!(info.frame_count, 6, "必须报出真实帧数");
        assert!(info.frame_delay_ms >= 10, "帧时长应钳到 ≥10ms");

        // 逐帧渲染：6 帧必须至少有两种不同像素（这是真动画，不是同一帧复制 6 份）。
        let mut first_px = Vec::new();
        let mut distinct = 0usize;
        let mut prev: Option<Vec<u8>> = None;
        for i in 0..6u32 {
            let (rc, fi, buf) = render_frame_bytes(ANIM, i);
            assert_eq!(rc, ERR_OK, "第 {i} 帧渲染失败");
            assert_eq!(fi.frame_count, 6, "第 {i} 帧也要报出总帧数");
            if i == 0 {
                first_px = buf[..4].to_vec();
            }
            if prev.as_ref() != Some(&buf) {
                distinct += 1;
                prev = Some(buf);
            }
        }
        assert!(
            distinct >= 2,
            "6 帧样本应至少有两种不同画面，实际 {distinct}"
        );

        // 越界回绕：第 6 帧 == 第 0 帧。
        let (rc, _, wrap) = render_frame_bytes(ANIM, 6);
        assert_eq!(rc, ERR_OK);
        assert_eq!(&wrap[..4], &first_px[..], "帧号 6 应取模回绕到帧 0");
    }
}
