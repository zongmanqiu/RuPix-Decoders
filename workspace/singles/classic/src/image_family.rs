//! Rust-native classic formats: PNM/PAM, PCX/PCC, DCX, WBMP, XBM, XPM, SGI.

use image::ImageDecoder;
use image_extras::{pcx, sgi, wbmp, xbm, xpm};
use rupix_abi::guard;
use rupix_abi::*;
use std::ffi::c_char;
use std::io::Cursor;

use super::{decode_dynamic, input, render_scan};

fn ext_is(ext: *const c_char, names: &[&str]) -> bool {
    let Some(ext) = (unsafe { rupix_abi::ext_str(ext) }) else {
        return false;
    };
    names.iter().any(|n| ext.eq_ignore_ascii_case(n))
}

fn probe_bytes(data: *const u8, len: usize, f: impl FnOnce(&[u8]) -> bool) -> i32 {
    guard(|| match input(data, len) {
        Some(bytes) if f(bytes) => 100,
        _ => 0,
    })
}

pub fn probe_pnm(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        b.len() >= 2
            && b[0] == b'P'
            && matches!(b[1], b'1'..=b'7')
            && (b.len() < 3 || b[2].is_ascii_whitespace() || b[2] == b'#')
    })
}

pub fn probe_pcx(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        // PCX header: byte 0 = 0x0A, byte 1 = version (0..=5), byte 2 = encoding
        // (1 = RLE), byte 3 = bits per pixel. The old test read encoding/bpp one
        // byte late, so only ext-based claims ever matched a real PCX.
        b.len() >= 128 && b[0] == 0x0A && (ext_is(ext, &["pcx", "pcc"]) || (b[1] <= 5 && b[2] == 1))
    })
}

fn dcx_page(data: &[u8]) -> Option<&[u8]> {
    if !data.starts_with(&[0xB1, 0x68, 0xDE, 0x3A]) {
        return None;
    }
    let off = u32::from_le_bytes(data.get(4..8)?.try_into().ok()?) as usize;
    let page = data.get(off..)?;
    (page.len() >= 128 && page.first() == Some(&0x0A)).then_some(page)
}

pub fn probe_dcx(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| dcx_page(b).is_some())
}

pub fn probe_wbmp(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        if b.len() < 4 || b[0] != 0 || b[1] != 0 {
            return false;
        }
        if ext_is(ext, &["wbmp"]) {
            return true;
        }
        let Ok(d) = wbmp::WbmpDecoder::new(Cursor::new(b)) else {
            return false;
        };
        let (w, h) = d.dimensions();
        let row = w.div_ceil(8) as usize;
        w > 0 && h > 0 && 4 + row * h as usize == b.len()
    })
}

pub fn probe_xbm(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        let head = &b[..b.len().min(512)];
        std::str::from_utf8(head).is_ok_and(|t| {
            let t = t.trim_start();
            t.starts_with("#define") && t.contains("_width") && t.contains("_height")
        })
    })
}

pub fn probe_xpm(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        let head = &b[..b.len().min(512)];
        std::str::from_utf8(head).is_ok_and(|t| {
            let t = t.trim_start();
            t.starts_with("/* XPM */") || t.starts_with("! XPM2") || t.starts_with("static char")
        })
    })
}

pub fn probe_sgi(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_bytes(data, len, |b| {
        b.len() >= 512 && b.starts_with(&[0x01, 0xDA])
    })
}

fn pnm_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| (ERR_CORRUPT, "PNM/PAM header not recognized"))?;
    let decoder = reader
        .into_decoder()
        .map_err(|_| (ERR_CORRUPT, "PNM/PAM decoder init failed"))?;
    decode_dynamic(decoder, req)
}

fn pcx_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder =
        pcx::PCXDecoder::new(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "PCX header failed"))?;
    decode_dynamic(decoder, req)
}

fn dcx_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let page = dcx_page(data).ok_or((ERR_CORRUPT, "DCX has no valid PCX page"))?;
    pcx_decode(page, req)
}

fn wbmp_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder = wbmp::WbmpDecoder::new(Cursor::new(data))
        .map_err(|_| (ERR_CORRUPT, "WBMP header failed"))?;
    decode_dynamic(decoder, req)
}

fn xbm_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder =
        xbm::XbmDecoder::new(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "XBM header failed"))?;
    decode_dynamic(decoder, req)
}

fn xpm_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder =
        xpm::XpmDecoder::new(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "XPM header failed"))?;
    decode_dynamic(decoder, req)
}

fn sgi_decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let decoder =
        sgi::SgiDecoder::new(Cursor::new(data)).map_err(|_| (ERR_CORRUPT, "SGI header failed"))?;
    decode_dynamic(decoder, req)
}

macro_rules! render_fn {
    ($name:ident, $decode:ident) => {
        #[allow(clippy::too_many_arguments)]
        pub fn $name(
            data: *const u8,
            len: usize,
            req: *const IvRenderReq,
            out: *mut u8,
            out_cap: usize,
            info: *mut IvRenderOut,
            _ext: *const c_char,
        ) -> i32 {
            render_scan(data, len, req, out, out_cap, info, $decode)
        }
    };
}

render_fn!(render_pnm, pnm_decode);
render_fn!(render_pcx, pcx_decode);
render_fn!(render_dcx, dcx_decode);
render_fn!(render_wbmp, wbmp_decode);
render_fn!(render_xbm, xbm_decode);
render_fn!(render_xpm, xpm_decode);
render_fn!(render_sgi, sgi_decode);

#[cfg(test)]
mod tests {
    use super::*;

    const PNM: &[u8] = b"P6\n1 1\n255\n\xff\x00\x00";

    #[test]
    fn pnm_magic_is_claimed_without_an_extension() {
        assert_eq!(probe_pnm(PNM.as_ptr(), PNM.len(), std::ptr::null()), 100);
        assert_eq!(
            probe_pnm(b"P8\n1 1\n255\n".as_ptr(), 11, std::ptr::null()),
            0
        );
    }

    #[test]
    fn pnm_decodes_rgb_to_rgba() {
        let (px, w, h) = pnm_decode(PNM, std::ptr::null()).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(px, [255, 0, 0, 255]);
    }

    #[test]
    fn dcx_without_a_page_is_not_claimed() {
        let bad = [0xB1, 0x68, 0xDE, 0x3A, 8, 0, 0, 0];
        assert_eq!(probe_dcx(bad.as_ptr(), bad.len(), std::ptr::null()), 0);
    }

    #[test]
    fn xpm_text_decodes() {
        let xpm = b"/* XPM */\nstatic char *x[] = {\n\"2 1 2 1\",\n\"a c #FF0000\",\n\"b c #00FF00\",\n\"ab\"};\n";
        assert_eq!(probe_xpm(xpm.as_ptr(), xpm.len(), std::ptr::null()), 100);
        let (px, w, h) = xpm_decode(xpm, std::ptr::null()).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(px, [255, 0, 0, 255, 0, 255, 0, 255]);
    }
}
