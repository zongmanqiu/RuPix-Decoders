//! RuPix 外挂解码插件：JPEG XL (JXL) → RGBA8。
//!
//! 本 crate 是一个自包含的 `cdylib`：ABI 契约来自 [`rupix_abi`]（唯一真源），
//! panic gate / last-error / 固定导出由 [`rupix_abi::export_plugin!`]
//! 一行生成。本文件只保留 jxl-oxide 的解码与像素展开逻辑。
//!
//! 它是 QOI 试点之后的**第二个真插件**，存在的意义是验证：同一份冻结 ABI 对一个
//! 真实第三方解码器（依赖树更深、有自己的线程池、有 f32 色彩管线）是否依然通用。
//!
//! # 两阶段与缓存
//!
//! 冻结 ABI 要求「查询阶段（`out=NULL`）回报尺寸/`bytes_needed`，宿主分配后
//! 再渲染」。JXL 的难点在于 **orientation 会改变输出宽高**，而尺寸只有解码后
//! 才知道。因此本插件在**查询阶段就全解码**，把 RGBA 缓存到线程本地；渲染阶段
//! 直接从缓存取（指纹不符才重解），避免同一张图解码两次。
//!
//! `image_all_channels()` 返回的缓冲**已应用 orientation**，所以回报
//! `orientation=1` + `already_oriented=1`，宿主不得再转一次。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印“正在创建库 … 和对象 …”，rustc 默认把
// 链接器 stdout 当作 lint 报出。这里显式静音，以满足“零警告”要求。
#![allow(linker_messages)]

use std::cell::RefCell;

use jxl_oxide::{JxlImage, JxlThreadPool};
use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立插件形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

// ---------------------------------------------------------------------------
// 固定导出（ABI 自述 + 三个样板函数）
// ---------------------------------------------------------------------------

#[cfg(feature = "standalone")]
export_plugin! {
    id: "jxl",
    name: "JPEG XL",
    extensions: "jxl",
    description: "解码 JPEG XL (JXL) 格式",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render（本插件的业务部分）
// ---------------------------------------------------------------------------

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 2 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if looks_like_jxl(slice) {
            100
        } else {
            0
        }
    })
}
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 {
            set_last_error("内部错误（宿主传入空数据）");
            return ERR_UNRECOGNIZED;
        }
        if info.is_null() {
            set_last_error("内部错误（宿主传入空输出指针）");
            return ERR_BUFFER_TOO_SMALL;
        }
        let input = unsafe { std::slice::from_raw_parts(data, len) };

        // budget（req 可为 null → 无预算）。
        let budget: u64 = if req.is_null() {
            0
        } else {
            (unsafe { (*req).budget_bytes }) as u64
        };

        let fp = fingerprint(input);

        if out.is_null() {
            // ── 查询阶段：JXL 应用完 orientation 的真实尺寸只有解过才知道，所以这里
            //    必然解码一次；结果**留在缓存**里，紧随其后的渲染与同一张图的每次换档都复用。
            let (w, h, bytes_needed) = match cached_entry(fp, input) {
                Ok(v) => v,
                Err(e) => {
                    set_last_error(e.msg());
                    return e.code();
                }
            };
            if budget != 0 && bytes_needed > budget {
                set_last_error("图片太大，超出宿主给的内存预算");
                return ERR_TOO_LARGE;
            }
            fill_info(info, w, h, bytes_needed);
            return ERR_OK;
        }

        // ── 渲染阶段：只**借用**缓存，不许 take 掉。旧写法把查询阶段那份一口拿走，
        //    于是同一张图的第二次渲染（换档、按 `1` 上原图档）只能重解一整遍——
        //    3840×2160 实测第二次请求 892.8 ms（生产 exe + stall_probe，2026-10-21 m77）。
        let (w, h, bytes_needed) = match cached_entry(fp, input) {
            Ok(v) => v,
            Err(e) => {
                set_last_error(e.msg());
                return e.code();
            }
        };
        if budget != 0 && bytes_needed > budget {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }
        let filled = CACHE.with(|c| {
            let b = c.borrow();
            match b.as_ref().filter(|x| x.fp == fp) {
                Some(e) => {
                    unsafe {
                        std::ptr::copy_nonoverlapping(e.rgba.as_ptr(), out, bytes_needed as usize);
                    }
                    true
                }
                None => false,
            }
        });
        if !filled {
            set_last_error("内部错误（刚写好的解码缓存不在原地）");
            return ERR_CORRUPT;
        }
        fill_info(info, w, h, bytes_needed);
        ERR_OK
    })
}
/// 缓存里有没有这一张（按输入指纹）；没有就解一遍并**留下**，供同一张图的下一次请求命中。
/// 返回 `(宽, 高, 需要的字节数)`——像素一律从缓存里取，不再另搬一份。
fn cached_entry(fp: u64, input: &[u8]) -> Result<(u32, u32, u64), JxlErr> {
    if let Some(hit) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|x| x.fp == fp)
            .map(|x| (x.w, x.h, x.rgba.len() as u64))
    }) {
        return Ok(hit);
    }
    let d = decode(input)?;
    let v = (d.width, d.height, d.rgba.len() as u64);
    CACHE.with(|c| {
        *c.borrow_mut() = Some(Cached {
            fp,
            w: d.width,
            h: d.height,
            rgba: d.rgba,
        })
    });
    Ok(v)
}

#[inline]
fn fill_info(info: *mut IvRenderOut, w: u32, h: u32, bytes_needed: u64) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = 1; // 已在解码时应用（见模块头）
    oi.already_oriented = 1;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0; // 宿主不得再转一次
    oi.page_count = 1; // v4：单页；ICC 不透传（jxl-oxide 的 ICC 出口后续再接）。
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// 解码缓存（线程本地，供**同一张图的每一次请求**复用：查询→渲染、渲染→换档渲染）
// ---------------------------------------------------------------------------

struct Cached {
    /// 输入指纹：整遍内容算出来的键（见 `fingerprint`）。宿主在同一块 mmap 上换一张
    /// 恰好同长度的图是完全可能的，按地址或按"前 4 KiB + 长度"命中就是静默给用户看另一张图。
    fp: u64,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

thread_local! {
    static CACHE: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

/// 输入指纹：**整遍** FNV-1a（offset basis / prime 取标准值，与 av 插件那份同源同判据）。
/// 原来只扫前 4 KiB 再混长度——那口键只服务"紧随其后的那一次渲染"（缓存当时被 `take`
/// 掉，弱键没有第二次命中的机会）。m77 把缓存改成留着之后，弱键就变成了
/// "两张同长度、前 4 KiB 相同的图 ⇒ 静默串图"的窗口，所以换成全量。
/// O(n) 一遍、常数极小（与 av 插件那份同一判据），跟一次解码（同图实测 893 ms）比可以忽略。
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^ (bytes.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

// ---------------------------------------------------------------------------
// JXL 解码（jxl-oxide）
// ---------------------------------------------------------------------------

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。超出即拒。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// 裸码流签名（codestream）：`FF 0A`。
const CODESTREAM_SIG: [u8; 2] = [0xff, 0x0a];
/// ISOBMFF 容器签名：`00 00 00 0C 'J' 'X' 'L' ' ' 0D 0A 87 0A`。
const CONTAINER_SIG: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0c, b'J', b'X', b'L', b' ', 0x0d, 0x0a, 0x87, 0x0a,
];

fn looks_like_jxl(bytes: &[u8]) -> bool {
    bytes.starts_with(&CODESTREAM_SIG) || bytes.starts_with(&CONTAINER_SIG)
}

struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
enum JxlErr {
    Unrecognized,
    Corrupt,
    Unsupported,
    TooLarge,
    Oom,
}

impl JxlErr {
    fn code(self) -> i32 {
        match self {
            JxlErr::Unrecognized => ERR_UNRECOGNIZED,
            JxlErr::Corrupt => ERR_CORRUPT,
            JxlErr::Unsupported => ERR_UNSUPPORTED,
            JxlErr::TooLarge => ERR_TOO_LARGE,
            JxlErr::Oom => ERR_OOM,
        }
    }

    fn msg(self) -> &'static str {
        match self {
            JxlErr::Unrecognized => "不是 JPEG XL 文件（魔数不对）",
            JxlErr::Corrupt => "JPEG XL 数据损坏或被截断",
            JxlErr::Unsupported => "该 JPEG XL 特性不支持",
            JxlErr::TooLarge => "图片超过像素上限",
            JxlErr::Oom => "分配像素缓冲时内存不足",
        }
    }

    /// 把 jxl-oxide 的错误串粗分到「不支持」/「损坏」两类，尽量保住 ABI 要的可区分性。
    fn from_msg(m: &str) -> Self {
        let l = m.to_ascii_lowercase();
        if l.contains("unsupported") || l.contains("该特性不支持") || l.contains("unimplemented")
        {
            JxlErr::Unsupported
        } else {
            JxlErr::Corrupt
        }
    }
}

/// 解码首帧为 RGBA8（非预乘），行主序，长度 `w*h*4`。orientation 已应用。
fn decode(bytes: &[u8]) -> Result<Decoded, JxlErr> {
    if !looks_like_jxl(bytes) {
        return Err(JxlErr::Unrecognized);
    }
    let image = JxlImage::builder()
        // ★ 显式交一个"无池"给解码器，别依赖 `default_pool()`：那是按 jxl-oxide 的
        // rayon feature 二选一的（`lib.rs:187-195`：开=`rayon_global()`、关=`none()`）。
        // 本仓 build.ps1 用一次 `cargo build --workspace` 编全部 DLL，cargo 会做
        // feature 统一——`rawler`(rupix-raw) 以 default features 依赖 jxl-oxide，
        // 于是 rayon 又被打开，DLL 里悄悄拉起全局线程池（插件规范 §六 第 5 条禁止）。
        // 在 API 层传 `none()`，无论谁把 feature 打开都是单线程、零 spawn。
        .pool(JxlThreadPool::none())
        .read(bytes)
        .map_err(|e| JxlErr::from_msg(&e.to_string()))?;
    let render = image
        .render_frame(0)
        .map_err(|e| JxlErr::from_msg(&e.to_string()))?;

    // `image_all_channels` 已应用 orientation，且含各 extra channel（alpha 在其中）。
    let fb = render.image_all_channels();
    let w = fb.width() as u32;
    let h = fb.height() as u32;
    if w == 0 || h == 0 {
        return Err(JxlErr::Corrupt);
    }
    let px = (w as u64).checked_mul(h as u64).ok_or(JxlErr::Corrupt)?;
    if px > MAX_PIXELS {
        return Err(JxlErr::TooLarge);
    }

    let ch = fb.channels();
    let samples = fb.buf();
    let n = px as usize;
    if ch == 0 || samples.len() < n.saturating_mul(ch) {
        return Err(JxlErr::Corrupt);
    }

    let mut rgba: Vec<u8> = Vec::new();
    rgba.try_reserve_exact(n * 4).map_err(|_| JxlErr::Oom)?;
    for i in 0..n {
        let b = i * ch;
        // 通道布局：1=灰度, 2=灰度+alpha, 3=RGB, 4=RGBA。
        // >4（含额外通道，如 black）时取前 3 当 RGB、补不透明 alpha——
        // 额外通道的顺序不保证，宁可当不透明也不误把黑通道当 alpha。
        let (r, g, bl, a) = match ch {
            1 => (samples[b], samples[b], samples[b], 1.0),
            2 => (samples[b], samples[b], samples[b], samples[b + 1]),
            3 => (samples[b], samples[b + 1], samples[b + 2], 1.0),
            4 => (samples[b], samples[b + 1], samples[b + 2], samples[b + 3]),
            _ => (samples[b], samples[b + 1], samples[b + 2], 1.0),
        };
        rgba.push(to_u8(r));
        rgba.push(to_u8(g));
        rgba.push(to_u8(bl));
        rgba.push(to_u8(a));
    }

    Ok(Decoded {
        width: w,
        height: h,
        rgba,
    })
}

/// f32（[0,1]） → u8。非有限值当 0；四舍五入。
#[inline]
fn to_u8(v: f32) -> u8 {
    if !v.is_finite() {
        return 0;
    }
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    /// 1×1 纯红（含 alpha=255）：验证端到端真解码，且颜色正确。
    const RED_1X1: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/1x1_red.jxl"
    ));
    /// 4×4 灰度：验证灰阶展开成 RGBA（alpha 补满）。
    const GRAY_4X4: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/4x4_gray.jxl"
    ));

    #[test]
    fn decodes_known_1x1_red() {
        let d = decode(RED_1X1).expect("decode ok");
        assert_eq!((d.width, d.height), (1, 1));
        assert_eq!(d.rgba.len(), 4);
        assert_eq!(d.rgba[0], 255, "R");
        assert_eq!(d.rgba[1], 0, "G");
        assert_eq!(d.rgba[2], 0, "B");
        assert_eq!(d.rgba[3], 255, "A");
    }

    #[test]
    fn decodes_gray_4x4_and_expands_alpha() {
        let d = decode(GRAY_4X4).expect("decode ok");
        assert_eq!((d.width, d.height), (4, 4));
        assert_eq!(d.rgba.len(), 4 * 4 * 4);
        // 灰度图：R=G=B，alpha 补 255。
        for p in d.rgba.chunks_exact(4) {
            assert_eq!(p[0], p[1]);
            assert_eq!(p[1], p[2]);
            assert_eq!(p[3], 255);
        }
    }

    #[test]
    fn probe_detects_magic() {
        assert_eq!(
            iv_probe(RED_1X1.as_ptr(), RED_1X1.len(), std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(iv_probe(RED_1X1.as_ptr(), 1, std::ptr::null()), 0);
    }

    #[test]
    fn bad_magic_is_unrecognized() {
        let bad = [0x00u8, 0x11, 0x22, 0x33];
        assert_eq!(decode(&bad).err().map(|e| e.code()), Some(1));
    }

    #[test]
    fn truncated_stream_is_corrupt_not_panic() {
        // 保留魔数、砍掉其余：应是错误码而非 panic。
        let truncated = &CODESTREAM_SIG[..];
        assert_eq!(decode(truncated).err().map(|e| e.code()), Some(2));
    }

    #[test]
    fn from_msg_classifies_unsupported() {
        assert!(matches!(
            JxlErr::from_msg("Unsupported feature: patches"),
            JxlErr::Unsupported
        ));
        assert!(matches!(
            JxlErr::from_msg("invalid codestream"),
            JxlErr::Corrupt
        ));
    }

    #[test]
    fn to_u8_clamps_and_handles_nan() {
        assert_eq!(to_u8(0.0), 0);
        assert_eq!(to_u8(1.0), 255);
        assert_eq!(to_u8(-3.0), 0);
        assert_eq!(to_u8(9.0), 255);
        assert_eq!(to_u8(f32::NAN), 0);
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 4);
    }

    /// 两阶段一致性：查询阶段回报的尺寸/`bytes_needed` 必须与渲染阶段一致。
    #[test]
    fn two_phase_is_consistent() {
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut {
            actual_w: 0,
            actual_h: 0,
            bytes_needed: 0,
            pixel_format: 0,
            bit_depth: 0,
            orientation: 0,
            already_oriented: 0,
            frame_count: 0,
            frame_delay_ms: 0,
            page_count: 0,
            icc_len: 0,
        };
        // 查询阶段。
        let c = iv_render(
            GRAY_4X4.as_ptr(),
            GRAY_4X4.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, 0);
        assert_eq!((info.actual_w, info.actual_h), (4, 4));
        assert_eq!(info.bytes_needed, 4 * 4 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.already_oriented, 1);

        // 渲染阶段。
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            GRAY_4X4.as_ptr(),
            GRAY_4X4.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, 0);
        assert_eq!(buf[3], 255, "alpha 补满");
    }

    fn req0() -> IvRenderReq {
        IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        }
    }

    /// 跑一次渲染（`out` 给尺寸、`buf` 收像素），返回错误码。
    fn render(bytes: &[u8], out: &mut IvRenderOut, buf: &mut [u8]) -> i32 {
        iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req0(),
            if buf.is_empty() {
                std::ptr::null_mut()
            } else {
                buf.as_mut_ptr()
            },
            buf.len(),
            out,
            std::ptr::null(),
        )
    }

    /// ★ 同一张图的**第二次渲染**必须命中缓存，而不是把缓存消费掉之后再解一遍
    /// （旧写法渲染阶段 `take()` 走人：3840×2160 实测第二次请求 892.8 ms，m77 端点）。
    /// 判据不靠计时：渲染完缓存必须还在那儿，且第二次渲染回来的像素逐字节相同。
    #[test]
    fn second_render_reuses_the_retained_decode() {
        let mut info = IvRenderOut::default();
        assert_eq!(render(GRAY_4X4, &mut info, &mut []), 0, "查询阶段");
        let n = info.bytes_needed as usize;
        let mut first = vec![0u8; n];
        assert_eq!(render(GRAY_4X4, &mut info, &mut first), 0, "第一次渲染");
        assert!(
            CACHE.with(|c| c.borrow().is_some()),
            "渲染把缓存 take 掉了——同一张图的下一次请求就得重解一整遍"
        );

        let mut again = vec![0u8; n];
        assert_eq!(render(GRAY_4X4, &mut info, &mut again), 0, "第二次渲染");
        assert_eq!(again, first, "换档拿到的必须是同一份像素");
    }

    /// 缓存留着之后，键必须是**整遍内容**的指纹：同长度、只差尾巴上一个字节的另一张图
    /// 不许拿到这张图的像素。（旧键只扫前 4 KiB + 长度，尾巴上的改动看不见。）
    #[test]
    fn fingerprint_covers_the_whole_input_not_just_a_prefix() {
        let a = vec![0xA5u8; 8192];
        let mut b = a.clone();
        let last = b.len() - 1;
        b[last] = 0x5A;
        assert_eq!(a.len(), b.len(), "必须同长度才测得到前缀之外的部分");
        assert_ne!(
            fingerprint(&a),
            fingerprint(&b),
            "尾巴上的改动没换键——缓存会把另一张图的像素当这张交出去"
        );
    }

    /// 承上一句的行为面：缓存里是 4×4 那张，喂一段同长度、内容不同的字节，
    /// **不许**回同一份像素（命中就等于张冠李戴）。
    #[test]
    fn a_different_file_cannot_take_the_retained_pixels() {
        let mut info = IvRenderOut::default();
        assert_eq!(render(GRAY_4X4, &mut info, &mut []), 0);
        let n = info.bytes_needed as usize;
        let mut mine = vec![0u8; n];
        assert_eq!(render(GRAY_4X4, &mut info, &mut mine), 0);

        let mut other = GRAY_4X4.to_vec();
        let last = other.len() - 1;
        other[last] ^= 0xFF;
        assert_eq!(other.len(), GRAY_4X4.len());

        let mut theirs = vec![0u8; n];
        let rc = render(&other, &mut info, &mut theirs);
        assert!(
            rc != 0 || theirs != mine,
            "另一张字节却拿到了缓存里那份像素——键的判据被人摘了"
        );
    }
}
