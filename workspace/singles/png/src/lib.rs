//! RuPix 外挂解码器：PNG → RGBA8（库：png crate）。
//!
//! # 为什么是这个库（G1 实测，不是"全网都说"）
//!
//! 2026-10-15 本机实测（本仓历史记录，run **r1792006637**，n=200 取 median，同一 run 内配对）：
//!
//! | 配置 | 合计 median ms | 相对第一名 |
//! |---|---|---|
//! | **png crate 直调** | **55.68** | **1.00×** |
//! | `image`（core 旧路线） | 65.99 | 1.19× |
//! | lodepng（C，decode32 直出 RGBA） | 103.37 | 1.86× |
//! | spng（C，libspng 公认冠军，Rgba8 输出） | 140.23 | 2.52× |
//!
//! 两个"C 绑定其实更快"的常识在本机被实测否掉了：`-sys` bindings 编译没问题（工具链不是
//! 借口），但 PNG 上确实追不上 png crate 的这条直调路线。WIC 基线 114.91 ms 也不作否决项
//! （用户已钉入"WIC 不复活"，弃它的理由是可移植性，见计划 §12 与本 run 的 §7）。
//!
//! **发布配置复测（2026-10-15，run r1792047693，n=200 × photo/snowflake × S/M/L，6/6 bit-exact）**：
//! 上面表格的 55.68 是选型时 EXPAND 口径的数字；**发布用的 `ALPHA|STRIP_16` 复测合计
//! 61.84 ms vs `image` 73.69 ms = 1.19×**——倍率与选型 run 完全一致，photo L 档还略快于
//! EXPAND（38.45 vs 38.64：库内扩 alpha 省掉 Rust 侧 to_rgba 循环）。跨 run 绝对值偏慢
//! 属机器负载差异，配对 run 内比较不受影响。harness 对照项 `png-alpha`（本仓历史记录）。
//!
//! # 为什么"快"的是调用方式而不是库
//!
//! core 走的 `image` 路线是 `PngDecoder` → `ImageBuffer` → `into_rgba8()`：先按源色彩类型
//! 分配一次，再逐像素搬到 RGBA8 第二次。本解码器把这两步压成一步：
//!
//! * `Transformations::ALPHA | STRIP_16` 让**库**在解每一行时就完成"扩到 8bit 带 alpha"，
//!   调色板 / RGB / 16bit 全部直接产出 `Rgba`；
//! * 产出正好是 `Rgba` 时（最常见路径）**解到宿主给的缓冲区里**，解码器侧零拷贝、零逐像素循环；
//! * 只有灰度族（库产出 `GrayscaleAlpha`）需要一次 2→4 扩展 —— 那是灰度图的固有代价，
//!   比让库出 1 通道再自己铺 4 通道少一趟内存往返。
//!
//! # 16bit 降位口径（与 `image` / libpng 一致，与 core 旧路线差 ≤1 LSB）
//!
//! `STRIP_16` 取**高字节**（0x10B3 → 0x10 = 16），`image` 的 16→8 走 ≈÷257 四舍五入（→ 17）。
//! libpng 也丢低字节，所以本解码器跟的是行业口径；测试里 16bit 族因此不比逐字节相等，
//! 而是"等于源高字节 + 与 image 差 ≤1 LSB"两条断言（见 `tests::parity_with_core_image_route_across_layouts`）。
//!
//! # 不接的部分
//!
//! APNG（acTL/fcTL）动画要逐帧合成与 dispose/blend 语义，core 已有 `apng_anim` 路线；
//! 本冠军只做静态帧，用 `ERR_UNSUPPORTED` **主动让位**（宿主按让位链交给 core），
//! 而不是硬解一张静帧冒充动画。判定见 [`iv_render`] 里的 `animated` 分支。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;
use std::slice;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "png",
    name: "PNG (png crate)",
    // ★ 只认领 png：apng 仍归 core（本解码器对 APNG 主动让位，见模块头）。
    //   宿主 `ext_claims_are_unique` 门禁保证一个后缀只有一个认领者。
    extensions: "png",
    description: "解码 PNG：png crate 直调，ALPHA|STRIP_16 让库直出 RGBA8；APNG 让位 core",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。与 core / jpeg / tiff 同口径。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// PNG 签名（规范固定 8 字节，含 \r\n 换行哨兵以防被文本传输破坏）。
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 合法 PNG 的最小字节数：签名 8 + IHDR 块（长度 4 + 类型 4 + 数据 13 + CRC 4）= 33。
const MIN_LEGAL_LEN: usize = 8 + 4 + 4 + 13 + 4;

type Reader<'d> = png_crate::Reader<Cursor<&'d [u8]>>;

// ---------------------------------------------------------------------------
// iv_probe / iv_render
// ---------------------------------------------------------------------------

/// 签名 + IHDR 头校验。返回 100 = 本解码器是该格式的实测冠军，宿主直接派给我。
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
        if data.is_null() || len < MIN_LEGAL_LEN {
            return 0;
        }
        let s = unsafe { slice::from_raw_parts(data, len) };
        if s[..8] != SIGNATURE {
            return 0;
        }
        // IHDR 必须是第一个块、长度恒 13：这条校验专门挡"以签名开头的垃圾"，
        // 否则一份坏 PNG 会把冠军 DLL 的整条解码跑一遍才失败。
        let ihdr_len = u32::from_be_bytes([s[8], s[9], s[10], s[11]]);
        if ihdr_len != 13 || s[12..16] != b"IHDR"[..] {
            return 0;
        }
        100
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
        let input = unsafe { slice::from_raw_parts(data, len) };

        // ── 阶段 1：只读块头拿尺寸（read_info 停在第一个 IDAT，不 inflate 像素）。──
        let head = match read_head(input) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };
        if head.animated {
            // 主动让位：不是"文件坏了"，是"这活不归冠军做"。宿主会按让位链问 core。
            set_last_error("APNG 动图由 core 解码器负责，本冠军解码器让位");
            return ERR_UNSUPPORTED;
        }

        let npixels = head.w as u64 * head.h as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码器上限");
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

        // ★ 方向：只逐块走 IDAT 之前的块表（kit 函数，微秒级），不解一张像素。
        let orientation = rupix_abi::png_exif_orientation(input).unwrap_or(1);
        let icc_len = head.icc.len() as u64;

        if out.is_null() {
            fill_info(
                info,
                head.w,
                head.h,
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

        // ── 阶段 2：真解码。库直出 RGBA 时直接写进宿主缓冲，解码器侧零搬运。──────
        if let Err((code, msg)) = decode_into(input, out, bytes_needed) {
            set_last_error(&msg);
            return code;
        }
        if !head.icc.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    head.icc.as_ptr(),
                    out.add(bytes_needed as usize),
                    head.icc.len(),
                );
            }
        }
        fill_info(
            info,
            head.w,
            head.h,
            bytes_needed,
            orientation,
            icc_len as u32,
        );
        ERR_OK
    })
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
    // 16bit 源由库的 STRIP_16 降到 8bit（取高字节，与 core / `image` 的 16→8 一致），
    // 调色板 / 灰度 / 1-2-4bit 同样被 ALPHA 变换升到 8bit，所以恒报 8。
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = orientation;
    // 本解码器从不摆正像素（`png` crate 出的就是文件里的原始行序），所以老实报 0 让宿主补转。
    // 旧注释说"PNG 没有方向语义"——不准确：`eXIf` 数据块（PNG 第三版登记过）带的就是
    // 同一枚 274 标签，手机截图与部分导出工具会写。读不到才报 1，行为与今天逐字节相同。
    oi.already_oriented = 0;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    // APNG 在阶段 1 就让位了，走到这里的一定是静帧。
    oi.page_count = 1;
    // iCCP 原样透传给宿主做色彩管理（本解码器自己不转）。
    oi.icc_len = icc_len;
}

// ---------------------------------------------------------------------------
// png crate 薄封装
// ---------------------------------------------------------------------------

/// 本解码器的冠军配置：ALPHA 让库把灰度/RGB/调色板一律扩成带 alpha 的 8bit，
/// STRIP_16 把 16bit 取高字节降位。解码器侧因此只可能见到 `Rgba` 或 `GrayscaleAlpha`。
fn open(data: &[u8]) -> Result<Reader<'_>, png_crate::DecodingError> {
    let mut dec = png_crate::Decoder::new(Cursor::new(data));
    dec.set_transformations(
        png_crate::Transformations::ALPHA | png_crate::Transformations::STRIP_16,
    );
    dec.read_info()
}

/// 尺寸 + 是否 APNG + 内嵌 ICC（`iCCP` 已由库解压）。
struct Head {
    w: u32,
    h: u32,
    animated: bool,
    icc: Vec<u8>,
}

fn read_head(data: &[u8]) -> Result<Head, (i32, String)> {
    let reader = open(data).map_err(dec_err)?;
    let i = reader.info();
    // 超上限整份丢弃（与宿主 ICC_CAP 同口径）。
    let icc = match &i.icc_profile {
        Some(p) if p.len() <= rupix_abi::ICC_MAX => p.to_vec(),
        _ => Vec::new(),
    };
    Ok(Head {
        w: i.width,
        h: i.height,
        animated: i.is_animated(),
        icc,
    })
}

/// 解出 RGBA8 写进宿主缓冲。
fn decode_into(data: &[u8], out: *mut u8, bytes_needed: u64) -> Result<(), (i32, String)> {
    use png_crate::{BitDepth, ColorType};
    let mut reader = open(data).map_err(dec_err)?;
    let i = reader.info();
    let (w, h) = (i.width as u64, i.height as u64);
    let npixels = w * h;

    // 库承诺的每像素字节数：只认这两种（见模块头）。其余一律"我不会"，交回 core，
    // 而不是猜一个映射把图解成错色——那比打不开更糟。
    let bpp = match reader.output_color_type() {
        (ColorType::Rgba, BitDepth::Eight) => 4usize,
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => 2usize,
        (ct, bd) => {
            return Err((
                ERR_UNSUPPORTED,
                format!("本解码器只接 RGBA8 与灰度α8 输出，此布局 {ct:?}/{bd:?} 交回 core"),
            ))
        }
    };
    let required = reader
        .output_buffer_size()
        .ok_or_else(|| (ERR_TOO_LARGE, "帧缓冲尺寸计算溢出".to_string()))?;
    if required as u64 != npixels * bpp as u64 || npixels * 4 != bytes_needed {
        // 布局理解与库不一致（或畸形尺寸）。同样老实让路。
        return Err((
            ERR_UNSUPPORTED,
            format!("库申报的缓冲 {required} 与 {w}x{h}x{bpp} 不符，交回 core"),
        ));
    }
    let dst = unsafe { slice::from_raw_parts_mut(out, bytes_needed as usize) };

    if bpp == 4 {
        // 最常见路径：库把 RGBA 行直接写进宿主缓冲（含隔行图的 Adam7 展开也是就地铺）。
        let frame = reader.next_frame(dst).map_err(dec_err)?;
        if (frame.width as u64, frame.height as u64) != (w, h) {
            return Err((ERR_CORRUPT, "帧尺寸与 IHDR 声明不符".into()));
        }
        return Ok(());
    }

    // 灰度族：库出 2 通道，最后一步 2→4 由解码器做（复制亮度到 RGB，搬 alpha）。
    let mut scratch = vec![0u8; required];
    reader.next_frame(&mut scratch).map_err(dec_err)?;
    for (s, d) in scratch.chunks_exact(2).zip(dst.chunks_exact_mut(4)) {
        let g = s[0];
        d[0] = g;
        d[1] = g;
        d[2] = g;
        d[3] = s[1];
    }
    Ok(())
}

/// 库的错误 → ABI 错误码。**"我不会解"与"文件坏了"必须分开报**（解码器规范 §六第 10 条）：
/// 混成一类会让让位链该走的时候不走。
fn dec_err(e: png_crate::DecodingError) -> (i32, String) {
    use png_crate::DecodingError as E;
    let code = match &e {
        // 解码器内部缓冲超预算 → 太大。
        E::LimitsExceeded => ERR_TOO_LARGE,
        // Parameter = 调用方缓冲/轮询用法不符。尺寸我们已预检，真撞上了如实报缓冲太小。
        E::Parameter(_) => ERR_BUFFER_TOO_SMALL,
        // Format = 不是合法 PNG；IoError = 内存流读不动了（截断）。两者都是"文件坏了"。
        E::Format(_) | E::IoError(_) => ERR_CORRUPT,
    };
    (code, format!("PNG 库报错 \"{e}\""))
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use png_crate::{BitDepth, ColorType, Encoder};
    use std::mem::size_of;

    const RED: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/red_1x1.png"
    ));
    const INTERLACED: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../testdata/golden/interlaced_16x12_rgb.png"
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

    /// 走宿主的两阶段协议，拿回 RGBA8。
    fn decode_via_decoder(data: &[u8]) -> Result<(u32, u32, Vec<u8>), (i32, String)> {
        let mut info = IvRenderOut::default();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if q != ERR_OK {
            return Err((q, last_err()));
        }
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
        if r != ERR_OK {
            return Err((r, last_err()));
        }
        Ok((info.actual_w, info.actual_h, buf))
    }

    fn last_err() -> String {
        let mut e = [0i8; 512];
        let need = unsafe { rupix_abi::write_last_error(e.as_mut_ptr(), e.len()) } as usize;
        let n = need.saturating_sub(1).min(e.len());
        String::from_utf8_lossy(&e[..n].iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }

    /// 编码一张 PNG（`samples` = 不含过滤字节的按行打包样本）。
    fn encode(
        w: u32,
        h: u32,
        color: ColorType,
        depth: BitDepth,
        samples: &[u8],
        palette: Option<&[u8]>,
        trns: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        if let Some(p) = palette {
            enc.set_palette(p.to_vec());
        }
        if let Some(t) = trns {
            enc.set_trns(t.to_vec());
        }
        let mut writer = enc.write_header().expect("写头");
        writer.write_image_data(samples).expect("写像素");
        writer.finish().expect("收尾");
        out
    }

    /// 把每像素一个样本压成 bit depth < 8 的按行字节（MSB first，PNG 规定）。
    fn pack(values: &[u8], depth: u8, w: usize, h: usize) -> Vec<u8> {
        let per_byte = 8 / depth as usize;
        let row_bytes = w.div_ceil(per_byte);
        let mut out = vec![0u8; row_bytes * h];
        for (i, &v) in values.iter().enumerate() {
            let (px, py) = (i % w, i / w);
            let slot = px % per_byte;
            let shift = 8 - depth as usize * (slot + 1);
            out[py * row_bytes + px / per_byte] |= v << shift;
        }
        out
    }

    /// 本解码器认识的每种源布局（16 种）与 core 的 `image` 路线逐字节比对。
    ///
    /// 冠军不许自称正确：两条路线用的是同一个 png crate，但**调用配置不同**
    /// （ALPHA|STRIP_16 vs image 自己的变换组合），差异只会在这里暴露。
    ///
    /// 8bit 源要求逐字节相等；16bit 源按 `Case::hi` 的两条断言走——实测本机
    /// `STRIP_16`（丢低字节）与 image 的 ÷257 四舍五入恒差 ≤1 LSB，这是口径差不是 bug。
    #[test]
    fn parity_with_core_image_route_across_layouts() {
        let w = 8usize;
        let h = 4usize;
        let n = w * h;

        // 确定性样本图案：每个像素都不同，任何通道错位都会露出来。
        let pat = |i: usize, mod_: usize| ((i * 37 + 11) % mod_) as u8;

        struct Case {
            label: &'static str,
            png: Vec<u8>,
            /// 16bit 源降位后解码器**应当**产出的精确字节（= 源样本的高字节）。8bit 族为 None。
            ///
            /// ★ 为什么 16bit 族不能与 `image` 逐字节相等：本解码器的 `STRIP_16` 丢低字节
            ///   （源 0x10B3 → 0x10 = 16），`image` 的 16→8 走 ≈÷257 四舍五入（→ 17）。
            ///   libpng 同样丢低字节，所以这是**口径差**、不是谁算错，本机实测差值恒 ≤1 LSB。
            ///   于是 16bit 族改成两条断言：① 解码器输出 == 源高字节（口径自查），
            ///   ② 与 image 路线逐字节差 ≤1 LSB。
            hi: Option<Vec<u8>>,
        }
        let mut cases: Vec<Case> = Vec::new();

        // ── 灰度族（库产出 GrayscaleAlpha，解码器做 2→4）─────────────────
        for (label, depth) in [("gray1", 1u8), ("gray2", 2), ("gray4", 4), ("gray8", 8)] {
            let max = (1usize << depth) - 1;
            let vals: Vec<u8> = (0..n).map(|i| pat(i, max + 1)).collect();
            let samples = if depth == 8 {
                vals
            } else {
                pack(&vals, depth, w, h)
            };
            let (d, ct) = (bit_depth(depth), ColorType::Grayscale);
            cases.push(Case {
                label,
                png: encode(w as u32, h as u32, ct, d, &samples, None, None),
                hi: None,
            });
        }
        // 灰度 + tRNS → 真 alpha，走灰度族的 alpha 分支。
        {
            let vals: Vec<u8> = (0..n).map(|i| pat(i, 256)).collect();
            cases.push(Case {
                label: "gray8_trns",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Grayscale,
                    BitDepth::Eight,
                    &vals,
                    None,
                    Some(&[vals[0], 0]),
                ),
                hi: None,
            });
        }
        // 灰度 + alpha 双通道 8bit（源就带 alpha）。
        {
            let mut samples = Vec::new();
            for i in 0..n {
                samples.push(pat(i, 256));
                samples.push((i * 53) as u8);
            }
            cases.push(Case {
                label: "grayalpha8",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::GrayscaleAlpha,
                    BitDepth::Eight,
                    &samples,
                    None,
                    None,
                ),
                hi: None,
            });
        }

        // 16bit 灰度族：源只有灰度（或灰度+alpha）两通道，解码器仍走 2→4 分支。
        for (label, with_alpha) in [("gray16", false), ("grayalpha16", true)] {
            let mut samples = Vec::new();
            let mut hi = Vec::new();
            for i in 0..n {
                let g = ((i * 761 + 33011) % 65536) as u16;
                samples.extend_from_slice(&g.to_be_bytes());
                let a = if with_alpha {
                    let a = ((i * 4093 + 9) % 65536) as u16;
                    samples.extend_from_slice(&a.to_be_bytes());
                    (a >> 8) as u8
                } else {
                    255
                };
                hi.extend_from_slice(&[(g >> 8) as u8, (g >> 8) as u8, (g >> 8) as u8, a]);
            }
            cases.push(Case {
                label,
                png: encode(
                    w as u32,
                    h as u32,
                    if with_alpha {
                        ColorType::GrayscaleAlpha
                    } else {
                        ColorType::Grayscale
                    },
                    BitDepth::Sixteen,
                    &samples,
                    None,
                    None,
                ),
                hi: Some(hi),
            });
        }

        // ── 彩色族（库直接产出 Rgba）───────────────────────────────────
        {
            let mut samples = Vec::new();
            for i in 0..n {
                samples.extend_from_slice(&[pat(i, 256), (i * 17) as u8, (i * 29) as u8]);
            }
            cases.push(Case {
                label: "rgb8",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Rgb,
                    BitDepth::Eight,
                    &samples,
                    None,
                    None,
                ),
                hi: None,
            });
        }
        {
            let mut samples = Vec::new();
            let mut hi = Vec::new();
            for i in 0..n {
                for c in 0..4 {
                    let v = (i * (c as usize + 7) * 9973) % 65536;
                    samples.extend_from_slice(&(v as u16).to_be_bytes());
                    hi.push((v >> 8) as u8);
                }
            }
            cases.push(Case {
                label: "rgba16_strip",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Rgba,
                    BitDepth::Sixteen,
                    &samples,
                    None,
                    None,
                ),
                hi: Some(hi),
            });
            for (label, mul) in [("rgb16_strip", 3usize), ("rgb16_strip2", 131)] {
                let mut s16 = Vec::new();
                let mut expect = Vec::new();
                for i in 0..n {
                    for c in 0..3usize {
                        let v = (i * (c + mul) * 9973) % 65536;
                        s16.extend_from_slice(&(v as u16).to_be_bytes());
                        expect.push((v >> 8) as u8);
                    }
                    expect.push(255); // EXPAND 补的 alpha = 0xFFFF，降位后 0xFF
                }
                cases.push(Case {
                    label,
                    png: encode(
                        w as u32,
                        h as u32,
                        ColorType::Rgb,
                        BitDepth::Sixteen,
                        &s16,
                        None,
                        None,
                    ),
                    hi: Some(expect),
                });
            }
        }

        // ── 调色板族（含 / 不含 tRNS）───────────────────────────────────
        {
            let palette: Vec<u8> = (0..16u8)
                .flat_map(|i| [i * 16, 255 - i * 8, i * 5])
                .collect();
            let vals: Vec<u8> = (0..n).map(|i| (i % 16) as u8).collect();
            cases.push(Case {
                label: "palette4",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Indexed,
                    BitDepth::Four,
                    &pack(&vals, 4, w, h),
                    Some(&palette),
                    None,
                ),
                hi: None,
            });
            let mut trns = vec![255u8; 16];
            trns[3] = 0;
            trns[9] = 128;
            cases.push(Case {
                label: "palette4_trns",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Indexed,
                    BitDepth::Four,
                    &pack(&vals, 4, w, h),
                    Some(&palette),
                    Some(&trns),
                ),
                hi: None,
            });
            // 8bit 调色板（每字节一个索引）。
            let idx8: Vec<u8> = (0..n).map(|i| (i % 16) as u8).collect();
            cases.push(Case {
                label: "palette8",
                png: encode(
                    w as u32,
                    h as u32,
                    ColorType::Indexed,
                    BitDepth::Eight,
                    &idx8,
                    Some(&palette),
                    None,
                ),
                hi: None,
            });
        }

        // ── 隔行（Adam7）：自制样本，已用 WIC 独立校验过像素 ────────────
        cases.push(Case {
            label: "interlaced_rgb8",
            png: INTERLACED.to_vec(),
            hi: None,
        });

        let mut checked = 0usize;
        for case in &cases {
            let (pw, ph, mine) = decode_via_decoder(&case.png).unwrap_or_else(|(code, msg)| {
                panic!("{}: 解码器解码失败 code={code} {msg}", case.label)
            });
            // 参照：core 现在走的 `image` 路线。
            let ref_img = image::ImageReader::new(std::io::Cursor::new(&case.png[..]))
                .with_guessed_format()
                .expect("image: 猜格式")
                .decode()
                .unwrap_or_else(|e| panic!("{}: image 路线失败 {e}", case.label));
            let ref_rgba = ref_img.into_rgba8();
            assert_eq!(
                (pw, ph),
                ref_rgba.dimensions(),
                "{}: 尺寸不一致",
                case.label
            );
            assert_eq!(
                mine.len(),
                ref_rgba.as_raw().len(),
                "{}: 字节数不一致",
                case.label
            );
            match &case.hi {
                // 16bit 源：口径自查 + 与 image 差 ≤1 LSB（见 Case::hi 注释）。
                Some(hi) => {
                    assert_eq!(
                        mine.as_slice(),
                        hi.as_slice(),
                        "{}: 解码器 16→8 不等于源高字节",
                        case.label
                    );
                    let worst = mine
                        .iter()
                        .zip(ref_rgba.as_raw())
                        .map(|(&a, &b)| a.abs_diff(b))
                        .max()
                        .unwrap_or(0);
                    assert!(
                        worst <= 1,
                        "{}: 与 image 路线差 {} LSB，超出 16→8 口径差预期",
                        case.label,
                        worst
                    );
                }
                // 8bit 源：两条路线必须逐字节相等。
                None => {
                    assert_eq!(
                        mine.as_slice(),
                        ref_rgba.as_raw().as_slice(),
                        "{}: 像素与 image 路线不一致",
                        case.label
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, cases.len(), "有用例没跑到");
        // 覆盖表写死在这里：新增布局必须同时进矩阵，否则这条断言会响。
        let labels: Vec<&str> = cases.iter().map(|c| c.label).collect();
        for want in [
            "gray1",
            "gray2",
            "gray4",
            "gray8",
            "gray8_trns",
            "grayalpha8",
            "gray16",
            "grayalpha16",
            "rgb8",
            "rgba16_strip",
            "rgb16_strip",
            "rgb16_strip2",
            "palette4",
            "palette4_trns",
            "palette8",
            "interlaced_rgb8",
        ] {
            assert!(labels.contains(&want), "支持矩阵缺少 {want}");
        }
    }

    fn bit_depth(d: u8) -> BitDepth {
        match d {
            1 => BitDepth::One,
            2 => BitDepth::Two,
            4 => BitDepth::Four,
            8 => BitDepth::Eight,
            16 => BitDepth::Sixteen,
            _ => panic!("非法位深 {d}"),
        }
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), rupix_abi::IV_ABI_VERSION);
    }

    #[test]
    fn probe_accepts_png_signature_and_rejects_lookalikes() {
        assert_eq!(iv_probe(RED.as_ptr(), RED.len(), std::ptr::null()), 100);
        assert_eq!(
            iv_probe(INTERLACED.as_ptr(), INTERLACED.len(), std::ptr::null()),
            100
        );
        // 签名对但 IHDR 长度不是 13 → 不认领。
        let mut bad = RED.to_vec();
        bad[8] = 0x00;
        bad[11] = 0x0E;
        assert_eq!(iv_probe(bad.as_ptr(), bad.len(), std::ptr::null()), 0);
        // JPEG / 垃圾 / 太短。
        assert_eq!(
            iv_probe(b"\xFF\xD8\xFF\xE0garbage".as_ptr(), 12, std::ptr::null()),
            0
        );
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(
            iv_probe(SIGNATURE.as_ptr(), SIGNATURE.len(), std::ptr::null()),
            0
        );
    }

    #[test]
    fn query_phase_reports_size_without_decoding() {
        let (w, h, buf) = decode_via_decoder(RED).expect("静帧应能解");
        assert_eq!((w, h), (1, 1));
        assert_eq!(buf, vec![255, 0, 0, 255]);

        let mut info = IvRenderOut::default();
        let r = iv_render(
            RED.as_ptr(),
            RED.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_OK);
        assert_eq!(info.bytes_needed, 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.bit_depth, 8);
        // 语料没有 `eXIf` → 报 1；`already_oriented = 0` 是契约值（本解码器从不摆正像素）。
        assert_eq!((info.orientation, info.already_oriented), (1, 0));
        assert_eq!((info.frame_count, info.page_count), (1, 1));
        assert_eq!(info.icc_len, 0);
    }

    /// 只跑查询阶段，拿回申报的那一枚 `IvRenderOut`。
    fn query(data: &[u8]) -> IvRenderOut {
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
        info
    }

    /// PNG 块尾 CRC：IEEE 802.3 反向多项式 0xEDB88320，覆盖**类型 + 数据**（不含长度字段）。
    fn chunk_crc(ty_and_data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in ty_and_data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    /// 在 IHDR 之后插一枚 `eXIf`，里面只带一条 274 Orientation = `v`（元数据块必须在 IDAT 前）。
    fn png_with_orientation(v: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 紧跟 8 字节头
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 一条标签
        tiff.extend_from_slice(&274u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        tiff.extend_from_slice(&v.to_le_bytes()); // 值落在 4 字节偏移字段的低半部
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"eXIf");
        chunk.extend_from_slice(&tiff);
        chunk.extend_from_slice(&chunk_crc(&chunk[4..]).to_be_bytes());
        // IHDR 数据定长 13 字节：签名 8 + 长度 4 + 类型 4 + 数据 13 + CRC 4。
        let ihdr_end = 8 + 4 + 4 + 13 + 4;
        let mut out = RED[..ihdr_end].to_vec();
        out.extend_from_slice(&chunk);
        out.extend_from_slice(&RED[ihdr_end..]);
        out
    }

    /// m48「竖拍照片不再横躺」在 PNG 上的落点：`eXIf` 里的 274 照实回报，转正交宿主。
    /// 顺手钉住两件事：插了元数据块不能把解码本身弄坏；没有 `eXIf` 时行为与今天相同。
    #[test]
    fn exif_chunk_orientation_is_reported_for_the_host_to_apply() {
        let data = png_with_orientation(6);
        let info = query(&data);
        assert_eq!(
            (info.actual_w, info.actual_h),
            (1, 1),
            "申报的仍是文件里的原始尺寸——摆正是宿主的事"
        );
        assert_eq!(
            (info.orientation, info.already_oriented),
            (6, 0),
            "读到 6 就报 6，并声明自己没转"
        );
        let (w, h, buf) = decode_via_decoder(&data).expect("带 eXIf 的 PNG 仍应能解");
        assert_eq!((w, h), (1, 1));
        assert_eq!(buf, vec![255, 0, 0, 255], "插块不许动像素");

        // 对照组：原语料没有 `eXIf` → 1/0。
        let plain = query(RED);
        assert_eq!((plain.orientation, plain.already_oriented), (1, 0));
    }

    /// 让位链的前提：不做的活必须用 `ERR_UNSUPPORTED` 主动让位，而不是硬出一张错的。
    #[test]
    fn apng_is_declined_with_unsupported_not_decoded_as_still() {
        let mut out = Vec::new();
        {
            let mut enc = Encoder::new(&mut out, 2, 2);
            enc.set_color(ColorType::Rgba);
            enc.set_depth(BitDepth::Eight);
            enc.set_animated(2, 0).expect("设动画");
            let mut writer = enc.write_header().expect("写头");
            let px: Vec<u8> = (0..16).map(|i| (i * 15) as u8).collect();
            writer.write_image_data(&px).expect("写帧 1");
            writer.finish().expect("收尾");
        }
        // 参照自检：png crate 自己认定这是 APNG（否则这条测试就没测到让位）。
        let reader = open(&out).expect("APNG 样本应当能读头");
        assert!(reader.info().is_animated(), "样本没做成 APNG");

        let mut info = IvRenderOut::default();
        let r = iv_render(
            out.as_ptr(),
            out.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_UNSUPPORTED, "APNG 应当主动让位");
        assert!(
            !last_err().starts_with("png: "),
            "规范 §六 第 11 条：不得自带解码器名前缀，实际「{}」",
            last_err()
        );
    }

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 2];
        let r = iv_render(
            INTERLACED.as_ptr(),
            INTERLACED.len(),
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
            budget_bytes: 16, // 16x12x4 = 768
            ..req()
        };
        assert_eq!(
            iv_render(
                INTERLACED.as_ptr(),
                INTERLACED.len(),
                &r,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null()
            ),
            ERR_TOO_LARGE
        );
    }

    /// 截断的 IDAT：文件坏了 → `ERR_CORRUPT`（不能让让位链误以为"我不会"而挨个试 DLL）。
    #[test]
    fn truncated_stream_is_corrupt_not_hung() {
        let full = decode_via_decoder(INTERLACED).expect("完整样本应能解");
        let cut = &INTERLACED[..INTERLACED.len() / 2];
        let mut info = IvRenderOut::default();
        let q = iv_render(
            cut.as_ptr(),
            cut.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        // 头还完整时查询能拿到尺寸；真解码必须失败且绝不能是 panic。
        if q == ERR_OK {
            let mut buf = vec![0u8; info.bytes_needed as usize];
            let r = iv_render(
                cut.as_ptr(),
                cut.len(),
                &req(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut info,
                std::ptr::null(),
            );
            assert_ne!(r, ERR_PANIC, "截断数据绝不能走到 panic 分支");
            assert_eq!(
                r,
                ERR_CORRUPT,
                "截断应归'文件坏了'，实际 code={r} err={}",
                last_err()
            );
        }
        assert_eq!(full.0, 16);
        assert_eq!(full.1, 12);
    }

    #[test]
    fn garbage_after_signature_is_corrupt() {
        let mut junk = SIGNATURE.to_vec();
        junk.extend_from_slice(
            b"IHDR\x00\x00\x00\x0d\x00\x00\x00\x04\x00\x00\x00\x04\x08\x02\x00\x00\x00",
        );
        junk.resize(120, 0xA5);
        let mut info = IvRenderOut::default();
        let r = iv_render(
            junk.as_ptr(),
            junk.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_ne!(r, ERR_OK);
        assert_ne!(r, ERR_PANIC);
        assert!(!last_err().is_empty(), "失败路径必须留下可读原因");
    }

    /// 自己算 CRC 改 IHDR 尺寸，让畸形头通过库的校验，看我们自己的闸是否真挡住。
    fn crc32(data: &[u8]) -> u32 {
        let mut c: u32 = 0xFFFF_FFFF;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    /// G5（输入线性）盲点：头里写巨大尺寸，必须在分配前被 MAX_PIXELS 挡住。
    ///
    /// 尺寸取 100000x100000（10 GPixel）而不是 0xFFFFFFFF：后者光"每行缓冲"就要
    /// 几十 GB，会在库的 read_info 里先炸掉，测不到我们自己的上限那一闸。
    #[test]
    fn huge_dimensions_rejected_before_alloc() {
        let mut bad = INTERLACED.to_vec();
        // IHDR 数据区起点 = 8(签名)+4(长度)+4(类型) = 16。
        bad[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        bad[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        // 块尾 4 字节 CRC 必须重算，否则库先报 CRC 不符（那就只测到库的校验）。
        let crc = crc32(&bad[12..29]);
        bad[29..33].copy_from_slice(&crc.to_be_bytes());
        let mut info = IvRenderOut::default();
        let r = iv_render(
            bad.as_ptr(),
            bad.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, ERR_TOO_LARGE, "超过 64 MPixel 必须在分配前挡下");
        assert_eq!(last_err(), "图片超过 6400 万像素的解码器上限");
    }

    /// 用编码器写一张带 `iCCP` 的 1×1 PNG（库负责压缩配置块）。
    fn png_with_icc(profile: &[u8]) -> Vec<u8> {
        let mut info = png_crate::Info::with_size(1, 1);
        info.icc_profile = Some(std::borrow::Cow::Owned(profile.to_vec()));
        let mut out = Vec::new();
        {
            let mut enc = Encoder::with_info(&mut out, info).expect("编码器初始化");
            enc.set_color(ColorType::Rgba);
            enc.set_depth(BitDepth::Eight);
            let mut w = enc.write_header().expect("写 PNG 头");
            w.write_image_data(&[255, 0, 0, 255]).expect("写像素");
        }
        out
    }

    /// m53-J：`iCCP` 原样透传（库已解压），像素段与 ICC 段相邻但不重叠。
    #[test]
    fn icc_profile_is_declared_and_appended_after_pixels() {
        let profile: Vec<u8> = (0..300u32).map(|i| (i * 11 % 253) as u8).collect();
        let data = png_with_icc(&profile);
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

        // 无 iCCP 的语料：申报 0（与历史行为相同）。
        let mut info = IvRenderOut::default();
        assert_eq!(
            iv_render(
                RED.as_ptr(),
                RED.len(),
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
