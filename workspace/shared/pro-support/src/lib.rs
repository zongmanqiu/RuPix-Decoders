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

// ---------------------------------------------------------------------------
// 矢量区域渲染（ABI v6）的共用件
// ---------------------------------------------------------------------------
//
// 这些不是"某个格式的知识"，而是**规范 §十六 的四条硬约定**在每个成员里都要
// 一模一样：指纹缓存键、线程内解析缓存、区域画布/密度换算、预乘→非预乘直写。
// 抄三份就是三个地方会各自漏一条（pro-wmf 已经有一份，见它的区域用例）。
//
// `doc_key` / `DocCache` / `region_canvas` 与 resvg 无关（PDF、PostScript 成员
// 也用它们），所以放在顶层；只有真正要画 resvg 场景的三个函数进 `vector`。

/// 文档字节的缓存键：FNV-1a 64 再混进长度（只当缓存键，不当安全哈希）。
///
/// 宿主同一份字节会连着换区域请求，键必须稳定且对单字节差异敏感。
pub fn doc_key(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^ (data.len() as u64)
}

/// 线程内的解析结果小缓存（宿主单解码线程 ⇒ 线程局部即可，不必上锁）。
///
/// 契约 §十六 第 4 条：同一份字节会**连续**用不同区域请求，解析结果必须在
/// 进程内缓存，不许每次重新解析文档。容量按"当前图 + 前后各一张"取。
pub struct DocCache<T> {
    cap: usize,
    slots: std::cell::RefCell<Vec<(u64, T)>>,
}

impl<T: Clone> DocCache<T> {
    /// `cap` 至少为 1；FIFO 淘汰（命中不调整顺序，够用且无额外分配）。
    pub const fn new(cap: usize) -> Self {
        Self {
            cap: if cap == 0 { 1 } else { cap },
            slots: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn get(&self, key: u64) -> Option<T> {
        self.slots
            .borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    }

    pub fn put(&self, key: u64, value: T) {
        let mut v = self.slots.borrow_mut();
        if let Some(slot) = v.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        v.push((key, value));
        while v.len() > self.cap {
            v.remove(0);
        }
    }
}

/// 区域请求的**画布与密度**（契约 §十六）。
///
/// 宿主给的 `target_w/target_h` 是"区域 × 密度"算出来的（两个方向同一个密度），
/// 所以正常情况下比例天然一致 ⇒ 直接按目标画布出图、密度取 `target_w / w`。
/// 比例真的不一致时（第三方宿主）**等比缩进**并把实际画布回填进
/// `actual_w/actual_h`——绝不拉伸变形（硬约定第 2 条）。
pub fn region_canvas(r: &IvRegionReq) -> (u32, u32, f32) {
    let (tw, th) = (r.target_w, r.target_h);
    if tw == 0 || th == 0 {
        // 1:1：画布就是区域本身。
        let cw = r.w.round().max(1.0) as u32;
        let ch = r.h.round().max(1.0) as u32;
        return (cw, ch, 1.0);
    }
    let dx = tw as f32 / r.w;
    let dy = th as f32 / r.h;
    if (dx - dy).abs() <= 1e-3 * dx.max(dy) {
        (tw, th, dx)
    } else {
        let d = dx.min(dy);
        (
            (r.w * d).round().max(1.0) as u32,
            (r.h * d).round().max(1.0) as u32,
            d,
        )
    }
}

#[cfg(feature = "resvg")]
pub mod vector {
    use rupix_abi::*;

    use super::check_dims;
    pub use super::{doc_key, region_canvas, DocCache};

    /// 文档坐标 → 区域画布的变换：先按密度缩放，再把区域左上角挪到画布原点。
    pub fn region_transform(r: &IvRegionReq, density: f32) -> resvg::tiny_skia::Transform {
        resvg::tiny_skia::Transform::from_row(
            density,
            0.0,
            0.0,
            density,
            -r.x * density,
            -r.y * density,
        )
    }

    /// 把 usvg 场景按 `transform` 光栅化进宿主缓冲（**非预乘** RGBA8）。
    ///
    /// 画布初始全透明 ⇒ 区域越出文档的部分天然是 alpha=0（硬约定第 3 条）。
    /// tiny-skia 输出预乘色，这里逐像素解预乘直写宿主缓冲（不再先分配整张 Vec）。
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
    pub fn rasterize_tree(
        tree: &resvg::usvg::Tree,
        w: u32,
        h: u32,
        transform: resvg::tiny_skia::Transform,
        out: *mut u8,
        out_cap: usize,
        bytes: u64,
    ) -> Result<(), (i32, &'static str)> {
        if out.is_null() || (out_cap as u64) < bytes {
            return Err((ERR_BUFFER_TOO_SMALL, "内部错误（宿主给的缓冲小于申报值）"));
        }
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "矢量画布分配失败"))?;
        resvg::render(tree, transform, &mut pixmap.as_mut());
        let src = pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes as usize) };
        for (d, c) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let a = c[3] as u16;
            if a == 0 || a == 255 {
                d.copy_from_slice(c);
            } else {
                let un = |v: u8| ((v as u16 * 255 + a / 2) / a).min(255) as u8;
                d[0] = un(c[0]);
                d[1] = un(c[1]);
                d[2] = un(c[2]);
                d[3] = c[3];
            }
        }
        Ok(())
    }

    /// 区域渲染的一整套（两阶段）：校验 → 画布 → 查询直接返回，写入阶段才光栅化。
    ///
    /// 返回 `(画布宽, 画布高, 字节数)`；`out == NULL` 时只算尺寸不碰像素。
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
    pub fn render_region(
        tree: &resvg::usvg::Tree,
        req: &IvRegionReq,
        out: *mut u8,
        out_cap: usize,
    ) -> Result<(u32, u32, u64), (i32, &'static str)> {
        if !(req.w > 0.0 && req.h > 0.0) {
            return Err((ERR_CORRUPT, "区域请求的宽或高不为正"));
        }
        let (cw, ch, density) = region_canvas(req);
        let (_, _, bytes) = check_dims(cw, ch, req.budget_bytes)?;
        if out.is_null() {
            return Ok((cw, ch, bytes));
        }
        rasterize_tree(
            tree,
            cw,
            ch,
            region_transform(req, density),
            out,
            out_cap,
            bytes,
        )?;
        Ok((cw, ch, bytes))
    }
}
