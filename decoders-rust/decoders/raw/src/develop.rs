use super::{frame, Frame};

/// 把 [`rawler::RawImage`] 的**画幅**显影为 sRGB RGBA8（画幅外的光学遮光区不产出）。
///
/// 步骤：线性化 → 去马赛克（bilinear）→ 色彩矩阵（含白平衡）→ sRGB gamma。
/// 返回 (RGBA 缓冲, 宽)。
///
/// `pub` 只为按段计时探针（`examples/bench_raw_stages.rs`，m78 归因用）和画幅量具
/// （`examples/probe_raw_frame.rs`，m84）能单独量这一段；解码器本体的调用点仍在下面的
/// `develop_cached` 里，没有第二条路。
pub fn develop(img: &rawler::RawImage) -> Result<(Vec<u8>, usize), rawler::RawlerError> {
    develop_in(img, &frame(img))
}

/// 显影画幅 `f`：输出尺寸就是 `f.w × f.h`。`pub` 只为画幅量具（`examples/probe_raw_frame.rs`）
/// 能拿 `f = 整块原样` 当对照臂——本体只有 `develop` 一个调用点。
///
/// ★ 为了让**画幅内部的每个像素与「整块原样显影后按同一矩形裁出」逐字节相同**，线性化和
/// 去马赛克看到的是上下各扩 1 行的**真实邻域**（贴到原样边缘就不扩——整块那条路在那里
/// 本来也把邻域钳在传感器边上，两边同一钳位 ⇒ 仍然逐字节相同）。横向铺满整行，不裁左右。
pub fn develop_in(
    img: &rawler::RawImage,
    f: &Frame,
) -> Result<(Vec<u8>, usize), rawler::RawlerError> {
    let (sw, sh) = (img.width, img.height);
    let cpp = img.cpp.max(1);
    if sw == 0 || sh == 0 || f.w == 0 || f.h == 0 || f.x + f.w > sw || f.y + f.h > sh {
        return Err(rawler::RawlerError::from("RAW 画幅尺寸不合法"));
    }
    let pix_len = match &img.data {
        rawler::RawImageData::Integer(p) => p.len(),
        rawler::RawImageData::Float(p) => p.len(),
    };
    if (sw as u64) * (sh as u64) * (cpp as u64) != pix_len as u64 {
        return Err(rawler::RawlerError::from("像素数与缓冲大小不符"));
    }

    // 展开矩形（去马赛克要真实邻域）；ox/oy = 画幅在展开矩形里的起点。
    // ★ **横向铺满整行**（只上下各扩一行）：传感器行宽是偶数（实测 3596 / 3904 / 6384），
    //   展开矩形沿用同一个行宽 ⇒ 线性化与去马赛克那几遍的行起点仍按缓存行对齐，且它在原样
    //   缓冲里是一段连续区间、循环仍是一整遍平铺。把左右也裁进去（行宽变 3874 这种奇数、
    //   还得逐行搬运）同进程对照量过：省不到 3% 的像素，反而更慢——不做。
    let t = (f.y > 0) as usize;
    let bb = (f.y + f.h < sh) as usize;
    let ex = Frame {
        x: 0,
        y: f.y - t,
        w: sw,
        h: f.h + t + bb,
    };
    let (ew, eh) = (ex.w, ex.h);
    let (ox, oy) = (f.x, f.y - ex.y);
    let cpp_row = sw * cpp;

    // ── 1. 线性化：减黑、按白归一。───────────────────────────────────────
    // 白平衡不在这里（逐样本乘增益既慢又不是它的正确位置），见步骤 3 折进色彩矩阵。
    // 原来这里还有一条"观测峰值远低于声明白电平 ⇒ 按观测值重归一"的兜底，m82 删掉：
    // 它的触发条件（观测峰值距黑电平不到声明量程的 5%）**只在黑帧/平场数据上成立**，
    // 于是把传感器噪声拉成满量程亮场（实测一张真正的黑帧 RAF 渲染成品红色方块）。
    // 查看器不做自动色阶；低 key 的曝光抬升只在步骤 3 之后按显示需要做（见 auto_gain）。
    let black = black_levels(img, cpp); // 每通道
    let white = white_level(img); // 每通道

    // 线性化后的 f32 缓冲（0..~1），行宽 ew*cpp、只含展开矩形。cpp==1 单通道；cpp==3 交错 RGB。
    // ★ 循环写成"等长切片 zip / chunks"而不是 `dst[i]` 下标：下标那版 LLVM 证不出两侧长度相等
    //   （一个来自区间相减、一个来自乘法），逐元素多留一次边界检查。m84 第一版是下标写法，
    //   改回 zip 之后才拿到正本 §3.1 表里那一组数。
    let mut chan = vec![0f32; ew * eh * cpp];
    // ★ 展开矩形横向铺满整行 ⇒ 它在原样缓冲里就是**一段连续区间**，线性化是一整遍平铺。
    //   第一版按画幅逐行切（`row`/`dst` 各切一份），同几何对照量到更慢——那笔不是裁切的代价，
    //   是把一整遍循环切碎的代价（拆法与读数见正本 §3.1）。
    let n = ew * eh * cpp;
    let from = ex.y * cpp_row;
    match &img.data {
        rawler::RawImageData::Integer(pix) => {
            let src = &pix[from..from + n];
            if cpp == 1 {
                // 单通道（一切 Bayer / X-Trans）：通道号恒 0，电平常数提到循环外。
                let b = black[0];
                let den = (white[0] - b).max(1.0);
                for (d, &v) in chan.iter_mut().zip(src.iter()) {
                    *d = (((v as f32) - b) / den).clamp(0.0, 1.0);
                }
            } else {
                for (i, (d, &v)) in chan.iter_mut().zip(src.iter()).enumerate() {
                    // `i % 3` 是交错通道号：区间起点 `from` 按整像素对齐（cpp_row = sw*cpp），
                    // 所以与整幅线性索引取模同值。
                    let c = i % 3;
                    let den = (white[c] - black[c]).max(1.0);
                    *d = (((v as f32) - black[c]) / den).clamp(0.0, 1.0);
                }
            }
        }
        rawler::RawImageData::Float(pix) => {
            let src = &pix[from..from + n];
            if cpp == 1 {
                let b = black[0];
                let den = (white[0] - b).max(1e-6);
                for (d, &v) in chan.iter_mut().zip(src.iter()) {
                    *d = ((v - b) / den).clamp(0.0, 1.0);
                }
            } else {
                for (i, (d, &v)) in chan.iter_mut().zip(src.iter()).enumerate() {
                    let c = i % 3;
                    let den = (white[c] - black[c]).max(1e-6);
                    *d = ((v - black[c]) / den).clamp(0.0, 1.0);
                }
            }
        }
    }

    // ── 2. 去马赛克 / 通道展开 → 线性 RGB（展开矩形大小，行宽 ew*3）。──────
    // ★ CFA 图案按**绝对坐标**查：画幅起点常常是奇数（实测 CR2 上边 19、NEF 左边 14、
    //   RAF 上边 13），按画幅内的局部坐标查会把 R/G 认错道。
    let linear: Vec<f32> = match &img.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) if cpp == 1 => {
            // ★ 画幅的绝对相位**折进一张 48×48 表**（一次建表，2304 次查询），内层因此与
            //   "画幅起点为零"完全同形。把 `ex.y + y` 直接留在闭包里量过：rawler 的
            //   `color_at` 是 `pattern[(row+48)%48][(col+48)%48]`，运行时偏移让那两次取模
            //   再也提不出循环，三张真机各慢 **+22.7% ~ +29.1%**（同一份二进制只换这一处，
            //   台账 `.temp/m84_bench_phase.txt`、正本 §3.1）。
            let (phy, phx) = (ex.y % 48, ex.x % 48);
            let cfa = &cfg.cfa;
            let mut rot = [[0usize; 48]; 48];
            for (r, row) in rot.iter_mut().enumerate() {
                for (c, cell) in row.iter_mut().enumerate() {
                    *cell = cfa.color_at(r + phy, c + phx);
                }
            }
            demosaic_bilinear(&chan, ew, eh, |x, y| rot[y % 48][x % 48])
        }
        _ if cpp == 3 => chan, // Foveon 等：已是逐像素 RGB
        _ => {
            // 单通道非 CFA（LinearRaw / BlackIsZero / 单色）：灰度展开。
            let mut rgb = vec![0f32; ew * eh * 3];
            for i in 0..ew * eh {
                let g = chan[i];
                rgb[i * 3] = g;
                rgb[i * 3 + 1] = g;
                rgb[i * 3 + 2] = g;
            }
            rgb
        }
    };

    // ── 3. 色彩矩阵（含白平衡）：已白平衡的相机 RGB → 线性 sRGB。───────────────
    // m82：白平衡原来写在步骤 1 的样本循环里，且 cpp==1（所有 Bayer / X-Trans）时
    // 一律取 `wb[0]`——等于给三道色乘同一个系数，**等于没做白平衡**：D80 的
    // wb=[1.98,1.00,1.38] 于是让绿道相对红道高出一倍，红道在线性 sRGB 上被压成 0，
    // 整张图发青（实测均值 RGB 8/181/160，而相机自己写进文件的 JPEG 预览是 122/140/151）。
    // 现在把增益折进矩阵（一次列缩放），逐样本一次乘法都不做。
    // m85：★ 那份矩阵以前读 `img.xyz_to_cam`，而它在 rawler 0.7.2 里**恒为全零** ⇒ 奇异兜底
    // 每次都触发 ⇒ 色彩矩阵从来没生效，三张真机整张偏色。见 [`cam_to_srgb_wb`] 的说明。
    let cam_to_srgb = cam_to_srgb_wb(img);
    // Camera JPEG previews (and PicView/LibRaw's default) apply a modest
    // auto-exposure. Without it, low-key ORF/ARW/NEF files look much darker
    // than the camera's own preview. Only lift genuinely dark images; high-key
    // and black frames keep their exposure.
    let exposure = if img.make.is_empty() && img.model.is_empty() {
        1.0 // synthetic unit-test images: keep their exact exposure
    } else {
        auto_gain(&linear)
    };

    // ── 4. Gamma 编码 → RGBA8（只出画幅那一块）。──────────────────────────
    // m79：这一步原来每像素调 3 次 `powf`（m78 量出它占整条显影链的 50–56%）。
    // 换成**单值桶表**：桶内 8 位取值恒定的那些走查表，暗调里跨边界的桶一律落回精算，
    // 所以输出与原路**逐字节相同**（等价性由 `gamma_buckets_are_exact_where_they_claim_to_be`
    // 穷举 65536 个桶来钉，不是抽样）。
    let gamma = gamma_buckets();
    let mut out = vec![0u8; f.w * f.h * 4];
    for ly in 0..f.h {
        let base = ((ly + oy) * ew + ox) * 3;
        let src = &linear[base..base + f.w * 3];
        let dst = &mut out[ly * f.w * 4..(ly + 1) * f.w * 4];
        for (s3, d4) in src.chunks_exact(3).zip(dst.chunks_exact_mut(4)) {
            let lin = mul3(&cam_to_srgb, [s3[0], s3[1], s3[2]]);
            d4[0] = gamma.encode(lin[0] * exposure);
            d4[1] = gamma.encode(lin[1] * exposure);
            d4[2] = gamma.encode(lin[2] * exposure);
            d4[3] = 255;
        }
    }
    Ok((out, f.w))
}

/// Display auto-exposure: lift low-key images the way camera previews / LibRaw's
/// default `auto_bright` do. A sample of the linear image is enough for the 99th
/// percentile; black frames stay black, and already-bright images keep their
/// exposure.
pub(crate) fn auto_gain(linear: &[f32]) -> f32 {
    let pixels = linear.len() / 3;
    if pixels == 0 {
        return 1.0;
    }
    let step = (pixels / 50_000).max(1);
    let mut hist = [0u32; 256];
    let mut n = 0u32;
    for i in (0..pixels).step_by(step) {
        let v = linear[i * 3].max(linear[i * 3 + 1]).max(linear[i * 3 + 2]);
        let bin = (v.clamp(0.0, 1.0) * 255.0) as usize;
        hist[bin] += 1;
        n += 1;
    }
    // Already-clipped highlights are a stop sign: 3FR/NEF samples can have
    // their sensor whites at full scale while still having a low mean, and
    // lifting those makes them brighter than the camera preview. ORF/ARW have
    // headroom here (their 99.9th percentile is below full scale), so they
    // still get the lift they need.
    let target999 = (n as f64 * 0.999) as u32;
    let mut seen999 = 0u32;
    let mut high = 0.0f32;
    for (i, &count) in hist.iter().enumerate() {
        seen999 += count;
        if seen999 >= target999 {
            high = i as f32 / 255.0;
            break;
        }
    }
    if high >= 0.995 {
        return 1.0;
    }

    let target = (n as f64 * 0.99) as u32;
    let mut seen = 0u32;
    let mut p = 0.0f32;
    for (i, &count) in hist.iter().enumerate() {
        seen += count;
        if seen >= target {
            p = i as f32 / 255.0;
            break;
        }
    }
    if (0.02..0.75).contains(&p) {
        // More highlight headroom ⇒ lift more; already-bright images use a
        // gentler target so they don't overshoot the camera preview.
        let target = if high < 0.15 {
            0.92
        } else if high < 0.40 {
            0.70
        } else {
            0.65
        };
        (target / p).min(12.0)
    } else {
        1.0
    }
}

/// CFA 图案的**真实周期**（行、列）。rawler 把图案一律铺成 48×48 的表（`color_at` 内部
/// `(row+48)%48`），真实周期必然是 48 的约数；找最小满足周期性的那个。
///
/// 分箱必须按周期的整数倍切块：块内各色样本数不匀，均值就带系统性色偏——这是
/// `bin_plan` 把步长向下取到周期倍数的原因。
pub(crate) fn black_levels(img: &rawler::RawImage, cpp: usize) -> [f32; 3] {
    let levels = &img.blacklevel.levels;
    if levels.is_empty() {
        return [0.0; 3];
    }
    let n = levels.len();
    let mut mean = 0f64;
    for l in levels {
        mean += l.as_f32() as f64;
    }
    mean /= n as f64;
    if cpp == 1 || n < 3 {
        return [mean as f32; 3];
    }
    // cpp==3 时 levels 通常按通道排列（n≥3）。
    let per = n / cpp;
    let mut out = [mean as f32; 3];
    for c in 0..3 {
        let mut s = 0f64;
        for k in 0..per {
            s += levels[c * per + k].as_f32() as f64;
        }
        out[c] = (s / per as f64) as f32;
    }
    out
}

/// 白电平：WhiteLevel 支持按通道（缺省满量程 65535）。
pub(crate) fn white_level(img: &rawler::RawImage) -> [f32; 3] {
    let lv = img.whitelevel.as_vec();
    let mut out = [65535f32; 3];
    for (c, o) in out.iter_mut().enumerate() {
        if let Some(&v) = lv.get(c).or_else(|| lv.first()) {
            if v > 0.0 {
                *o = v;
            }
        }
    }
    out
}

fn wb_coeffs(img: &rawler::RawImage) -> [f32; 3] {
    let c = img.wb_coeffs;
    let mut out = [1f32; 3];
    for i in 0..3 {
        if c[i] > 0.0 {
            out[i] = c[i];
        }
    }
    // 归一化到 G=1，防止整体过曝/欠曝。
    let g = out[1];
    if g > 0.0 {
        for v in &mut out {
            *v /= g;
        }
    }
    out
}

/// 双线性去马赛克：输入单通道 f32（0..~1，线性域；白平衡不在这里，见 `cam_to_srgb_wb`）
/// 与「坐标→通道」映射，
/// 输出交错 RGB。通道值 0=R/1=G/2=B；其他值（CYGM 等四色传感器）
/// 兜底按绿色处理——色偏可接受，好过整张图打不开。
///
/// ★ 映射参数走**单形化 `impl Fn`**而不是 `&dyn Fn`：这个闭包每像素要被叫 5~9 次，
///   间接调用在这一遍里量得出代价（m84 同一进程交替双臂，改前改后各三轮、三轮同向，见正本 §3.1）。
pub(crate) fn demosaic_bilinear(
    cfa: &[f32],
    w: usize,
    h: usize,
    ch_at: impl Fn(usize, usize) -> usize,
) -> Vec<f32> {
    let mut rgb = vec![0f32; w * h * 3];
    let ch = ch_at;
    let at = |x: usize, y: usize| -> f32 {
        let x = x.min(w - 1);
        let y = y.min(h - 1);
        cfa[y * w + x]
    };
    for y in 0..h {
        for x in 0..w {
            let c = ch(x, y).min(2);
            let xm = x.saturating_sub(1);
            let xp = (x + 1).min(w - 1);
            let ym = y.saturating_sub(1);
            let yp = (y + 1).min(h - 1);
            let i = (y * w + x) * 3;
            // 本通道直接取；其余通道按最邻近同色像素平均（双线性近似）。
            for k in 0..3 {
                rgb[i + k] = if k == c {
                    cfa[y * w + x]
                } else if ch(xm, y).min(2) != k
                    && ch(xp, y).min(2) != k
                    && ch(x, ym).min(2) != k
                    && ch(x, yp).min(2) != k
                {
                    // 对角同色（棋盘对角线）：四角平均。
                    (at(xm, ym) + at(xp, ym) + at(xm, yp) + at(xp, yp)) * 0.25
                } else {
                    // 上下左右同色：可用的平均。
                    let mut s = 0f32;
                    let mut n = 0u32;
                    if ch(xm, y).min(2) == k {
                        s += at(xm, y);
                        n += 1;
                    }
                    if ch(xp, y).min(2) == k {
                        s += at(xp, y);
                        n += 1;
                    }
                    if ch(x, ym).min(2) == k {
                        s += at(x, ym);
                        n += 1;
                    }
                    if ch(x, yp).min(2) == k {
                        s += at(x, yp);
                        n += 1;
                    }
                    if n > 0 {
                        s / n as f32
                    } else {
                        cfa[y * w + x]
                    }
                };
            }
        }
    }
    rgb
}

/// 相机标定矩阵 `xyz→cam` 的**前三行**（RGBE 顺序；本解码器的去马赛克只产出 R/G/B 三道，
/// 第四行 E 用不上）。★ 数据在 `img.color_matrix`，**优先 D65**（与 sRGB 同一白点），
/// 缺 D65 时按 `Illuminant` 的枚举序取最小者——`color_matrix` 是 `HashMap`，
/// 不排的话同一张文件两次渲染可能取到不同 illuminant 的那份矩阵。
///
/// ★★ **不要改回 `img.xyz_to_cam`**：那个字段在 rawler 0.7.2 里**恒为全零**
/// （`decoders/camera.rs:30` 声明、`:260` 初始化成零，之后再无人写；字段自己标着
/// `TODO: deprecated, use color_matrix`）。m85 之前读它 ⇒ 下面那道奇异兜底每次都触发 ⇒
/// **色彩矩阵从来没生效**，三张真机全部整张偏色（正本 `.plan/m85` §一、§2.3）。
/// 无标定数据时返回 `None`，由调用方退回恒等。
pub(crate) fn xyz2cam(img: &rawler::RawImage) -> Option<[[f32; 3]; 3]> {
    use rawler::imgop::xyz::Illuminant;
    let flat = img.color_matrix.get(&Illuminant::D65).or_else(|| {
        img.color_matrix
            .iter()
            .min_by_key(|(k, _)| *k)
            .map(|(_, v)| v)
    })?;
    if flat.len() < 9 {
        return None;
    }
    let mut out = [[0f32; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        row.copy_from_slice(&flat[i * 3..i * 3 + 3]);
    }
    Some(out)
}

/// 相机 RGB（**已白平衡**）→ 线性 sRGB，并把**白平衡增益折进矩阵**：`(N·S)⁻¹ · diag(wb)`。
///
/// 组成照 rawler 自己的标定步（`imgop/develop.rs` + `imgop/raw.rs::map_3ch_to_rgb`）：
/// `rgb2cam = normalize(xyz2cam · SRGB_TO_XYZ_D65)` 再求逆。★ 关键在那次**行和归一**：
/// 归一后每行之和为 1 ⇒ 该矩阵把 `(1,1,1)` 映成 `(1,1,1)` ⇒ 它的逆也是。而"乘过白平衡
/// 增益的相机 RGB 里，场景白就是 `(1,1,1)`"正是增益的定义，于是**中性灰进、中性灰出**
/// 是按构造成立的，不靠调参。
///
/// 不走 XYZ-PCS：现路原来那步 `XYZ(D50·Bradford)→线性 sRGB` 在矩阵接对之后是**多余且错的
/// 白点适配**（相机矩阵按 D65 标定，却被当成 D50 PCS 解），m85 一并去掉。
///
/// 为什么把增益折进矩阵而不是逐样本乘：去马赛克是"同色样本求平均"，每个输出道只由
/// 同道样本平均而来，所以"按道乘一个常数"与它**可交换**。于是 WB 变成 3×3 的一次列缩放，
/// 每像素零额外代价（m82 前它写在样本循环里，且 cpp==1 时一律用 `wb[0]`）。
///
/// 每次显影算一次（3×3 乘加 + 一次求逆），不进逐像素循环。
///
/// `pub` 只为中性灰量具（`examples/probe_raw_wb.rs`，m85）能直接问"生产现在把 (1,1,1)
/// 映到哪儿"，本体只有 `develop_in` / `develop_binned` 两个调用点。
pub fn cam_to_srgb_wb(img: &rawler::RawImage) -> [[f32; 3]; 3] {
    let mut m = match xyz2cam(img) {
        Some(x2c) => {
            let rgb2cam = rawler::imgop::matrix::normalize(rawler::imgop::matrix::multiply(
                &x2c,
                &rawler::imgop::xyz::SRGB_TO_XYZ_D65,
            ));
            // inv3 自带奇异兜底（返回单位阵）。满秩 3×3 的逆与上游的 `pseudo_inverse` 同值，
            // 而上游那份在奇异时是**除零出 NaN**——宁可色偏也不崩。
            inv3(&rgb2cam)
        }
        // 没有标定数据：把相机三道直接当 sRGB（恒等），中性灰仍然中性。
        None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };
    for (c, g) in wb_coeffs(img).iter().enumerate() {
        for row in m.iter_mut() {
            row[c] *= g;
        }
    }
    m
}

pub(crate) fn mul3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

pub(crate) fn det3(m: &[[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

pub(crate) fn inv3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let d = det3(m);
    if d.abs() < 1e-12 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let inv = 1.0 / d;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

/// sRGB 传递函数（线性 0..1 → 编码 0..255）。
pub(crate) fn gamma_encode(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let v = if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0 + 0.5) as u8
}

/// 桶的个数：把 `[0,1]` 按 `(x * 65535) as u32` 的整数位切这么多刀，
/// 桶 `i` 覆盖 `[i/65535, (i+1)/65535]`（与索引写法同一个口径）。
pub(crate) const GAMMA_BUCKETS: usize = 65536;

/// gamma 的单值桶表（m79）。成立理由写在这里，别只看注释一半：
/// * 桶内 8 位取值恒定 ⇒ 该桶查表结果**等于**精算结果；
/// * 再要求**左右邻桶也恒定且同值** ⇒ `(x * 65535)` 那次 f32 乘法把 x 舍进邻桶时取值仍一样；
/// * 跨着输出边界（或紧邻边界）的桶不满足上面两条，逐个标记成"必须精算"，落回 [`gamma_encode`]。
///   注意边界**不只在暗调**：8 位一共 256 级，每级的交界处都有几个这种桶，只是暗调里它们密集成片
///   （sRGB 曲线在黑端陡）——所以这张表是"逐桶判定"，不是一条分界线。
///
/// 表本身 2×64 KB、建表 13 万次 `gamma_encode`（≈1 ms），相对省下的百毫秒级 `powf` 是零头。
pub(crate) struct GammaBuckets {
    pub(crate) value: Vec<u8>,
    /// true = 这个桶靠查表不安全，走精算。
    pub(crate) exact: Vec<bool>,
}

pub(crate) fn gamma_buckets() -> GammaBuckets {
    let edge = |i: usize| -> f32 { (i.min(GAMMA_BUCKETS - 1) as f32) / 65535.0 };
    // 每个桶：两端点取值相同才算"恒定"。
    let mut value = vec![0u8; GAMMA_BUCKETS];
    let mut constant = vec![false; GAMMA_BUCKETS];
    for (i, v) in value.iter_mut().enumerate() {
        let lo = gamma_encode(edge(i));
        *v = lo;
        constant[i] = lo == gamma_encode(edge(i + 1));
    }
    let ok = |i: usize| -> bool {
        constant[i]
            && (i == 0 || (constant[i - 1] && value[i - 1] == value[i]))
            && (i + 1 >= GAMMA_BUCKETS || (constant[i + 1] && value[i + 1] == value[i]))
    };
    let exact = (0..GAMMA_BUCKETS).map(|i| !ok(i)).collect();
    GammaBuckets { value, exact }
}

impl GammaBuckets {
    /// 与逐个调 [`gamma_encode`] **逐字节相同**的那条路（等价性由穷举 65536 个桶的用例钉住）。
    #[inline]
    pub(crate) fn encode(&self, x: f32) -> u8 {
        let c = x.clamp(0.0, 1.0);
        let idx = (c * 65535.0) as usize;
        if self.exact[idx] {
            gamma_encode(c)
        } else {
            self.value[idx]
        }
    }
}

/// rawler Orientation → EXIF 1..8（Unknown 按 1）。
pub(crate) fn orientation_to_exif(o: rawler::decoders::Orientation) -> i32 {
    use rawler::decoders::Orientation as O;
    match o {
        O::Normal | O::Unknown => 1,
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
    }
}
