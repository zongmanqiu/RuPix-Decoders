//! RuPix 发布档 `pro`：专业/低频格式（JPEG2000 / JPEG XR / EXR / KTX2 /
//! DICOM / 科学影像 / 设计源文件 / 矢量）。
//!
//! 与 `common` 一样，本 crate **只做装配**：依赖表 + `MEMBERS` 路由表，
//! 一行解码逻辑都不写。每个成员都是 `bench/singles/pro-*` 的单库。
//!
//! 已知缺口（没有可用纯 Rust 光栅器，暂不认领）：`pict/pct`（crate 要 nightly）、
//! `swf`（解析器无光栅化）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::*;
use rupix_abi::{export_plugin, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

export_plugin! {
    id: "pro",
    name: "Professional Formats",
    extensions: "jp2;j2k;j2c;jpc;jpx;jpf;jpt;\
jxr;wdp;hdp;exr;ktx;ktx2;dcm;dicom;\
nii;nrrd;mrc;mrcs;ccp4;map;\
pcx;pcc;dcx;icns;wbmp;xbm;xcf;wmf;ase;aseprite;djvu;djv",
    description: "专业档：JPEG2000/JPEG XR/EXR/KTX2/DICOM/科学影像/设计源文件/WMF/DjVu",
    is_vector: 0,
    has_alpha: 1,
}

rupix_abi::export_caps!(rupix_abi::CAP_FULL);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_takes_a_known_magic() {
        let jp2 = b"\x00\x00\x00\x0CjP  \x0D\x0A\x87\x0A\x00\x00\x00\x00";
        assert_eq!(iv_probe(jp2.as_ptr(), jp2.len(), std::ptr::null()), 100);
        assert_eq!(iv_probe(b"not an image".as_ptr(), 12, std::ptr::null()), 0);
    }
}
