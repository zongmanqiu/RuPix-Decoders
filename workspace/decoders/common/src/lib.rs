//! RuPix 合并解码器 `common`：把常见位图的「每格式冠军」装进同一枚 DLL。
//!
//! 合并的动机只有一个——**省体积**：每枚 DLL 都自带一份底座（空壳 + Rust 运行时，
//! 实测约 150 KB/枚，`qoi` 那枚 192 KB 反推得到）。八枚成员单飞 ≈ 8 份底座，
//! 合一枚只剩 1 份，而库本体一分不少。
//!
//! # 三条硬约束
//!
//! 1. **本 crate 不含任何解码逻辑**，只做装配：要换某个库，改 `Cargo.toml` 的依赖；
//!    要改某个格式的解法，去那个成员的 crate 里改。
//! 2. **成员一律 `default-features = false`**，拿到的只有 `probe_impl` / `render_impl`；
//!    `iv_*` 导出与自述宏仍留在各成员自己的独立构建里（`standalone` feature）。
//! 3. **路由逻辑不在本文件**：探针取最高分、查询与渲染两阶段的让位全在
//!    [`rupix_merge_router`]。那段代码有三个合并档共用，边界条件只该有一份。
//!
//! 让位链下沉到 DLL 内部是合并最大的副作用：过去"冠军报 UNSUPPORTED → 宿主找下一个
//! 认领者 → core 兜底"要跨三个 DLL，现在是同一枚 DLL 内的一次循环。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

export_decoder! {
    id: "common",
    name: "Common Raster Set",
    extensions: "jpg;jpeg;jpe;jfif;jif;jfi;exif;jps;mpo;png;apng;gif;tif;tiff;qoi;\
webp;bmp;dib;rle;ico;icon;cur;ani;pnm;ppm;pgm;pbm;pam;dds;ff;tga",
    description: "常见位图合集：照片、截图与图标",
    is_vector: 0,
    has_alpha: 1,
}

// 只有整解：合并后不再声明 CAP_SCALED（jpeg 的 turbo 腿已关，缩放归基座），
// 也不声明 CAP_MULTIFRAME（成员里只有 gif/core 会出多帧，逐格式的能力差异
// 目前没有"按后缀查能力"的接口，宁可不报）。
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

rupix_abi::export_metadata! {
    "version" => "0.2.3",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-28",
}

/// 成员表。**顺序不影响路由**（按探针置信度排序），只影响同分时的先后。
///
/// 同分只可能出现在两类：两个成员都报 100（例如一个 `.jpg` 文件同时被当成别的
/// 魔数——不会发生），或都报 0（那就没人认领）。所以这张表的顺序可以随便排。
static MEMBERS: &[Member] = &[
    Member {
        id: "jpeg",
        probe: jpeg::probe_impl,
        render: jpeg::render_impl,
    },
    Member {
        id: "png",
        probe: png::probe_impl,
        render: png::render_impl,
    },
    Member {
        id: "gif",
        probe: gif::probe_impl,
        render: gif::render_impl,
    },
    Member {
        id: "tiff",
        probe: tiff::probe_impl,
        render: tiff::render_impl,
    },
    Member {
        id: "qoi",
        probe: qoi::probe_impl,
        render: qoi::render_impl,
    },
    // 通用兜底放最后：它报 50，本来就压不过报 100 的冠军；放最后只是让同分时
    // "专才优先"，也方便出问题时一眼看出兜底有没有接住。
    Member {
        id: "core",
        probe: core_rs::probe_impl,
        render: core_rs::render_impl,
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

    const JPG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/golden_16x12.jpg"
    ));
    const PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/red_1x1.png"
    ));
    /// CMYK TIFF：`tiff` crate 拒收（报 UNSUPPORTED），`image` 能解。
    /// 这是让位链唯一的现成自然样本。
    const CMYK_TIF: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/cmyk_8x8.tif"
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

    fn probe(data: &[u8], ext: Option<&str>) -> i32 {
        let ext = ext.map(|e| {
            let mut v = e.as_bytes().to_vec();
            v.push(0);
            v
        });
        let p = ext.as_ref().map_or(std::ptr::null(), |v| v.as_ptr().cast());
        iv_probe(data.as_ptr(), data.len(), p)
    }

    /// 查询 → 按申报尺寸分配 → 渲染，两步走一遍（与宿主真实调用顺序一致）。
    fn render(data: &[u8], ext: Option<&str>) -> (i32, IvRenderOut, Vec<u8>) {
        render_frame(data, ext, 0)
    }

    /// 两阶段渲染指定帧（`frame_index` 决定要不要走让位链）。
    fn render_frame(
        data: &[u8],
        ext: Option<&str>,
        frame_index: u32,
    ) -> (i32, IvRenderOut, Vec<u8>) {
        let ext = ext.map(|e| {
            let mut v = e.as_bytes().to_vec();
            v.push(0);
            v
        });
        let e = ext.as_ref().map_or(std::ptr::null(), |v| v.as_ptr().cast());
        let r = IvRenderReq {
            frame_index,
            ..req()
        };
        let mut info = IvRenderOut::default();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            &r,
            std::ptr::null_mut(),
            0,
            &mut info,
            e,
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
            e,
        );
        (code, info, buf)
    }

    #[test]
    fn probe_takes_the_highest_member_confidence() {
        // 冠军报 100，兜底 core 也认（报 50）——合并解码器必须报 100。
        assert_eq!(probe(JPG, Some("jpg")), 100);
        assert_eq!(probe(PNG, Some("png")), 100);
        // 没有任何成员认得的字节：0。
        assert_eq!(probe(b"not an image at all", None), 0);
    }

    #[test]
    fn two_phase_render_reports_then_fills() {
        let (code, info, px) = render(JPG, Some("jpg"));
        assert_eq!(code, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (16, 12));
        assert_eq!(info.bytes_needed, 16 * 12 * 4);
        assert_eq!(px.len() as u64, info.bytes_needed);
        assert_eq!(info.pixel_format, PF_RGBA8);
    }

    #[test]
    fn fallback_chain_hands_over_to_the_generalist() {
        // 单问 tiff 成员：它拒收这张 CMYK（这是 tiff crate 的能力边界，非本解码器引入）。
        let mut info = IvRenderOut::default();
        let r = req();
        let ext = b"tif\0";
        let solo = tiff::render_impl(
            CMYK_TIF.as_ptr(),
            CMYK_TIF.len(),
            &r,
            std::ptr::null_mut(),
            0,
            &mut info,
            ext.as_ptr().cast(),
        );
        assert_eq!(solo, ERR_UNSUPPORTED, "tiff crate 应拒收 CMYK TIFF");
        // 经过 common 的同一条路径：tiff 让位 → core 接住 → 解得开。
        let (code, info2, px) = render(CMYK_TIF, Some("tif"));
        assert_eq!(code, ERR_OK, "让位链应落到 core");
        assert_eq!((info2.actual_w, info2.actual_h), (8, 8));
        assert_eq!(px.len(), 8 * 8 * 4);
    }

    #[test]
    fn unknown_bytes_are_not_claimed_and_not_rendered() {
        let junk = b"definitely not an image, just ascii bytes";
        assert_eq!(probe(junk, None), 0);
        let (code, _, _) = render(junk, None);
        assert_eq!(code, ERR_UNRECOGNIZED);
    }

    /// 动图回归（2026-10-25 实测踩到）：合并后宿主只看得到 `common` 一枚解码器，
    /// 所以「`gif` 只服务第 0 帧、后续帧交给 `core` 的会话缓存」这条让位必须在
    /// **DLL 内部**走完。第一版合并只在查询阶段让位、渲染阶段死守赢家，
    /// 于是 `sample-gif.gif` 从第 2 帧起每次都报「该文件用了此解码器不支持的特性」——
    /// 画面停在第 1 帧，而宿主侧完全不知道还能找谁。
    #[test]
    fn animation_frames_fall_back_inside_the_dll() {
        const ANIM_GIF: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../testdata/golden/anim_2x2_2f.gif"
        ));

        // 第 0 帧由 gif 冠军出（纯红）。
        let (code0, info0, px0) = render_frame(ANIM_GIF, Some("gif"), 0);
        assert_eq!(code0, ERR_OK);
        assert_eq!((info0.actual_w, info0.actual_h), (2, 2));
        assert_eq!(&px0[..4], &[255, 0, 0, 255], "第 0 帧应是纯红");

        // 第 2 帧 gif 必然软失败，必须由 core 接住（纯蓝）。
        let (code1, info1, px1) = render_frame(ANIM_GIF, Some("gif"), 1);
        assert_eq!(code1, ERR_OK, "第 2 帧必须在 DLL 内部让位给 core");
        assert_eq!((info1.actual_w, info1.actual_h), (2, 2));
        assert_eq!(&px1[..4], &[0, 0, 255, 255], "第 2 帧应是纯蓝");
        assert_ne!(px0, px1, "两帧内容必须真的不同（不是把第 1 帧又画一遍）");
    }

    #[test]
    fn tga_is_claimed_by_extension_only() {
        // TGA 没有魔数：core 只能靠扩展名认领，合并解码器必须把 ext 原样传下去。
        let mut tga = vec![0u8; 18];
        tga[2] = 2; // uncompressed true-color
        let (code, _, _) = render(&tga, Some("tga"));
        // 16 个 0 字节的像素数据 → core 会报坏文件；关键是**认领并走到 core**，
        // 而不是报"没有成员认领"。
        assert_ne!(code, ERR_UNRECOGNIZED, "ext=tga 应被 core 认领");
    }

    #[test]
    fn jps_and_mpo_reuse_the_jpeg_member() {
        for ext in ["jps", "mpo"] {
            assert_eq!(probe(JPG, Some(ext)), 100, "{ext}: JPEG 成员应认领");
            let (code, info, px) = render(JPG, Some(ext));
            assert_eq!(code, ERR_OK, "{ext}: 应走 JPEG 解码");
            assert_eq!((info.actual_w, info.actual_h), (16, 12));
            assert_eq!(px.len(), 16 * 12 * 4);
        }
    }
}
