//! PICT (Apple QuickDraw picture), decoded through the standalone
//! `oxideav-pict` image-crate API.

use rupix_abi::guard;
use rupix_abi::*;
use std::ffi::c_char;

use super::{input, render_scan};

fn options(req: *const IvRenderReq) -> oxideav_pict::DecodeOptions {
    let budget = pro_support::budget_of(req) as u64;
    let max_bytes = if budget == 0 {
        pro_support::MAX_PIXELS * 4
    } else {
        budget
    };
    oxideav_pict::DecodeOptions::new()
        .with_max_pixels(pro_support::MAX_PIXELS)
        .with_max_bytes(max_bytes)
}

fn decode(
    data: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let img = oxideav_pict::decode_with(data, &options(req))
        .map_err(|_| (ERR_CORRUPT, "PICT 光栅解析失败"))?;
    let (w, h) = (img.width(), img.height());
    pro_support::check_dims(w, h, pro_support::budget_of(req))?;
    Ok((img.to_rgba8(), w, h))
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| match input(data, len) {
        Some(bytes) if oxideav_pict::probe(bytes) => 100,
        _ => 0,
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(clippy::too_many_arguments)]
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_scan(data, len, req, out, out_cap, info, decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_input_is_not_pict() {
        assert_eq!(probe_impl(b"PICT".as_ptr(), 4, std::ptr::null()), 0);
    }

    #[test]
    fn generated_pict_round_trips() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255];
        let bytes =
            oxideav_pict::encode_rgba8(2, 1, &rgba, &oxideav_pict::EncodeOptions::default())
                .unwrap();
        assert_eq!(
            probe_impl(bytes.as_ptr(), bytes.len(), std::ptr::null()),
            100
        );
        let (px, w, h) = decode(&bytes, std::ptr::null()).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(px, rgba);
    }
}
