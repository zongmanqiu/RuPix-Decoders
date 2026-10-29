//! RuPix `texture` DLL assembly. Format logic lives in
//! `workspace/singles/texture` (`rupix-texture-impl`); this file only wires the
//! member table and ABI exports.

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;
use texture_impl as impls;
use texture_ktx2 as ktx_impl;

export_decoder! {
    id: "texture",
    name: "GPU & Game Textures",
    extensions: "dds;ktx;ktx2;astc;pvr;basis;blp;vtf",
    description: "GPU/游戏纹理：DDS/KTX/KTX2/ASTC/PVR/Basis/BLP/VTF",
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
        id: "dds",
        probe: impls::probe_dds,
        render: impls::render_dds,
    },
    Member {
        id: "ktx2",
        probe: ktx_impl::probe_impl,
        render: ktx_impl::render_impl,
    },
    Member {
        id: "astc",
        probe: impls::probe_astc,
        render: impls::render_astc,
    },
    Member {
        id: "pvr",
        probe: impls::probe_pvr,
        render: impls::render_pvr,
    },
    Member {
        id: "basis",
        probe: impls::probe_basis,
        render: impls::render_basis,
    },
    Member {
        id: "blp",
        probe: impls::probe_blp,
        render: impls::render_blp,
    },
    Member {
        id: "vtf",
        probe: impls::probe_vtf,
        render: impls::render_vtf,
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
    fn members_are_unique() {
        let mut ids: Vec<_> = MEMBERS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MEMBERS.len());
    }
}
