//! RuPix pro 档成员：PCX / WBMP / XBM / ICNS。
//!
//! 这四个都是“库很小、格式很老/很专”的格式；分成一个成员只为减少 DLL 底座，
//! 不是把它们当成一个格式。探针按魔数/文本头识别，渲染时分派到对应库。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-small",
    name: "Professional Small Formats (PCX/WBMP/XBM/ICNS)",
    extensions: "pcx;pcc;dcx;wbmp;xbm;icns",
    description: "专业档小格式：PCX / WBMP / XBM / ICNS",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pcx,
    Dcx,
    Wbmp,
    Xbm,
    Icns,
}

fn dcx_first_pcx(data: &[u8]) -> Option<&[u8]> {
    if data.get(0..4)? != [0xB1, 0x68, 0xDE, 0x3A] {
        return None;
    }
    let off = u32::from_le_bytes(data.get(4..8)?.try_into().ok()?) as usize;
    let page = data.get(off..)?;
    (page.len() >= 128 && page.first() == Some(&0x0A)).then_some(page)
}

fn detect(data: &[u8], ext: Option<&str>) -> Option<Kind> {
    if data.starts_with(b"icns") {
        return Some(Kind::Icns);
    }
    if dcx_first_pcx(data).is_some() {
        return Some(Kind::Dcx);
    }
    if data.first() == Some(&0x0A) && data.len() >= 128 {
        return Some(Kind::Pcx);
    }
    // XBM 是 ASCII 文本，但其余格式完全可能是非 UTF-8 二进制；这里若用 `?`
    // 提前返回，后面的 WBMP 分支永远走不到（WBMP 头恰好以小端 0 开头）。
    if let Ok(head) = std::str::from_utf8(&data[..data.len().min(256)]) {
        if head.trim_start().starts_with("#define") {
            return Some(Kind::Xbm);
        }
    }
    // WBMP 头很弱（type=0, fix=0）；ext 点名或“头 + 像素长度正好等于文件长度”才认。
    let ext_wbmp = ext.is_some_and(|e| e.eq_ignore_ascii_case("wbmp"));
    if data.len() >= 4 && data[0] == 0 && data[1] == 0 {
        if let Ok(d) = wbmp::Decoder::new(Cursor::new(data)) {
            let (w, h) = d.dimensions();
            let row = w.div_ceil(8);
            if ext_wbmp || (w > 0 && h > 0 && 4 + row as usize * h as usize == data.len()) {
                return Some(Kind::Wbmp);
            }
        }
    }
    None
}

fn decode(data: &[u8], kind: Kind) -> Result<(u32, u32, Vec<u8>), (i32, &'static str)> {
    match kind {
        Kind::Pcx => {
            let mut r = pcx::Reader::new(Cursor::new(data))
                .map_err(|_| (ERR_CORRUPT, "PCX 文件头解析失败"))?;
            let (w, h) = (r.width() as u32, r.height() as u32);
            if w == 0 || h == 0 {
                return Err((ERR_CORRUPT, "PCX 声明的宽或高为 0"));
            }
            let mut rgb = vec![0u8; w as usize * h as usize * 3];
            r.read_rgb_pixels(&mut rgb)
                .map_err(|_| (ERR_CORRUPT, "PCX 像素数据损坏"))?;
            Ok((w, h, support::rgba_from_rgb(&rgb)))
        }
        Kind::Dcx => {
            let page = dcx_first_pcx(data).ok_or((ERR_CORRUPT, "DCX 里没有有效 PCX 页"))?;
            decode(page, Kind::Pcx)
        }
        Kind::Wbmp => {
            let mut d = wbmp::Decoder::new(Cursor::new(data))
                .map_err(|_| (ERR_CORRUPT, "WBMP 文件头解析失败"))?;
            let (w, h) = d.dimensions();
            if w == 0 || h == 0 {
                return Err((ERR_CORRUPT, "WBMP 声明的宽或高为 0"));
            }
            let mut mono = vec![0u8; w as usize * h as usize];
            d.read_image_data(&mut mono)
                .map_err(|_| (ERR_CORRUPT, "WBMP 像素数据损坏"))?;
            let mut rgba = Vec::with_capacity(mono.len() * 4);
            for v in mono {
                let g = if v == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            Ok((w, h, rgba))
        }
        Kind::Xbm => {
            let d = xbm::Decoder::new(Cursor::new(data))
                .map_err(|_| (ERR_CORRUPT, "XBM 文件头解析失败"))?;
            let (w, h) = (d.width(), d.height());
            if w == 0 || h == 0 {
                return Err((ERR_CORRUPT, "XBM 声明的宽或高为 0"));
            }
            let mono = d
                .decode_to_vec()
                .map_err(|_| (ERR_CORRUPT, "XBM 像素数据损坏"))?;
            let mut rgba = Vec::with_capacity(mono.len() * 4);
            for v in mono {
                let g = if v == 0 { 255 } else { 0 };
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            Ok((w, h, rgba))
        }
        Kind::Icns => {
            let family = icns::IconFamily::read(Cursor::new(data))
                .map_err(|_| (ERR_CORRUPT, "ICNS 容器解析失败"))?;
            let best = family
                .available_icons()
                .into_iter()
                .max_by_key(|t| t.pixel_width() as u64 * t.pixel_height() as u64)
                .ok_or((ERR_CORRUPT, "ICNS 里没有可用图标"))?;
            let img = family
                .get_icon_with_type(best)
                .map_err(|_| (ERR_CORRUPT, "ICNS 图标解码失败"))?;
            let rgba = img.convert_to(icns::PixelFormat::RGBA);
            let (w, h) = (rgba.width(), rgba.height());
            Ok((w, h, rgba.into_data().into_vec()))
        }
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
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = unsafe { rupix_abi::ext_str(ext) };
        if detect(slice, ext).is_some() {
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
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = unsafe { rupix_abi::ext_str(ext) };
        let Some(kind) = detect(slice, ext) else {
            set_last_error("不是本解码器能解的小格式字节特征");
            return ERR_UNRECOGNIZED;
        };
        let (w, h, rgba) = match decode(slice, kind) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let (_, _, bytes) = match support::check_dims(w, h, support::budget_of(req)) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if rgba.len() as u64 != bytes {
            set_last_error("像素数与文件头不符");
            return ERR_CORRUPT;
        }
        if out.is_null() {
            support::fill_info(info, w, h, bytes, 1, 0);
            return ERR_OK;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_only_known_magics() {
        assert_eq!(
            probe_impl([b'i', b'c', b'n', b's', 0, 0].as_ptr(), 6, std::ptr::null()),
            100
        );
        assert_eq!(probe_impl(b"nope".as_ptr(), 4, std::ptr::null()), 0);
        assert_eq!(probe_impl(std::ptr::null(), 0, std::ptr::null()), 0);
    }

    #[test]
    fn wbmp_binary_is_recognized() {
        let wbmp = [0u8, 0, 8, 2, 0xAA, 0x55];
        assert_eq!(probe_impl(wbmp.as_ptr(), wbmp.len(), std::ptr::null()), 100);
    }

    #[test]
    fn xbm_text_is_recognized() {
        let xbm = b"#define x_width 8\n#define x_height 2\nstatic unsigned char x_bits[] = { 0x00, 0xff };\n";
        assert_eq!(probe_impl(xbm.as_ptr(), xbm.len(), std::ptr::null()), 100);
    }

    #[test]
    fn dcx_first_page_is_decoded() {
        let mut pcx = Vec::new();
        {
            let mut w = pcx::WriterRgb::new(&mut pcx, (1, 1), (72, 72)).expect("pcx writer");
            w.write_row(&[255, 0, 0]).expect("pcx row");
            w.finish().expect("pcx finish");
        }
        let off = 4100usize;
        let mut dcx = vec![0u8; off];
        dcx[0..4].copy_from_slice(&[0xB1, 0x68, 0xDE, 0x3A]);
        dcx[4..8].copy_from_slice(&(off as u32).to_le_bytes());
        dcx.extend_from_slice(&pcx);
        assert_eq!(probe_impl(dcx.as_ptr(), dcx.len(), std::ptr::null()), 100);
        let (w, h, rgba) = decode(&dcx, Kind::Dcx).expect("dcx");
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba, [255, 0, 0, 255]);
    }
}
