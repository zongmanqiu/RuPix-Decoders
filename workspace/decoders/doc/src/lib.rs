//! RuPix `doc` tier: one DLL for document, CAD, metafile, legacy vector and
//! DjVu members. The legacy C++ libraries are behind a static C ABI shim.

use rupix_abi::*;
use rupix_merge_router::{Member, RegionMember};

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

rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_REGION);
rupix_abi::export_metadata! {
    "version" => "0.2.2",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-11-01",
    "vector_exts" => "dwg;dwt;dxf;wmf;wmz;emf;emz;pdf;\
ps;ps1;ps2;ps3;prn;eps;ai;xps;oxps;\
hp;hpg;plo;hg;hgl;plt;hp1;hp2;hpgl;hpgl2;gl2",
    // 能按区域重渲的成员级子集：EMF/EMZ 的 `emfsdk` 只有目标宽高、没有视口 API，
    // 所以它们**不**在这条通道里（宿主对它们走整幅路线）。
    "region_exts" => "dwg;dwt;dxf;wmf;wmz;pdf;\
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

/// 区域通道成员表（ABI v6）：只放**真能按区域重画**的成员。
///
/// EMF/EMZ 的 `emfsdk` 只接受目标宽高、没有视口/变换参数，XPS/OXPS 当前还只
/// 出空白页——把它们放进来就等于把"整篇缩到那一小块"的内容错误图交给宿主。
static REGION_MEMBERS: &[RegionMember] = &[
    RegionMember {
        id: "cad",
        region: doc_cad::export_region_impl,
    },
    RegionMember {
        id: "meta",
        region: pro_wmf::export_region_impl,
    },
    RegionMember {
        id: "hpgl",
        region: doc_hpgl::export_region_impl,
    },
    RegionMember {
        id: "pdf",
        region: doc_pdf::export_region_impl,
    },
    RegionMember {
        id: "print",
        region: doc_print::export_region_impl,
    },
    RegionMember {
        id: "xps",
        region: doc_xps::export_region_impl,
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

fn region_impl(
    data: *const u8,
    len: usize,
    req: *const IvRegionReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const std::ffi::c_char,
) -> i32 {
    rupix_merge_router::render_region(REGION_MEMBERS, data, len, req, out, out_cap, info, ext)
}

rupix_abi::export_region!(region_impl);

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

    /// 区域通道在**装配层**接通：DXF 经合并路由落到 cad 成员，画布跟目标走。
    #[test]
    fn region_route_reaches_the_cad_member() {
        let dxf = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n100\n21\n100\n0\nENDSEC\n0\nEOF\n";
        let req = IvRegionReq {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
            target_w: 40,
            target_h: 40,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render_region(
                dxf.as_ptr(),
                dxf.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert_eq!((info.actual_w, info.actual_h), (40, 40));
    }

    /// EMF/EMZ 不在这条通道里：请求会如实失败（宿主不会对它发区域请求）。
    #[test]
    fn region_route_rejects_members_without_a_viewport() {
        let mut info = IvRenderOut::default();
        let req = IvRegionReq {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            target_w: 10,
            target_h: 10,
            ..Default::default()
        };
        let code = iv_render_region(
            b"nope".as_ptr(),
            4,
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_ne!(code, ERR_OK);
    }
}
