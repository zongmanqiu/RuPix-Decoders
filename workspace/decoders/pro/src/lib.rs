//! RuPix 发布档 `pro`：专业/低频格式（JPEG2000 / JPEG XR / EXR / KTX2 /
//! DICOM / 科学影像 / 设计源文件 / 矢量）。
//!
//! 与 `common` 一样，本 crate **只做装配**：依赖表 + `MEMBERS` 路由表，
//! 一行解码逻辑都不写。每个成员都是 `singles/pro-*` 的单库。
//!
//! 已知缺口（没有可用纯 Rust 光栅器，暂不认领）：`pict/pct`（crate 要 nightly）、
//! `swf`（解析器无光栅化）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

export_decoder! {
    id: "pro",
    name: "Professional Formats",
    extensions: "jp2;j2k;j2c;jpc;jpx;jpf;jpt;\
jxr;wdp;hdp;exr;ktx;ktx2;dcm;dicom;\
nii;nrrd;mrc;mrcs;ccp4;map;\
pcx;pcc;dcx;icns;wbmp;xbm;xcf;wmf;ase;aseprite;djvu;djv;psd;hdr",
    description: "专业格式：医学/科学影像与设计源文件",
    is_vector: 0,
    has_alpha: 1,
}

// ★ 能力位：**不**声明 `CAP_VECTOR`。
//
// 为什么：规范 §十五曾规定"声明 `CAP_VECTOR` 的解码器必须实现 `iv_export_svg`
// （把矢量场景交成 SVG 文档给宿主渲染）"。那条路线已随 2026-10-28 的收口**整条退役**
// ——矢量格式一律由**解码器自己**按目标尺寸光栅化（宪章 §5.2），宿主不再用自己的
// resvg 渲染解码器认领的矢量。留着 `CAP_VECTOR` 会让宿主以为本档能交出矢量文档，
// 而两条渲染路径并存 = 同一格式的像素可能因机器而异。
//
// ★ `CAP_REGION`（ABI v6）：本档实现了可选导出 `iv_render_region`（转给 WMF 成员），
// 宿主据此知道「这枚 DLL 能按区域重渲」——矢量图放大到 4000% 也无损的那条通道。
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_REGION);

rupix_abi::export_metadata! {
    "version" => "0.3.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-28",
    // 成员级矢量声明（规范 §十五）：合并档的 is_vector 是 DLL 级单值（=0，
    // 位图成员占多数），哪些后缀是矢量只能走 metadata 键。新键只许追加在末尾。
    "vector_exts" => "wmf;wmz",
}

static MEMBERS: &[Member] = &[
    Member {
        id: "jp2",
        probe: pro_jp2::probe_impl,
        render: pro_jp2::render_impl,
    },
    Member {
        id: "jxr",
        probe: pro_jxr::probe_impl,
        render: pro_jxr::render_impl,
    },
    Member {
        id: "exr",
        probe: pro_exr::probe_impl,
        render: pro_exr::render_impl,
    },
    Member {
        id: "psd",
        probe: pro_psd::probe_impl,
        render: pro_psd::render_impl,
    },
    Member {
        id: "hdr",
        probe: pro_hdr::probe_impl,
        render: pro_hdr::render_impl,
    },
    Member {
        id: "ktx2",
        probe: pro_ktx2::probe_impl,
        render: pro_ktx2::render_impl,
    },
    Member {
        id: "dicom",
        probe: pro_dicom::probe_impl,
        render: pro_dicom::render_impl,
    },
    Member {
        id: "sci",
        probe: pro_sci::probe_impl,
        render: pro_sci::render_impl,
    },
    Member {
        id: "small",
        probe: pro_small::probe_impl,
        render: pro_small::render_impl,
    },
    Member {
        id: "xcf",
        probe: pro_xcf::probe_impl,
        render: pro_xcf::render_impl,
    },
    Member {
        id: "ase",
        probe: pro_ase::probe_impl,
        render: pro_ase::render_impl,
    },
    Member {
        id: "djvu",
        probe: pro_djvu::probe_impl,
        render: pro_djvu::render_impl,
    },
    Member {
        id: "wmf",
        probe: pro_wmf::probe_impl,
        render: pro_wmf::render_impl,
    },
];

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
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
    ext: *const c_char,
) -> i32 {
    guard(|| rupix_merge_router::render(MEMBERS, data, len, req, out, out_cap, info, ext))
}

/// 可选导出 `iv_render_region`（**ABI v6**，规范 §十六）：把文档的指定区域按
/// 目标画布**直接光栅化**——这是"矢量图放大到 4000% 也无损"的那条通道。
///
/// 与整幅渲染同一分层：pro 档唯一的矢量成员是 `wmf`，直接转发，
/// 不走 MEMBERS 路由（那张表只有 probe/render 两型）。区域能力成员另立一张
/// [`RegionMember`] 表——位图成员本来就不该有区域通道，让它们填空位只会
/// 让每个合并档都多写一遍废话。
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render_region(
    data: *const u8,
    len: usize,
    req: *const IvRegionReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| {
        rupix_merge_router::render_region(REGION_MEMBERS, data, len, req, out, out_cap, info, ext)
    })
}

/// 支持区域渲染的成员（ABI v6）。pro 档今天只有 `wmf` 这一枚矢量成员。
static REGION_MEMBERS: &[rupix_merge_router::RegionMember] = &[rupix_merge_router::RegionMember {
    id: "wmf",
    region: pro_wmf::export_region_impl,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_takes_a_known_magic() {
        let jp2 = b"\x00\x00\x00\x0CjP  \x0D\x0A\x87\x0A\x00\x00\x00\x00";
        assert_eq!(iv_probe(jp2.as_ptr(), jp2.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"not an image".as_ptr(), 12, std::ptr::null()), 0);
    }

    /// 成员级矢量声明（宿主 m152 对接面）：`is_vector` 是 DLL 级单值，
    /// 哪些后缀能矢量重渲只能由 metadata 键 `vector_exts` 声明；新键只许追加在
    /// `fields` 末尾（规范 §十四）。键名/值/位置任何一个漂了这条就红。
    #[test]
    fn metadata_declares_member_level_vector_exts() {
        let meta = iv_metadata();
        assert!(!meta.is_null());
        let meta = unsafe { &*meta };
        let fields = unsafe { std::slice::from_raw_parts(meta.fields, meta.field_count as usize) };
        let key = |f: &rupix_abi::IvMetadataField| unsafe {
            std::ffi::CStr::from_ptr(f.key).to_string_lossy()
        };
        let val = |f: &rupix_abi::IvMetadataField| unsafe {
            std::ffi::CStr::from_ptr(f.value).to_string_lossy()
        };
        let find = |name: &str| {
            fields
                .iter()
                .find(|f| key(f) == name)
                .map(|f| val(f).to_string())
                .unwrap_or_default()
        };
        assert_eq!(find("vector_exts"), "wmf;wmz");
        // DLL 级自述保持 0：矢量属性由上面的键声明，不改冻结的自述结构体。
        assert_eq!(unsafe { &*iv_decoder_info() }.is_vector, 0);
    }

    /// 能力位：**不**声明 `CAP_VECTOR`（本档不实现 `iv_export_svg`——那条"宿主自己
    /// 渲染矢量"的路线已退役），但**必须**声明 `CAP_REGION`（ABI v6：实现了
    /// `iv_render_region` 就要说一声）。
    ///
    /// 这条红过一次——实现先落地、能力位后补，中间隔了一轮，宿主按契约筛候选时
    /// 把本档整枚排除，WMF 一张都出不来。
    #[test]
    fn caps_declare_region_capability_and_not_vector_export() {
        assert_eq!(iv_caps(), rupix_abi::CAP_FULL | rupix_abi::CAP_REGION);
        assert_ne!(iv_caps() & rupix_abi::CAP_REGION, 0);
        assert_eq!(
            iv_caps() & rupix_abi::CAP_VECTOR,
            0,
            "不实现 iv_export_svg 就不许声明 CAP_VECTOR（否则宿主以为有导出通道）"
        );
    }

    /// 区域通道（ABI v6）走档包装：不是任何成员认领的字节 → 让位语义
    /// （`ERR_UNRECOGNIZED`，宿主据此落回整幅路线，不算错）。
    #[test]
    fn render_region_rejects_bytes_no_member_claims() {
        let req = IvRegionReq {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            target_w: 10,
            target_h: 10,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render_region(
                b"not an image".as_ptr(),
                12,
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_UNRECOGNIZED
        );
        assert_eq!(info.actual_w, 0);
        assert_eq!(info.actual_h, 0);
    }

    const PSD: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/psd_red_2x2.psd"
    ));
    const HDR: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/hdr_red_2x2.hdr"
    ));

    fn ext_ptr(ext: &str) -> std::ffi::CString {
        std::ffi::CString::new(ext).expect("ext")
    }

    fn render(data: &[u8], ext: &str) -> (i32, IvRenderOut, Vec<u8>) {
        let ext = ext_ptr(ext);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            ext.as_ptr(),
        );
        if q != ERR_OK {
            return (q, info, Vec::new());
        }
        let mut out = vec![0u8; info.bytes_needed as usize];
        let code = iv_render(
            data.as_ptr(),
            data.len(),
            &req,
            out.as_mut_ptr(),
            out.len(),
            &mut info,
            ext.as_ptr(),
        );
        (code, info, out)
    }

    #[test]
    fn psd_and_hdr_render_through_the_pro_entry_point() {
        for (bytes, ext) in [(PSD, "psd"), (HDR, "hdr")] {
            assert_eq!(iv_probe(bytes.as_ptr(), bytes.len(), std::ptr::null()), 100);
            let (code, info, px) = render(bytes, ext);
            assert_eq!(code, ERR_OK, "{ext}: pro render failed");
            assert_eq!((info.actual_w, info.actual_h), (2, 2));
            assert!(
                px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]),
                "{ext}: expected solid red"
            );
        }
    }
}
