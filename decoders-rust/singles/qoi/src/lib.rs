//! RuPix 外挂解码器：Quite OK Image (QOI) → RGBA8。
//!
//! 这个 crate 是一个自包含的 `cdylib`：ABI 契约来自 [`rupix_abi`]（唯一真源），
//! panic gate / last-error / 固定导出由 [`rupix_abi::export_decoder!`]
//! 一行生成。本文件只保留 QOI 规范的解码实现（零第三方依赖，只用 std）。
//!
//! 所有导出函数都套在 panic gate 里：任何内部 panic 都会被 `catch_unwind`
//! 拦截并转成错误码，宿主因此可以安全地隔离坏解码器。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印“正在创建库 ... 和对象 ...”，rustc 默认把
// 链接器 stdout 当作 lint 报出。这里显式静音，以满足“零警告”要求。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// `guard` 与 `c_char` 是 impl 层就在用的（合并库也要），只有自述宏属独立解码器形态。
#[cfg(feature = "standalone")]
use rupix_abi::export_decoder;
use rupix_abi::guard;
use std::ffi::c_char;

// ---------------------------------------------------------------------------
// 固定导出（ABI 自述 + 三个样板函数）
// ---------------------------------------------------------------------------

#[cfg(feature = "standalone")]
export_decoder! {
    id: "qoi",
    name: "Quite OK Image",
    extensions: "qoi",
    description: "解码 Quite OK Image (QOI) 格式",
    is_vector: 0,
    has_alpha: 1,
}

const BIT_DEPTH_8: u32 = 8;

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render（本解码器的业务部分）
// ---------------------------------------------------------------------------

/// 魔数判定。`ext` 对 QOI 无意义（有 `qoif` 签名就够），只为与其它解码器同签名。
///
/// ★ 签名与 `iv_probe` **逐字相同**（裸指针，不是切片）：合并库里的 12 个 `probe_impl`
/// 必须能塞进同一张路由表，参数形状不一致就得在网关里写特例。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        i32::from(slice.starts_with(b"qoif")) * 100
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_impl(data, len, ext)
}

/// 两阶段渲染。`out == None` = 查询阶段；`info == None` = 宿主传了空指针（错误）。
fn render_body(
    data: &[u8],
    budget: u32,
    out: Option<&mut [u8]>,
    info: Option<&mut IvRenderOut>,
) -> i32 {
    // 两阶段共用：先解析 14 字节头。
    let header = match parse_header(data) {
        Ok(h) => h,
        Err(e) => {
            set_last_error(e.msg());
            return e.code();
        }
    };

    let npixels = header.width as u64 * header.height as u64;
    let bytes_needed = npixels * 4;

    if budget != 0 && bytes_needed > budget as u64 {
        set_last_error("图片太大，超出宿主给的内存预算");
        return ERR_TOO_LARGE;
    }

    let Some(info) = info else {
        set_last_error("内部错误（宿主传入空输出指针）");
        return ERR_BUFFER_TOO_SMALL;
    };

    // 查询阶段：没有输出缓冲，只回报尺寸/格式。
    let Some(out) = out else {
        fill_info(info, header.width, header.height, bytes_needed);
        return ERR_OK;
    };

    // 渲染阶段。
    if (out.len() as u64) < bytes_needed {
        set_last_error("内部错误（宿主给的缓冲小于申报值）");
        return ERR_BUFFER_TOO_SMALL;
    }

    // QOI 没有缩放解码：始终原生尺寸，由宿主缩放。
    let (_h, pixels) = match decode(data) {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e.msg());
            return e.code();
        }
    };

    debug_assert!(pixels.len() as u64 == bytes_needed);
    out[..bytes_needed as usize].copy_from_slice(&pixels);
    fill_info(info, header.width, header.height, bytes_needed);
    ERR_OK
}

/// 两阶段渲染的 ABI 形态（与 `iv_render` 逐字同签名），合并库直接调这一层。
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
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };
        let out_slice = if out.is_null() {
            None
        } else {
            Some(unsafe { std::slice::from_raw_parts_mut(out, out_cap) })
        };
        render_body(input, budget, out_slice, Some(unsafe { &mut *info }))
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
}

#[inline]
fn fill_info(info: &mut IvRenderOut, w: u32, h: u32, bytes_needed: u64) {
    let oi = info;
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = 1;
    oi.already_oriented = 1;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1; // v4：单页；ICC 不透传。
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// QOI 解码器（规范：https://qoiformat.org/qoi-specification.pdf）
// ---------------------------------------------------------------------------

const QOI_OP_INDEX: u8 = 0x00; // 0b00xxxxxx
const QOI_OP_DIFF: u8 = 0x40; // 0b01xxxxxx
const QOI_OP_LUMA: u8 = 0x80; // 0b10xxxxxx
const QOI_OP_RUN: u8 = 0xc0; // 0b11xxxxxx
const QOI_OP_RGB: u8 = 0xfe;
const QOI_OP_RGBA: u8 = 0xff;
const QOI_MASK_2: u8 = 0xc0;

const HEADER_LEN: usize = 14;
/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。
///
/// 超尺寸报 `ERR_TOO_LARGE`，**不是** `ERR_CORRUPT`：宿主侧两者都中止下探，但
/// UI 文案不同（「太大」vs「文件已损坏」），规范 §五 校验表把这格钉死了，
/// 其余 11 个解码器同场景一律 TOO_LARGE。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct QoiHeader {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug)]
enum QoiError {
    Unrecognized,
    Corrupt,
    Unsupported,
    Oom,
    TooLarge,
}

impl QoiError {
    fn code(self) -> i32 {
        match self {
            QoiError::Unrecognized => ERR_UNRECOGNIZED,
            QoiError::Corrupt => ERR_CORRUPT,
            QoiError::Unsupported => ERR_UNSUPPORTED,
            QoiError::Oom => ERR_OOM,
            QoiError::TooLarge => ERR_TOO_LARGE,
        }
    }

    fn msg(self) -> &'static str {
        match self {
            QoiError::Unrecognized => "不是 QOI 文件（魔数不对）",
            QoiError::Corrupt => "QOI 数据损坏或被截断",
            QoiError::Unsupported => "该 QOI 子格式不支持",
            QoiError::Oom => "分配像素缓冲时内存不足",
            QoiError::TooLarge => "图片超过 6400 万像素的解码器上限",
        }
    }
}

fn parse_header(data: &[u8]) -> Result<QoiHeader, QoiError> {
    if data.len() < HEADER_LEN {
        return Err(QoiError::Corrupt);
    }
    if &data[0..4] != b"qoif" {
        return Err(QoiError::Unrecognized);
    }
    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let channels = data[12];
    let colorspace = data[13];

    if width == 0 || height == 0 {
        return Err(QoiError::Corrupt);
    }
    if channels != 3 && channels != 4 {
        return Err(QoiError::Unsupported);
    }
    if colorspace > 1 {
        return Err(QoiError::Unsupported);
    }
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or(QoiError::Corrupt)?;
    if pixels > MAX_PIXELS {
        return Err(QoiError::TooLarge);
    }
    Ok(QoiHeader { width, height })
}

fn qoi_hash(px: [u8; 4]) -> usize {
    let h = px[0] as usize * 3 + px[1] as usize * 5 + px[2] as usize * 7 + px[3] as usize * 11;
    h % 64
}

/// 完整解码为 RGBA8（非预乘），行主序，长度 `w*h*4`。
fn decode(data: &[u8]) -> Result<(QoiHeader, Vec<u8>), QoiError> {
    let header = parse_header(data)?;
    let npixels = header.width as usize * header.height as usize;
    let out_len = npixels * 4;

    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(out_len).map_err(|_| QoiError::Oom)?;
    out.resize(out_len, 0);

    let mut index = [[0u8; 4]; 64];
    let mut px = [0u8, 0u8, 0u8, 255u8];
    let mut p = HEADER_LEN;
    let end = data.len();

    let mut run: usize = 0;
    let mut written: usize = 0;

    while written < npixels {
        if run > 0 {
            run -= 1;
        } else {
            if p >= end {
                return Err(QoiError::Corrupt);
            }
            let b1 = data[p];
            p += 1;

            if b1 == QOI_OP_RGB {
                if p + 3 > end {
                    return Err(QoiError::Corrupt);
                }
                px[0] = data[p];
                px[1] = data[p + 1];
                px[2] = data[p + 2];
                p += 3;
            } else if b1 == QOI_OP_RGBA {
                if p + 4 > end {
                    return Err(QoiError::Corrupt);
                }
                px[0] = data[p];
                px[1] = data[p + 1];
                px[2] = data[p + 2];
                px[3] = data[p + 3];
                p += 4;
            } else {
                match b1 & QOI_MASK_2 {
                    QOI_OP_INDEX => {
                        px = index[(b1 & 0x3f) as usize];
                    }
                    QOI_OP_DIFF => {
                        let dr = ((b1 >> 4) & 0x03) as i32 - 2;
                        let dg = ((b1 >> 2) & 0x03) as i32 - 2;
                        let db = (b1 & 0x03) as i32 - 2;
                        px[0] = px[0].wrapping_add(dr as u8);
                        px[1] = px[1].wrapping_add(dg as u8);
                        px[2] = px[2].wrapping_add(db as u8);
                    }
                    QOI_OP_LUMA => {
                        if p >= end {
                            return Err(QoiError::Corrupt);
                        }
                        let b2 = data[p];
                        p += 1;
                        let dg = (b2 & 0x3f) as i32 - 32;
                        let dr = dg + (((b2 >> 4) & 0x0f) as i32) - 8;
                        let db = dg + ((b2 & 0x0f) as i32) - 8;
                        px[0] = px[0].wrapping_add(dr as u8);
                        px[1] = px[1].wrapping_add(dg as u8);
                        px[2] = px[2].wrapping_add(db as u8);
                    }
                    QOI_OP_RUN => {
                        // 本次循环输出 1 个像素，剩余 run 个在后续循环输出。
                        run = (b1 & 0x3f) as usize;
                    }
                    _ => return Err(QoiError::Corrupt),
                }
            }

            // 索引只在读到新 op 后更新（run 延续期间不更新）。
            index[qoi_hash(px)] = px;
        }

        let o = written * 4;
        out[o] = px[0];
        out[o + 1] = px[1];
        out[o + 2] = px[2];
        out[o + 3] = px[3];
        written += 1;
    }

    Ok((header, out))
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    use rupix_abi as kit;

    // 2x2 红色图（RGBA），手工构造的 QOI 字节流：
    //   header: "qoif" w=2 h=2 channels=4 colorspace=0
    //   pixel0: QOI_OP_RGB (0xFE) 255,0,0
    //   其余 3 个相同像素  QOI_OP_RUN (0xC2)  → run 长度 3
    //   end marker: 7x00 + 0x01
    const RED_2X2: &[u8] = &[
        0x71, 0x6f, 0x69, 0x66, // 'q','o','i','f'
        0x00, 0x00, 0x00, 0x02, // width = 2 (BE)
        0x00, 0x00, 0x00, 0x02, // height = 2 (BE)
        0x04, // channels = 4
        0x00, // colorspace = 0 (sRGB)
        0xfe, 0xff, 0x00, 0x00, // QOI_OP_RGB r=255 g=0 b=0
        0xc2, // QOI_OP_RUN, (0xc2 & 0x3f) = 2 → 再输出 2 个 → 共 3 个
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // end marker
    ];

    #[test]
    fn decodes_known_2x2_red() {
        let (h, px) = decode(RED_2X2).expect("decode ok");
        assert_eq!(h.width, 2);
        assert_eq!(h.height, 2);
        let expected: [u8; 16] = [
            255, 0, 0, 255, //
            255, 0, 0, 255, //
            255, 0, 0, 255, //
            255, 0, 0, 255, //
        ];
        assert_eq!(px.as_slice(), &expected);
    }

    #[test]
    fn probe_detects_magic() {
        assert_eq!(
            iv_probe(RED_2X2.as_ptr(), RED_2X2.len(), std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
        assert_eq!(iv_probe(RED_2X2.as_ptr(), 2, std::ptr::null()), 0);
    }

    #[test]
    fn bad_magic_is_unrecognized() {
        let mut bad = RED_2X2.to_vec();
        bad[0] = b'x';
        match parse_header(&bad) {
            Err(QoiError::Unrecognized) => {}
            other => panic!(
                "expected Unrecognized, got {:?}",
                other.err().map(|e| e.code())
            ),
        }
        assert_eq!(decode(&bad).err().map(|e| e.code()), Some(1));
    }

    #[test]
    fn truncated_header_is_corrupt() {
        assert_eq!(parse_header(b"qoif").err().map(|e| e.code()), Some(2));
        assert_eq!(parse_header(b"qo").err().map(|e| e.code()), Some(2));
    }

    #[test]
    fn zero_dimensions_rejected() {
        let mut z = RED_2X2.to_vec();
        // height = 0
        z[11] = 0;
        assert_eq!(decode(&z).err().map(|e| e.code()), Some(2));
    }

    #[test]
    fn bad_channels_is_unsupported() {
        let mut z = RED_2X2.to_vec();
        z[12] = 5;
        assert_eq!(decode(&z).err().map(|e| e.code()), Some(3));
    }

    #[test]
    fn truncated_pixel_stream_is_corrupt() {
        // 去掉像素数据，只剩 header → 解码时应报 corrupt。
        let header_only = &RED_2X2[..HEADER_LEN];
        assert_eq!(decode(header_only).err().map(|e| e.code()), Some(2));
    }

    #[test]
    fn oversized_dimensions_are_too_large() {
        // 10000×10000 = 100 MPixel > 64 MPixel 上限。必须报 TOO_LARGE，
        // 不是 CORRUPT——规范 §五 校验表钉死这一格，其余 11 家同场景一律
        // TOO_LARGE；报 2 会让宿主 UI 把"太大"说成"文件已损坏"。
        let mut z = RED_2X2.to_vec();
        z[4..8].copy_from_slice(&10000u32.to_be_bytes());
        z[8..12].copy_from_slice(&10000u32.to_be_bytes());
        assert_eq!(
            parse_header(&z).err().map(|e| e.code()),
            Some(ERR_TOO_LARGE)
        );
        // 端到端（查询阶段）同样必须是 7，且 last-error 给得出原因。
        let mut info = IvRenderOut::default();
        let c = iv_render(
            z.as_ptr(),
            z.len(),
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, ERR_TOO_LARGE);
        let mut ebuf = [0i8; 64];
        let n = unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        let got = unsafe { std::ffi::CStr::from_ptr(ebuf.as_ptr()) }.to_string_lossy();
        assert_eq!(
            got.as_ref(),
            "图片超过 6400 万像素的解码器上限",
            "回报 {n} 字节"
        );
    }

    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(iv_abi_version(), 5);
    }

    /// 两阶段一致性：查询回报的尺寸/bytes_needed 与渲染输出一致。
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
        let mut info = IvRenderOut::default();
        let c = iv_render(
            RED_2X2.as_ptr(),
            RED_2X2.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(c, 0);
        assert_eq!((info.actual_w, info.actual_h), (2, 2));
        assert_eq!(info.bytes_needed, 2 * 2 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);

        let mut buf = vec![0u8; info.bytes_needed as usize];
        let r = iv_render(
            RED_2X2.as_ptr(),
            RED_2X2.len(),
            &req,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(r, 0);
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        // 渲染后 last-error 保持为空（首次为空串）。
        let mut ebuf = [0i8; 16];
        let need = unsafe { kit::write_last_error(ebuf.as_mut_ptr(), ebuf.len()) };
        assert_eq!(need, 1, "last error 应为空串");
    }
}
