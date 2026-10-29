//! RuPix 合并解码器 `modern`：HEIC/HEIF/AVIF + JPEG XL + JNG。
//!
//! 为什么这些格式放一起而不是并进 `common`：
//!
//! 1. **成熟度是整批里最低的**（`heic` 0.1.x、`jxl` 0.8.x），崩溃域要与 27 个日常
//!    格式隔开——一张坏 HEIC 三振出局，不该连累 JPEG/PNG；
//! 2. 两者都是**重依赖、低频但强绑定**的现代格式：用 iPhone 照片的人几乎一定也会
//!    碰到别人发来的 JXL，反之亦然；
//! 3. 都是现代/容器格式（HEIF、JXL、JNG）；将来接新格式时同族共享解析代码。
//!
//! 路由不在本文件：探针取最高分、两阶段让位都在 [`rupix_merge_router`]，四个合并档
//! 共用同一份边界条件。本 crate 只做装配——换库改 `Cargo.toml`，改格式去成员 crate。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

export_decoder! {
    id: "modern",
    name: "Modern Formats (HEIC/AVIF/JXL/JNG)",
    extensions: "heic;heif;hif;avif;avifs;jxl;jng;qoi;ff",
    description: "现代格式：手机照片、高效压缩图像与 QOI/Farbfeld",
    is_vector: 0,
    has_alpha: 1,
}

// 只有整解：HEIC/AVIF/JXL 都没有"解码期降采样"的口子（预测编码不给这个口子），
// 缩放一律交给基座。
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

rupix_abi::export_metadata! {
    "version" => "0.3.2",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-29",
}

static MEMBERS: &[Member] = &[
    Member {
        id: "av",
        probe: av::probe_impl,
        render: av::render_impl,
    },
    Member {
        id: "jng",
        probe: jng::probe_impl,
        render: jng::render_impl,
    },
    Member {
        id: "jxl",
        probe: jxl::probe_impl,
        render: jxl::render_impl,
    },
    Member {
        id: "qoi",
        probe: qoi::probe_impl,
        render: qoi::render_impl,
    },
    Member {
        id: "ff",
        probe: ff::probe_impl,
        render: ff::render_impl,
    },
];

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| rupix_merge_router::probe(MEMBERS, data, len, ext))
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
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

    const HEIC: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/1x1_red.avif"
    ));
    const JXL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/1x1_red.jxl"
    ));
    const JNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/jng_64x48_alpha.jng"
    ));
    const HEIC_REAL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/heic_640x480.heic"
    ));

    fn req() -> IvRenderReq {
        IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        }
    }

    /// 查询 → 按申报尺寸分配 → 渲染（与宿主真实调用顺序一致）。
    fn render(data: &[u8], ext: &[u8]) -> (i32, IvRenderOut, Vec<u8>) {
        let mut info = IvRenderOut::default();
        let r = req();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            &r,
            std::ptr::null_mut(),
            0,
            &mut info,
            ext.as_ptr().cast(),
        );
        if q != ERR_OK {
            return (q, info, Vec::new());
        }
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let code = iv_render(
            data.as_ptr(),
            data.len(),
            &r,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            ext.as_ptr().cast(),
        );
        (code, info, buf)
    }

    #[test]
    fn claims_only_its_own_seven_suffixes() {
        assert_eq!(iv_probe(HEIC.as_ptr(), HEIC.len(), c"avif".as_ptr()), 100);
        assert_eq!(iv_probe(JXL.as_ptr(), JXL.len(), c"jxl".as_ptr()), 100);
        assert_eq!(iv_probe(JNG.as_ptr(), JNG.len(), c"jng".as_ptr()), 100);
        assert_eq!(
            iv_probe(HEIC_REAL.as_ptr(), HEIC_REAL.len(), c"heic".as_ptr()),
            100
        );
        // 不是它的后缀：谁都不认，合并解码器就不该认。
        let junk = b"definitely not an image";
        assert_eq!(iv_probe(junk.as_ptr(), junk.len(), std::ptr::null()), 0);
    }

    #[test]
    fn all_members_render_through_one_entry_point() {
        let (code, info, px) = render(HEIC, b"avif\0");
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (1, 1));
        assert_eq!(px.len(), 4);

        let (code, info, px) = render(JXL, b"jxl\0");
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (1, 1));
        assert_eq!(px.len(), 4);

        let (code, info, px) = render(JNG, b"jng\0");
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (64, 48));
        assert_eq!(px.len(), 64 * 48 * 4);

        let (code, info, px) = render(HEIC_REAL, b"heic\0");
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (640, 480));
        assert_eq!(px.len(), 640 * 480 * 4);
    }

    #[test]
    fn unknown_bytes_are_not_claimed_and_not_rendered() {
        let junk = b"definitely not an image";
        let (code, _, _) = render(junk, b"heic\0");
        assert_eq!(code, ERR_UNRECOGNIZED);
    }
}
