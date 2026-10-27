use std::ffi::c_char;
use std::sync::Arc;

use flashraw::{DecodeOptions, FlashRaw, OutputFormat};
use rupix_abi::ERR_CORRUPT;

type DecodeErr = (i32, String);
type Developed = (Arc<Vec<u8>>, u32, u32);

/// rawler 0.7.2 implements the X3F container but leaves its compressed sensor
/// path unimplemented. flashraw is the fallback only for that format.
pub(crate) fn is_x3f(ext: *const c_char) -> bool {
    if ext.is_null() {
        return false;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_bytes()
        .eq_ignore_ascii_case(b"x3f")
}

pub(crate) fn dummy(input: &[u8]) -> Result<(u32, u32), DecodeErr> {
    let raw = FlashRaw::open(input).map_err(err)?;
    let meta = raw.metadata();
    Ok((meta.width, meta.height))
}

pub(crate) fn develop(input: &[u8]) -> Result<Developed, DecodeErr> {
    let raw = FlashRaw::open(input).map_err(err)?;
    let frame = raw
        .decode(DecodeOptions {
            output: OutputFormat::Rgba8,
            ..Default::default()
        })
        .map_err(err)?;
    let rgba = frame
        .as_u8()
        .ok_or_else(|| (ERR_CORRUPT, "X3F fallback returned non-RGBA8".into()))?;
    Ok((Arc::new(rgba.to_vec()), frame.width(), frame.height()))
}

fn err(e: flashraw::Error) -> DecodeErr {
    (ERR_CORRUPT, format!("X3F fallback: {e}"))
}
