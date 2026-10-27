use super::develop::{auto_gain, black_levels, cam_to_srgb_wb, gamma_buckets, mul3, white_level};
use super::frame;

pub fn cfa_period(cfa: &rawler::cfa::CFA) -> (usize, usize) {
    let period_1d = |probe: &dyn Fn(usize) -> bool| -> usize {
        for p in 1..=48usize {
            if probe(p) {
                return p;
            }
        }
        48
    };
    let ph = period_1d(&|p| {
        (0..48).all(|r| (0..48).all(|c| cfa.color_at(r, c) == cfa.color_at(r + p, c)))
    });
    let pw = period_1d(&|p| {
        (0..48).all(|r| (0..48).all(|c| cfa.color_at(r, c) == cfa.color_at(r, c + p)))
    });
    (ph, pw)
}

/// 这一档能不能走分箱、以及分箱后的尺寸。返回 `Some((步长, 输出宽, 输出高))`，
/// `None` = 不走分箱（请求无档位、这档缩不出"不比显示尺寸小"的步长、原样是 f32、
/// 或图案不是 R/G/B 齐备的常规 CFA——四色传感器如 CYGM 的样本分配在本仓现有口径下
/// 会退化，宁可回全尺寸）。
///
/// ★ 尺寸与步长上限一律按**画幅**（[`frame`]）算，不按整块原样：申报给宿主的档位必须
/// 与 [`develop_binned`] 真正产出的那块对得上，而那条腿只出画幅。
pub fn bin_plan(
    img: &rawler::RawImage,
    target_w: u32,
    target_h: u32,
) -> Option<(usize, usize, usize)> {
    let f = frame(img);
    let (w, h) = (f.w, f.h);
    if target_w == 0 || target_h == 0 || w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = (target_w as usize, target_h as usize);
    if tw >= w && th >= h {
        return None; // 这一档本来就比原图大：没有可缩的余地
    }
    // 只有 u16 原样走分箱：f32 原样（部分 DNG）在本仓是极少数，不为它维护第二条累加口径。
    if !matches!(&img.data, rawler::RawImageData::Integer(_)) {
        return None;
    }
    let cfg = match &img.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) if img.cpp.max(1) == 1 => cfg,
        _ => return None,
    };
    let cfa = &cfg.cfa;
    let (ph, pw) = cfa_period(cfa);
    // 相位表按正方形索引（`(y0 % p)*p + x0 % p`），行列周期不等就没法用一张表描述。
    if ph != pw {
        return None;
    }
    let p = ph;
    // 图案必须是不带第四色的 R/G/B 三色 CFA。四色传感器（CYGM/QRBC 等）在本仓现有口径里
    // 被 `.min(2)` 压到蓝，分箱后那些"其实是青/黄"的样本会污染蓝道均值——宁可回全尺寸。
    let mut tile = [0usize; 3];
    for r in 0..p {
        for c in 0..p {
            match cfa.color_at(r, c) {
                k @ 0..=2 => tile[k] += 1,
                _ => return None,
            }
        }
    }
    if tile[0] == 0 || tile[1] == 0 || tile[2] == 0 {
        return None;
    }
    // 上限由**显示尺寸**定，不是由那一框定：宿主按短板维 fit，显示尺寸 = 原图 ×
    // `min(tw/w, th/h)`，所以「缩完不许让宿主放大」这条立约（与 TIFF 的 `scaled_step` 同一口径）
    // 落成 `step ≤ ⌊max(w/tw, h/th)⌋`。
    //
    // ★ 这里还要再防一层**方向**：解码器报的 orientation 可能是转置的（竖构图 RAW 的
    // 原样是横的），宿主拿到档位后要先转再 fit，于是约束变成按交换后的框算。
    // 取两种朝向里**更严**的那个上限——最坏只少省一档，但不会出现"档位转完比显示尺寸小、
    // 被宿主放大"这种发灰的呈现缺陷（判据只看呈现像素，见正本 §二）。
    let cap = (w / tw).max(h / th).min((w / th).max(h / tw));
    let cap = cap.min(w).min(h);
    // 从上限往下找**第一个**每个相位块都同时含 R/G/B 的步长（`bin_phase_counts`）。
    // 步长不必是周期的整数倍：3×3 的 Bayer 块含 4R4G1B，各色都有，照样能分箱——
    // 这条比"整数倍"宽，正是 X-Trans（6×6 图案、步长 3 就各色齐备）能上这条路的缘故。
    for step in (2..=cap).rev() {
        if bin_phase_counts(cfa, p, step).is_some() {
            return Some((step, w.div_ceil(step), h.div_ceil(step)));
        }
    }
    None
}

/// 步长 `step` 下**每个相位块**里 R/G/B 各有多少样本，按 `(y0 % p)*p + (x0 % p)` 索引。
///
/// 块起点只有 `p × p` 种相位（图案以 p 为周期），所以一张 p² 的小表就够描述整张图的
/// 取样构成。返回 `None` = 存在某个相位的块缺某道色——那块像素算不出该道色，只能回全尺寸。
///
/// 计数用一张 `(p+step)²` 的积分图（前缀和），每次询问 O(1)：直接遍历 p² 个块、每块 step²
/// 像素在大步长下是 p²·step² 量级，最坏（p=48）会到百万级，不值当。
fn bin_phase_counts(cfa: &rawler::cfa::CFA, p: usize, step: usize) -> Option<Vec<[u32; 3]>> {
    let n = p + step;
    // 每道的 (n+1)×(n+1) 前缀和：ps[k][(r+1)*(n+1)+(c+1)] = 左上 r×c 矩形内该道色的样本数。
    let mut ps = vec![[0u32; 3]; (n + 1) * (n + 1)];
    for r in 0..n {
        // 上一行的前缀和只读、当前行要写，两段不重叠 ⇒ split_at_mut 一次拿到（借用检查
        // 认不了"这两个切片其实不相交"）。`row_acc[i]` 即 ps[(r+1)*(n+1)+i]。
        let (up_to_prev, row_acc) = ps.split_at_mut((r + 1) * (n + 1));
        let prev_row = &up_to_prev[r * (n + 1)..(r + 1) * (n + 1)];
        let mut run = [0u32; 3];
        for c in 0..n {
            let ch = cfa.color_at(r, c);
            if ch > 2 {
                return None; // 第四色：这条路不伺候（与 `bin_plan` 同一判据，别只在一处设闸）
            }
            run[ch] += 1;
            for k in 0..3 {
                row_acc[c + 1][k] = prev_row[c + 1][k] + run[k];
            }
        }
    }
    let sum = |r0: usize, c0: usize, k: usize| -> u32 {
        // 半开区间 [r0, r0+step) × [c0, c0+step) 的该道色样本数。
        let (r1, c1) = (r0 + step, c0 + step);
        ps[r1 * (n + 1) + c1][k] + ps[r0 * (n + 1) + c0][k]
            - ps[r0 * (n + 1) + c1][k]
            - ps[r1 * (n + 1) + c0][k]
    };
    let mut out = vec![[0u32; 3]; p * p];
    for a in 0..p {
        for b in 0..p {
            let cell = &mut out[a * p + b];
            for (k, v) in cell.iter_mut().enumerate() {
                *v = sum(a, b, k);
            }
            if cell[0] == 0 || cell[1] == 0 || cell[2] == 0 {
                return None;
            }
        }
    }
    Some(out)
}

/// 分箱显影：`step × step` 的块内**同色样本求平均**，直接产出低清档 RGBA8。
///
/// 为什么用分箱而不是"隔 step 行取样"：CFA 上隔行取样会把采样点全落在同一种颜色上，
/// 低清档立刻起马赛克状伪色；分箱把一个块里 R/G/B 各自的样本求均值，**天然完成去马赛克**
/// 且不产生混叠——与"全尺寸显影后宿主面积平均"是同一种数学，只是不必先解出整张。
///
/// 与全尺寸那条腿有一处刻意的差别：本路先对一个块的同色样本求平均再归一化，
/// 全尺寸那条是逐样本 clamp 到 [0,1] 后再去马赛克。差别只在饱和端（块内含爆光样本时），
/// 平滑区完全一致；画质判据按正本 §二，落在**呈现像素**上量，不落在口径争论上。
pub fn develop_binned(
    img: &rawler::RawImage,
    step: usize,
) -> Result<(Vec<u8>, usize, usize), rawler::RawlerError> {
    let (sw, sh) = (img.width, img.height);
    let f = frame(img);
    let (w, h) = (f.w, f.h);
    let cfg = match &img.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) => cfg,
        _ => return Err(rawler::RawlerError::from("分箱显影只支持 CFA 图案")),
    };
    let cfa = &cfg.cfa;
    let (ph, pw) = cfa_period(cfa);
    if ph != pw || step < 2 || step > w || step > h || f.x + w > sw || f.y + h > sh {
        return Err(rawler::RawlerError::from("分箱步长不在合法范围"));
    }
    let p = ph;
    // 相位表：与 `bin_plan` 同一枚判据（谁缺哪道色、每块各道几个样本），不写第二份。
    let counts = bin_phase_counts(cfa, p, step)
        .ok_or_else(|| rawler::RawlerError::from("这一分箱步长下有相位块凑不齐 R/G/B"))?;
    let (ow, oh) = (w.div_ceil(step), h.div_ceil(step));
    // 电平口径与全尺寸那条腿**完全同一**（含 m82 删掉自动重归一这条）。
    let black = black_levels(img, 1);
    let white = white_level(img);

    // 每图案行的"列→通道"表（通道口径与 `demosaic_bilinear` 一致：`.min(2)`；
    // 能走到这里的图案已被 `bin_phase_counts` 验过只有 R/G/B，`.min(2)` 是恒等）。
    // 预计算它，是为了内层循环一次取数只做一次数组索引，不做取模。
    // ★ 按**绝对列号**建表（整块原样的宽度），因为块起点是画幅坐标——画幅左缘不一定是偶数。
    let chan_rows: Vec<Vec<u8>> = (0..p)
        .map(|pr| (0..sw).map(|x| cfa.color_at(pr, x).min(2) as u8).collect())
        .collect();

    // ── 1. 一整遍原始数据：按块累加同色 RAW 之和。────────────────────────
    // 外层按输出行走、内层按源行走：同一源行只扫一次，块内每像素只做一次数组索引。
    let pix = match &img.data {
        rawler::RawImageData::Integer(p) => p,
        _ => return Err(rawler::RawlerError::from("分箱显影只支持 u16 原样数据")),
    };
    // 块起点是**绝对坐标**：右/下缘凑不满一块就钳到画幅内最后一个整块（钳完仍在画幅里）。
    let x0s: Vec<usize> = (0..ow).map(|ox| f.x + (ox * step).min(w - step)).collect();
    let mut acc = vec![[0f64; 3]; ow * oh];
    for oy in 0..oh {
        let y0 = f.y + (oy * step).min(h - step);
        let base = oy * ow;
        for j in 0..step {
            let y = y0 + j;
            let chrow = &chan_rows[y % ph];
            let src = &pix[y * sw..(y + 1) * sw];
            for ox in 0..ow {
                let x0 = x0s[ox];
                let sums = &mut acc[base + ox];
                for k in 0..step {
                    let x = x0 + k;
                    sums[chrow[x] as usize] += src[x] as f64;
                }
            }
        }
    }

    // ── 2. 块均值 → 线性化 → 色彩矩阵（含白平衡）→ gamma。低清档只有百万级像素量，
    //        这一步的成本与输出尺寸同阶，与传感器尺寸无关。────────────────
    let gamma = gamma_buckets();
    // 与全尺寸那条腿**同一口径**：白平衡折进色彩矩阵，不在逐样本上乘。
    let cam_to_srgb = cam_to_srgb_wb(img);
    let mut linear = vec![0f32; ow * oh * 3];
    for oy in 0..oh {
        let py = (f.y + (oy * step).min(h - step)) % p;
        for ox in 0..ow {
            let px = x0s[ox] % p;
            let cnt = &counts[py * p + px];
            let sums = acc[oy * ow + ox];
            let i = (oy * ow + ox) * 3;
            let mut cam = [0f32; 3];
            for c in 0..3 {
                let mean = (sums[c] / cnt[c] as f64) as f32;
                let den = (white[c] - black[c]).max(1.0);
                cam[c] = ((mean - black[c]) / den).clamp(0.0, 1.0);
            }
            linear[i..i + 3].copy_from_slice(&mul3(&cam_to_srgb, cam));
        }
    }
    // The first screen often uses this binned path, while zoom/1:1 later uses
    // `develop`. They must share the same display exposure or the image visibly
    // changes brightness when the full-resolution request lands.
    let exposure = if img.make.is_empty() && img.model.is_empty() {
        1.0
    } else {
        auto_gain(&linear)
    };
    let mut out = vec![0u8; ow * oh * 4];
    for (i, d4) in out.chunks_exact_mut(4).enumerate() {
        let rgb = &linear[i * 3..i * 3 + 3];
        d4[0] = gamma.encode(rgb[0] * exposure);
        d4[1] = gamma.encode(rgb[1] * exposure);
        d4[2] = gamma.encode(rgb[2] * exposure);
        d4[3] = 255;
    }
    Ok((out, ow, oh))
}
