//! RuPix `doc-legacy` tier: CMX/CCX, WPG, ZMF, ZBR only.
//!
//! Kept separate from `doc` so the large C++ static dependency
//! (`libcdr`/`libwpg`/`libzmf` + `librevenge` + ICU) is an optional download.
//! The implementation lives in `workspace/singles/doc-legacy`; this crate only
//! owns the ABI exports and routing.

use rupix_abi::*;
use rupix_merge_router::Member;

export_decoder! {
    id: "doc-legacy",
    name: "Legacy Document Graphics",
    extensions: "cmx;ccx;wpg;zmf;zbr",
    description: "旧版文档矢量：CMX/CCX、WPG、ZMF、ZBR",
    is_vector: 0,
    has_alpha: 1,
}

rupix_abi::export_caps!(rupix_abi::CAP_FULL);
rupix_abi::export_metadata! {
    "version" => "0.1.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-29",
    "vector_exts" => "cmx;ccx;wpg;zmf;zbr",
}

static MEMBERS: &[Member] = &[Member {
    id: "legacy",
    probe: doc_legacy_impl::probe_impl,
    render: doc_legacy_impl::render_impl,
}];

#[no_mangle]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const std::ffi::c_char) -> i32 {
    guard(|| rupix_merge_router::probe(MEMBERS, data, len, ext))
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const std::ffi::c_char,
) -> i32 {
    guard(|| rupix_merge_router::render(MEMBERS, data, len, req, out, out_cap, info, ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_extension() {
        assert_eq!(iv_probe(b"not an image".as_ptr(), 12, c"pdf".as_ptr()), 0);
    }
}
