/// 画幅矩形：整块传感器原样里**该交回宿主的那一块**（绝对 CFA 像素坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    pub(crate) fn whole(w: usize, h: usize) -> Self {
        Self { x: 0, y: 0, w, h }
    }
}

/// 画幅在哪：`crop_area` 优先、回落 `active_area`、再回落整块原样——与 rawler 自己的
/// `CropDefault`（`imgop/develop.rs` 里 `crop_area.or(active_area)`）同一优先级。
/// ★ **只读元数据**，所以查询阶段（dummy）与渲染阶段（真解）算出同一个矩形——
/// m83 立的「申报尺寸＝宿主解释缓冲区的依据」靠这条撑着。
///
/// 元数据越界 ⇒ 钳进原样；裁出 0 宽或 0 高 ⇒ 当作没有画幅。除此之外**不加任何启发式**：
/// 遮光区不一定黑（实测一张 NEF 那圈写着满量程，原样均值 3311.7 ≈ 白电平 3880），
/// 所以也不按亮度反推裁切线（与 m82「查看器不替照片做自动色阶」同一条精神）。
pub fn frame(img: &rawler::RawImage) -> Frame {
    let (w, h) = (img.width, img.height);
    let Some(r) = img.crop_area.or(img.active_area) else {
        return Frame::whole(w, h);
    };
    let x0 = r.x().min(w);
    let y0 = r.y().min(h);
    let x1 = r.x().saturating_add(r.width()).min(w);
    let y1 = r.y().saturating_add(r.height()).min(h);
    if x1 <= x0 || y1 <= y0 {
        return Frame::whole(w, h);
    }
    Frame {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}
