//! RuPix 外挂解码器：Radiance HDR → RGBA8（库：`image` 的 `hdr` feature）。
//!
//! # 为什么是这个库（2026-10-25 晚，stb 单头 C 库的纯 Rust 替身）
//!
//! HDR（Radiance RGBE）在 crates.io 上只有两处实现：`image` 的 `hdr`
//! feature 与 zune 家族的 `zune-hdr`。选前者有三个理由：`hdr` feature 在
//! 上游**不引入任何新 crate**（`hdr = []`，bytemuck / byteorder-lite /
//! num-traits 是 image 的固定依赖，树里早有）；`core` 成员用的就是同一个
//! `image`，同一枚 DLL 里不会出现两份 HDR 解码器；`image` 是 crates.io 上
//! 下载量最大的图像 crate，维护活跃度不是一个量级。
//! 换掉 stb 之后本解码器**零新增第三方依赖**（除 image 本身已在树里）。
//!
//! # 一条实现级硬约束：必须走 float 路径
//!
//! stb 的 HDR 有两条输出路：8bit 路自带 `pow(v*2, 1/2.2)` gamma 色调映射，
//! 对 > 1.0 的高光直接截断；float 路（`stbi_loadf`）出线性 f32。
//! photo 内容实测 PSNR：**8bit 路 13.4 dB（不可用）vs float 路 51.2 dB
//! （与基线持平）**。image 的 `HdrDecoder` 与 stb 的 float 路同构
//! （`color_type() == Rgb32F`，逐扫描线 `read_image` 写 3×f32），
//! 所以这里拿到的天然就是线性 float——**不要把 `read_image` 的结果再套
//! gamma 或截断**，那一行就是 51.2 dB 口径的全体。
//!
//! f32 → u8 的归一化与 stb 时代逐字同口径（image `color.rs::normalize_float`）：
//! `clamp(v, 0, 1)` 后 `* 255.0` 四舍五入。NaN / ≥ 1.0 → 1.0。
//!
//! # 能力边界（对着 image 0.25.10 的 hdr 源码逐条核过，别凭记忆改）
//!
//! | 特性 | image 的 HdrDecoder | 本解码器 |
//! |---|---|---|
//! | 签名 | 严查只认 `#?RADIANCE` | probe 两种都认（`#?RADIANCE` / `#?RGBE`），后者走非严查构造 |
//! | 方向 | 只认 `-Y H +X W` | 其余方向 → `ERR_UNSUPPORTED`（照实报，不猜） |
//! | FORMAT | 出现时必须为 `32-bit_rle_rgbe` | 同上；XYZE 不支持 |
//! | 扫描线 | 新版逐分量 RLE + 旧版行程 | 库内处理，解码器不碰 |
//! | 色彩 | 线性 RGB float | 解码器侧归一化成 RGBA8 |
//!
//! 方向只认一种是 image 的已知缺口：真实文件里 `-Y H +X W` 占绝大多数，
//! 但确有别的写法。probe 因此**仍然认领**（签名对就是 Radiance 文件），
//! 由 render 报 `ERR_UNSUPPORTED`——把"我不认这格式"和"这格式我用不了"
//! 分开报，是解码器规范 §六 第 10 条。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use image::codecs::hdr::HdrDecoder;
use image::error::ImageError;
use image::ImageDecoder;
use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "hdr",
    name: "Radiance HDR (image)",
    extensions: "hdr",
    description: "解码 Radiance HDR（RGBE → 线性 float → RGBA8，不套 gamma）",
    is_vector: 0,
    has_alpha: 0,
}

// 只有整解：HDR 没有"原生降采样"（zune/image 都不出缩略图腿），也没有
// 多帧 / 多页 / 矢量语义。谎报 CAP_SCALED 的代价不是崩，而是宿主把缩放
// 请求发过来、我们照样整解，白付一次全图解码。
#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL);

// ---------------------------------------------------------------------------
// 常量（全部对着 image 0.25.10 的 codecs/hdr 核过，别凭记忆改）
// ---------------------------------------------------------------------------

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素。RGBA8 输出 ≈ 256 MiB，**float 中间缓冲是 12 B/px
/// ≈ 768 MiB**（`read_image` 要一整块 w*h*12 才肯写）。宿主 `budget_bytes`
/// 按输出尺寸先行拦截，这一条是畸形头的最后一道闸。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Radiance 签名。stb 的 `stbi__hdr_test` 逐字：两种都认，带换行。
/// image 严查模式只认前者（`SIGNATURE = b"#?RADIANCE"`），后者走非严查构造
/// ——非严查模式下签名那一行会被当作注释跳过，其余头部解析完全一致。
const HDR_SIG_RADIANCE: &[u8] = b"#?RADIANCE\n";
const HDR_SIG_RGBE: &[u8] = b"#?RGBE\n";

/// `Rgb32F` 每像素 12 字节（3×f32），`read_image` 的目标缓冲就是这个尺寸。
const FLOAT_BPP: u64 = 12;

/// 认领时一并带回的信息：走哪种构造（严查 / 非严查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// `#?RADIANCE\n` → `HdrDecoder::new`（严查签名）。
    Radiance,
    /// `#?RGBE\n` → `HdrDecoder::new_nonstrict`（老式 Radiance 签名）。
    OldRadiance,
}

/// 判断字节流是否属于本解码器能解的 HDR 形态。
fn classify(data: &[u8]) -> Option<Kind> {
    if data.len() >= HDR_SIG_RADIANCE.len() {
        if data.starts_with(HDR_SIG_RADIANCE) {
            return Some(Kind::Radiance);
        }
        if data.starts_with(HDR_SIG_RGBE) {
            return Some(Kind::OldRadiance);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render
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
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        // hdr 在全链只有这一家认领 → 认了就是冠军置信度，没有 50 兜底分。
        if classify(slice).is_some() {
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
        let input = unsafe { std::slice::from_raw_parts(data, len) };
        let Some(kind) = classify(input) else {
            set_last_error("不是本解码器能解的 HDR 字节特征");
            return ERR_UNRECOGNIZED;
        };

        // ── 阶段 1：只解析头拿尺寸（HdrDecoder::new 只读头部，不解像素）。──
        let (w, h) = match header_dimensions(input, kind) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };
        if w == 0 || h == 0 {
            set_last_error("HDR 文件头声明的宽或高为 0");
            return ERR_CORRUPT;
        }
        let npixels = w as u64 * h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码上限");
            return ERR_TOO_LARGE;
        }
        let bytes_needed = npixels * 4;
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };
        if budget != 0 && bytes_needed > budget as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }
        if info.is_null() {
            set_last_error("内部错误（宿主传入空输出指针）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // 查询阶段：out 为空指针，只回报尺寸/格式。
        if out.is_null() {
            fill_info(info, w, h, bytes_needed);
            return ERR_OK;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // ── 阶段 2：真解码。────────────────────────────────────────────
        // float 中间缓冲：w*h*12。两道尺寸闸都在它之前，所以这里只会为
        // "我们确认解得起"的图分内存。
        let dec = match kind {
            Kind::Radiance => HdrDecoder::new(Cursor::new(input)),
            Kind::OldRadiance => HdrDecoder::new_nonstrict(Cursor::new(input)),
        };
        let dec = match dec {
            Ok(d) => d,
            Err(e) => {
                let (code, msg) = classify_hdr(&e);
                set_last_error(&msg);
                return code;
            }
        };
        // 库回报的尺寸必须与阶段 1 读到的一致——`read_image` 按**自己的**
        // 尺寸断言缓冲长度（`assert_eq!(buf.len(), total_bytes())`），对不上
        // 就是 assert panic，被 gate 转成 ERR_PANIC 喂宿主三振黑名单。
        let (dw, dh) = dec.dimensions();
        if (dw, dh) != (w, h) {
            set_last_error("HDR 头字段前后不一致");
            return ERR_CORRUPT;
        }
        let mut floats = vec![0u8; (npixels * FLOAT_BPP) as usize];
        if let Err(e) = dec.read_image(&mut floats) {
            let (code, msg) = classify_hdr(&e);
            set_last_error(&msg);
            return code;
        }
        let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes_needed as usize) };
        for (d, s) in dst
            .chunks_exact_mut(4)
            .zip(floats.chunks_exact(FLOAT_BPP as usize))
        {
            for k in 0..3 {
                // `read_image` 经 bytemuck 写入的是**本机序** f32；
                // 这里按 4 字节手工拼数组，避免对固定长度 slice 的 try_into().unwrap()。
                let v = f32::from_ne_bytes([s[k * 4], s[k * 4 + 1], s[k * 4 + 2], s[k * 4 + 3]]);
                d[k] = hdr_to_u8(v);
            }
            d[3] = 255;
        }
        fill_info(info, w, h, bytes_needed);
        ERR_OK
    })
}

#[inline]
fn fill_info(info: *mut IvRenderOut, w: u32, h: u32, bytes_needed: u64) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = 1;
    oi.already_oriented = 1; // HDR 没有方向语义，宿主不必再转
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1;
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// image 薄封装
// ---------------------------------------------------------------------------

/// 只解析头拿尺寸。`Cursor` 借用输入字节，不复制。
fn header_dimensions(input: &[u8], kind: Kind) -> Result<(u32, u32), (i32, String)> {
    let dec = match kind {
        Kind::Radiance => HdrDecoder::new(Cursor::new(input)),
        Kind::OldRadiance => HdrDecoder::new_nonstrict(Cursor::new(input)),
    };
    match dec {
        Ok(d) => Ok(d.dimensions()),
        Err(e) => Err(classify_hdr(&e)),
    }
}

/// 把 image 的错误映射到 ABI 错误码。
///
/// **按枚举分支归类，不按字符串前缀猜**：`IoError` = 文件截断（坏），
/// `Unsupported` = 方向/FORMAT 这类"认得但用不了"（不许演变成"文件损坏"），
/// `Limits` = image 自己的尺寸溢出闸（太大），其余 `Decoding` 归坏文件。
fn classify_hdr(e: &ImageError) -> (i32, String) {
    match e {
        ImageError::IoError(_) => (ERR_CORRUPT, "HDR 数据不完整或已损坏".to_string()),
        ImageError::Unsupported(_) => (ERR_UNSUPPORTED, format!("HDR 特性不支持：{e}")),
        ImageError::Limits(_) => (ERR_TOO_LARGE, format!("HDR 尺寸超出范围：{e}")),
        _ => (ERR_CORRUPT, format!("HDR 解码失败：{e}")),
    }
}

/// 线性 float → u8。
///
/// **这一行就是 51.2 dB 口径的全体**：`clamp(v,0,1)` 后 `*255` 四舍五入。
/// 改成截断（`as u8` 少了 round）或套 gamma 都会掉 PSNR——bench 量过
/// 8bit 路 13.4 dB。NaN / ≥ 1.0 → 1.0（高光不炸）。
#[inline]
fn hdr_to_u8(v: f32) -> u8 {
    let clamped = if v.is_nan() || v >= 1.0 {
        1.0
    } else {
        v.max(0.0)
    };
    (clamped * 255.0).round() as u8
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    use rupix_abi as kit;

    // ---- 夹具构造 --------------------------------------------------------

    /// 手工构造最小合法 Radiance HDR（旧式 flat RGBE，非 RLE 扫描线）：
    /// 2x2 纯红 (1.0, 0, 0)。
    ///
    /// RGBE → 线性 float 的公式是 `v = m * 2^(e-136)`：要精确 1.0 须
    /// m=128, e=129（128·2^-7）。m=128/e=128 只能到 0.5——stb 时代第一版
    /// 测试值在这里错过一次，别再错。
    fn hdr_red_2x2() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.extend_from_slice(b"FORMAT=32-bit_rle_rgbe\n");
        v.push(b'\n'); // 头部结束空行
        v.extend_from_slice(b"-Y 2 +X 2\n");
        // width=2 < 8：flat 路，每像素 4 字节裸 RGBE。
        for _ in 0..4 {
            v.extend_from_slice(&[128, 0, 0, 129]); // → (1.0, 0, 0)
        }
        v
    }

    /// 老式 `#?RGBE` 签名：image 严查模式不认，必须走非严查构造。
    fn hdr_red_2x2_old_sig() -> Vec<u8> {
        let mut v = hdr_red_2x2();
        v[..HDR_SIG_RGBE.len()].copy_from_slice(HDR_SIG_RGBE);
        v
    }

    /// 2x2，四个像素线性值分别是 0.0 / 0.5 / 1.0 / 2.0（最后一个被钳到 255）。
    fn hdr_gradient_2x2() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.extend_from_slice(b"FORMAT=32-bit_rle_rgbe\n");
        v.push(b'\n');
        v.extend_from_slice(b"-Y 2 +X 2\n");
        // RGBE：v = m·2^(e-136)。0.0 → (0,0,0,0)；0.5 → (128,0,0,128)；
        // 1.0 → (128,0,0,129)；2.0 → (128,0,0,130)。
        for e in [0u8, 128, 129, 130] {
            v.extend_from_slice(&[128, 128, 128, e]);
        }
        v
    }

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

    fn render_to_buf(data: &[u8]) -> (Vec<u8>, u32, u32) {
        let mut info = IvRenderOut::default();
        let c = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK, "查询阶段失败");
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK, "渲染阶段失败");
        (buf, info.actual_w, info.actual_h)
    }

    fn render_code(data: &[u8]) -> i32 {
        let mut info = IvRenderOut::default();
        // 必须给真缓冲：out 为空指针时 render 在查询阶段就返回，
        // "probe 认领但数据坏"的用例会永远拿不到 CORRUPT。
        let mut buf = vec![0u8; 1 << 20];
        iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        )
    }

    fn last_err() -> String {
        let mut e = [0i8; 256];
        let need = unsafe { kit::write_last_error(e.as_mut_ptr(), e.len()) } as usize;
        let n = need.saturating_sub(1).min(e.len());
        String::from_utf8_lossy(&e[..n].iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }

    // ---- 布局与认领 -------------------------------------------------------

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 5);
    }

    #[test]
    fn probe_claims_both_radiance_signatures() {
        let hdr = hdr_red_2x2();
        assert_eq!(iv_probe(hdr.as_ptr(), hdr.len(), std::ptr::null()), 100);
        let old = hdr_red_2x2_old_sig();
        assert_eq!(iv_probe(old.as_ptr(), old.len(), std::ptr::null()), 100);
    }

    #[test]
    fn probe_rejects_lookalikes() {
        assert_eq!(iv_probe(b"nope!!!".as_ptr(), 8, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(iv_probe(b"#?RAD".as_ptr(), 5, std::ptr::null()), 0);
        // 别的格式的字节不许被误认
        assert_eq!(
            iv_probe(b"\x89PNG\r\n\x1a\n".as_ptr(), 8, std::ptr::null()),
            0
        );
        assert_eq!(iv_probe(b"8BPS\x00\x01".as_ptr(), 6, std::ptr::null()), 0);
    }

    // ---- 解码 -------------------------------------------------------------

    #[test]
    fn decodes_hdr_red_2x2_via_float_path() {
        let (buf, w, h) = render_to_buf(&hdr_red_2x2());
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            &buf[..],
            &[
                255, 0, 0, 255, //
                255, 0, 0, 255, //
                255, 0, 0, 255, //
                255, 0, 0, 255,
            ]
        );
    }

    /// 老式 `#?RGBE` 签名走非严查构造，结果必须与严查路逐字节相同。
    #[test]
    fn old_signature_decodes_the_same_as_strict() {
        let (a, _, _) = render_to_buf(&hdr_red_2x2());
        let (b, _, _) = render_to_buf(&hdr_red_2x2_old_sig());
        assert_eq!(a, b);
    }

    /// 归一化口径：0.0 → 0、0.5 → 128（四舍五入）、1.0 → 255、2.0 → 255（钳住）。
    /// 这一条钉住 f32→u8 的全部语义——改了 round / clamp 就会红。
    #[test]
    fn normalization_clamps_and_rounds() {
        let (buf, _, _) = render_to_buf(&hdr_gradient_2x2());
        assert_eq!(&buf[0..4], &[0, 0, 0, 255], "0.0 → 0");
        assert_eq!(&buf[4..8], &[128, 128, 128, 255], "0.5 → 四舍五入到 128");
        assert_eq!(&buf[8..12], &[255, 255, 255, 255], "1.0 → 255");
        assert_eq!(&buf[12..16], &[255, 255, 255, 255], "2.0 被钳到 255");
    }

    /// 新版逐分量 RLE 扫描线（`[2,2,<128,e]` 头）：image 的主路径，
    /// 与 flat 路必须给同一答案。
    #[test]
    fn new_rle_scanlines_decode_the_same_as_flat() {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.extend_from_slice(b"FORMAT=32-bit_rle_rgbe\n");
        v.push(b'\n');
        v.extend_from_slice(b"-Y 2 +X 2\n");
        // 每行：先 4 字节扫描线头 `[2,2,hi,lo]`（width=2），再按 R/G/B/E
        // 四个分量各写一段。段头 ≤128 表示“字面量个数”，其后就是原文；
        // >128 表示“重复次数 = 段头-128”，其后只有 1 个字节。
        // 这里 2 个像素同值 (128,0,0,129)，四个分量都用 "2 个字面量"。
        for _ in 0..2 {
            v.extend_from_slice(&[2, 2, 0, 2]); // 扫描线头：width = 2
            v.extend_from_slice(&[2, 128, 128]); // R
            v.extend_from_slice(&[2, 0, 0]); // G
            v.extend_from_slice(&[2, 0, 0]); // B
            v.extend_from_slice(&[2, 129, 129]); // E
        }
        let (buf, w, h) = render_to_buf(&v);
        assert_eq!((w, h), (2, 2));
        assert!(buf.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    // ---- 坏文件与能力边界 -------------------------------------------------

    #[test]
    fn garbage_body_is_corrupt() {
        // 签名在但头体乱 → 头解析失败 → CORRUPT。
        let mut hdr = b"#?RADIANCE\nnot a resolution line\n".to_vec();
        hdr.extend_from_slice(&[0u8; 32]);
        assert_eq!(iv_probe(hdr.as_ptr(), hdr.len(), std::ptr::null()), 100);
        assert_eq!(render_code(&hdr), ERR_CORRUPT);
    }

    /// 截在头部中间：EOF 在头里 → CORRUPT。
    #[test]
    fn truncated_header_is_corrupt() {
        let hdr = hdr_red_2x2();
        let cut = &hdr[..hdr.len() - 20];
        assert_eq!(iv_probe(cut.as_ptr(), cut.len(), std::ptr::null()), 100);
        assert_eq!(render_code(cut), ERR_CORRUPT);
    }

    /// 像素数据被截掉：flat 扫描线读不满 → I/O 错 → CORRUPT。
    #[test]
    fn truncated_pixels_is_corrupt() {
        let hdr = hdr_red_2x2();
        let cut = &hdr[..hdr.len() - 4];
        assert_eq!(iv_probe(cut.as_ptr(), cut.len(), std::ptr::null()), 100);
        assert_eq!(render_code(cut), ERR_CORRUPT);
    }

    /// 认得但用不了：非 `-Y/+X` 方向。image 只实现这一种方向，
    /// 必须报 UNSUPPORTED（"这格式我用不了"）而不是 CORRUPT（"文件坏了"）。
    #[test]
    fn unsupported_orientation_is_unsupported_not_corrupt() {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.push(b'\n');
        v.extend_from_slice(b"+Y 2 -X 2\n"); // 另一种合法 Radiance 方向
        v.extend_from_slice(&[0u8; 16]);
        assert_eq!(iv_probe(v.as_ptr(), v.len(), std::ptr::null()), 100);
        assert_eq!(render_code(&v), ERR_UNSUPPORTED);
    }

    /// 认得但用不了：`FORMAT=XYZE`。
    #[test]
    fn xyz_format_is_unsupported() {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.extend_from_slice(b"FORMAT=XYZE\n");
        v.push(b'\n');
        v.extend_from_slice(b"-Y 2 +X 2\n");
        v.extend_from_slice(&[0u8; 16]);
        assert_eq!(render_code(&v), ERR_UNSUPPORTED);
    }

    /// 头撒谎的巨大尺寸必须在**分配之前**被 MAX_PIXELS 挡住。
    #[test]
    fn huge_dimensions_rejected_before_alloc() {
        let mut v = Vec::new();
        v.extend_from_slice(HDR_SIG_RADIANCE);
        v.push(b'\n');
        v.extend_from_slice(b"-Y 60000 +X 60000\n"); // 3.6 GPixel
        v.extend_from_slice(&[0u8; 16]);
        let mut info = IvRenderOut::default();
        let r = iv_render(
            v.as_ptr(),
            v.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_TOO_LARGE, "畸形尺寸必须被上限挡住，实际 {}", r);
    }

    #[test]
    fn unrecognized_bytes_never_reach_decode() {
        let bad = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b";
        assert_eq!(iv_probe(bad.as_ptr(), bad.len(), std::ptr::null()), 0);
        assert_eq!(render_code(bad), ERR_UNRECOGNIZED);
    }

    // ---- 两阶段协议 -------------------------------------------------------

    #[test]
    fn two_phase_is_consistent() {
        let hdr = hdr_red_2x2();
        let mut info = IvRenderOut::default();
        let c = iv_render(
            hdr.as_ptr(),
            hdr.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (2, 2));
        assert_eq!(info.bytes_needed, 2 * 2 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        assert_eq!((info.orientation, info.already_oriented), (1, 1));
        assert_eq!(info.frame_count, 1);
        assert_eq!(info.page_count, 1);
        assert_eq!(info.icc_len, 0);
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            hdr.as_ptr(),
            hdr.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        let mut ebuf = [0i8; 16];
        let need = unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        assert_eq!(need, 1, "last error 应为空串");
    }

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let hdr = hdr_red_2x2();
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 8];
        let r = iv_render(
            hdr.as_ptr(),
            hdr.len(),
            &req(),
            tiny.as_mut_ptr(),
            tiny.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_BUFFER_TOO_SMALL);
    }

    #[test]
    fn budget_exceeded_is_too_large() {
        let hdr = hdr_red_2x2();
        let r = IvRenderReq {
            budget_bytes: 8, // 2x2x4 = 16 > 8
            ..req()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                hdr.as_ptr(),
                hdr.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_TOO_LARGE
        );
    }

    #[test]
    fn last_error_is_set_on_failure() {
        let code = render_code(b"\x00\x01\x02\x03");
        assert_eq!(code, ERR_UNRECOGNIZED);
        let msg = last_err();
        assert!(!msg.is_empty(), "失败路径应写入非空 last error");
        assert!(
            !msg.starts_with("hdr: "),
            "规范 §六 第 11 条：不得自带解码器名前缀，实际「{msg}」"
        );
    }
}
