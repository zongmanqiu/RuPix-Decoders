//! RuPix doc member: EMF/EMZ through `emfsdk`'s portable raster renderer.
//!
//! `emfsdk` returns PNG bytes; this member decodes them to RGBA8 and enforces
//! the shared 64 MP / budget limits. EMZ is gzip-wrapped EMF and is accepted
//! only when the caller names `.emz` (gzip magic alone is too generic).
//!
//! ★ **超采样（2026-11-01）**：`emfsdk` 的栅格器没有抗锯齿——`set_pixel` 与
//! 扫描线填充都写硬像素，文字轮廓、圆、斜线放大后全是台阶（真机报障
//! "文字坑坑洼洼"）。它也没有视口参数，改不了它的绘制。所以这里在预算允许时
//! 按 2× 请求画布、拿到结果再盒式降采样：4 个样本一平均就是抗锯齿。
//! 预算外的超大档退回 1×（高倍率下硬边本身已经足够细，不值得再付 4 倍内存）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use emfsdk::render::{decode_metafile_as_raster_with_options, DecodedMetafile, RenderOptions};
use pro_support as support;
use rupix_abi::*;
use std::borrow::Cow;
use std::ffi::c_char;
use std::io::Read;

const EMZ_MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_PIXELS: u32 = 64 * 1024 * 1024;
/// 2× 超采样只在"2× 后的画布"落在这个像素预算内时做（≈ 12 MP ≈ 48 MB RGBA）。
const SUPER_SAMPLE_BUDGET: u64 = 12 * 1024 * 1024;

fn is_gzip(slice: &[u8]) -> bool {
    slice.starts_with(&[0x1F, 0x8B])
}

fn ext_str(ext: *const c_char) -> Option<String> {
    if ext.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ext) }
        .to_str()
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let mut out = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(data).take(EMZ_MAX_BYTES as u64 + 1);
    decoder
        .read_to_end(&mut out)
        .map_err(|_| (ERR_CORRUPT, "EMZ 的 gzip 包装损坏"))?;
    if out.len() > EMZ_MAX_BYTES {
        return Err((ERR_TOO_LARGE, "EMZ 解包后超过 64 MiB 上限"));
    }
    Ok(out)
}

fn payload(data: &[u8]) -> Result<Cow<'_, [u8]>, (i32, &'static str)> {
    if is_gzip(data) {
        Ok(Cow::Owned(gunzip(data)?))
    } else {
        Ok(Cow::Borrowed(data))
    }
}

fn is_emf(bytes: &[u8]) -> bool {
    matches!(
        emfsdk::Metafile::from_bytes(bytes),
        Ok(emfsdk::Metafile::Emf(_))
    )
}

fn render_metafile(
    bytes: &[u8],
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let (target_w, target_h) = if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    };
    if target_w == 0 || target_h == 0 {
        return render_at(bytes, target_w, target_h, 1, req);
    }
    let pixels_2x = target_w as u64 * 2 * target_h as u64 * 2;
    let factor = if pixels_2x <= SUPER_SAMPLE_BUDGET {
        2
    } else {
        1
    };
    render_at(bytes, target_w, target_h, factor, req)
}

/// 按 `factor` 倍画布渲染；拿到 2× 结果时盒式降采样回目标尺寸（抗锯齿）。
fn render_at(
    bytes: &[u8],
    target_w: u32,
    target_h: u32,
    factor: u32,
    req: *const IvRenderReq,
) -> Result<(Vec<u8>, u32, u32), (i32, &'static str)> {
    let options = RenderOptions {
        target_width_px: (target_w > 0).then_some(target_w * factor),
        target_height_px: (target_h > 0).then_some(target_h * factor),
        max_pixels: Some(MAX_PIXELS),
    };
    let DecodedMetafile { data, .. } =
        decode_metafile_as_raster_with_options(bytes, Some("image/emf"), options)
            .map_err(|_| (ERR_CORRUPT, "EMF 光栅化失败"))?
            .ok_or((ERR_CORRUPT, "EMF 头无效"))?;
    let image = image::load_from_memory(&data).map_err(|_| (ERR_CORRUPT, "EMF 光栅结果无效"))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    if factor > 1 && w == target_w * factor && h == target_h * factor {
        let pixels = downsample(rgba.as_raw(), w, h, factor);
        let (_, _, expected) = support::check_dims(target_w, target_h, support::budget_of(req))?;
        if pixels.len() as u64 != expected {
            return Err((ERR_CORRUPT, "EMF 降采样尺寸与像素缓冲不一致"));
        }
        return Ok((pixels, target_w, target_h));
    }
    // 位图成员只会回自然尺寸（它不认画布大小），原样返回。
    let (_, _, expected) = support::check_dims(w, h, support::budget_of(req))?;
    let pixels = rgba.into_raw();
    if pixels.len() as u64 != expected {
        return Err((ERR_CORRUPT, "EMF 光栅尺寸与像素缓冲不一致"));
    }
    Ok((pixels, w, h))
}

/// `factor × factor` 盒式平均（只支持整数倍）：4 个样本一平均就是抗锯齿。
fn downsample(src: &[u8], w: u32, h: u32, factor: u32) -> Vec<u8> {
    let (dw, dh) = (w / factor, h / factor);
    let n = factor * factor;
    let mut out = Vec::with_capacity((dw * dh * 4) as usize);
    for y in 0..dh {
        for x in 0..dw {
            let mut acc = [0u32; 4];
            for sy in 0..factor {
                for sx in 0..factor {
                    let i = (((y * factor + sy) * w + x * factor + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            for c in acc {
                out.push(((c + n / 2) / n) as u8);
            }
        }
    }
    out
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 8 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let Some(payload) = payload(slice).ok() else {
            return 0;
        };
        if is_emf(&payload) {
            100
        } else {
            0
        }
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
#[allow(clippy::too_many_arguments)]
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let ext = ext_str(ext);
        let payload = match payload(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if !is_emf(&payload) {
            set_last_error("不是 EMF/EMZ 字节特征");
            return ERR_UNRECOGNIZED;
        }
        if ext.as_deref() == Some("emz") && !is_gzip(slice) {
            set_last_error("EMZ 缺少 gzip 包装");
            return ERR_CORRUPT;
        }
        let (rgba, w, h) = match render_metafile(&payload, req) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, rgba.len() as u64, 1, 0);
            return ERR_OK;
        }
        support::write_rgba(out, out_cap, &rgba, &[], info, w, h, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emfsdk::{EmfMetafile, EmfRecord, EMR_EOF, EMR_HEADER};

    fn minimal_emf() -> Vec<u8> {
        let mut data = vec![0; 100];
        data[8..12].copy_from_slice(&1i32.to_le_bytes());
        data[12..16].copy_from_slice(&1i32.to_le_bytes());
        data[32..36].copy_from_slice(&emfsdk::emf::EMF_SIGNATURE.to_le_bytes());
        let header = EmfRecord::new(EMR_HEADER, data);
        let eof = EmfRecord::new(EMR_EOF, vec![0; 12]);
        EmfMetafile {
            records: vec![header, eof],
            trailing_data: Vec::new(),
        }
        .to_bytes()
        .expect("minimal EMF")
    }

    #[test]
    fn rejects_non_emf_bytes() {
        assert_eq!(probe_impl(b"not emf".as_ptr(), 7, std::ptr::null()), 0);
    }

    /// 2×2 → 1×1 的盒式平均：这条抗锯齿路径只有一个判据，就是"四取一"。
    #[test]
    fn downsample_averages_2x2_blocks() {
        let src = [
            0u8, 0, 0, 255, //
            100, 0, 0, 255, //
            0, 200, 0, 255, //
            0, 0, 100, 255,
        ];
        assert_eq!(downsample(&src, 2, 2, 2), vec![25, 50, 25, 255]);
    }

    #[test]
    fn claims_and_queries_minimal_emf() {
        let data = minimal_emf();
        assert_eq!(probe_impl(data.as_ptr(), data.len(), std::ptr::null()), 100);
        let req = IvRenderReq::default();
        let mut info = IvRenderOut::default();
        assert_eq!(
            render_impl(
                data.as_ptr(),
                data.len(),
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_OK
        );
        assert!(info.actual_w > 0 && info.actual_h > 0);
    }

    const EMR_SELECTOBJECT: u32 = 37;
    const EMR_EXTCREATEFONTINDIRECTW: u32 = 82;
    const EMR_EXTTEXTOUTW: u32 = 84;

    /// 一张只写一行字的 EMF：先建字体对象（字族名进 LOGFONTW），选中它，再 ExtTextOutW。
    fn text_emf(text: &str, family: &str, width: i32, height: i32) -> Vec<u8> {
        let mut header = vec![0u8; 100];
        header[0..4].copy_from_slice(&0i32.to_le_bytes());
        header[4..8].copy_from_slice(&0i32.to_le_bytes());
        header[8..12].copy_from_slice(&(width - 1).to_le_bytes());
        header[12..16].copy_from_slice(&(height - 1).to_le_bytes());
        header[32..36].copy_from_slice(&emfsdk::emf::EMF_SIGNATURE.to_le_bytes());

        // 字高取负 = 按字符高度（EMF 的惯例）；lfFaceName 是 32 个 UTF-16 码元的定长块。
        let mut logfont = vec![0u8; 96];
        logfont[0..4].copy_from_slice(&1u32.to_le_bytes());
        logfont[4..8].copy_from_slice(&(-64i32).to_le_bytes());
        for (index, unit) in family.encode_utf16().take(31).enumerate() {
            logfont[32 + index * 2..34 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }

        let mut select = vec![0u8; 4];
        select[0..4].copy_from_slice(&1u32.to_le_bytes());

        // EMRTEXT 从记录体偏移 28 开始；offString 是相对记录开头的偏移。
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut body = vec![0u8; 68 + units.len() * 2];
        body[28..32].copy_from_slice(&(width / 8).to_le_bytes());
        body[32..36].copy_from_slice(&(height * 2 / 3).to_le_bytes());
        body[36..40].copy_from_slice(&(units.len() as u32).to_le_bytes());
        body[40..44].copy_from_slice(&76u32.to_le_bytes());
        for (index, unit) in units.iter().enumerate() {
            body[68 + index * 2..70 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        // EMF 的每条记录都必须 4 字节对齐；补零只加在字符串之后，不影响 nChars。
        while !(body.len() + 8).is_multiple_of(4) {
            body.push(0);
        }

        EmfMetafile {
            records: vec![
                EmfRecord::new(EMR_HEADER, header),
                EmfRecord::new(EMR_EXTCREATEFONTINDIRECTW, logfont),
                EmfRecord::new(EMR_SELECTOBJECT, select),
                EmfRecord::new(EMR_EXTTEXTOUTW, body),
                EmfRecord::new(EMR_EOF, vec![0; 12]),
            ],
            trailing_data: Vec::new(),
        }
        .to_bytes()
        .expect("text EMF")
    }

    fn ink_of(text: &str) -> Vec<u8> {
        let data = text_emf(text, "Arial", 240, 160);
        let (pixels, _, _) = render_at(&data, 0, 0, 1, std::ptr::null()).expect("render text emf");
        pixels
    }

    /// ★ 上游 emfsdk 只用一款通用无衬线、且忽略 LOGFONT 的字族名：汉字 `glyph_index`
    /// 拿到 None，整段落进 5×7 点阵——而点阵对**所有**非 ASCII 字符都画同一个方框。
    /// 所以判据是"两个不同的汉字必须画出不同的字形"：老代码上它们逐字节相同。
    #[test]
    fn cjk_glyphs_come_from_a_real_font() {
        let installed = [
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/simsun.ttc",
            "C:/Windows/Fonts/msjh.ttc",
            "C:/Windows/Fonts/msgothic.ttc",
            "C:/Windows/Fonts/malgun.ttf",
        ]
        .iter()
        .any(|path| std::path::Path::new(path).exists());
        let zhong = ink_of("中");
        let tian = ink_of("田");
        assert!(
            zhong.iter().any(|channel| *channel < 128),
            "字没画出来（整幅空白）"
        );
        if installed {
            assert_ne!(zhong, tian, "两种汉字画成了同一张图 ⇒ 落回了 5×7 点阵兜底");
        } else {
            // 没有中日韩字库时找不到真字形是预期行为，这条分支本身就是"兜底仍出图"的判据。
            assert_eq!(zhong, tian, "无字库时应退化为同一点阵兜底");
        }
    }
}
