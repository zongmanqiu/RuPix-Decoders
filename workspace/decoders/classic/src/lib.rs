//! RuPix `classic` DLL assembly. Format logic lives in
//! `workspace/singles/classic` (crate `rupix-classic`); this file only wires the
//! member table and the ABI export table.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use classic_impl::iff;
use classic_impl::image_family;
use classic_impl::pict;
use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

export_decoder! {
    id: "classic",
    name: "Classic Raster Formats",
    extensions: "pcx;pcc;dcx;pnm;ppm;pgm;pbm;pam;wbmp;xbm;xpm;pict;pct;pic;ilbm;lbm;iff;sgi;iris;bw",
    description: "经典位图：PCX/DCX、PNM/PAM、WBMP/XBM/XPM、PICT、ILBM、SGI",
    is_vector: 0,
    has_alpha: 1,
}

rupix_abi::export_caps!(rupix_abi::CAP_FULL);

rupix_abi::export_metadata! {
    "version" => "0.1.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-29",
}

static MEMBERS: &[Member] = &[
    Member {
        id: "pnm",
        probe: image_family::probe_pnm,
        render: image_family::render_pnm,
    },
    Member {
        id: "pcx",
        probe: image_family::probe_pcx,
        render: image_family::render_pcx,
    },
    Member {
        id: "dcx",
        probe: image_family::probe_dcx,
        render: image_family::render_dcx,
    },
    Member {
        id: "wbmp",
        probe: image_family::probe_wbmp,
        render: image_family::render_wbmp,
    },
    Member {
        id: "xbm",
        probe: image_family::probe_xbm,
        render: image_family::render_xbm,
    },
    Member {
        id: "xpm",
        probe: image_family::probe_xpm,
        render: image_family::render_xpm,
    },
    Member {
        id: "sgi",
        probe: image_family::probe_sgi,
        render: image_family::render_sgi,
    },
    Member {
        id: "pict",
        probe: pict::probe_impl,
        render: pict::render_impl,
    },
    Member {
        id: "ilbm",
        probe: iff::probe_impl,
        render: iff::render_impl,
    },
];

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| rupix_merge_router::probe(MEMBERS, data, len, ext))
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
    guard(|| rupix_merge_router::render(MEMBERS, data, len, req, out, out_cap, info, ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_is_unique_and_known() {
        let mut ids: Vec<_> = MEMBERS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MEMBERS.len());
    }
}
