//! RuPix 外挂解码插件：TIFF → RGBA8（库：`tiff` crate，直调）。
//!
//! # 为什么是这个库（G1 实测，不是"全网都说"）
//!
//! 2026-10-15 本机实测（`.plan/bench/`，run r1792006432，n=200 取 median，同一 run 内配对，
//! 两种内容 snowflake / photo × 三档尺寸 S/M/L）：
//!
//! | 配置 | 合计 median ms | 相对第一名 |
//! |---|---|---|
//! | **`tiff` crate 直调（本插件路线）** | **27.00** | **1.00×** |
//! | `image`（core 旧路线） | 33.51 | 1.24× |
//! | WIC 基线 | 63.55 | 2.35× |
//!
//! 结论与 JPEG 一致：**快的不是换库，是换调用方式**。`image` 的 TIFF 适配层把
//! `DecodingResult` 先填进自己的 `ImageBuffer`，宿主再 `into_rgba8()` 搬一次；直调只留
//! 一次转换。libtiff（C）本机没测过，所以不宣称它更快——未实测的候选不进 roster。
//! WIC 更快的是 GIF，不是 TIFF；TIFF 上 WIC 慢 2.35×。
//!
//! # 能力边界（超出即 `ERR_UNSUPPORTED`，由 core 以 50 置信度兜底）
//!
//! 本插件只接**逐像素映射有把握**的布局：Gray / GrayA / RGB / RGBA，8 或 16 bit，
//! 连续采样（PlanarConfiguration=1）。Palette / CMYK / CMYKA / YCbCr / Multiband /
//! 浮点 / 位打包（1·4 bit）/ 分平面一律返回 `ERR_UNSUPPORTED`，并把 last-error 写成
//! 具体原因。宿主看到 `ERR_UNSUPPORTED` 会走嗅探兜底（`plugins::Host::try_decode`），
//! 于是 `core` 的 `image` 路线接手——**冠军提速不以功能倒退为代价**，这条交接规则
//! 见本仓库 `docs/插件开发规范.md`「置信度分层」。
//!
//! # 原生降采样（m36，`CAP_SCALED`）
//!
//! 未压缩（Compression=1）+ 连续采样 + 8bit + 条带式（非瓦片）的 TIFF，**字节就是像素
//! 本身**：给定行号就能算出那一段字节。于是宿主点亮 `REQ_ALLOW_SCALED` 时按 `step` 行
//! 读 1 行、行内 `step` 列面积平均，直接写进宿主缓冲——105 MiB 的 6055² RGB 首屏只读
//! 约 1/4 的字节。这与 JPEG 的 DCT 缩放是**两种不同性质**的收益：JPEG 省不掉 Huffman
//! 熵解码（所以 m35 加了码率闸），而未压缩 TIFF 的成本就在读字节上，跳过行等于跳过解压。
//!
//! 同一张行寻址表也让**整图直写**受益（`step == 1`）：省掉库那边"3 通道 Vec → 4 通道
//! Vec → memcpy 进宿主缓冲"的三趟中转。压缩 / 瓦片 / 分平面 / 16bit / 反相一律回落到
//! 今天那条逐字节不变的库路径——`CAP_SCALED` 因此在少数文件上是"点了但白整解一次"，
//! 不是错图（规范 §十三 对谎报的判据是请求能否出画）。
//!
//! ABI 契约来自 [`rupix_abi`]，panic gate / last-error / 固定导出由
//! [`rupix_abi::export_plugin!`] 生成。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立插件形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_plugin, guard};
use std::ffi::c_char;
use tiff::decoder::DecodingResult;
use tiff::tags::Tag;
use tiff::{ColorType, TiffError};

#[cfg(feature = "standalone")]
export_plugin! {
    id: "tiff",
    name: "TIFF (tiff crate)",
    extensions: "tif;tiff",
    description: "解码 TIFF（RGB/RGBA/Gray 8/16bit 连续采样），tiff crate 直出 RGBA8",
    is_vector: 0,
    has_alpha: 1,
}

// 能力位图（规范 §十三）。`CAP_SCALED` **只在未压缩条带那种文件上真会降采样**：
// 压缩（LZW/Deflate/PackBits）、瓦片、分平面、16bit 一律回整解——那是"白付一次整解"，
// 不是错图，位图上不算谎报（谎报的判据是宿主据此发出的请求能不能出画）。
#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_SCALED);

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。与 core / jpeg 同口径。
/// TIFF 的 `read_image()` 会自己按尺寸分配，所以这道检查必须在调用**之前**。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// iv_probe / iv_render
// ---------------------------------------------------------------------------

/// TIFF 魔数：`II` + 42（小端）或 `MM` + 42（大端）；BigTIFF 的版本号是 43。
/// 四种组合都不可能有别的含义，认领是安全的。
fn is_tiff_magic(s: &[u8]) -> bool {
    if s.len() < 4 {
        return false;
    }
    matches!(
        (s[0], s[1], s[2], s[3]),
        (0x49, 0x49, 0x2A, 0x00)
            | (0x4D, 0x4D, 0x00, 0x2A)
            | (0x49, 0x49, 0x2B, 0x00)
            | (0x4D, 0x4D, 0x00, 0x2B)
    )
}

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
        if data.is_null() || len < 8 {
            return 0;
        }
        let s = unsafe { std::slice::from_raw_parts(data, len) };
        if is_tiff_magic(s) {
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
        if !is_tiff_magic(input) {
            set_last_error("TIFF 魔数不对（应为 II/MM + 42）");
            return ERR_UNRECOGNIZED;
        }

        let page = if req.is_null() {
            0
        } else {
            unsafe { (*req).page_index }
        };

        // ── 阶段 1：只读 IFD 拿尺寸与布局（不解采样数据）。────────────────
        let shape = match read_shape(input, page) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };

        let npixels = shape.w as u64 * shape.h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的插件上限");
            return ERR_TOO_LARGE;
        }
        let r: IvRenderReq = if req.is_null() {
            IvRenderReq::default()
        } else {
            unsafe { *req }
        };
        // 出哪一档、走哪条腿。查询与渲染两阶段各算一次，必须算出同一个答案——
        // 宿主按查询申报的 `bytes_needed` 一次性分配，渲染就得正好写那么多字节。
        let plan = plan(&shape, &r, npixels);
        let bytes_needed = plan.w as u64 * plan.h as u64 * 4;
        // ★ 方向：整个 TIFF 文件就是一张 EXIF 块（II/MM + 42 + IFD0），所以 kit 那个
        //   只读 IFD0 的函数在这里正好逐字适用；本插件只解首页（`read_shape` 对
        //   page != 0 直接 ERR_UNSUPPORTED），因此"IFD0 的方向"与"这张页的方向"是同一件事。
        //   读到就照实报 + `already_oriented = 0` 交宿主补转（m48）；读不到报 1。
        let orientation = rupix_abi::exif_orientation(input).unwrap_or(1);
        let icc_len = shape.icc.len() as u64;

        if r.budget_bytes != 0 && bytes_needed > r.budget_bytes as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }

        if out.is_null() {
            fill_info(
                info,
                plan.w,
                plan.h,
                bytes_needed,
                orientation,
                icc_len as u32,
            );
            return ERR_OK;
        }
        if (out_cap as u64) < bytes_needed + icc_len {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        // ── 阶段 2：真解码。字节就是像素的文件**直写宿主缓冲**（m36）。─────
        if let (Some(f), Some(st)) = (plan.fast, shape.strips.as_ref()) {
            if let Err(msg) = render_strips_into(input, st, f, &shape, out, &plan) {
                set_last_error(&msg);
                return ERR_CORRUPT;
            }
            copy_icc(out, bytes_needed, &shape.icc);
            fill_info(
                info,
                plan.w,
                plan.h,
                bytes_needed,
                orientation,
                icc_len as u32,
            );
            return ERR_OK;
        }

        let rgba = match decode_rgba(input, &shape) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };
        if rgba.len() as u64 != bytes_needed {
            set_last_error("像素数与文件头不符");
            return ERR_CORRUPT;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
        }
        copy_icc(out, bytes_needed, &shape.icc);
        fill_info(
            info,
            plan.w,
            plan.h,
            bytes_needed,
            orientation,
            icc_len as u32,
        );
        ERR_OK
    })
}
/// 把 ICC 段写到像素段之后（宿主按 `bytes_needed + icc_len` 分配）。
fn copy_icc(out: *mut u8, bytes_needed: u64, icc: &[u8]) {
    if !icc.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(icc.as_ptr(), out.add(bytes_needed as usize), icc.len());
        }
    }
}

/// 这一次请求出多大一档、走哪条腿。
struct Plan {
    w: u32,
    h: u32,
    fast: Option<Fast>,
}

/// 直写腿的参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fast {
    /// 降采样比：`1` = 不缩放（整图直写，只省掉库那两次整图中转）。
    step: u32,
    /// `true` = 垂直方向每 `step` 行**只读 1 行**（取样，省字节）；`false` = 那些行
    /// 全读进来做真面积平均（不省字节，但和"整解后再缩小"逐像素等价）。
    sample: bool,
}

/// 只有**小格子**才允许跳行取样：显示框长边不超过这个数时，一格本来就没几个像素，
/// 颗粒感看不出来，而省下的字节是数量级的（240×180 缩略图实测 50×）。主画面超过它
/// 一律走真平均——未压缩 TIFF 的取样在**高频内容**上是看得出来的（实测：噪声图
/// 1/11 档与真平均的逐像素 MAD = 17/255，照片只有 2.4，所以判据是"给谁看"不是"多大"）。
const SAMPLE_BOX_MAX: u32 = 512;

fn plan(shape: &Shape, r: &IvRenderReq, npixels: u64) -> Plan {
    let plain = |step: u32| Plan {
        w: shape.w,
        h: shape.h,
        fast: Some(Fast {
            step,
            sample: false,
        }),
    };
    // 没有行寻址表就只会整解：档位申报也必须按原图尺寸来，不然查询报小、渲染给大，
    // 宿主按申报值分配的缓冲当场越界。
    if shape.strips.is_none() {
        return Plan {
            w: shape.w,
            h: shape.h,
            fast: None,
        };
    }
    let Some(step) = scaled_step(r, shape.w, shape.h).filter(|_| npixels >= MIN_SCALE_PIXELS)
    else {
        return plain(1);
    };
    let (ow, oh) = (shape.w.div_ceil(step), shape.h.div_ceil(step));
    if ow < MIN_OUT_EDGE || oh < MIN_OUT_EDGE {
        return plain(1);
    }
    Plan {
        w: ow,
        h: oh,
        fast: Some(Fast {
            step,
            sample: r.target_w.max(r.target_h) <= SAMPLE_BOX_MAX,
        }),
    }
}

/// 缩放步长。宿主点亮 [`REQ_ALLOW_SCALED`]、target 确实比原图小、且**缩完不会让宿主
/// 放大**才给 `Some(≥2)`，其余一律 `None`（= 整解，与今天逐字节相同）。
///
/// 取 `s = ⌊max(w/target_w, h/target_h)⌋`，而不是 jpeg 腿那种"两维都 ≥ target"的保守
/// 档：显示尺寸由 fit 的**短板维**决定，而 `max` 的那一维正是短板维，`s ≤ 短板比` 就
/// 保证缩后两维都还够得着显示尺寸（推导与验算见宿主 `.plan/m36`）。同一张 6055² 配
/// 2560×1440 窗口，这条给 1/4 档，"两边都 ≥"只给到 1/2。
fn scaled_step(r: &IvRenderReq, w: u32, h: u32) -> Option<u32> {
    if r.flags & REQ_ALLOW_SCALED == 0 || r.target_w == 0 || r.target_h == 0 {
        return None;
    }
    let (tw, th) = (r.target_w as u64, r.target_h as u64);
    if tw >= w as u64 && th >= h as u64 {
        return None;
    }
    let s = (w as u64 / tw).max(h as u64 / th);
    (s >= 2).then_some(s.min(u32::MAX as u64) as u32)
}

/// 直写宿主缓冲。两条腿共用行寻址，只差"那 `step` 行读不读全"：
///
/// - [`Fast::sample`] = `true`：每 `step` 行只读 1 行（垂直取样），行内 `step` 列平均。
///   省的是**字节**，代价是高频内容上的颗粒 / 锯齿——所以只给小格子（见
///   [`SAMPLE_BOX_MAX`] 那条注释里的实测 MAD）。
/// - `false`：`step × step` 块全读进来做真面积平均。不省字节，但结果与"整解再缩小"
///   等价，而且省掉了整解那三趟整图中转与宿主侧那次 `fit_to`（实测 6055² 上 384 → 130 ms）。
///
/// JPEG 的 DCT 缩放没这笔账：它的 1/8 档天生就是 8×8 块的均值（DC 系数），所以 m35
/// 那条腿不需要这个开关。
fn render_strips_into(
    input: &[u8],
    st: &Strips,
    f: Fast,
    shape: &Shape,
    out: *mut u8,
    plan: &Plan,
) -> Result<(), String> {
    let (ow, oh) = (plan.w as usize, plan.h as usize);
    let ch = shape.layout.channels;
    let need = ow * oh * 4;
    // 宿主已按 `bytes_needed` 验过容量，这里按申报尺寸精确切。
    let buf = unsafe { std::slice::from_raw_parts_mut(out, need) };
    let step = f.step as usize;
    if f.step == 1 {
        for j in 0..oh {
            let row = st
                .row(input, j as u64)
                .ok_or_else(|| "条带行号越界（文件与头里声明的对不上）".to_string())?;
            let o = j * ow * 4;
            write_row_plain(row, ch, &mut buf[o..o + ow * 4]);
        }
        return Ok(());
    }
    if f.sample {
        for j in 0..oh {
            let row = st
                .row(input, j as u64 * step as u64)
                .ok_or_else(|| "条带行号越界（文件与头里声明的对不上）".to_string())?;
            let o = j * ow * 4;
            write_row_boxed(row, ch, step, ow, &mut buf[o..o + ow * 4]);
        }
        return Ok(());
    }
    // 真面积平均：一个输出行 = `step` 个源行的块均值。累加器只有 `ow × ch` 个 u32
    // （6055² 的 1/11 档 = 6.6 KB），所以这条路**不整图驻留**，边读边收。
    let mut acc = vec![0u32; ow * ch];
    for j in 0..oh {
        let y0 = j as u64 * step as u64;
        let y1 = (y0 + step as u64).min(shape.h as u64);
        acc.iter_mut().for_each(|v| *v = 0);
        for r in y0..y1 {
            let row = st
                .row(input, r)
                .ok_or_else(|| "条带行号越界（文件与头里声明的对不上）".to_string())?;
            accumulate_row(row, ch, step, &mut acc);
        }
        let o = j * ow * 4;
        let dst = &mut buf[o..o + ow * 4];
        write_averaged_row(&acc, shape, j as u32, f.step, ow, dst);
    }
    Ok(())
}

/// 一格真实参与平均的像素数（边缘格不满 `step × step`）。
fn cell_pixels(w: u32, h: u32, i: u32, j: u32, step: u32) -> u32 {
    let cw = (w - i * step).min(step);
    let chh = (h - j * step).min(step);
    cw * chh
}

/// 真平均腿的一行输出：`acc`（`ow × ch` 个和）→ RGBA8。
///
/// 3/4 通道单独列出来只为消掉逐像素的运行时 `ch` 分支与 `put_px8` 调用；求和方式与
/// 取整式 `(sum + cell/2)/cell` 一字不改，边缘格仍按**真实样本数**除。
fn write_averaged_row(acc: &[u32], shape: &Shape, j: u32, step: u32, ow: usize, dst: &mut [u8]) {
    let (w, h, ch) = (shape.w, shape.h, shape.layout.channels);
    let chh = (h - j * step).min(step);
    match ch {
        3 => {
            for i in 0..ow {
                let cell = (w - i as u32 * step).min(step) * chh;
                let half = cell / 2;
                let a = &acc[i * 3..i * 3 + 3];
                let d = &mut dst[i * 4..i * 4 + 4];
                d[0] = ((a[0] + half) / cell) as u8;
                d[1] = ((a[1] + half) / cell) as u8;
                d[2] = ((a[2] + half) / cell) as u8;
                d[3] = 255;
            }
        }
        4 => {
            for i in 0..ow {
                let cell = (w - i as u32 * step).min(step) * chh;
                let half = cell / 2;
                let a = &acc[i * 4..i * 4 + 4];
                let d = &mut dst[i * 4..i * 4 + 4];
                d[0] = ((a[0] + half) / cell) as u8;
                d[1] = ((a[1] + half) / cell) as u8;
                d[2] = ((a[2] + half) / cell) as u8;
                d[3] = ((a[3] + half) / cell) as u8;
            }
        }
        _ => {
            for i in 0..ow {
                let cell = cell_pixels(w, h, i as u32, j, step);
                let half = cell / 2;
                let mut v = [0u8; 4];
                for (c, val) in v.iter_mut().enumerate().take(ch) {
                    *val = ((acc[i * ch + c] + half) / cell) as u8;
                }
                put_px8(&mut dst[i * 4..i * 4 + 4], &v[..ch], ch);
            }
        }
    }
}

/// 把一行里每 `step` 个像素的和累加进 `acc`（`acc.len() == ow * ch`）。
///
/// RGB / RGBA 是本文件最常见的两种布局（且行宽是通道数的整数倍），单列出来让三个/四个
/// 累加链各自独立，编译器才有机会向量化；其余通道走同一套通用写法。
fn accumulate_row(row: &[u8], ch: usize, step: usize, acc: &mut [u32]) {
    match ch {
        3 => {
            let wpx = row.len() / 3;
            for (i, cell) in acc.chunks_exact_mut(3).enumerate() {
                let x0 = i * step;
                if x0 >= wpx {
                    break;
                }
                let x1 = (x0 + step).min(wpx);
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for p in row[x0 * 3..x1 * 3].chunks_exact(3) {
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                }
                cell[0] += r;
                cell[1] += g;
                cell[2] += b;
            }
        }
        4 => {
            let wpx = row.len() / 4;
            for (i, cell) in acc.chunks_exact_mut(4).enumerate() {
                let x0 = i * step;
                if x0 >= wpx {
                    break;
                }
                let x1 = (x0 + step).min(wpx);
                let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
                for p in row[x0 * 4..x1 * 4].chunks_exact(4) {
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    a += p[3] as u32;
                }
                cell[0] += r;
                cell[1] += g;
                cell[2] += b;
                cell[3] += a;
            }
        }
        _ => {
            let wpx = row.len() / ch;
            for (i, cell) in acc.chunks_mut(ch).enumerate() {
                let x0 = i * step;
                if x0 >= wpx {
                    break;
                }
                let x1 = (x0 + step).min(wpx);
                for p in row[x0 * ch..x1 * ch].chunks_exact(ch) {
                    for (c, v) in cell.iter_mut().enumerate() {
                        *v += p[c] as u32;
                    }
                }
            }
        }
    }
}

/// 整行原样展开成 RGBA8（`step == 1`）。RGBA 输入仍是**同布局整行拷贝**。
fn write_row_plain(row: &[u8], ch: usize, dst: &mut [u8]) {
    match ch {
        4 => dst.copy_from_slice(row),
        3 => {
            for (src, d) in row.chunks_exact(3).zip(dst.chunks_exact_mut(4)) {
                d[0] = src[0];
                d[1] = src[1];
                d[2] = src[2];
                d[3] = 255;
            }
        }
        1 => {
            for (s, d) in row.iter().zip(dst.chunks_exact_mut(4)) {
                d[0] = *s;
                d[1] = *s;
                d[2] = *s;
                d[3] = 255;
            }
        }
        _ => {
            for (i, src) in row.chunks_exact(ch).enumerate() {
                let o = i * 4;
                put_px8(&mut dst[o..o + 4], src, ch);
            }
        }
    }
}

/// 一行里每 `step` 个像素平均成一个输出像素（水平方向是真面积平均）。
fn write_row_boxed(row: &[u8], ch: usize, step: usize, ow: usize, dst: &mut [u8]) {
    let wpx = row.len() / ch;
    for i in 0..ow {
        let x0 = i * step;
        let x1 = (x0 + step).min(wpx);
        let n = (x1 - x0) as u32;
        let mut sum = [0u32; 4];
        for p in row[x0 * ch..x1 * ch].chunks_exact(ch) {
            for (c, v) in sum.iter_mut().enumerate().take(ch) {
                *v += p[c] as u32;
            }
        }
        let half = n / 2;
        let mut v = [0u8; 4];
        for (c, val) in v.iter_mut().enumerate().take(ch) {
            *val = ((sum[c] + half) / n) as u8;
        }
        let o = i * 4;
        put_px8(&mut dst[o..o + 4], &v[..ch], ch);
    }
}

#[inline]
fn fill_info(
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    bytes_needed: u64,
    orientation: i32,
    icc_len: u32,
) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    // 16bit 源在这里降到 8bit（与 core / `image` 的 16→8 取高字节一致），所以恒报 8。
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = orientation;
    // 本插件从不摆正像素（tiff crate 与 m36 的直写快路径出的都是文件里的原始行序），
    // 所以老实报 0 让宿主补转。旧注释"声明已定向以免宿主二次旋转"站不住：二次旋转只在
    // "转了却不声明"时发生，如实回报才是契约给的路（`av` / `raw` 一直是这么写的）。
    oi.already_oriented = 0;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    // 只出首页：本插件不实现多页导航（宿主今天也一律发 page_index=0）。
    oi.page_count = 1;
    oi.icc_len = icc_len;
}

// ---------------------------------------------------------------------------
// tiff crate 薄封装
// ---------------------------------------------------------------------------

/// 首页的形状：尺寸 + 已确认可映射的像素布局 + （能按行寻址时）未压缩条带表。
#[derive(Clone)]
struct Shape {
    w: u32,
    h: u32,
    layout: Layout,
    /// `Some` = 这文件的字节**就是**像素本身（未压缩、连续采样、8bit、条带式），
    /// 给定行号就能定位到那一段字节。m36 的跨行取样与"直写宿主缓冲"都靠它；
    /// 任何一条不成立就是 `None`，那条路逐字节等同今天（库整解 + 宿主补齐）。
    strips: Option<Strips>,
    /// IFD0 的 `IccProfile`(34675)：原样透传给宿主做色彩管理（超限整份丢弃）。
    icc: Vec<u8>,
}

/// 未压缩条带的行寻址表：`行 r 的起始字节 = offsets[r / rows] + (r % rows) * row_bytes`。
#[derive(Clone)]
struct Strips {
    /// 一行的字节数 = `w * channels`（连续采样 + 8bit，故无行内对齐填充）。
    row_bytes: u64,
    /// 每条带行数。TIFF 规格：没有 278 标签时整图算一条带。
    rows: u64,
    offsets: Vec<u64>,
}

impl Strips {
    /// 取第 `r` 行的字节。越界一律 `None`——不 panic，让调用方老实报错。
    fn row<'a>(&self, input: &'a [u8], r: u64) -> Option<&'a [u8]> {
        let k = r / self.rows;
        let base = *self.offsets.get(usize::try_from(k).ok()?)?;
        let off = base.checked_add((r % self.rows).checked_mul(self.row_bytes)?)?;
        let end = off.checked_add(self.row_bytes)?;
        if end > input.len() as u64 {
            return None;
        }
        input.get(off as usize..end as usize)
    }
}

/// 条带数上界：上界取自主机侧的条带**推算值**（由声明尺寸与 RowsPerStrip 算出），
/// 不是文件里那个可以自己填的计数（规范 §六 第 10 条：轮数上界来自长度而非内容）。
const MAX_STRIPS: u64 = 100_000;

/// 低于这个像素数不缩放：整解本就在毫秒级以下，跨行取样省下的字节抵不上页对齐的
/// 零头，而档位取粗了的代价是直接少给首屏像素（与 jpeg 腿的 `GATE_MIN_PIXELS` 同义）。
const MIN_SCALE_PIXELS: u64 = 1_000_000;

/// 缩后单边下限：低于它就宁可整解。防的是"极扁 / 极窄"这种长条图——按比例缩完只剩
/// 几个像素，宿主得放大几十倍才填满窗口，那不是加速是毁图。
const MIN_OUT_EDGE: u32 = 64;

/// 本插件认识的布局：通道数 + 每通道位宽（8 或 16）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Layout {
    channels: usize,
    wide: bool,
}

/// 色彩类型 → 布局。返回 `None` = 不接，交回 core 兜底（映射没把握就不抢，
/// 抢了猜错会把图解成一屏错色，那比"打不开"更糟）。
fn layout_of(ct: ColorType) -> Option<Layout> {
    let (channels, bits) = match ct {
        ColorType::Gray(b) => (1usize, b),
        ColorType::GrayA(b) => (2, b),
        ColorType::RGB(b) => (3, b),
        ColorType::RGBA(b) => (4, b),
        // Palette / CMYK / CMYKA / YCbCr / Multiband / 其它：不接。
        _ => return None,
    };
    match bits {
        8 => Some(Layout {
            channels,
            wide: false,
        }),
        16 => Some(Layout {
            channels,
            wide: true,
        }),
        // 1/4 bit 位打包、32 bit 浮点：不接（`image` 侧对这些另有分支）。
        _ => None,
    }
}

fn dec_err(e: TiffError) -> (i32, String) {
    match e {
        TiffError::UnsupportedError(inner) => (
            ERR_UNSUPPORTED,
            format!("该 TIFF 子格式不支持，库报错 \"{inner}\""),
        ),
        TiffError::LimitsExceeded => (ERR_TOO_LARGE, "解码器内部上限被超出".into()),
        TiffError::IntSizeError => (ERR_TOO_LARGE, "尺寸计算溢出".into()),
        TiffError::IoError(inner) => (ERR_CORRUPT, format!("TIFF 读不了，库报错 \"{inner}\"")),
        TiffError::FormatError(inner) => {
            (ERR_CORRUPT, format!("TIFF 格式错误，库报错 \"{inner}\""))
        }
        TiffError::UsageError(inner) => (
            ERR_CORRUPT,
            format!("内部错误（调用方式不符），库报错 \"{inner}\""),
        ),
    }
}

/// 只读 IFD：拿尺寸、确认可接的布局与连续采样。`decode_headers` 级开销，微秒。
fn read_shape(data: &[u8], page: u32) -> Result<Shape, (i32, String)> {
    let mut dec = tiff::decoder::Decoder::new(std::io::Cursor::new(data)).map_err(dec_err)?;
    if page != 0 {
        return Err((
            ERR_UNSUPPORTED,
            format!("本插件只解第一页，不支持请求第 {page} 页"),
        ));
    }
    let (w, h) = dec.dimensions().map_err(dec_err)?;
    if w == 0 || h == 0 {
        return Err((ERR_CORRUPT, "文件头声明的宽或高为 0".into()));
    }
    let layout = layout_of(dec.colortype().map_err(dec_err)?).ok_or((
        ERR_UNSUPPORTED,
        "颜色布局超出本插件支持范围，交回 core".to_string(),
    ))?;
    // 分平面（PlanarConfiguration=2）：样本按通道分成整幅平面，拼接顺序与逐像素
    // 不同。`image` 为此写了 interleave 分支；本插件不复制那套逻辑，直接让路。
    let planar = dec
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .ok()
        .flatten()
        .unwrap_or(1);
    if planar != 1 {
        return Err((ERR_UNSUPPORTED, format!("不支持的平面配置 {planar}")));
    }
    let strips = strip_table(&mut dec, data, w, h, layout);
    // ICC 是 IFD0 的 UNDEFINED/BYTE 数组；库把两种类型都映射成字节序列。
    // 读不到 / 超上限 ⇒ 空（超限整份丢弃，不截断）。
    let icc = dec
        .find_tag_unsigned_vec::<u8>(Tag::IccProfile)
        .ok()
        .flatten()
        .filter(|v| v.len() <= rupix_abi::ICC_MAX)
        .unwrap_or_default();
    Ok(Shape {
        w,
        h,
        layout,
        strips,
        icc,
    })
}

/// 这文件的字节能不能按行直接寻址（m36 快路径的前提）。逐条把关，任何一条不过就
/// 返回 `None` 交回库路径——畸形文件自有库去如实拒绝，这里不重复那套判断，只做
/// "我有把握按字节算"这一件事。
fn strip_table<R: std::io::Read + std::io::Seek>(
    dec: &mut tiff::decoder::Decoder<R>,
    input: &[u8],
    w: u32,
    h: u32,
    layout: Layout,
) -> Option<Strips> {
    // 压缩：跳字节的前提根本不存在（LZW / Deflate / PackBits 的后一行依赖前一行）。
    if dec.find_tag_unsigned::<u16>(Tag::Compression).ok()? != Some(1) {
        return None;
    }
    // 预测器只作用于压缩样本；未压缩却写了预测器 = 两本账对不上，不猜。
    if dec
        .find_tag_unsigned::<u16>(Tag::Predictor)
        .ok()
        .flatten()
        .unwrap_or(1)
        != 1
    {
        return None;
    }
    // 瓦片（TileWidth）是二维寻址、比条带更省，但那是另一套路子，这轮不接。
    if dec
        .find_tag_unsigned::<u32>(Tag::TileWidth)
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    // 样本格式必须是无符号；位宽必须**标签里写明**且一律 8（缺省值是 1bit，不能当 8 用）。
    if dec
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .ok()?
        .map(|v| v.iter().any(|&s| s != 1))
        .unwrap_or(false)
    {
        return None;
    }
    let bits = dec.find_tag_unsigned_vec::<u16>(Tag::BitsPerSample).ok()?;
    if bits.as_deref().unwrap_or(&[]).is_empty() || bits.as_deref().unwrap().iter().any(|&b| b != 8)
    {
        return None;
    }
    // 反相（Photometric=0 WhiteIsZero）由库在 `read_image()` 里做，这条直写路不复制
    // 它的口径；1 = BlackIsZero（灰度正向）、2 = RGB 才是字节即像素。
    let photo = dec
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .ok()??;
    let gray = matches!(layout.channels, 1 | 2);
    if photo != if gray { 1 } else { 2 } {
        return None;
    }
    // 行宽按**样本数**（277）算，而映射按库报的通道数走：两者必须相等，否则一行字节
    // 不是 `w × channels` 个样本，按这个行宽切 would 错位成错色。
    // （`RGBA` 有两条来路：4 样本直接就是，或 3 样本 + 附加 alpha——库两种都报 RGBA，
    // 所以这里只看 277，不去猜 338 的写法。）
    let samples = dec
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .ok()?
        .unwrap_or(1) as usize;
    if samples != layout.channels {
        return None;
    }
    // 条带表：行数上界由声明尺寸推算（不信文件里的计数），越界或跨度出文件即让路。
    let rows = dec
        .find_tag_unsigned::<u64>(Tag::RowsPerStrip)
        .ok()?
        .unwrap_or(u32::MAX as u64);
    if rows == 0 {
        return None;
    }
    let need = (h as u64).div_ceil(rows);
    if need == 0 || need > MAX_STRIPS {
        return None;
    }
    let offsets = dec.find_tag_unsigned_vec::<u64>(Tag::StripOffsets).ok()?;
    let counts = dec
        .find_tag_unsigned_vec::<u64>(Tag::StripByteCounts)
        .ok()?;
    let (offsets, counts) = (offsets?, counts?);
    if offsets.len() as u64 != need || counts.len() as u64 != need {
        return None;
    }
    let row_bytes = w as u64 * layout.channels as u64;
    if row_bytes == 0 || row_bytes > input.len() as u64 {
        return None;
    }
    for k in 0..need as usize {
        let kr = k as u64 * rows;
        let span = rows.min(h as u64 - kr).checked_mul(row_bytes)?;
        if counts[k] < span || offsets[k].checked_add(span)? > input.len() as u64 {
            return None;
        }
    }
    Some(Strips {
        row_bytes,
        rows,
        offsets,
    })
}

/// 解码成 RGBA8（行主序，长度 `w*h*4`）。
fn decode_rgba(data: &[u8], shape: &Shape) -> Result<Vec<u8>, (i32, String)> {
    let mut dec = tiff::decoder::Decoder::new(std::io::Cursor::new(data)).map_err(dec_err)?;
    // 先取原始样本，再一次成型。库里已解出压缩（LZW / Deflate / PackBits…）。
    let (w, h) = dec.dimensions().map_err(dec_err)?;
    if (w, h) != (shape.w, shape.h) {
        return Err((ERR_CORRUPT, "两次遍历之间尺寸变了".into()));
    }
    let ct = dec.colortype().map_err(dec_err)?;
    if layout_of(ct) != Some(shape.layout) {
        return Err((ERR_CORRUPT, "两次遍历之间颜色类型变了".into()));
    }
    match dec.read_image().map_err(dec_err)? {
        DecodingResult::U8(px) => Ok(to_rgba8(&px, shape.layout)),
        DecodingResult::U16(px) => Ok(to_rgba8_16(&px, shape.layout)),
        // 有符号 / 更宽 / 浮点样本：`layout_of` 只放行 8·16bit 无符号，正常走不到这里；
        // 真到了（例如 SampleFormat 与 BitsPerSample 自相矛盾的畸形文件）就老实让路，
        // 由 core 的 `image` 路线接手——它对这些分支有处理。
        other => Err((
            ERR_UNSUPPORTED,
            format!("不支持的采样类型 {other:?}，交回 core"),
        )),
    }
}

/// 单样本(8bit) → RGBA8 的**唯一一张**映射表。两条腿共用：快路径与库路径的映射要是
/// 不一致，同一张文件在"档位图"和"原图档"之间会**变色**——那种缺陷只有肉眼看得到。
#[inline]
fn put_px8(d: &mut [u8], s: &[u8], ch: usize) {
    match ch {
        1 => {
            d[0] = s[0];
            d[1] = s[0];
            d[2] = s[0];
            d[3] = 255;
        }
        2 => {
            d[0] = s[0];
            d[1] = s[0];
            d[2] = s[0];
            d[3] = s[1];
        }
        3 => {
            d[0] = s[0];
            d[1] = s[1];
            d[2] = s[2];
            d[3] = 255;
        }
        _ => d.copy_from_slice(&s[..4]),
    }
}

/// 8bit 样本 → RGBA8。RGBA 输入是**同布局拷贝**，不做逐像素转换（死法 3 的防线）。
fn to_rgba8(px: &[u8], layout: Layout) -> Vec<u8> {
    if layout.channels == 4 {
        return px.to_vec();
    }
    let mut out = vec![0u8; px.len() / layout.channels * 4];
    for (i, src) in px.chunks_exact(layout.channels).enumerate() {
        let o = i * 4;
        put_px8(&mut out[o..o + 4], src, layout.channels);
    }
    out
}

/// 16bit 样本 → RGBA8：取高字节，与 `image` 的 16→8 降位一致（换位方式就会色差）。
fn to_rgba8_16(px: &[u16], layout: Layout) -> Vec<u8> {
    let hi = |v: u16| (v >> 8) as u8;
    let mut out = vec![0u8; px.len() / layout.channels * 4];
    for (i, src) in px.chunks_exact(layout.channels).enumerate() {
        let o = i * 4;
        match layout.channels {
            1 => {
                let g = hi(src[0]);
                out[o] = g;
                out[o + 1] = g;
                out[o + 2] = g;
                out[o + 3] = 255;
            }
            2 => {
                let g = hi(src[0]);
                out[o] = g;
                out[o + 1] = g;
                out[o + 2] = g;
                out[o + 3] = hi(src[1]);
            }
            3 => {
                out[o] = hi(src[0]);
                out[o + 1] = hi(src[1]);
                out[o + 2] = hi(src[2]);
                out[o + 3] = 255;
            }
            _ => {
                out[o] = hi(src[0]);
                out[o + 1] = hi(src[1]);
                out[o + 2] = hi(src[2]);
                out[o + 3] = hi(src[3]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    const RGB: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/golden_16x12_rgb.tif"
    ));
    const GREY: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/grey_6x4.tif"
    ));
    const RGBA: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/rgba_8x8.tif"
    ));
    const GREY16: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/grey16_4x4.tif"
    ));
    const PALETTE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/palette_8x8.tif"
    ));
    const CMYK: &[u8] = include_bytes!(concat!(
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

    // ── m36 快路径的造样本工具 ──────────────────────────────────────────

    /// 造一枚**经典 TIFF（小端、条带式）**：未压缩、连续采样、8bit。
    /// 样本值 = `x + y + c`（三位数结构，错位 / 漏行 / 平均错都会当场露馅）。
    /// `rows` = RowsPerStrip（`rows >= h` 就是单条带），`photo` 直接写进 262 标签，
    /// 想造畸形头就拿返回值改字节。
    fn raw_tiff(ch: usize, w: u32, h: u32, rows: u32, photo: u16) -> Vec<u8> {
        raw_tiff_tagged(ch, w, h, rows, photo, None)
    }

    /// 同上，多一枚 **274 Orientation**（0x0112；m48：插件照实回报方向的用例要一张真能
    /// 解码、又带着方向标签的 TIFF；手工挪字节不如让构造器自己排）。
    fn raw_tiff_tagged(
        ch: usize,
        w: u32,
        h: u32,
        rows: u32,
        photo: u16,
        orientation: Option<u16>,
    ) -> Vec<u8> {
        fn e16(v: u16) -> [u8; 2] {
            v.to_le_bytes()
        }
        fn e32(v: u32) -> [u8; 4] {
            v.to_le_bytes()
        }
        /// 一条 IFD 条目（12 字节）：`val` 就是写进后 4 字节的值——**没有偏移参数**；要外置数组的标签（258/273/279）先用 `[0; 4]` 占位，布局算好后由下面那段回填循环写进 `t[8..]`。
        fn entry(tag: u16, ft: u16, count: u32, val: [u8; 4]) -> [u8; 12] {
            let mut out = [0u8; 12];
            out[..2].copy_from_slice(&e16(tag));
            out[2..4].copy_from_slice(&e16(ft));
            out[4..8].copy_from_slice(&e32(count));
            out[8..].copy_from_slice(&val);
            out
        }
        const FT_SHORT: u16 = 3;
        const FT_LONG: u16 = 4;

        let nstrips = ((h as u64).div_ceil(rows.max(1) as u64)) as u32;
        let row_bytes = (w as usize) * ch;
        // 标签按号升序（规格要求，库也按这个顺序读）。
        let mut tags: Vec<[u8; 12]> = vec![
            entry(256, FT_LONG, 1, e32(w)),
            entry(257, FT_LONG, 1, e32(h)),
            entry(259, FT_SHORT, 1, {
                let mut v = [0u8; 4];
                v[..2].copy_from_slice(&e16(1)); // Compression = 1（无压缩）
                v
            }),
            entry(262, FT_SHORT, 1, {
                let mut v = [0u8; 4];
                v[..2].copy_from_slice(&e16(photo));
                v
            }),
            entry(273, FT_LONG, nstrips, [0; 4]), // 偏移外置，位置下面回填
            entry(277, FT_SHORT, 1, {
                let mut v = [0u8; 4];
                v[..2].copy_from_slice(&e16(ch as u16));
                v
            }),
            entry(278, FT_LONG, 1, e32(rows)),
            entry(279, FT_LONG, nstrips, [0; 4]),
        ];
        // BitsPerSample：≤4 字节的值规格允许内联在条目里，>4 必须外置成数组。
        let bits_inline = ch * 2 <= 4;
        let mut bits_val = [0u8; 4];
        for c in 0..ch.min(2) {
            bits_val[c * 2..c * 2 + 2].copy_from_slice(&e16(8));
        }
        tags.push(entry(258, FT_SHORT, ch as u32, bits_val));
        if let Some(o) = orientation {
            let mut v = [0u8; 4];
            v[..2].copy_from_slice(&e16(o));
            tags.push(entry(274, FT_SHORT, 1, v));
        }
        tags.sort_by_key(|t| u16::from_le_bytes([t[0], t[1]]));

        let ifd_at = 8usize;
        let after_ifd = ifd_at + 2 + tags.len() * 12 + 4;
        // ★ 同一条规则也管条带表：**count == 1 时值就写在条目里**，只有多条目带才外置
        //   数组（早先无条件写指针，单条带文件就被读成"偏移 122"这种荒谬值）。
        let multi = nstrips > 1;
        let bits_at = after_ifd;
        let offs_at = bits_at + if bits_inline { 0 } else { ch * 2 };
        let cnts_at = offs_at + if multi { nstrips as usize * 4 } else { 0 };
        let data_at = cnts_at + if multi { nstrips as usize * 4 } else { 0 };
        // 每条带的行数与字节跨度（最后一条可能不满）。
        let mut offs = Vec::new();
        let mut cnts = Vec::new();
        let mut cur = data_at;
        let mut first_span = 0usize;
        for k in 0..nstrips {
            let kr = k as u64 * rows as u64;
            let rows_k = (rows as u64).min((h as u64).saturating_sub(kr));
            let span = rows_k as usize * row_bytes;
            if k == 0 {
                first_span = span;
            }
            offs.extend_from_slice(&e32(cur as u32));
            cnts.extend_from_slice(&e32(span as u32));
            cur += span;
        }
        assert_eq!(
            cur,
            data_at + (w as usize * h as usize) * ch,
            "条带跨度没拼满整图"
        );
        let mut body: Vec<u8> = Vec::new(); // 数据区（IFD 之后）
        if !bits_inline {
            for _ in 0..ch {
                body.extend_from_slice(&e16(8));
            }
        }
        if multi {
            body.extend_from_slice(&offs);
            body.extend_from_slice(&cnts);
        }
        assert_eq!(
            body.len(),
            data_at - after_ifd,
            "外置区与算好的数据起点没对上"
        );
        // 像素本体。
        let mut px = Vec::with_capacity((w as usize * h as usize) * ch);
        for y in 0..h {
            for x in 0..w {
                for c in 0..ch {
                    px.push((x as u8).wrapping_add(y as u8).wrapping_add(c as u8));
                }
            }
        }
        body.extend_from_slice(&px);

        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&e16(42));
        out.extend_from_slice(&e32(ifd_at as u32));
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&e16(tags.len() as u16));
        for t in tags.iter() {
            let mut t = *t;
            // 回填外置/内联的值：258（非内联）/ 273 / 279。
            match u16::from_le_bytes([t[0], t[1]]) {
                258 if !bits_inline => t[8..].copy_from_slice(&e32(bits_at as u32)),
                // 多条目带写数组指针，单条目带直接写值（规格：count==1 内联）。
                273 => {
                    let v = if multi { offs_at } else { data_at };
                    t[8..].copy_from_slice(&e32(v as u32));
                }
                279 => {
                    let v = if multi { cnts_at } else { first_span };
                    t[8..].copy_from_slice(&e32(v as u32));
                }
                _ => {}
            }
            ifd.extend_from_slice(&t);
        }
        ifd.extend_from_slice(&e32(0)); // next IFD
        assert_eq!(ifd.len(), 2 + tags.len() * 12 + 4);
        out.extend_from_slice(&ifd);
        out.extend_from_slice(&body);
        out
    }

    /// 走 ABI 问一次档位（`out = NULL`）。
    fn query(bytes: &[u8], r: &IvRenderReq) -> (i32, IvRenderOut) {
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            r,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        (c, info)
    }

    #[test]
    fn caps_declare_scaled_now() {
        // 位图必须如实：这枚插件**只在未压缩条带上**真降采样，压缩/瓦片/16bit 回整解
        // （那属于"白付一次整解"，不是错图）。这里钉住声明的位，别让改动悄悄改掉它。
        assert_eq!(iv_caps(), CAP_FULL | CAP_SCALED);
    }

    #[test]
    fn uncompressed_strips_are_addressable_and_direct_write_matches_library() {
        // 1 = Gray、3 = RGB、4 = RGBA（`GrayA` 走不到：这版库把"灰度 + 2 样本"报成
        // Multiband，本插件从来不接那一档）。
        for ch in [1usize, 3, 4] {
            let f = raw_tiff(ch, 40, 24, 24, if ch >= 3 { 2 } else { 1 });
            let shape = read_shape(&f, 0).expect("未压缩 8bit 连续采样应可接");
            let st = shape
                .strips
                .as_ref()
                .unwrap_or_else(|| panic!("{ch} 通道应可行寻址"));
            // 库路径（整解 + 转换）与直写路径必须逐字节相同——两条腿的映射表不一致
            // 就会在"档位图 ↔ 原图档"之间变色，那只有肉眼看得到。
            let lib = decode_rgba(&f, &shape).expect("库路径");
            let mut fast = vec![0u8; lib.len()];
            let p = Plan {
                w: shape.w,
                h: shape.h,
                fast: Some(Fast {
                    step: 1,
                    sample: false,
                }),
            };
            render_strips_into(
                &f,
                st,
                *p.fast.as_ref().unwrap(),
                &shape,
                fast.as_mut_ptr(),
                &p,
            )
            .expect("直写");
            assert_eq!(fast, lib, "{ch} 通道：整图直写与库路径不一致");
        }
    }

    /// 跨行取样的**滤波口径**：水平是 `step` 列面积平均，垂直是每 `step` 行取 1 行。
    /// 这条既钉住实现，也钉住"垂直不是平均"这个已知代价（改成真平均要连字节一起读，
    /// 那就没有加速了——见 [`render_strips_into`] 的 ★ 段）。
    #[test]
    fn boxed_row_sampling_filter_is_exact() {
        let (w, h, step) = (8u32, 8u32, 2u32);
        let f = raw_tiff(3, w, h, h, 2);
        let shape = read_shape(&f, 0).expect("应可行寻址");
        let st = shape.strips.as_ref().unwrap();
        let (ow, oh) = (w / step, h / step);
        let mut buf = vec![0u8; (ow * oh * 4) as usize];
        let p = Plan {
            w: ow,
            h: oh,
            fast: Some(Fast { step, sample: true }),
        };
        let fmode = *p.fast.as_ref().unwrap();
        render_strips_into(&f, st, fmode, &shape, buf.as_mut_ptr(), &p).expect("取样直写");
        for j in 0..oh {
            for i in 0..ow {
                // 源行 = j*step（垂直单行取样）；水平对 [i*step, i*step+step) 求均值。
                let y = (j * step) as i32;
                let mut sum = [0i32; 3];
                for k in 0..step as i32 {
                    let x = (i * step) as i32 + k;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += (x + y + c as i32) & 0xFF;
                    }
                }
                let o = (j as usize * ow as usize + i as usize) * 4;
                let half = (step / 2) as i32;
                let got: Vec<u8> = sum
                    .iter()
                    .map(|&v| ((v + half) / step as i32) as u8)
                    .collect();
                assert_eq!(&buf[o..o + 3], &got[..], "输出像素 ({i},{j}) 的平均不对");
                assert_eq!(buf[o + 3], 255, "无 alpha 源应填满不透明");
            }
        }
    }

    /// 通道特化的等价性：`ch = 1/3/4` × 三组**不整除**的尺寸/步长，真平均与取样两条腿
    /// 都逐像素对照独立参考值（含右边缘、下边缘的不满格）。
    ///
    /// 特化只改循环结构，不改求和与取整；这条用例就是"只改结构"的判据——任何一处
    /// 边缘格少算/多除，都会在这里逐像素露馅。
    #[test]
    fn scaled_modes_handle_ragged_edges_for_every_layout() {
        for ch in [1usize, 3, 4] {
            for (w, h, step) in [(13u32, 11u32, 3u32), (7, 5, 2), (10, 6, 4)] {
                let f = raw_tiff(ch, w, h, h, if ch >= 3 { 2 } else { 1 });
                let shape = read_shape(&f, 0).expect("应可行寻址");
                let st = shape.strips.as_ref().unwrap();
                let (ow, oh) = (w.div_ceil(step), h.div_ceil(step));
                let px = |x: u32, y: u32, c: u32| ((x + y + c) & 0xFF) as i32;

                // 真平均腿：两维都按该格**真实样本数**平均。
                let mut buf = vec![0u8; (ow * oh * 4) as usize];
                let p = Plan {
                    w: ow,
                    h: oh,
                    fast: Some(Fast {
                        step,
                        sample: false,
                    }),
                };
                let mode = *p.fast.as_ref().unwrap();
                render_strips_into(&f, st, mode, &shape, buf.as_mut_ptr(), &p).expect("真平均");
                for j in 0..oh {
                    for i in 0..ow {
                        let x1 = (i * step + step).min(w);
                        let y1 = (j * step + step).min(h);
                        let cell = ((x1 - i * step) * (y1 - j * step)) as i32;
                        let mut sum = [0i32; 4];
                        for y in j * step..y1 {
                            for x in i * step..x1 {
                                for (c, s) in sum.iter_mut().enumerate().take(ch) {
                                    *s += px(x, y, c as u32);
                                }
                            }
                        }
                        let o = ((j * ow + i) * 4) as usize;
                        for c in 0..ch {
                            assert_eq!(
                                buf[o + c],
                                ((sum[c] + cell / 2) / cell) as u8,
                                "ch={ch} {w}×{h} step={step} 真平均 ({i},{j}) 通道 {c}"
                            );
                        }
                        if ch < 4 {
                            assert_eq!(buf[o + 3], 255, "无 alpha 源应填满不透明");
                        }
                    }
                }

                // 取样腿：垂直每 step 行取 1 行，水平仍是面积平均。
                let mut buf = vec![0u8; (ow * oh * 4) as usize];
                let p = Plan {
                    w: ow,
                    h: oh,
                    fast: Some(Fast { step, sample: true }),
                };
                let mode = *p.fast.as_ref().unwrap();
                render_strips_into(&f, st, mode, &shape, buf.as_mut_ptr(), &p).expect("取样");
                for j in 0..oh {
                    for i in 0..ow {
                        let x0 = i * step;
                        let x1 = (x0 + step).min(w);
                        let n = (x1 - x0) as i32;
                        let mut sum = [0i32; 4];
                        for x in x0..x1 {
                            for (c, s) in sum.iter_mut().enumerate().take(ch) {
                                *s += px(x, j * step, c as u32);
                            }
                        }
                        let o = ((j * ow + i) * 4) as usize;
                        for c in 0..ch {
                            assert_eq!(
                                buf[o + c],
                                ((sum[c] + n / 2) / n) as u8,
                                "ch={ch} {w}×{h} step={step} 取样 ({i},{j}) 通道 {c}"
                            );
                        }
                        if ch < 4 {
                            assert_eq!(buf[o + 3], 255, "无 alpha 源应填满不透明");
                        }
                    }
                }
            }
        }
    }

    /// 取样腿的准入判据：只有**小格子**（缩略图那一档）才跳行，主画面一律真平均。
    #[test]
    fn only_small_boxes_get_sampled() {
        let shape = Shape {
            w: 6055,
            h: 6055,
            layout: Layout {
                channels: 3,
                wide: false,
            },
            strips: Some(Strips {
                row_bytes: 6055 * 3,
                rows: 6055,
                offsets: vec![140],
            }),
            icc: Vec::new(),
        };
        for (bw, bh, want) in [
            (240u32, 180u32, true),
            (512, 512, true),
            (513, 400, false),
            (2560, 1440, false),
        ] {
            let r = IvRenderReq {
                target_w: bw,
                target_h: bh,
                flags: REQ_ALLOW_SCALED,
                ..req()
            };
            let p = plan(&shape, &r, 6055 * 6055);
            assert_eq!(
                p.fast.map(|f| f.sample),
                Some(want),
                "框 {bw}×{bh} 的取样判定不对"
            );
        }
    }

    /// 主画面那一档：**`step × step` 块全读进来做真平均**，结果必须与"整解再面积缩小"
    /// 等价（这条就是 [`Fast::sample`] 那条注释里"高频内容看得出颗粒"的对策）。
    #[test]
    fn averaged_mode_is_a_true_block_mean() {
        let (w, h, step) = (8u32, 8u32, 2u32);
        let f = raw_tiff(3, w, h, h, 2);
        let shape = read_shape(&f, 0).expect("应可行寻址");
        let st = shape.strips.as_ref().unwrap();
        let (ow, oh) = (w / step, h / step);
        let mut buf = vec![0u8; (ow * oh * 4) as usize];
        let p = Plan {
            w: ow,
            h: oh,
            fast: Some(Fast {
                step,
                sample: false,
            }),
        };
        let mode = *p.fast.as_ref().unwrap();
        render_strips_into(&f, st, mode, &shape, buf.as_mut_ptr(), &p).expect("真平均");
        for j in 0..oh {
            for i in 0..ow {
                // 参考值 = 该 `step × step` 块内每个样本的算术平均（两维都平均）。
                let mut sum = [0i32; 3];
                for y in (j * step)..(j * step + step) {
                    for x in (i * step)..(i * step + step) {
                        for (c, s) in sum.iter_mut().enumerate() {
                            *s += ((x + y) as u8 as i32 + c as i32) & 0xFF;
                        }
                    }
                }
                let n = (step * step) as i32;
                let o = (j as usize * ow as usize + i as usize) * 4;
                for c in 0..3 {
                    let want = ((sum[c] + n / 2) / n) as u8;
                    assert_eq!(buf[o + c], want, "块({i},{j})通道{c}不是真平均");
                }
                assert_eq!(buf[o + 3], 255);
            }
        }
    }

    /// `scaled_step` 的立约判据：**缩完不许让宿主放大**。显示尺寸由 fit 的短板维决定，
    /// 取 `⌊max(w/tw, h/th)⌋` 后两维都仍 ≥ 显示尺寸——拿一批长宽比扫一遍这个不变式。
    #[test]
    fn scaled_plan_never_lets_the_host_upscale() {
        fn fit(w: u32, h: u32, bw: u32, bh: u32) -> (u32, u32) {
            let s = (bw as f64 / w as f64).min(bh as f64 / h as f64).min(1.0);
            (
                (w as f64 * s).floor().max(1.0) as u32,
                (h as f64 * s).floor().max(1.0) as u32,
            )
        }
        let boxes = [
            (2560u32, 1440u32),
            (1920, 1080),
            (240, 180),
            (120, 90),
            (3000, 3000),
        ];
        let shapes = [
            (6055u32, 6055u32),
            (6055, 2000),
            (2000, 6055),
            (4000, 3000),
            (12_000, 1_000),
            (900, 900),
            (100, 100_000),
        ];
        let scaled = IvRenderReq {
            flags: REQ_ALLOW_SCALED,
            ..req()
        };
        for &(w, h) in &shapes {
            for &(bw, bh) in &boxes {
                let shape = Shape {
                    w,
                    h,
                    layout: Layout {
                        channels: 3,
                        wide: false,
                    },
                    icc: Vec::new(),
                    strips: Some(Strips {
                        row_bytes: w as u64 * 3,
                        rows: h as u64,
                        offsets: vec![0],
                    }),
                };
                let npixels = w as u64 * h as u64;
                let p = plan(&shape, &scaled, npixels);
                let (dw, dh) = fit(w, h, bw, bh);
                assert!(
                    p.w >= dw && p.h >= dh && p.w <= w && p.h <= h,
                    "{w}×{h} 框 {bw}×{bh}：档位 {}×{} 越过显示尺寸 {dw}×{dh}",
                    p.w,
                    p.h,
                );
                // 申报尺寸与步数必须自洽：档位就是"原图按 step 取整"，且快路径的
                // 有无与档位同源——只报小档却整解会让宿主把大图塞进按档位分配的缓冲。
                let s = p.fast.map(|f| f.step).unwrap_or(1);
                assert_eq!(p.w, w.div_ceil(s));
                assert_eq!(p.h, h.div_ceil(s));
                assert_eq!(p.fast.is_some(), shape.strips.is_some());
            }
        }
    }

    #[test]
    fn scaled_query_reports_scaled_bytes_and_small_images_stay_plain() {
        // 够大的图（≥ MIN_SCALE_PIXELS）：查询阶段就得按档位申报，宿主据此分配。
        let big = raw_tiff(3, 1600, 1200, 1200, 2);
        let r = IvRenderReq {
            target_w: 800,
            target_h: 600,
            flags: REQ_ALLOW_SCALED,
            ..req()
        };
        let (c, info) = query(&big, &r);
        assert_eq!(c, ERR_OK);
        assert_eq!((info.actual_w, info.actual_h), (800, 600), "1/2 档");
        assert_eq!(info.bytes_needed, 800 * 600 * 4);
        // 同一份文件不带缩放意图：仍是原图尺寸（清晰度底线那条路一字不改）。
        let (c2, plain) = query(&big, &req());
        assert_eq!(c2, ERR_OK);
        assert_eq!((plain.actual_w, plain.actual_h), (1600, 1200));

        // 小图不劳这条路（整解本就在毫秒以下）：点亮了也按原图报。
        let small = raw_tiff(3, 40, 24, 24, 2);
        let r2 = IvRenderReq {
            target_w: 8,
            target_h: 8,
            flags: REQ_ALLOW_SCALED,
            ..req()
        };
        let (_, si) = query(&small, &r2);
        assert_eq!((si.actual_w, si.actual_h), (40, 24), "小图不缩放");
    }

    /// 快路径的准入闸：**没把握就整解**，绝不按错误的行宽切出错色。
    #[test]
    fn addressability_gates_reject_what_we_cannot_index() {
        // 压缩条带（现役黄金样本是 LZW）：跳字节的前提不存在。
        let lzw = read_shape(RGB, 0).expect("LZW 样本仍可整解");
        assert!(lzw.strips.is_none(), "压缩 TIFF 不得走直写");

        // 反相灰度（Photometric=0 = WhiteIsZero）：反相是库在 read_image 里做的，
        // 直写这条路不复制它的口径——接了就会少一次反相，整张图黑白颠倒。
        let wzero = raw_tiff(1, 40, 24, 24, 0);
        let s = read_shape(&wzero, 0).expect("WhiteIsZero 灰度仍可接");
        assert!(s.strips.is_none(), "反相布局不得走直写，否则会少一次反相");
        let lib = decode_rgba(&wzero, &s).expect("库路径反相");
        let mut buf = vec![0u8; lib.len()];
        let mut info = IvRenderOut::default();
        let r = iv_render(
            wzero.as_ptr(),
            wzero.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(buf, lib, "反相文件必须走库路径");

        // 条带偏移被改到文件外：行寻址表当场作废，且必须老实报错（不许 panic、不许越界读）。
        let mut bad = raw_tiff(3, 40, 24, 24, 2);
        let ifd_at = 8usize;
        let n = u16::from_le_bytes([bad[ifd_at], bad[ifd_at + 1]]) as usize;
        let mut patched = false;
        for i in 0..n {
            let e = ifd_at + 2 + i * 12;
            // 单条目带的偏移是**内联值**：把条带起点推到文件之外。
            if u16::from_le_bytes([bad[e], bad[e + 1]]) == 273 {
                bad[e + 8..e + 12].copy_from_slice(&u32::MAX.to_le_bytes());
                patched = true;
            }
        }
        assert!(patched, "样本里应能找到 273");
        assert!(
            read_shape(&bad, 0)
                .map(|s| s.strips.is_none())
                .unwrap_or(true),
            "越界条带偏移不得准入直写"
        );
        let mut info = IvRenderOut::default();
        let mut buf = vec![0u8; 40 * 24 * 4];
        let c = iv_render(
            bad.as_ptr(),
            bad.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_ne!(c, ERR_OK, "越界条带必须报错，绝不能返回成功");
        assert_ne!(c, ERR_PANIC, "更不许 panic");

        // RowsPerStrip = 0：除零风险，闸在寻址表里挡掉。
        let mut zero = raw_tiff(3, 40, 24, 24, 2);
        for i in 0..u16::from_le_bytes([zero[8], zero[9]]) as usize {
            let e = 10 + i * 12;
            if u16::from_le_bytes([zero[e], zero[e + 1]]) == 278 {
                zero[e + 8..e + 12].copy_from_slice(&0u32.to_le_bytes());
            }
        }
        assert!(
            read_shape(&zero, 0)
                .map(|s| s.strips.is_none())
                .unwrap_or(true),
            "RowsPerStrip=0 不得准入直写"
        );
    }

    /// 多条目带（RowsPerStrip < 行数）也要能寻址——现役黄金样本恰好是单条带。
    #[test]
    fn multi_strip_files_are_addressable_too() {
        for rows in [1u32, 5, 7] {
            let f = raw_tiff(3, 40, 24, rows, 2);
            let shape = read_shape(&f, 0).expect("多条目带应可接");
            let st = shape
                .strips
                .as_ref()
                .unwrap_or_else(|| panic!("{rows} 行/条带"));
            let lib = decode_rgba(&f, &shape).expect("库路径");
            let mut fast = vec![0u8; lib.len()];
            let p = Plan {
                w: 40,
                h: 24,
                fast: Some(Fast {
                    step: 1,
                    sample: false,
                }),
            };
            render_strips_into(
                &f,
                st,
                *p.fast.as_ref().unwrap(),
                &shape,
                fast.as_mut_ptr(),
                &p,
            )
            .expect("直写");
            assert_eq!(fast, lib, "{rows} 行/条带：跨条带边界算错了行偏移");
        }
    }

    fn last_err() -> String {
        let mut ebuf = [0i8; 256];
        let need = unsafe { rupix_abi::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        if need <= 1 {
            return String::new();
        }
        let s: Vec<u8> = ebuf[..(need as usize - 1)]
            .iter()
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&s).into_owned()
    }

    /// 两阶段完整解码，返回 RGBA8 像素。
    fn render(bytes: &[u8]) -> Result<(IvRenderOut, Vec<u8>), i32> {
        let mut info = IvRenderOut::default();
        let q = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if q != ERR_OK {
            return Err(q);
        }
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            bytes.as_ptr(),
            bytes.len(),
            &req(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        if r != ERR_OK {
            return Err(r);
        }
        Ok((info, buf))
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 4);
    }

    #[test]
    #[allow(clippy::manual_c_str_literals)] // 末尾 \x00 是 IFD 偏移字段本身，不是字符串结束符
    fn probe_accepts_tiff_magic_both_endians() {
        assert_eq!(iv_probe(RGB.as_ptr(), RGB.len(), std::ptr::null()), 100);
        // 小端 II + 42 / 大端 MM + 42
        assert_eq!(
            iv_probe(b"II\x2A\x00\x08\x00\x00\x00".as_ptr(), 8, std::ptr::null()),
            100
        );
        assert_eq!(
            iv_probe(b"MM\x00\x2A\x00\x00\x00\x08".as_ptr(), 8, std::ptr::null()),
            100
        );
        // BigTIFF 版本号是 43，两个字节序都认领
        assert_eq!(
            iv_probe(b"II\x2B\x00\x08\x00\x00\x00".as_ptr(), 8, std::ptr::null()),
            100
        );
        assert_eq!(
            iv_probe(b"MM\x00\x2B\x00\x00\x00\x08".as_ptr(), 8, std::ptr::null()),
            100
        );
        // 版本号既不是 42 也不是 43 → 不认领；PNG / 太短 / 空指针同样不认领
        assert_eq!(
            iv_probe(b"MM\x00\x2C\x00\x00\x00\x08".as_ptr(), 8, std::ptr::null()),
            0
        );
        assert_eq!(
            iv_probe(b"\x89PNG\r\n\x1a\n".as_ptr(), 8, std::ptr::null()),
            0
        );
        assert_eq!(iv_probe(b"II*\0junk".as_ptr(), 3, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
    }

    #[test]
    fn rgb_lzw_roundtrip() {
        let (info, px) = render(RGB).expect("RGB LZW 应可解");
        assert_eq!((info.actual_w, info.actual_h), (16, 12));
        assert_eq!(info.bytes_needed, 16 * 12 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        // 语料没有 274 标签 → 报 1；`already_oriented = 0` 是契约值（本插件从不摆正像素）。
        assert_eq!((info.orientation, info.already_oriented), (1, 0));
        assert_eq!(info.page_count, 1);
        assert!(px.iter().any(|&v| v > 0), "必须真解码出内容");
        assert!(
            px.chunks_exact(4).all(|p| p[3] == 255),
            "无 alpha 源应填满不透明"
        );
    }

    /// m48「竖拍照片不再横躺」在 TIFF 上的落点：带 274 标签就照实回报，并声明"我没转"。
    /// 用**真能解码**的构造样本走查询→渲染两阶段，防止只有查询报对、渲染又写回 1。
    #[test]
    fn orientation_tag_is_reported_for_the_host_to_apply() {
        let with = raw_tiff_tagged(3, 4, 2, 2, 2, Some(6));
        let (info, px) = render(&with).expect("带方向标签的构造 TIFF 应可解");
        assert_eq!(
            (info.actual_w, info.actual_h),
            (4, 2),
            "申报的仍是文件里的原始行序——摆正是宿主的事"
        );
        assert_eq!((info.orientation, info.already_oriented), (6, 0));
        assert_eq!(px.len(), 4 * 2 * 4);

        // 对照组：同一枚构造器不写标签 → 1/0，与历史行为逐字节相同。
        let (info, _) = render(&raw_tiff(3, 4, 2, 2, 2)).expect("无标签版应可解");
        assert_eq!((info.orientation, info.already_oriented), (1, 0));
    }

    /// 冠军路径的关键特征：RGBA8 源是**同布局拷贝**，不是逐像素重建。
    #[test]
    fn rgba_source_keeps_its_alpha() {
        let (info, px) = render(RGBA).expect("RGBA deflate 应可解");
        assert_eq!((info.actual_w, info.actual_h), (8, 8));
        // 测试样本的 alpha 由 x*32 生成：第 0 列 0、第 7 列 224。
        assert_eq!(px[3], 0, "首像素 alpha 应来自源文件");
        assert_eq!(px[(7 * 4) + 3], 224, "末列 alpha 应逐列递增");
    }

    #[test]
    fn grey_and_grey16_expand_to_rgb() {
        let (_, px) = render(GREY).expect("Gray8 应可解");
        assert!(
            px.chunks_exact(4)
                .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255),
            "灰度应复制三通道"
        );

        let (info, px16) = render(GREY16).expect("Gray16 应可解");
        assert_eq!((info.actual_w, info.actual_h), (4, 4));
        assert_eq!(info.bit_depth, 8, "16bit 源降到 RGBA8 后仍报 8");
        // 样本值 = (x*4+y)*4096 → 高字节 = (x*4+y)*16：像素 1（x=1）= 16384 → 64。
        assert_eq!(&px16[..4], &[0, 0, 0, 255]);
        assert_eq!(&px16[4..8], &[64, 64, 64, 255]);
    }

    /// 能力边界：Palette / CMYK 必须报 `ERR_UNSUPPORTED`（不是 corrupt、不是 panic），
    /// 宿主据此走嗅探兜底把文件交给 core——提速不该以功能倒退为代价。
    #[test]
    fn outside_support_reports_unsupported() {
        for (name, bytes) in [("palette", PALETTE), ("cmyk", CMYK)] {
            let mut info = IvRenderOut::default();
            let c = iv_render(
                bytes.as_ptr(),
                bytes.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            );
            assert_eq!(c, ERR_UNSUPPORTED, "{name}: 应报不支持，实际 {c}");
            let e = last_err();
            assert!(
                !e.is_empty() && !e.starts_with("tiff: "),
                "{name}: last-error 要写清原因、且不带插件名前缀，实际「{e}」"
            );
        }
    }

    #[test]
    fn query_phase_matches_render_phase() {
        let mut info = IvRenderOut::default();
        let q = iv_render(
            RGB.as_ptr(),
            RGB.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(q, ERR_OK);
        assert_eq!(info.bytes_needed, 16 * 12 * 4);
        let (_, px) = render(RGB).expect("渲染阶段应成功");
        assert_eq!(px.len(), info.bytes_needed as usize, "两阶段字节数必须一致");
    }

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 16];
        let r = iv_render(
            RGB.as_ptr(),
            RGB.len(),
            &req(),
            tiny.as_mut_ptr(),
            tiny.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_BUFFER_TOO_SMALL);
    }

    #[test]
    fn byte_budget_is_honoured() {
        let mut info = IvRenderOut::default();
        let r = IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 64,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        };
        assert_eq!(
            iv_render(
                RGB.as_ptr(),
                RGB.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_TOO_LARGE
        );
    }

    /// G5（输入线性）盲点：IFD 里写巨大尺寸，必须在 `read_image()` 自己分配前挡住。
    #[test]
    fn huge_dimensions_rejected_before_alloc() {
        let mut bad = RGB.to_vec();
        // ImageWidth(256) / ImageLength(257) 的条目值：找 IFD 里 tag=256/257 的 SHORT 值改写。
        // 样本是 LZW RGB，尺寸按 SHORT 存 → 直接找 `00 01 00 01`（16,12 的小端 SHORT 对）不通用，
        // 这里用「置 0xFFFF」的粗暴改法：定位第一个 tag 256 条目，把它的值改成 0xFFFF。
        let dir_off = u32::from_le_bytes([bad[4], bad[5], bad[6], bad[7]]) as usize;
        let count = u16::from_le_bytes([bad[dir_off], bad[dir_off + 1]]) as usize;
        let mut patched = false;
        for i in 0..count {
            let e = dir_off + 2 + i * 12;
            let tag = u16::from_le_bytes([bad[e], bad[e + 1]]);
            if tag == 256 || tag == 257 {
                bad[e + 8] = 0xFF;
                bad[e + 9] = 0xFF;
                patched = true;
            }
        }
        assert!(patched, "语料里应能找到 tag 256/257");
        let mut info = IvRenderOut::default();
        let c = iv_render(
            bad.as_ptr(),
            bad.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert!(
            c == ERR_TOO_LARGE || c == ERR_CORRUPT || c == ERR_UNSUPPORTED,
            "畸形尺寸必须被挡下（绝不能是 OK 或 panic），实际 code={c}"
        );
    }

    #[test]
    fn truncated_and_garbage_input_report_honestly() {
        // 只剩魔数 + IFD 偏移：头都读不全 → corrupt，不 panic。
        let head = &RGB[..8];
        let mut info = IvRenderOut::default();
        let c = iv_render(
            head.as_ptr(),
            head.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert!(c == ERR_CORRUPT || c == ERR_UNSUPPORTED, "截断头 code={c}");

        // 魔数对、内容是垃圾：同样老实报错。
        let mut junk = b"II*\x00".to_vec();
        junk.resize(64, 0);
        let c = iv_render(
            junk.as_ptr(),
            junk.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_ne!(c, ERR_OK, "垃圾内容绝不能返回成功");
    }

    #[test]
    fn non_tiff_input_is_unrecognized_not_corrupt() {
        let mut info = IvRenderOut::default();
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(
            iv_render(
                png.as_ptr(),
                png.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_UNRECOGNIZED,
            "魔数不对要老实说不认，让宿主换插件"
        );
    }

    #[test]
    fn second_page_reports_unsupported_instead_of_serving_page_zero() {
        let mut info = IvRenderOut::default();
        let r = IvRenderReq {
            page_index: 1,
            ..req()
        };
        let c = iv_render(
            RGB.as_ptr(),
            RGB.len(),
            &r,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_UNSUPPORTED, "首页插件不得冒充第 1 页");
    }

    #[test]
    fn last_error_is_chinese_and_cleared_on_success() {
        let mut info = IvRenderOut::default();
        // 魔数对、IFD 全是垃圾：失败路径必须写带 `tiff:` 前缀的原因。
        iv_render(
            b"II*\x00junk!".as_ptr(),
            8,
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert!(
            !last_err().starts_with("tiff: "),
            "规范 §六 第 11 条：不得自带插件名前缀，实际「{}」",
            last_err()
        );

        // 成功路径不残留错误（kit 的 guard 在返回 ERR_OK 时清空 thread-local）。
        render(RGB).expect("解码成功");
        assert_eq!(last_err(), "", "成功后不应残留错误");
    }

    /// 手写一张 2×2 RGB、带 `IccProfile`(34675) 的小 TIFF。
    /// （`tiff` 编码器不给 `ImageEncoder` 暴露 `write_tag`，所以照本模块既有做法手排 IFD。）
    fn tiff_with_icc(profile: &[u8]) -> Vec<u8> {
        fn e16(v: u16) -> [u8; 2] {
            v.to_le_bytes()
        }
        fn e32(v: u32) -> [u8; 4] {
            v.to_le_bytes()
        }
        fn entry(tag: u16, ft: u16, count: u32, val: [u8; 4]) -> [u8; 12] {
            let mut out = [0u8; 12];
            out[..2].copy_from_slice(&e16(tag));
            out[2..4].copy_from_slice(&e16(ft));
            out[4..8].copy_from_slice(&e32(count));
            out[8..].copy_from_slice(&val);
            out
        }
        let n_entries = 10usize;
        let ifd_at = 8usize;
        let bits_at = ifd_at + 2 + n_entries * 12 + 4;
        let icc_at = bits_at + 6;
        let data_at = icc_at + profile.len();
        let mut tags = vec![
            entry(256, 4, 1, e32(2)),                                  // Width
            entry(257, 4, 1, e32(2)),                                  // Height
            entry(258, 3, 3, e32(bits_at as u32)),                     // BitsPerSample（外置）
            entry(259, 3, 1, [1, 0, 0, 0]),                            // Compression = 1
            entry(262, 3, 1, [2, 0, 0, 0]),                            // Photometric = RGB
            entry(273, 4, 1, e32(data_at as u32)),                     // StripOffsets
            entry(277, 3, 1, [3, 0, 0, 0]),                            // SamplesPerPixel
            entry(278, 4, 1, e32(2)),                                  // RowsPerStrip
            entry(279, 4, 1, e32(12)),                                 // StripByteCounts
            entry(34675, 1, profile.len() as u32, e32(icc_at as u32)), // IccProfile
        ];
        tags.sort_by_key(|t| u16::from_le_bytes([t[0], t[1]]));

        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&e16(42));
        out.extend_from_slice(&e32(ifd_at as u32));
        out.extend_from_slice(&e16(n_entries as u16));
        for t in tags {
            out.extend_from_slice(&t);
        }
        out.extend_from_slice(&e32(0)); // 没有下一个 IFD
        for _ in 0..3 {
            out.extend_from_slice(&e16(8)); // BitsPerSample
        }
        out.extend_from_slice(profile);
        out.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(out.len(), data_at + 12);
        out
    }

    /// m53-J：`IccProfile` 在查询/渲染两阶段都如实申报，且尾部字节就是原配置。
    #[test]
    fn icc_profile_is_declared_and_appended_after_pixels() {
        let profile: Vec<u8> = (0..280u32).map(|i| (i * 13 % 247) as u8).collect();
        let data = tiff_with_icc(&profile);
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(info.icc_len as usize, profile.len());
        let mut buf = vec![0u8; info.bytes_needed as usize + info.icc_len as usize];
        assert_eq!(
            iv_render(
                data.as_ptr(),
                data.len(),
                &req(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        assert_eq!(&buf[info.bytes_needed as usize..], &profile[..]);

        // 语料没有 ICC：申报 0。
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                RGB.as_ptr(),
                RGB.len(),
                &req(),
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_OK
        );
        assert_eq!(info.icc_len, 0);
    }
}
