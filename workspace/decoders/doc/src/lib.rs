//! RuPix `doc` tier: one DLL for document, CAD, metafile, legacy vector and
//! DjVu members. The legacy C++ libraries are behind a static C ABI shim.

use rupix_abi::*;
use rupix_merge_router::Member;

export_decoder! {
    id: "doc",
    name: "Document & CAD Formats",
    extensions: "dwg;dwt;dxf;wmf;wmz;djvu;djv;emf;emz;pdf;\
ps;ps1;ps2;ps3;prn;eps;ai;xps;oxps;\
hp;hpg;plo;hg;hgl;plt;hp1;hp2;hpgl;hpgl2;gl2",
    description: "常见文档/CAD/元文件/DjVu/PDF/PostScript/XPS/HPGL",
    is_vector: 0,
    has_alpha: 1,
}

rupix_abi::export_caps!(rupix_abi::CAP_FULL);
rupix_abi::export_metadata! {
    "version" => "0.1.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-29",
    "vector_exts" => "dwg;dwt;dxf;wmf;wmz;emf;emz;pdf;\
ps;ps1;ps2;ps3;prn;eps;ai;xps;oxps;\
hp;hpg;plo;hg;hgl;plt;hp1;hp2;hpgl;hpgl2;gl2",
}

static MEMBERS: &[Member] = &[
    Member {
        id: "cad",
        probe: doc_cad::probe_impl,
        render: doc_cad::render_impl,
    },
    Member {
        id: "meta",
        probe: pro_wmf::probe_impl,
        render: pro_wmf::render_impl,
    },
    Member {
        id: "djvu",
        probe: pro_djvu::probe_impl,
        render: pro_djvu::render_impl,
    },
    Member {
        id: "emf",
        probe: doc_emf::probe_impl,
        render: doc_emf::render_impl,
    },
    Member {
        id: "hpgl",
        probe: doc_hpgl::probe_impl,
        render: doc_hpgl::render_impl,
    },
    Member {
        id: "pdf",
        probe: doc_pdf::probe_impl,
        render: doc_pdf::render_impl,
    },
    Member {
        id: "print",
        probe: doc_print::probe_impl,
        render: doc_print::render_impl,
    },
    Member {
        id: "xps",
        probe: doc_xps::probe_impl,
        render: doc_xps::render_impl,
    },
];

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
    fn doc_claims_dwg_and_dxf() {
        let dxf = b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(iv_probe(dxf.as_ptr(), dxf.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"AC1027".as_ptr(), 6, std::ptr::null()), 100);
        assert_eq!(iv_probe(b"not cad".as_ptr(), 7, std::ptr::null()), 0);
    }

    #[test]
    fn doc_claims_metafile_and_djvu() {
        assert_eq!(
            iv_probe(b"\xD7\xCD\xC6\x9A".as_ptr(), 4, std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"AT&TFORM".as_ptr(), 8, std::ptr::null()), 100);
        let hpgl = b"IN;SP1;PU0,0;PD100,0;";
        assert_eq!(iv_probe(hpgl.as_ptr(), hpgl.len(), c"plt".as_ptr()), 100);
        assert_eq!(iv_probe(b"%PDF-1.7".as_ptr(), 8, std::ptr::null()), 100);
        let ps = b"%!PS\n100 100 moveto 200 200 lineto stroke showpage\n";
        assert_eq!(iv_probe(ps.as_ptr(), ps.len(), c"ps".as_ptr()), 100);
        assert_eq!(iv_probe(b"PK\x03\x04".as_ptr(), 4, c"xps".as_ptr()), 100);
    }
}
