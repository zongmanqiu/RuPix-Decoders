#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;

const BIT_DEPTH_8: u32 = 8;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const JNG_SIGNATURE: [u8; 8] = [0x8b, b'J', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[cfg(feature = "standalone")]
export_decoder! {
    id: "jng",
    name: "JPEG Network Graphics",
    extensions: "jng",
    description: "JNG：JPEG 图像 + PNG/JNG alpha",
    is_vector: 0,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

#[cfg(feature = "standalone")]
rupix_abi::export_metadata! {
    "version" => "0.1.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-28",
}

#[derive(Clone, Copy)]
struct Header {
    width: u32,
    height: u32,
    color_type: u8,
    sample_depth: u8,
    alpha_depth: u8,
    alpha_compression: u8,
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_header(data: &[u8]) -> Result<Header, i32> {
    if data.len() < 16 {
        return Err(ERR_CORRUPT);
    }
    let width = be32(data, 0).unwrap_or(0);
    let height = be32(data, 4).unwrap_or(0);
    if width == 0 || height == 0 {
        return Err(ERR_CORRUPT);
    }
    let header = Header {
        width,
        height,
        color_type: data[8],
        sample_depth: data[9],
        alpha_depth: data[12],
        alpha_compression: data[13],
    };
    if !matches!(header.color_type, 8 | 10 | 12 | 14)
        || !matches!(header.sample_depth, 8 | 12 | 20)
        || !matches!(header.alpha_compression, 0 | 8)
    {
        return Err(ERR_UNSUPPORTED);
    }
    if header.sample_depth != 8 {
        return Err(ERR_UNSUPPORTED);
    }
    if header.color_type >= 12 && header.alpha_depth == 0 {
        return Err(ERR_CORRUPT);
    }
    if header.color_type < 12 && header.alpha_depth != 0 {
        return Err(ERR_CORRUPT);
    }
    Ok(header)
}

fn decode_jpeg(data: &[u8]) -> Result<(u32, u32, Vec<u8>), i32> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    use zune_jpeg::JpegDecoder;

    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    if decoder.decode_headers().is_err() {
        return Err(ERR_CORRUPT);
    }
    let info = decoder.info().ok_or(ERR_CORRUPT)?;
    let width = info.width as u32;
    let height = info.height as u32;
    if width == 0 || height == 0 {
        return Err(ERR_CORRUPT);
    }
    let pixels = decoder.decode().map_err(|_| ERR_CORRUPT)?;
    let expected = width as u64 * height as u64 * 4;
    if pixels.len() as u64 != expected {
        return Err(ERR_CORRUPT);
    }
    Ok((width, height, pixels))
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32fast::hash(&crc_input).to_be_bytes());
}

fn decode_alpha_png(idat: &[u8], width: u32, height: u32, bit_depth: u8) -> Result<Vec<u8>, i32> {
    if !matches!(bit_depth, 1 | 2 | 4 | 8 | 16) {
        return Err(ERR_UNSUPPORTED);
    }
    let mut png = Vec::new();
    png.extend_from_slice(&PNG_SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[bit_depth, 0, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", idat);
    png_chunk(&mut png, b"IEND", &[]);

    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| ERR_CORRUPT)?;
    let buffer_size = reader.output_buffer_size().ok_or(ERR_CORRUPT)?;
    let mut buffer = vec![0u8; buffer_size];
    let frame = reader.next_frame(&mut buffer).map_err(|_| ERR_CORRUPT)?;
    let expected = width as usize * height as usize;
    if frame.buffer_size() != expected {
        return Err(ERR_CORRUPT);
    }
    Ok(buffer[..expected].to_vec())
}

fn decode_alpha_jpeg(data: &[u8]) -> Result<(u32, u32, Vec<u8>), i32> {
    let (width, height, rgba) = decode_jpeg(data)?;
    let count = width as usize * height as usize;
    let mut alpha = Vec::with_capacity(count);
    for pixel in rgba.chunks_exact(4) {
        alpha.push(pixel[0]);
    }
    Ok((width, height, alpha))
}

/// parse_chunks 的产物：头 + JDAT + IDAT + JDAA 四段原始数据。
type Chunks = (Header, Vec<u8>, Vec<u8>, Vec<u8>);

fn parse_chunks(data: &[u8]) -> Result<Chunks, i32> {
    if data.len() < 8 || data[..8] != JNG_SIGNATURE {
        return Err(ERR_UNRECOGNIZED);
    }
    let mut offset = 8usize;
    let mut header = None;
    let mut jdat = Vec::new();
    let mut idat = Vec::new();
    let mut jdaa = Vec::new();
    while offset < data.len() {
        let length = be32(data, offset).ok_or(ERR_CORRUPT)? as usize;
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .ok_or(ERR_CORRUPT)?;
        if end > data.len() {
            return Err(ERR_CORRUPT);
        }
        let kind = &data[offset + 4..offset + 8];
        let body = &data[offset + 8..offset + 8 + length];
        match kind {
            b"JHDR" => header = Some(parse_header(body)?),
            b"JDAT" => jdat.extend_from_slice(body),
            b"IDAT" => idat.extend_from_slice(body),
            b"JDAA" => jdaa.extend_from_slice(body),
            b"JSEP" => break,
            b"IEND" => break,
            _ => {}
        }
        offset = end;
    }
    let header = header.ok_or(ERR_CORRUPT)?;
    if jdat.is_empty() {
        return Err(ERR_CORRUPT);
    }
    Ok((header, jdat, idat, jdaa))
}

fn decode_alpha(header: Header, idat: &[u8], jdaa: &[u8]) -> Result<Vec<u8>, i32> {
    if header.color_type < 12 {
        return Ok(Vec::new());
    }
    let alpha = match header.alpha_compression {
        0 => {
            if idat.is_empty() {
                return Err(ERR_CORRUPT);
            }
            decode_alpha_png(idat, header.width, header.height, header.alpha_depth)?
        }
        8 => {
            if jdaa.is_empty() {
                return Err(ERR_CORRUPT);
            }
            let (width, height, alpha) = decode_alpha_jpeg(jdaa)?;
            if width != header.width || height != header.height {
                return Err(ERR_CORRUPT);
            }
            alpha
        }
        _ => return Err(ERR_UNSUPPORTED),
    };
    if alpha.len() != header.width as usize * header.height as usize {
        return Err(ERR_CORRUPT);
    }
    Ok(alpha)
}

fn fill_info(info: *mut IvRenderOut, width: u32, height: u32, bytes_needed: u64) {
    let out = unsafe { &mut *info };
    out.actual_w = width;
    out.actual_h = height;
    out.bytes_needed = bytes_needed;
    out.pixel_format = PF_RGBA8;
    out.bit_depth = BIT_DEPTH_8;
    out.orientation = 1;
    out.already_oriented = 1;
    out.frame_count = 1;
    out.frame_delay_ms = 0;
    out.page_count = 1;
    out.icc_len = 0;
}

fn render(
    data: &[u8],
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
) -> i32 {
    if info.is_null() {
        return ERR_BUFFER_TOO_SMALL;
    }
    let (header, jdat, idat, jdaa) = match parse_chunks(data) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let npixels = header.width as u64 * header.height as u64;
    if npixels > MAX_PIXELS {
        return ERR_TOO_LARGE;
    }
    let bytes_needed = npixels * 4;
    let budget = if req.is_null() {
        0
    } else {
        unsafe { (*req).budget_bytes }
    };
    if budget != 0 && bytes_needed > budget as u64 {
        return ERR_TOO_LARGE;
    }
    if out.is_null() {
        fill_info(info, header.width, header.height, bytes_needed);
        return ERR_OK;
    }
    if (out_cap as u64) < bytes_needed {
        return ERR_BUFFER_TOO_SMALL;
    }
    let (width, height, mut rgba) = match decode_jpeg(&jdat) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if width != header.width || height != header.height {
        return ERR_CORRUPT;
    }
    let alpha = match decode_alpha(header, &idat, &jdaa) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if !alpha.is_empty() {
        for (pixel, a) in rgba.chunks_exact_mut(4).zip(alpha) {
            pixel[3] = a;
        }
    }
    unsafe {
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
    }
    fill_info(info, header.width, header.height, bytes_needed);
    ERR_OK
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    if data.is_null() || len < 8 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    if bytes[..8] == JNG_SIGNATURE {
        100
    } else {
        0
    }
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
        if data.is_null() || len == 0 {
            set_last_error("empty JNG input");
            return ERR_UNRECOGNIZED;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        render(bytes, req, out, out_cap, info)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/jng_64x48_alpha.jng"
    ));

    fn req() -> IvRenderReq {
        IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        }
    }

    #[test]
    fn probes_jng_signature_only() {
        assert_eq!(probe_impl(JNG.as_ptr(), JNG.len(), std::ptr::null()), 100);
        assert_eq!(probe_impl(b"not a jng".as_ptr(), 9, std::ptr::null()), 0);
    }

    #[test]
    fn renders_jpeg_with_png_alpha() {
        let mut info = IvRenderOut::default();
        let request = req();
        let code = render_impl(
            JNG.as_ptr(),
            JNG.len(),
            &request,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (64, 48));
        let mut pixels = vec![0u8; info.bytes_needed as usize];
        let code = render_impl(
            JNG.as_ptr(),
            JNG.len(),
            &request,
            pixels.as_mut_ptr(),
            pixels.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(code, ERR_OK);
        assert_eq!(pixels.len(), 64 * 48 * 4);
        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[(64 * 48 - 1) * 4 + 3], 255);
    }

    #[test]
    fn rejects_truncated_chunks() {
        let mut broken = JNG.to_vec();
        broken.truncate(broken.len() - 10);
        let mut info = IvRenderOut::default();
        let code = render_impl(
            broken.as_ptr(),
            broken.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_ne!(code, ERR_OK);
    }
}
