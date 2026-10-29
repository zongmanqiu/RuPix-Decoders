//! RuPix 发布档 `pro`：专业创作/HDR/工程格式（JPEG2000 / JPEG XR / EXR /
//! PSD / HDR / XCF / ASE）。
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
jxr;wdp;hdp;exr;\
xcf;ase;aseprite;psd;hdr",
    description: "专业格式：创作、HDR 与工程格式",
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
// ★ WMF/DjVu 已迁出 pro（2026-10-28，doc 档接管）；本档不再声明 CAP_REGION。
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

rupix_abi::export_metadata! {
    "version" => "0.4.2",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-29",
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
        id: "xcf",
        probe: pro_xcf::probe_impl,
        render: pro_xcf::render_impl,
    },
    Member {
        id: "ase",
        probe: pro_ase::probe_impl,
        render: pro_ase::render_impl,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_takes_a_known_magic() {
        let jp2 = b"\x00\x00\x00\x0CjP  \x0D\x0A\x87\x0A\x00\x00\x00\x00";
        assert_eq!(iv_probe(jp2.as_ptr(), jp2.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"not an image".as_ptr(), 12, std::ptr::null()), 0);
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
