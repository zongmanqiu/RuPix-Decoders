//! RuPix 解码插件：**现代格式**（av）—— HEIC / HEIF / AVIF。
//!
//! 解码库分工（都是纯 Rust、无 C 依赖，编译进本 DLL）：
//! - **HEIC/HEIF** → [`heif_oxide`]：ISO/IEC 23008-12 容器 + 纯 Rust HEVC。
//!   iPhone 照片的 grid 拼图、8/10-bit、Display P3、`irot`/`imir`/`clap` 方向
//!   全部在解码器内处理完，输出即显示就绪的 sRGB（`already_oriented = 1`）。
//! - **AVIF** → [`zenavif`]：rav1d-safe 纯 Rust AV1 解码。8/10/12-bit、全部
//!   色度抽样、alpha、动图（取首帧）。
//!
//! # 为什么 sniff 看 `ftyp` brand
//!
//! 两族文件都是 ISOBMFF 容器，载荷（HEVC vs AV1）藏在 `mdat` 里，魔数层面
//! 只有 `ftyp` 的 major/compatible brand 能区分。清单里的扩展名只是让宿主把
//! 文件路由到本插件，**真正的分流看 brand**（与 core 插件"内容嗅探"同一哲学）。
//!
//! # 已知限制（有意为之，不是缺陷）
//!
//! - HEIC 单色 (4:0:0) alpha 辅助图解不出（底层 HEVC 解码器只支持 4:2:0），
//!   输出无 alpha——比整个文件打不开好。
//! - AVIF 的 `irot`/`imir` 方向元数据 zenavif **不应用**：本插件自己走 `ipco` 读出来，
//!   按 EXIF orientation 语义如实回报（`already_oriented = 0`，由宿主补转）。
//!   覆盖的是单独 `irot`（0/90/180/270 CCW）与单独 `imir`；两者**同时**在场时应用
//!   顺序由 `ipma` 关联次序决定（ISO 23008-12 §6.5.1），本插件不解析 ipma，
//!   此时保守报方向 1（躺平）而不是猜一个错的。
//! - 动图取首帧（与宿主"看图器只显一帧"的模型一致）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立插件形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;

use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "standalone")]
export_plugin! {
    id: "av",
    name: "Modern Formats (HEIC/AVIF)",
    // `.heif` 也常见 `.hif`（部分相机）；avifs 是动图序列扩展名。
    extensions: "heic;heif;hif;avif;avifs",
    description: "现代格式：HEIC/HEIF（iPhone 照片）与 AVIF",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB），与 core 插件同一道闸。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// 文件分族（由 `ftyp` brand 判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// HEVC 载荷的 HEIF（.heic/.heif）。
    Heif,
    /// AV1 载荷的 HEIF（AVIF）。
    Avif,
}

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render
// ---------------------------------------------------------------------------

/// 魔数嗅探：`ftyp` box 的 major/compatible brand 命中两族之一 → 100。
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
        if data.is_null() {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        match sniff(slice) {
            Some(_) => 100,
            None => 0,
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

        let kind = match sniff(input) {
            Some(k) => k,
            None => {
                set_last_error("不是 HEIF/AVIF 容器（ftyp 品牌不匹配）");
                return ERR_UNRECOGNIZED;
            }
        };

        // ── 阶段 1：容器头。─────────────────────────────────────────────
        // HEIC 的纯 Rust HEVC 一次要 0.2 s 级（3000×4000 实测一次请求 222 ms，含读盘与容器，
        // `bench_decode`，m77），查询阶段绝不能真解码——否则"查询 + 渲染"就是双倍等待。
        // AVIF 例外（见下）。
        // 容器方向只喂 HEIF 的换边尺寸（heif-oxide 自己在解码器内转）；AVIF 的
        // 方向由 decode_avif 现读现报，见 DecodedRgba 的第 4 项。
        let (w, h, _) = container_dimensions(input);
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };

        // AVIF 的 ispe 会撒谎：grid 拼图的最终尺寸未必能从 ipco 精确还原
        // （color_grid_alpha_nogrid 样本 ispe 报 80×80，zenavif 实际拼出
        // 80×128），而宿主按查询结果一次性分配缓冲、无重试——查询阶段必须
        // 报**真实**尺寸。所以 AVIF 查询直接真解码（rav1d 纯 Rust 足够快），
        // 结果缓存在线程局部，紧随其后的渲染调用零重复解码。HEIC 仅在容器
        // 读不到 ispe 的罕见情况下被迫在查询阶段真解码。
        let query_needs_decode = matches!(kind, Kind::Avif) || w == 0 || h == 0;

        if out.is_null() {
            if query_needs_decode {
                let d = match decode_cached(kind, input) {
                    Ok(d) => d,
                    Err(code) => return code,
                };
                let bytes_needed = d.0.len() as u64;
                if d.1 as u64 * d.2 as u64 > MAX_PIXELS {
                    set_last_error("图片超过 6400 万像素的插件上限");
                    return ERR_TOO_LARGE;
                }
                if budget != 0 && bytes_needed > budget as u64 {
                    set_last_error("图片太大，超出宿主给的内存预算");
                    return ERR_TOO_LARGE;
                }
                fill_info_oriented(info, d.1, d.2, bytes_needed, d.3, d.4);
                return ERR_OK;
            }
            // HEIF + ispe 可信：微秒级查询，不碰 mdat。
            let npixels = w as u64 * h as u64;
            if npixels > MAX_PIXELS {
                set_last_error("图片超过 6400 万像素的插件上限");
                return ERR_TOO_LARGE;
            }
            let bytes_needed = npixels * 4;
            // budget 在查询阶段就要拦：宿主按查询结果分配缓冲，查询放行的
            // 超大图到渲染阶段才拒，白编一圈。
            if budget != 0 && bytes_needed > budget as u64 {
                set_last_error("图片太大，超出宿主给的内存预算");
                return ERR_TOO_LARGE;
            }
            fill_info(info, w, h, bytes_needed);
            return ERR_OK;
        }

        // ── 阶段 2：真解码（同一份输入字节只解一次：HEIF 与 AVIF 都先查内容键缓存）。──
        let d = match decode_cached(kind, input) {
            Ok(d) => d,
            Err(code) => return code,
        };
        let (rgba, dw, dh, orientation, already_oriented) = (&d.0, d.1, d.2, d.3, d.4);
        let bytes_needed = rgba.len() as u64;
        // budget 渲染阶段同样拦（查询被跳过时的兜底）。
        if budget != 0 && bytes_needed > budget as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }
        // HEIF 的容器 ispe 与解码不符 = 文件损坏；ispe 是旋转前尺寸，宽高互换
        // 后一致也算（heif-oxide 应用 irot）。AVIF 的 ispe 本就不可信。
        if kind == Kind::Heif && w != 0 && h != 0 && (dw != w || dh != h) && (dw != h || dh != w) {
            set_last_error("解出的尺寸与容器 ispe 声明不符");
            return ERR_CORRUPT;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
        }
        fill_info_oriented(info, dw, dh, bytes_needed, orientation, already_oriented);
        ERR_OK
    })
}
/// HEIF 查询阶段回填：`heif-oxide` 在解码器内就把 irot/imir/clap 应用完了，
/// 输出即显示就绪，所以这里报 `orientation=1 / already_oriented=1`（宿主不再转）。
/// AVIF 不走这条路（`query_needs_decode` 恒真），它的方向由 `decode_avif` 现读现报。
#[inline]
fn fill_info(info: *mut IvRenderOut, w: u32, h: u32, bytes_needed: u64) {
    fill_info_oriented(info, w, h, bytes_needed, 1, 1);
}

#[inline]
fn fill_info_oriented(
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    bytes_needed: u64,
    orientation: i32,
    already_oriented: i32,
) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = orientation;
    oi.already_oriented = already_oriented;
    // v4：单页；ICC 不透传。frame/delay 缺省 0 = 静态（heic 动图后续再接）。
    oi.page_count = 1;
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// ftyp brand 嗅探
// ---------------------------------------------------------------------------

/// 解析 `ftyp` box：返回 (major brand, compatible brands)。
///
/// ISOBMFF 头：4 字节 size + 'ftyp' + 4 字节 major brand + 4 字节 minor + N×4 字节
/// compatible brands。size 为 0（到文件尾）或 1（64 位 ExtendsSize）的文件实际
/// 罕见，兼容 brands 直接扫到缓冲尾为止。
fn ftyp_brands(data: &[u8]) -> Option<(&[u8], Vec<&[u8]>)> {
    if data.len() < 16 || &data[4..8] != b"ftyp" {
        return None;
    }
    let major = &data[8..12];
    let mut compat = Vec::new();
    // Compatible brands 从偏移 16 开始，4 字节一个；扫一段合理的上限防异常文件。
    let rest = &data[16..data.len().min(16 + 4 * 64)];
    for b in rest.chunks_exact(4) {
        compat.push(b);
    }
    Some((major, compat))
}

const AVIF_BRANDS: [&[u8; 4]; 5] = [b"avif", b"avis", b"av01", b"tmap", b"maf1"];
const HEIF_BRANDS: [&[u8; 4]; 11] = [
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1",
    b"mif2",
];

fn brand_kind(b: &[u8]) -> Option<Kind> {
    if AVIF_BRANDS.iter().any(|k| *k == b) {
        return Some(Kind::Avif);
    }
    if HEIF_BRANDS.iter().any(|k| *k == b) {
        return Some(Kind::Heif);
    }
    None
}

/// 分族判定：major brand 优先，其次 compatible brands（AVIF 优先——mif1 文件
/// 常把 avif 列在 compatible 里，反过来 HEIC 文件不会列 avif）。
fn sniff(data: &[u8]) -> Option<Kind> {
    let (major, compat) = ftyp_brands(data)?;
    if let Some(k) = brand_kind(major) {
        return Some(k);
    }
    compat.iter().find_map(|b| brand_kind(b))
}

// ---------------------------------------------------------------------------
// 容器尺寸解析（查询阶段）：meta → iprp → ipco/ispe
// ---------------------------------------------------------------------------

/// ISOBMFF box 头：返回 (类型, 内容区)。
fn box_parts(b: &[u8]) -> Option<([u8; 4], &[u8])> {
    if b.len() < 8 {
        return None;
    }
    let size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let mut ty = [0u8; 4];
    ty.copy_from_slice(&b[4..8]);
    if size == 0 {
        // 到文件尾。
        return Some((ty, &b[8..]));
    }
    if size == 1 {
        // 64 位 largesize：8 字节头 + 8 字节 size。
        if b.len() < 16 {
            return None;
        }
        let large =
            u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]) as usize;
        let end = large.min(b.len()).max(16);
        return Some((ty, &b[16..end]));
    }
    if size < 8 || size > b.len() {
        return None;
    }
    Some((ty, &b[8..size]))
}

/// 在 `container` 里找第一个类型为 `ty` 的子 box 的内容。
fn find_box<'a>(container: &'a [u8], ty: &[u8; 4]) -> Option<&'a [u8]> {
    let mut cur = container;
    while !cur.is_empty() {
        let (t, body) = box_parts(cur)?;
        if t == *ty {
            return Some(body);
        }
        // box_parts 给的是"按 size 截断"的内容区；前进量 = 头 + 内容。
        let size = if cur.len() >= 8 {
            let s = u32::from_be_bytes([cur[0], cur[1], cur[2], cur[3]]) as usize;
            if s == 0 {
                cur.len()
            } else if s == 1 {
                if cur.len() < 16 {
                    return None;
                }
                (u64::from_be_bytes([
                    cur[8], cur[9], cur[10], cur[11], cur[12], cur[13], cur[14], cur[15],
                ]) as usize)
                    .min(cur.len())
                    .max(16)
            } else {
                s
            }
        } else {
            return None;
        };
        cur = &cur[size..];
    }
    None
}

/// full box：内容区前 4 字节是 version+flags。
fn full_box_payload(body: &[u8]) -> Option<&[u8]> {
    if body.len() < 4 {
        return None;
    }
    Some(&body[4..])
}

/// 当前 box 的总前进量（头 + 内容）。
fn box_advance(b: &[u8]) -> Option<usize> {
    if b.len() < 8 {
        return None;
    }
    let s = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    if s == 0 {
        return Some(b.len());
    }
    if s == 1 {
        if b.len() < 16 {
            return None;
        }
        let large =
            u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]) as usize;
        return Some(large.min(b.len()).max(16));
    }
    if s < 8 || s > b.len() {
        return None;
    }
    Some(s)
}

/// ispe body（full box 之后）：width u32 + height u32。
fn ispe_dims(body: &[u8]) -> Option<(u32, u32)> {
    let p = full_box_payload(body)?;
    if p.len() < 8 {
        return None;
    }
    let w = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
    let h = u32::from_be_bytes([p[4], p[5], p[6], p[7]]);
    Some((w, h))
}

/// 容器尺寸与显示方向：meta → iprp → ipco 里**像素最多**的那个 ispe，配上 irot/imir。
///
/// 不走 pitm/ipma 精确关联：真实文件里 ispe 有多份（color/alpha/grid 瓦片/
/// 派生图），"最大者 = 最终呈现图"在 iPhone HEIC（瓦片小、grid 大）与
/// color+alpha 双 item 文件上都成立；而 ipma 关联在这类文件上容易选中
/// alpha 辅助图的小 ispe。多出来的闲 ispe 只会让 MAX_PIXELS 闸更严，无害。
///
/// `irot` 的 1 字节值是**逆时针** 90° 的档数（0=不转、1=90 CCW、2=180、3=270 CCW）。
/// 这条不是凭记忆：`.3rd/libheif-1.23.6` 的 `Box_irot::parse` 写的就是
/// `m_rotation = rotation * 90` 且成员注释 "in degrees (CCW)"，`file.cc` 的
/// `add_orientation_properties` 把 EXIF 6（90° CW）编成 ccw=270、EXIF 8（270° CW）
/// 编成 ccw=90；我们 HEIC 路线用的 `heif-oxide` 也是同一口径（"angle * 90 degrees
/// anti-clockwise"）。两家独立实现一致，才敢按它回报宿主。
fn container_dimensions(data: &[u8]) -> (u32, u32, i32) {
    let Some(orient) = container_transform(data) else {
        return (0, 0, 1);
    };
    let Some(meta_box) = find_box(data, b"meta") else {
        return (0, 0, 1);
    };
    // meta 是 full box；iprp 在其 payload 里。
    let meta_payload = full_box_payload(meta_box).unwrap_or(meta_box);
    let Some(iprp) = find_box(meta_payload, b"iprp") else {
        return (0, 0, 1);
    };
    let Some(ipco) = find_box(iprp, b"ipco") else {
        return (0, 0, 1);
    };

    let mut best = (0u32, 0u32);
    let mut best_px = 0u64;
    let mut cur = ipco;
    while !cur.is_empty() {
        let Some((t, body)) = box_parts(cur) else {
            break;
        };
        if t == *b"ispe" {
            if let Some((w, h)) = ispe_dims(body) {
                let px = w as u64 * h as u64;
                if px > best_px {
                    best_px = px;
                    best = (w, h);
                }
            }
        }
        let Some(adv) = box_advance(cur) else { break };
        cur = &cur[adv..];
    }
    // 90/270（奇数档）一定换边，与是否叠加镜像无关。
    let (w, h) = if orient.1 { (best.1, best.0) } else { best };
    (w, h, orient.0)
}

/// 只读 ipco 里的 `irot` / `imir`，折算成 EXIF 方向（1–8）＋是否换边。
///
/// 认不准的一律返回 `Some((1, false))` 之外的**保守值**：irot 与 imir 同时在时，
/// 应用顺序由 `ipma` 的关联次序决定（ISO 23008-12 §6.5.1，libheif 按 MIAF 的
/// "裁剪→旋转→镜像" 写），本插件不解析 ipma 关联，**宁可躺平也不猜**——猜错
/// 方向比不转更难看了。
fn container_transform(data: &[u8]) -> Option<(i32, bool)> {
    let meta_box = find_box(data, b"meta")?;
    let meta_payload = full_box_payload(meta_box).unwrap_or(meta_box);
    let iprp = find_box(meta_payload, b"iprp")?;
    let ipco = find_box(iprp, b"ipco")?;

    let mut irot: Option<u8> = None;
    let mut imir: Option<u8> = None;
    let mut cur = ipco;
    while !cur.is_empty() {
        let Some((t, body)) = box_parts(cur) else {
            break;
        };
        // `irot` / `imir` 是**裸 Box**（没有 version/flags），内容区就是那一个字节。
        // 依据：heif-oxide `parse_property` 对二者直接 `r.u8()` 且不调 `full_box_header`
        // （只有 `ispe`/`ipma` 调），libheif 的 `Box_irot`/`Box_imir` 均 `extends Box`
        // 而非 FullBox。真实样本 `abc_color_irot_alpha_irot.avif` 里的 irot 是 9 字节
        // 盒（8 头 + 1 值 = 1）；若按 full box 剥掉 4 字节就一个字节都不剩，方向一律
        // 躺平成 1，旋转过的 AVIF 显示歪。
        let one_byte = body.first().copied();
        if t == *b"irot" {
            if let Some(v) = one_byte {
                irot = Some(v & 0x3); // 与 libheif/heif-oxide 同步掩掉越界值
            }
        } else if t == *b"imir" {
            if let Some(v) = one_byte {
                imir = Some(v & 0x1);
            }
        }
        let Some(adv) = box_advance(cur) else { break };
        cur = &cur[adv..];
    }

    let swap = matches!(irot, Some(1) | Some(3));
    let orientation = match (irot, imir) {
        (None, None) | (Some(0), None) => 1,
        (Some(1), None) => 8, // 90° CCW
        (Some(2), None) => 3, // 180°
        (Some(3), None) => 6, // 270° CCW = 90° CW
        // imir axis 0 = 绕竖轴（左右翻），axis 1 = 绕横轴（上下翻）；与宿主
        // core/geometry.rs::oriented 的 2=左右镜像、4=上下镜像逐字对齐。
        (None, Some(0)) => 2,
        (None, Some(1)) => 4,
        _ => 1,
    };
    Some((orientation, swap))
}

type DecodedRgba = (Vec<u8>, u32, u32, i32, i32); // (rgba, w, h, orientation, already_oriented)

// ---------------------------------------------------------------------------
// 阶段 2：真解码
// ---------------------------------------------------------------------------

thread_local! {
    /// 同一份输入字节只解一次：查询 → 渲染两次 FFI 调用共用，**渲染 → 渲染也共用**
    /// （宿主按档位二次请求——例如按 `1` 上原图档——拿的是同一张图的同一份全尺寸像素）。
    /// 键 = (输入长度, 内容指纹)，**不是**地址：宿主释放一块 mmap 之后，在**同一个
    /// 地址**映射另一个恰好同长度的文件是完全可能的，那时按地址命中就是静默给用户
    /// 看另一张图。指纹是整遍 FNV-1a，O(n) 且常数极小，与一次 HEVC/AV1 解码相比可以忽略
    /// （判据与下面 `decode_cached` 的收益同一批实测），它买的是"绝不张冠李戴"。
    /// 仅存于调用线程，无跨线程问题。
    static DECODE_CACHE: RefCell<Option<(usize, u64, Rc<DecodedRgba>)>> = const { RefCell::new(None) };
}

/// 输入的 64 位内容指纹（FNV-1a 64，offset basis / prime 取标准值）。
fn fingerprint(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 解码入口（带内容键缓存，HEIF 与 AVIF 同一份）。**同一输入字节只真解一次**：
/// 查询 → 渲染复用，第二次渲染（换档、按 `1` 上原图档）也复用。
///
/// 为什么 HEIF 以前不进这份缓存：它查询阶段不解码（ispe 够用），写的人只想到
/// "查询+渲染"这一对，于是**每次渲染各自真解一遍**——端点实测：3000×4000 的 HEIC
/// 按 `1` 上原图档那一次请求 **262.8 ms**，而形状一样的 AVIF 第二次请求命中缓存只要
/// **30.9 ms**（生产 exe + `build/stall_probe.ps1`，2026-10-21 m77，读数在 `.plan/m77-*`）。
fn decode_cached(kind: Kind, data: &[u8]) -> Result<Rc<DecodedRgba>, i32> {
    let key = (data.len(), fingerprint(data));
    let cached = DECODE_CACHE.with(|c| {
        let mut b = c.borrow_mut();
        match b.as_ref() {
            Some(k) if (k.0, k.1) == key => Some(Rc::clone(&k.2)),
            _ => {
                *b = None;
                None
            }
        }
    });
    if let Some(d) = cached {
        return Ok(d);
    }
    let decoded = Rc::new(match kind {
        Kind::Heif => decode_heif(data)?,
        Kind::Avif => decode_avif(data)?,
    });
    DECODE_CACHE.with(|c| *c.borrow_mut() = Some((key.0, key.1, Rc::clone(&decoded))));
    Ok(decoded)
}

/// HEIC/HEIF：heif-oxide 一把解完（方向已应用）。
fn decode_heif(data: &[u8]) -> Result<DecodedRgba, i32> {
    match heif_oxide::decode_bytes(data) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            Ok((rgba, img.width, img.height, 1, 1))
        }
        Err(e) => {
            let s = e.to_string();
            set_last_error(&format!("heif-oxide 库报错 \"{s}\""));
            let ls = s.to_ascii_lowercase();
            if ls.contains("unsupported") || ls.contains("该特性不支持") || ls.contains("codec")
            {
                Err(ERR_UNSUPPORTED)
            } else if ls.contains("图片过大") || ls.contains("limit") {
                Err(ERR_TOO_LARGE)
            } else {
                Err(ERR_CORRUPT)
            }
        }
    }
}

/// AVIF：zenavif 解码（动图取首帧；方向元数据转成 EXIF orientation 回报宿主）。
fn decode_avif(data: &[u8]) -> Result<DecodedRgba, i32> {
    // 静图优先；动图（Error::Unsupported）退到 decode_animation 取首帧。
    let buf = match zenavif::decode(data) {
        Ok(b) => b,
        Err(_) => {
            let anim = zenavif::decode_animation(data).map_err(|e| {
                set_last_error(&format!("zenavif 库报错 \"{e}\""));
                map_zenavif_err(&e.to_string())
            })?;
            anim.frames
                .into_iter()
                .next()
                .map(|f| f.pixels)
                .ok_or_else(|| {
                    set_last_error("动图没有任何帧");
                    ERR_CORRUPT
                })?
        }
    };

    let (w, h) = (buf.width(), buf.height());
    let rgba = pixel_buffer_to_rgba8(&buf)?;
    // zenavif 不应用 irot/imir。方向从容器现读（常数级开销，不碰 mdat），
    // 按 EXIF 语义**如实**回报并声明 already_oriented=0，让宿主
    // core::geometry.rs::oriented 补转——像素与 actual_w/h 都是**未转**的存储态，
    // 宿主按 5..=8 自行换边，两边对得上。
    let orientation = container_transform(data).map(|(o, _)| o).unwrap_or(1);
    Ok((rgba, w, h, orientation, 0))
}

/// zenavif 的 PixelBuffer → RGBA8（非预乘）。
///
/// 解码输出格式随源变化（无 alpha 源给 RGB8/GRAY8、高比特深给 *16），
/// 按 descriptor 的 PixelFormat 分派；`as_contiguous_pixels` 内部处理行 stride。
fn pixel_buffer_to_rgba8(buf: &zenavif::PixelBuffer) -> Result<Vec<u8>, i32> {
    use rgb::{Gray, RGB16, RGB8, RGBA16, RGBA8};
    use zenpixels::descriptor::PixelFormat;

    let w = buf.width();
    let h = buf.height();
    let mut v = Vec::with_capacity(w as usize * h as usize * 4);

    let unsupported = || {
        set_last_error(&format!("意外的像素格式 {:?}", buf.descriptor().format));
        Err::<Vec<u8>, i32>(ERR_UNSUPPORTED)
    };

    match buf.descriptor().format {
        PixelFormat::Rgba8 => {
            let Some(px) = buf.as_contiguous_pixels::<RGBA8>() else {
                return unsupported();
            };
            for p in px {
                v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
            }
        }
        PixelFormat::Rgb8 => {
            let Some(px) = buf.as_contiguous_pixels::<RGB8>() else {
                return unsupported();
            };
            for p in px {
                v.extend_from_slice(&[p.r, p.g, p.b, 255]);
            }
        }
        PixelFormat::Rgba16 => {
            let Some(px) = buf.as_contiguous_pixels::<RGBA16>() else {
                return unsupported();
            };
            for p in px {
                v.extend_from_slice(&[
                    (p.r >> 8) as u8,
                    (p.g >> 8) as u8,
                    (p.b >> 8) as u8,
                    (p.a >> 8) as u8,
                ]);
            }
        }
        PixelFormat::Rgb16 => {
            let Some(px) = buf.as_contiguous_pixels::<RGB16>() else {
                return unsupported();
            };
            for p in px {
                v.extend_from_slice(&[(p.r >> 8) as u8, (p.g >> 8) as u8, (p.b >> 8) as u8, 255]);
            }
        }
        PixelFormat::Gray8 => {
            let Some(px) = buf.as_contiguous_pixels::<Gray<u8>>() else {
                return unsupported();
            };
            for p in px {
                let g = p.value();
                v.extend_from_slice(&[g, g, g, 255]);
            }
        }
        PixelFormat::Gray16 => {
            let Some(px) = buf.as_contiguous_pixels::<Gray<u16>>() else {
                return unsupported();
            };
            for p in px {
                let g = (p.value() >> 8) as u8;
                v.extend_from_slice(&[g, g, g, 255]);
            }
        }
        PixelFormat::GrayA8 => {
            let Some(px) = buf.as_contiguous_pixels::<zenpixels::GrayAlpha8>() else {
                return unsupported();
            };
            for p in px {
                v.extend_from_slice(&[p.v, p.v, p.v, p.a]);
            }
        }
        // GrayA16 没有 zenpixels::Pixel 实现（GRAY16 同理少见）；遇到就走 UNSUPPORTED。
        PixelFormat::GrayA16 => return unsupported(),
        _ => return unsupported(),
    }

    if v.len() != w as usize * h as usize * 4 {
        set_last_error("转换后的尺寸与缓冲尺寸不符");
        return Err(ERR_CORRUPT);
    }
    Ok(v)
}

/// zenavif 的错误（`At<Error>` 包装，不便按变体匹配）按文案归类。
fn map_zenavif_err(s: &str) -> i32 {
    let ls = s.to_ascii_lowercase();
    if ls.contains("图片过大") {
        ERR_TOO_LARGE
    } else if ls.contains("unsupported") || ls.contains("该特性不支持") {
        ERR_UNSUPPORTED
    } else {
        ERR_CORRUPT
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;
    use std::path::PathBuf;

    use rupix_abi as kit;

    fn td(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../testdata/golden")
            .join(name)
    }

    fn load(name: &str) -> Vec<u8> {
        std::fs::read(td(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
    }

    /// 伪造一个最小 ftyp：major=heic，compatible=[mif1, heic]。
    fn fake_ftyp(major: &[u8; 4], compat: &[&[u8; 4]]) -> Vec<u8> {
        let mut v = Vec::new();
        let size = 16 + compat.len() * 4;
        v.extend_from_slice(&(size as u32).to_be_bytes());
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0, 0, 0, 0]); // minor version
        for c in compat {
            v.extend_from_slice(*c);
        }
        v
    }

    #[test]
    fn probe_accepts_brands() {
        let heic = fake_ftyp(b"heic", &[b"mif1", b"heic"]);
        assert_eq!(iv_probe(heic.as_ptr(), heic.len(), std::ptr::null()), 100);

        let avif = fake_ftyp(b"avif", &[b"mif1", b"avis"]);
        assert_eq!(iv_probe(avif.as_ptr(), avif.len(), std::ptr::null()), 100);

        // mif1 major + avif compatible（AAC 等工具会这么写）→ AVIF。
        let mif_avif = fake_ftyp(b"mif1", &[b"avif"]);
        assert_eq!(
            iv_probe(mif_avif.as_ptr(), mif_avif.len(), std::ptr::null()),
            100
        );
    }

    #[test]
    fn probe_rejects_non_heif() {
        let png_header = b"\x89PNG\r\n\x1a\n0000IHDR".to_vec();
        assert_eq!(
            iv_probe(png_header.as_ptr(), png_header.len(), std::ptr::null()),
            0
        );
        let garbage = vec![0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        assert_eq!(
            iv_probe(garbage.as_ptr(), garbage.len(), std::ptr::null()),
            0
        );

        // ftyp 但 brand 两族都不认（如 mp4）。
        let mp4 = fake_ftyp(b"isom", &[b"mp41"]);
        assert_eq!(iv_probe(mp4.as_ptr(), mp4.len(), std::ptr::null()), 0);
    }

    /// 真实小样本：probe 命中 + 查询尺寸 + 渲染成功。
    fn roundtrip(name: &str, want_w: u32, want_h: u32) -> Vec<u8> {
        let bytes = load(name);
        assert_eq!(
            iv_probe(bytes.as_ptr(), bytes.len(), std::ptr::null()),
            100,
            "{name}: probe 未识别"
        );

        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK, "{name}: 查询阶段失败");
        assert_eq!(
            (info.actual_w, info.actual_h),
            (want_w, want_h),
            "{name}: 容器尺寸不符"
        );
        assert_eq!(info.bytes_needed, want_w as u64 * want_h as u64 * 4);

        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK, "{name}: 渲染阶段失败");
        assert_eq!(
            (info.actual_w, info.actual_h),
            (want_w, want_h),
            "{name}: 解码尺寸不符"
        );
        buf
    }

    #[test]
    fn avif_1x1_red() {
        let buf = roundtrip("1x1_red.avif", 1, 1);
        // 红（10-bit 样本会有一点量化误差，容忍 ±8）。
        assert!(buf[0] > 200, "R={} 应接近 255", buf[0]);
        assert!(buf[1] < 60, "G={} 应接近 0", buf[1]);
        assert!(buf[2] < 60, "B={} 应接近 0", buf[2]);
    }

    #[test]
    fn avif_4x4() {
        roundtrip("4x4.avif", 4, 4);
    }

    #[test]
    fn avif_4x4_gray() {
        let buf = roundtrip("4x4_gray.avif", 4, 4);
        // 灰度图 R=G=B（zenavif 单色源转 RGB 后各通道相等）。
        let (r, g, b) = (buf[0], buf[1], buf[2]);
        assert!(
            r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2,
            "灰度应 R≈G≈B，实为 {r},{g},{b}"
        );
    }

    /// grid 拼图 + alpha：AVIF 的"iPhone 照片等价物"，必须拼对尺寸。
    #[test]
    fn avif_grid_alpha() {
        // color_grid_alpha_nogrid：源尺寸未知但一定是 w×h×4 字节成功。
        let bytes = load("color_grid_alpha_nogrid.avif");
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        let (w, h) = (info.actual_w, info.actual_h);
        assert!(w > 0 && h > 0, "grid 样本应有非零尺寸");
        let mut buf = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(buf.len(), (w * h * 4) as usize);
    }

    /// irot 旋转样本：zenavif 不应用旋转，容器 ispe 是旋转**前**的尺寸时
    /// 渲染尺寸等于 ispe（不崩、不撒谎即可）。
    #[test]
    fn avif_irot_decodes() {
        let bytes = load("abc_color_irot_alpha_irot.avif");
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK, "irot 样本查询失败");
        let (w, h) = (info.actual_w, info.actual_h);
        // zenavif 返回的是**未旋转**的像素，所以必须上报 already_oriented=0，
        // 由宿主补转；谎报 1 会让旋转图直接躺平显示。
        assert_eq!(info.already_oriented, 0);
        assert_eq!(info.orientation, 8, "该样本容器里 irot=1（90° CCW）");
        assert_eq!(
            info.orientation,
            container_transform(&bytes).map(|(o, _)| o).unwrap_or(1),
            "上报的方向必须与容器 irot/imir 算出来的一致"
        );
        let mut buf = vec![0u8; info.bytes_needed as usize];
        assert_eq!(
            iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(buf.len(), (w * h * 4) as usize);
    }

    #[test]
    fn budget_is_enforced() {
        let bytes = load("1x1_red.avif");
        // 1x1 只要 4 字节；给 2 字节预算必须报太大。
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 2,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_TOO_LARGE);
    }

    #[test]
    fn short_buffer_is_rejected() {
        let bytes = load("1x1_red.avif");
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let mut small = vec![0u8; 2];
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req,
            small.as_mut_ptr(),
            small.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_BUFFER_TOO_SMALL);
    }

    /// 声明的扩展名清单必须齐。
    #[test]
    fn declares_extension_list() {
        let info = unsafe { &*iv_plugin_info() };
        assert_eq!(info.abi_version, IV_ABI_VERSION);
        let exts = unsafe { std::ffi::CStr::from_ptr(info.extensions) }
            .to_str()
            .unwrap();
        for must in ["heic", "heif", "hif", "avif", "avifs"] {
            assert!(
                exts.split(';').any(|e| e == must),
                "扩展名清单缺 {must}：{exts}"
            );
        }
        assert_eq!(info.is_vector, 0);
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 4);
    }

    /// 渲染成功后 last-error 必须为空。
    #[test]
    fn success_clears_last_error() {
        let bytes = load("1x1_red.avif");
        let req = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        let mut info = IvRenderOut::default();
        let mut buf = vec![0u8; 4];
        assert_eq!(
            iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req,
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        let mut ebuf = [0i8; 16];
        assert_eq!(
            unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) },
            1
        );
    }

    // ---- 容器头方向解析（缺陷②b / ⑤ 的判据锁）--------------------------

    /// 造一个 box：4 字节 size + 4 字节类型 + body。
    fn iso(ty: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(body.len() + 8);
        v.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
        v.extend_from_slice(ty);
        v.extend_from_slice(body);
        v
    }

    /// `irot` / `imir` 属性 box：**裸 Box**，payload 就是那一个字节（无 version+flags）。
    /// 依据见 `container_transform` 的注释（libheif `Box_irot: public Box`、
    /// heif-oxide 对二者直接 `r.u8()`、真实样本里 irot 的 size=9）。
    fn prop(ty: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        iso(ty, payload)
    }

    /// 按 ISO 23008-12 的真实层级拼一段容器头：
    /// `meta`(FullBox) → `iprp`(普通 box) → `ipco`(**也是普通 box**) → 属性 box。
    /// 属性层各自：`ispe` 是 FullBox（带 4 字节 version+flags），`irot`/`imir` 不带。
    fn container_with(props: &[Vec<u8>]) -> Vec<u8> {
        let mut ipco_body = Vec::new();
        for p in props {
            ipco_body.extend_from_slice(p);
        }
        let iprp = iso(b"iprp", &iso(b"ipco", &ipco_body));
        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&iprp);
        let mut f = fake_ftyp(b"heic", &[b"mif1"]);
        f.extend_from_slice(&iso(b"meta", &meta_body));
        f
    }

    /// `ispe` 的 body：4 字节 version+flags + w + h（它是真 FullBox）。
    fn ispe_prop(w: u32, h: u32) -> Vec<u8> {
        let mut body = vec![0u8; 4];
        body.extend_from_slice(&w.to_be_bytes());
        body.extend_from_slice(&h.to_be_bytes());
        iso(b"ispe", &body)
    }

    /// 尺寸 + 方向一起读：`irot` 原值按 **CCW×90°** 折算成 EXIF——
    /// 0→1、1→8（90° CCW）、2→3、3→6（270° CCW = 90° CW）；奇数档必须换边。
    /// 口径出处见 `container_dimensions` 的注释（libheif 与 heif-oxide 两家一致）。
    #[test]
    fn container_ispe_with_irot_reports_exif_orientation() {
        for (angle, want_o) in [(0u8, 1i32), (1, 8), (2, 3), (3, 6)] {
            let f = container_with(&[ispe_prop(40, 20), prop(b"irot", &[angle])]);
            let (w, h, o) = container_dimensions(&f);
            assert_eq!(o, want_o, "irot={angle} 应折成 EXIF {want_o}");
            let want_dims = if angle % 2 == 1 { (20, 40) } else { (40, 20) };
            assert_eq!((w, h), want_dims, "irot={angle} 的换边判定");
        }
    }

    /// ★ `ipco` 是**普通 box**，属性子 box 紧跟 8 字节头之后。
    /// 早先按 FullBox 剥了 4 字节，那一刀把第一个属性（ispe）的头当成 version+flags
    /// 吃掉、后续整排错位，真实 HEIF 的查询阶段静默退化成 (0,0)，于是 HEIC 在查询
    /// 与渲染各解一遍（整趟解码翻倍——12 MP 实测一趟 222 ms，m77）。这条锁住"不许剥"。
    #[test]
    fn ipco_children_are_read_without_a_fullbox_strip() {
        let f = container_with(&[ispe_prop(64, 32)]);
        assert_eq!(
            container_dimensions(&f),
            (64, 32, 1),
            "ipco 不该有 version+flags 可剥，剥了就读不到 ispe"
        );
    }

    /// 真实样本收口：`abc_color_irot_alpha_irot.avif` 的 ispe=512×256、irot=1
    /// （90° CCW）→ 显示尺寸 256×512、EXIF 8。合成夹具会跟着我的误解一起错，
    /// 真实文件不会——这条同时验 ipco 不剥头、irot 读首字节、CCW 折算三件事。
    #[test]
    fn real_avif_sample_rotation_matches_container_bytes() {
        let bytes = load("abc_color_irot_alpha_irot.avif");
        assert_eq!(
            container_dimensions(&bytes),
            (256, 512, 8),
            "ispe 512×256 + irot=1(CCW90) → 显示 256×512 / EXIF 8"
        );
    }

    /// 单独 `imir`：axis 0 = 绕竖轴（左右翻）→ EXIF 2，axis 1 = 绕横轴（上下翻）→ EXIF 4。
    /// `irot` 与 `imir` 同时在：应用顺序由 `ipma` 关联次序决定，本插件不解析 ipma，
    /// **保守报方向 1**；但 90° 旋转一定换边，尺寸照常换。
    #[test]
    fn imir_alone_maps_to_flip_and_combo_stays_conservative() {
        let ispe = ispe_prop(40, 20);

        let f = container_with(&[ispe.clone(), prop(b"imir", &[0])]);
        assert_eq!(container_dimensions(&f), (40, 20, 2));
        let f = container_with(&[ispe.clone(), prop(b"imir", &[1])]);
        assert_eq!(container_dimensions(&f), (40, 20, 4));

        let f = container_with(&[ispe, prop(b"irot", &[1]), prop(b"imir", &[0])]);
        let (w, h, o) = container_dimensions(&f);
        assert_eq!(o, 1, "irot+imir 组合不猜方向");
        assert_eq!((w, h), (20, 40), "但 90° 旋转一定换边");
    }

    /// 真实 iPhone 样张形状的容器：`ipco` 裸 Box，里面三个 ispe（两张缩略图 +
    /// 主图）+ 两个 9 字节 irot。形状逐字段照 `.plan/bench/corpus-real/orange.heic`
    /// 的实测盒树复刻（主图 ispe 4000×3000、irot=3 即 270° CCW），入库只留结构、
    /// 不带 2.5 MB 的 HEVC 载荷。
    ///
    /// 锁两件事：① "像素最多的 ispe = 主图" 的选择法不会被缩略图带偏；
    /// ② 奇数档 irot 换边 → 显示 3000×4000、EXIF 6（竖持照片，物理上说得通）。
    #[test]
    fn heic_shaped_container_picks_main_image_and_rotates() {
        let f = container_with(&[
            iso(b"hvcC", &[0x01; 40]),
            ispe_prop(512, 512),   // thumb 派生图
            ispe_prop(4000, 3000), // 主图
            prop(b"irot", &[3]),
            iso(b"hvcC", &[0x01; 40]),
            ispe_prop(512, 384), // 另一张缩略图
            prop(b"irot", &[3]),
        ]);
        assert_eq!(container_dimensions(&f), (3000, 4000, 6));
    }

    /// 缓存键的承重项：**同长度、不同内容**必须给出不同指纹。
    /// 早先的键是 (裸地址, 长度)——宿主在同一地址放另一个同长度 AVIF 就会串图。
    #[test]
    fn fingerprint_separates_same_length_different_content() {
        let a = load("4x4.avif");
        let mut b = a.clone();
        let last = b.len() - 1;
        b[last] ^= 0xFF;
        assert_eq!(a.len(), b.len(), "两个样本必须同长度才测得到要害");
        assert_ne!(fingerprint(&a), fingerprint(&b), "同长度不同内容必须换键");
        assert_eq!(fingerprint(&a), fingerprint(&a.clone()), "同一输入必须同键");
    }

    /// ★ HEIF 的渲染也走那份内容键缓存（m77）。判据**不靠计时**：先把键手填进缓存，
    /// 再拿一段必然解不动的字节去调 `decode_cached(Kind::Heif, …)`——
    /// 命中就该原样拿到缓存里那份（Ok + 同一枚 `Rc`）；把旧写法"HEIF 每次真解"退回去，
    /// 这里立刻变成 Err（那段字节真的解不出来）。
    #[test]
    fn heif_render_hits_the_shared_cache() {
        const PX: u32 = 3;
        let bytes: Vec<u8> = b"these bytes are not a real HEIC but that is the point".to_vec();
        let primed = Rc::new((vec![7u8; (PX * PX * 4) as usize], PX, PX, 1, 1));
        let keep = Rc::clone(&primed);
        let key = (bytes.len(), fingerprint(&bytes));
        DECODE_CACHE.with(|c| *c.borrow_mut() = Some((key.0, key.1, primed)));

        let got = decode_cached(Kind::Heif, &bytes).expect("命中缓存就不该去真解这段字节");
        assert!(
            Rc::ptr_eq(&got, &keep),
            "第二次渲染必须复用同一份解码结果，不是重解一遍"
        );
        assert_eq!((got.1, got.2), (PX, PX));
        assert_eq!(got.0[0], 7, "拿到的必须是缓存里那份像素");
    }

    /// 同长度、不同内容**不许**命中（指纹承重）。上一枚测试填了键，这里换成改过一
    /// 个字节、长度相同的另一段——判据若只看长度就会假命中，把上一张图的像素交给这一张。
    #[test]
    fn same_length_different_content_cannot_hit_the_heif_cache() {
        let bytes: Vec<u8> = b"these bytes are not a real HEIC but that is the point".to_vec();
        let mut other = bytes.clone();
        let last = other.len() - 1;
        other[last] ^= 0xFF;
        assert_eq!(other.len(), bytes.len(), "必须同长度才测得到指纹");

        let key = (bytes.len(), fingerprint(&bytes));
        DECODE_CACHE
            .with(|c| *c.borrow_mut() = Some((key.0, key.1, Rc::new((Vec::new(), 1, 1, 1, 1)))));
        // 走真解：这段字节不是 HEIC，必须报错，而不是回缓存里那张 1×1。
        assert!(
            decode_cached(Kind::Heif, &other).is_err(),
            "同长度不同内容却拿到了缓存——指纹判据被人摘了"
        );
    }
}
