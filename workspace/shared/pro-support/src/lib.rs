//! RuPix pro 档共用的小工具：尺寸闸、RGBA 缓冲、两阶段输出回填。
//!
//! 这不是解码器（没有 `cdylib`、没有 `iv_*` 导出），只被 `singles/pro-*`
//! 与 `decoders/pro` 共享，避免每个成员各抄一份边界代码。

use rupix_abi::*;

/// 所有解码器同口径的分配闸（64 MP）。
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// 检查尺寸/预算，返回 `(w, h, bytes_needed)`。
///
/// 预算按整图 RGBA 字节数校验（与整解口径一致；缩放腿不能拿小图绕过闸）。
pub fn check_dims(w: u32, h: u32, budget: u32) -> Result<(u32, u32, u64), (i32, &'static str)> {
    if w == 0 || h == 0 {
        return Err((ERR_CORRUPT, "文件头声明的宽或高为 0"));
    }
    let npixels = w as u64 * h as u64;
    if npixels > MAX_PIXELS {
        return Err((ERR_TOO_LARGE, "图片超过 6400 万像素的解码上限"));
    }
    let bytes = npixels * 4;
    if budget != 0 && bytes > budget as u64 {
        return Err((ERR_TOO_LARGE, "图片太大，超出宿主给的内存预算"));
    }
    Ok((w, h, bytes))
}

/// 读 `IvRenderReq.budget_bytes`（`req` 可为空）。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn budget_of(req: *const IvRenderReq) -> u32 {
    if req.is_null() {
        0
    } else {
        unsafe { (*req).budget_bytes }
    }
}

/// 写回 `IvRenderOut`。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn fill_info(
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    bytes_needed: u64,
    orientation: i32,
    icc_len: u32,
) {
    if info.is_null() {
        return;
    }
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = 8;
    oi.orientation = orientation;
    oi.already_oriented = 0;
    oi.frame_count = 1;
    oi.frame_delay_ms = 0;
    oi.page_count = 1;
    oi.icc_len = icc_len;
}

/// 两阶段出口：校验缓冲、写像素与 ICC、填 info。
///
/// 调用方已保证 `w*h*4 == px.len()`。返回 `ERR_OK` 或 `ERR_BUFFER_TOO_SMALL`。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
#[allow(clippy::too_many_arguments)] // C ABI 的两阶段出口形状固定，不再打包
pub fn write_rgba(
    out: *mut u8,
    out_cap: usize,
    px: &[u8],
    icc: &[u8],
    info: *mut IvRenderOut,
    w: u32,
    h: u32,
    orientation: i32,
) -> i32 {
    let bytes = px.len() as u64;
    let icc_len = icc.len() as u64;
    if out.is_null() || info.is_null() {
        set_last_error("内部错误（空输出指针）");
        return ERR_BUFFER_TOO_SMALL;
    }
    if (out_cap as u64) < bytes + icc_len {
        set_last_error("内部错误（宿主给的缓冲小于申报值）");
        return ERR_BUFFER_TOO_SMALL;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(px.as_ptr(), out, px.len());
        if !icc.is_empty() {
            std::ptr::copy_nonoverlapping(icc.as_ptr(), out.add(px.len()), icc.len());
        }
    }
    fill_info(info, w, h, bytes, orientation, icc_len as u32);
    ERR_OK
}

/// RGB8 紧排 → RGBA8（alpha=255）。
pub fn rgba_from_rgb(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() / 3 * 4);
    for c in px.chunks_exact(3) {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    out
}

/// 灰度 8 位 → RGBA8（alpha=255）。
pub fn rgba_from_gray(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &g in px {
        out.extend_from_slice(&[g, g, g, 255]);
    }
    out
}

/// CMYK8（Adobe 口径：0 = 满墨）→ RGBA8。
pub fn rgba_from_cmyk(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() / 4 * 4);
    for c in px.chunks_exact(4) {
        let k = 255 - c[3] as u16;
        let r = (255 - c[0] as u16) * k / 255;
        let g = (255 - c[1] as u16) * k / 255;
        let b = (255 - c[2] as u16) * k / 255;
        out.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
    }
    out
}

/// RGBA8 紧排原样拷贝（只补长度检查）。
pub fn rgba_passthrough(px: &[u8]) -> Vec<u8> {
    px.to_vec()
}
