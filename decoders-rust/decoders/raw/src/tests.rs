use super::*;
use crate::develop::{
    demosaic_bilinear, det3, gamma_buckets, gamma_encode, inv3, mul3, orientation_to_exif, xyz2cam,
    GAMMA_BUCKETS,
};

#[test]
fn probe_detects_dedicated_magics() {
    let raf = b"FUJIFILMCCD-RAW \0\0\0";
    let x3f = b"FOVb\0\0\0\0\0\0\0\0";
    let mrw = b"\0MRM\0\0\0\0\0\0\0\0";
    let iiq = b"IIII\0\0\0\x80\0\0\0\0\0\0";
    let cr3 = b"\0\0\0\x18ftypcrx \0\0\0\0";
    assert_eq!(probe_buf(raf), 100);
    assert_eq!(probe_buf(x3f), 100);
    assert_eq!(probe_buf(mrw), 100);
    assert_eq!(probe_buf(iiq), 100);
    assert_eq!(probe_buf(cr3), 100);
}

#[test]
fn probe_detects_tiff_family() {
    let le = b"II*\0\x08\0\0\0\0\0\0\0";
    let be = b"MM\0*\0\0\0\x08\0\0\0\0";
    assert_eq!(probe_buf(le), 100);
    assert_eq!(probe_buf(be), 100);
}

#[test]
fn probe_rejects_garbage_and_short() {
    assert_eq!(probe_buf(b"not a raw file at all!"), 0);
    assert_eq!(probe_buf(b"RIFF\0\0\0\0WEBPVP8 "), 0);
    assert_eq!(probe_buf(b"II"), 0); // 太短
}

fn probe_buf(b: &[u8]) -> i32 {
    probe_buf_ext(b, None)
}

fn probe_buf_ext(b: &[u8], ext: Option<&str>) -> i32 {
    let cap = b.len();
    let mut buf = b.to_vec();
    let p = buf.as_mut_ptr();
    std::mem::forget(buf);
    let e = ext.map(|s| {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    });
    let ep = e.as_ref().map_or(std::ptr::null(), |v| v.as_ptr().cast());
    let r = iv_probe(p, cap, ep);
    unsafe {
        drop(Vec::from_raw_parts(p, cap, cap));
    }
    r
}

/// 档的自述后缀必须是两成员的并集：相机 RAW 的 29 个 + rawpx 的 12 个
/// （`.raw` 重叠一个）= 40。合并时改漏一个成员的后缀，宿主就少认领一种
/// 格式——这类漏改不报错、只静默少格式，所以拿自述钉住。
#[test]
fn self_described_extensions_are_the_union_of_both_members() {
    let info = unsafe { &*iv_decoder_info() };
    let exts = unsafe { std::ffi::CStr::from_ptr(info.extensions) }
        .to_str()
        .expect("extensions 不是 UTF-8");
    let list: Vec<&str> = exts.split(';').collect();
    // 相机 RAW 侧各取一个代表（含与 rawpx 重叠的 .raw）。
    for e in ["cr2", "dng", "raf", "raw"] {
        assert!(list.contains(&e), "自述后缀少了相机 RAW 的 .{e}");
    }
    // 裸像素侧各取一个代表（紧排与 YUV 各一）。
    for e in ["rgba", "gray", "yuv", "nv12"] {
        assert!(list.contains(&e), "自述后缀少了 rawpx 的 .{e}");
    }
    assert_eq!(list.len(), 40, "并集应为 29 + 12 - 1（.raw 重叠）= 40");
}

/// 无魔数的裸数据字节：档必须**不认领**（rawpx 的 probe 恒 0，rawler 只认
/// 魔数）。这是 `.raw` 仲裁的另一半——常规打开不能按扩展名把任意二进制抢
/// 进来，参数化那条路只走 `iv_render_raw`。
#[test]
fn headerless_bytes_are_not_claimed_even_with_raw_ext() {
    let body = b"\x01\x02\x03\x04\x05\x06\x07\x08";
    assert_eq!(probe_buf_ext(body, Some("rgba")), 0);
    assert_eq!(probe_buf_ext(body, Some("raw")), 0);
    assert_eq!(probe_buf_ext(body, Some("yuv")), 0);
}

/// `iv_render_raw` 转发：2×2 BGRA8（rawpx 的域）经档导出走完整两阶段。
/// 这条钉的是"rawpx 并入档"的接线——转发漏一个参数或查询阶段没接上，
/// 宿主参数窗打开的 `.raw`/`.rgba` 就会直接坏掉。
#[test]
fn render_raw_forwards_to_the_rawpx_member() {
    let src: Vec<u8> = (10..=25).collect();
    let req = IvRenderReq::default();
    let mut info = IvRenderOut::default();
    // 查询阶段。
    let q = iv_render_raw(
        src.as_ptr(),
        src.len(),
        2,
        2,
        PF_BGRA8,
        &req,
        std::ptr::null_mut(),
        0,
        &mut info,
    );
    assert_eq!(q, ERR_OK);
    assert_eq!((info.actual_w, info.actual_h), (2, 2));
    assert_eq!(info.bytes_needed, 16);
    assert_eq!(info.pixel_format, PF_RGBA8);
    // 渲染阶段：BGRA(10,11,12,13) → RGBA(12,11,10,13)。
    let mut buf = vec![0u8; info.bytes_needed as usize];
    let r = iv_render_raw(
        src.as_ptr(),
        src.len(),
        2,
        2,
        PF_BGRA8,
        &req,
        buf.as_mut_ptr(),
        buf.len(),
        &mut info,
    );
    assert_eq!(r, ERR_OK);
    assert_eq!(&buf[0..4], &[12, 11, 10, 13]);
    assert_eq!(&buf[12..16], &[24, 23, 22, 25]);
}

/// 参数给的尺寸与文件字节数不符 → ERR_CORRUPT（与 rawpx 单库同口径；
/// 错误码要穿过档的转发原样到达宿主）。
#[test]
fn render_raw_size_mismatch_is_corrupt() {
    let src = [0u8; 5];
    let mut info = IvRenderOut::default();
    let code = iv_render_raw(
        src.as_ptr(),
        src.len(),
        2,
        2,
        PF_RGBA8,
        std::ptr::null(),
        std::ptr::null_mut(),
        0,
        &mut info,
    );
    assert_eq!(code, ERR_CORRUPT);
}

#[test]
fn orientation_mapping_is_exif() {
    use rawler::decoders::Orientation as O;
    assert_eq!(orientation_to_exif(O::Normal), 1);
    assert_eq!(orientation_to_exif(O::HorizontalFlip), 2);
    assert_eq!(orientation_to_exif(O::Rotate180), 3);
    assert_eq!(orientation_to_exif(O::VerticalFlip), 4);
    assert_eq!(orientation_to_exif(O::Transpose), 5);
    assert_eq!(orientation_to_exif(O::Rotate90), 6);
    assert_eq!(orientation_to_exif(O::Transverse), 7);
    assert_eq!(orientation_to_exif(O::Rotate270), 8);
    assert_eq!(orientation_to_exif(O::Unknown), 1);
}

#[test]
fn matrix_inverse_roundtrips() {
    let m = [[2.0, 1.0, 0.0], [1.0, 3.0, 1.0], [0.0, 1.0, 2.0]];
    let mi = inv3(&m);
    let prod = mul3(&m, mul3(&mi, [0.7, -0.3, 1.1]));
    assert!((prod[0] - 0.7).abs() < 1e-4);
    assert!((prod[1] + 0.3).abs() < 1e-4);
    assert!((prod[2] - 1.1).abs() < 1e-4);
    // 奇异矩阵回退单位阵。
    let sing = [[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]];
    assert!(det3(&sing).abs() < 1e-9);
}

#[test]
fn gamma_encode_clamps_and_midtones() {
    assert_eq!(gamma_encode(-1.0), 0);
    assert_eq!(gamma_encode(2.0), 255);
    let mid = gamma_encode(0.5);
    // sRGB(0.5) ≈ 0.735 → ~188
    assert!((188..=190).contains(&mid), "mid={}", mid);
}

/// m79 的等价性判据（穷举，不是抽样）：**凡标成"可查表"的桶**，必须桶内恒定、且与左右邻桶同值
/// ——这两条就是"查表 = 精算"的全部依据。另外钉住**快路覆盖得住**：需要精算的桶不能过半，
/// 否则这条改写只是在给自己再算一遍。
#[test]
fn gamma_buckets_are_exact_where_they_claim_to_be() {
    let g = gamma_buckets();
    let edge = |i: usize| -> f32 { (i.min(GAMMA_BUCKETS - 1) as f32) / 65535.0 };
    let mut fast = 0usize;
    for i in 0..GAMMA_BUCKETS {
        if g.exact[i] {
            continue;
        }
        fast += 1;
        let lo = gamma_encode(edge(i));
        assert_eq!(lo, gamma_encode(edge(i + 1)), "桶 {i} 桶内不恒定");
        assert_eq!(g.value[i], lo, "桶 {i} 表值错");
        if i > 0 {
            let p = gamma_encode(edge(i - 1));
            assert_eq!(p, gamma_encode(edge(i)), "邻桶 {}-{} 之间不恒定", i - 1, i);
            assert_eq!(g.value[i - 1], p, "邻桶 {i} 左邻表值错");
        }
        if i + 1 < GAMMA_BUCKETS {
            assert_eq!(g.value[i + 1], g.value[i], "邻桶 {}+1 表值不同", i);
        }
    }
    assert!(
        fast * 2 > GAMMA_BUCKETS,
        "可查表的桶只剩 {fast}/{GAMMA_BUCKETS}，快路没覆盖住"
    );
}

/// 整定义域对撞：桶边界、桶中心，再加 20 万个确定性的伪随机取样点，
/// 每个都要求 `encode == gamma_encode`。把桶值改 1 或把暗调分界撤掉，这条立刻红。
#[test]
fn gamma_buckets_match_direct_encode_over_the_whole_domain() {
    let g = gamma_buckets();
    for i in 0..GAMMA_BUCKETS {
        let lo = (i as f32) / 65535.0;
        assert_eq!(g.encode(lo), gamma_encode(lo), "桶界 {i}");
        if i + 1 < GAMMA_BUCKETS {
            let mid = (i as f32 + 0.5) / 65535.0;
            assert_eq!(g.encode(mid), gamma_encode(mid), "桶心 {i}");
        }
    }
    let mut state: u32 = 0x1234_5678;
    for _ in 0..200_000 {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let x = (state >> 8) as f32 / (1u32 << 24) as f32;
        assert_eq!(g.encode(x), gamma_encode(x), "x={x}");
    }
    // 界外与 NaN：钳位语义必须和原路一致。
    for x in [-1.0f32, 0.0, 1.0, 2.0, 1e9, f32::NAN] {
        assert_eq!(g.encode(x), gamma_encode(x), "x={x}");
    }
}

/// 合成 4×2 RGGB 图案验证双线性去马赛克的本通道直通。
#[test]
fn demosaic_keeps_native_samples() {
    // RGGB：y0 = R G / y1 = G B
    let w = 4;
    let h = 2;
    let mut cfa = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            cfa[y * w + x] = ((y * w + x) % 7) as f32 / 8.0;
        }
    }
    let rgb = demosaic_bilinear(&cfa, w, h, |x, y| [0usize, 1, 1, 2][(y & 1) * 2 + (x & 1)]);
    // R 位置 (0,0)：R 通道 = 原值。
    assert!((rgb[0] - cfa[0]).abs() < 1e-6);
    // G 位置 (1,0)：G 通道 = 原值。
    assert!((rgb[3 + 1] - cfa[1]).abs() < 1e-6);
    // B 位置 (1,1)：B 通道 = 原值。
    assert!((rgb[(w + 1) * 3 + 2] - cfa[w + 1]).abs() < 1e-6);
    // 所有输出都在 0..1。
    assert!(rgb.iter().all(|&v| (0.0..=1.0).contains(&v)));
}

/// 线性 sRGB(D65) → XYZ 那份矩阵（`rawler::imgop::xyz::SRGB_TO_XYZ_D65` 的逆用不到，
/// 这里只要正变换本身，好让夹具凑出"相机就是 sRGB"这一特例）。
#[allow(clippy::excessive_precision)] // 上游常数按原样抄录，7 位有效数字不是笔误
const SRGB_TO_XYZ_D65: [f32; 9] = [
    0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920,
    0.9503041,
];

/// XYZ → 线性 sRGB(D65)，即上一行的逆（自算，避免把 9 个常数抄两遍抄错）。
fn xyz_to_srgb_d65() -> [f32; 9] {
    let m = [
        [SRGB_TO_XYZ_D65[0], SRGB_TO_XYZ_D65[1], SRGB_TO_XYZ_D65[2]],
        [SRGB_TO_XYZ_D65[3], SRGB_TO_XYZ_D65[4], SRGB_TO_XYZ_D65[5]],
        [SRGB_TO_XYZ_D65[6], SRGB_TO_XYZ_D65[7], SRGB_TO_XYZ_D65[8]],
    ];
    let i = inv3(&m);
    [
        i[0][0], i[0][1], i[0][2], i[1][0], i[1][1], i[1][2], i[2][0], i[2][1], i[2][2],
    ]
}

/// 造一张可控的 RGGB 合成 RAW（行主序：偶行 RGRG…、奇行 GBGB…），并指定
/// 写进 `color_matrix[D65]` 的 xyz→cam（9 个数，行主序）。
///
/// 默认夹具取「XYZ→线性 sRGB(D65)」，于是 `cam_to_srgb_wb` 里的
/// `normalize(cm · SRGB_TO_XYZ_D65) = normalize(I) = I`、再求逆还是 I ⇒ 输出直接反映
/// 线性化与白平衡这两步本身（要测矩阵本身就用 [`synthetic_rggb_cm`] 换一份真机的）。
///
/// ★★ `xyz_to_cam` **一律留全零**——那正是真机文件里 rawler 交出来的样子（见 [`xyz2cam`]
/// 的说明）。谁把矩阵来源改回那个字段，这几条端到端用例立刻全红：零矩阵触发恒等兜底，
/// 中性灰不再灰。m85 之前它恒为零而门禁 22 条全绿，就是靠这一行把住的下半段。
/// ★ 不许把 `cm` 改成单位阵：单位阵意味着 camR=X、camG=Y、camB=Z，经 XYZ→sRGB 之后
/// 红色算成负数被钳到 0，用例就只在"钳位"上成立，测不到被测试的东西。
fn synthetic_rggb(
    w: usize,
    h: usize,
    wb: [f32; 4],
    white: u32,
    black: u16,
    vals: Vec<u16>,
) -> rawler::RawImage {
    synthetic_rggb_cm(w, h, wb, white, black, vals, xyz_to_srgb_d65())
}

/// [`synthetic_rggb`] 的另一档：色彩矩阵由调用方给（真机的 xyz→cam 前三行）。
fn synthetic_rggb_cm(
    w: usize,
    h: usize,
    wb: [f32; 4],
    white: u32,
    black: u16,
    vals: Vec<u16>,
    cm: [f32; 9],
) -> rawler::RawImage {
    use rawler::cfa::{PlaneColor, CFA};
    use rawler::rawimage::CFAConfig;
    assert_eq!(
        vals.len(),
        w * h,
        "样本数必须与尺寸相符，否则测的是越界读写"
    );
    let mut color_matrix = std::collections::HashMap::new();
    color_matrix.insert(
        rawler::imgop::xyz::Illuminant::D65,
        rawler::imgop::xyz::FlatColorMatrix::from(cm.to_vec()),
    );
    rawler::RawImage {
        camera: Default::default(),
        make: String::new(),
        model: String::new(),
        clean_make: String::new(),
        clean_model: String::new(),
        width: w,
        height: h,
        cpp: 1,
        bps: 16,
        wb_coeffs: wb,
        whitelevel: rawler::rawimage::WhiteLevel::new(vec![white]),
        blacklevel: rawler::rawimage::BlackLevel::new(&[black], 1, 1, 1),
        xyz_to_cam: Default::default(),
        photometric: rawler::rawimage::RawPhotometricInterpretation::Cfa(CFAConfig {
            cfa: CFA::new("RGGB"),
            colors: PlaneColor::new("RGB"),
        }),
        active_area: None,
        crop_area: None,
        blackareas: Vec::new(),
        orientation: rawler::decoders::Orientation::Normal,
        data: rawler::RawImageData::Integer(vals),
        color_matrix,
        dng_tags: Default::default(),
    }
}

/// 合成 RawImage 端到端走 develop：线性化 → 去马赛克 → 矩阵 → gamma。
/// 这一条走"增益全 1"的中性口径，只钉数值通路，不断白平衡（白平衡见下面两条）。
#[test]
fn develop_synthetic_rggb() {
    // RGGB：R=3000, G=10000/11000, B=20000（u16 域）。
    let img = synthetic_rggb(
        4,
        2,
        [1.0, 1.0, 1.0, 0.0],
        65535,
        0,
        vec![3000, 10000, 3000, 10000, 11000, 20000, 11000, 20000],
    );
    let (rgba, ow) = develop(&img).expect("develop");
    assert_eq!(ow, 4);
    assert_eq!(rgba.len(), 4 * 2 * 4);
    // (0,0) 是 R 位置：R ≈ gamma(3000/65535) ≈ gamma(0.046) ≈ 0.23 → ~59。
    let r0 = rgba[0] as i32;
    assert!((40..=90).contains(&r0), "r0={}", r0);
    // (1,0) 是 G 位置：G ≈ gamma(10000/65535) ≈ gamma(0.153) ≈ 0.43 → ~110。
    let g0 = rgba[4 + 1] as i32;
    assert!((95..=130).contains(&g0), "g0={}", g0);
    // (1,1) 是 B 位置：B ≈ gamma(20000/65535) ≈ gamma(0.305) ≈ 0.59 → ~150。
    let b0 = rgba[(4 + 1) * 4 + 2] as i32;
    assert!((135..=170).contains(&b0), "b0={}", b0);
    // alpha 全 255。
    assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
}

/// ★ **白平衡必须按道用**（m82 逮到的缺陷；把增益退回"一律用 wb[0]"这条就红）。
///
/// 原代码在 cpp==1（一切 Bayer / X-Trans）时把所有样本一律乘 `wb[0]`——三道乘同一个
/// 系数等于**没做白平衡**，而色彩矩阵的前提恰恰是"输入已白平衡"，于是红道在线性 sRGB
/// 上被算成负数、钳到 0，真机照片整张发青。实测 NEF 均值 RGB = 8/181/160，
/// 而同一张文件里相机自己写下的 JPEG 预览是 122/140/151。
///
/// 用例把三道增益设为互不相等的 [2,1,4]，并把原样配成"乘完增益后三道相等"
/// （R=0.25、G=0.50、B=0.125 满量程 ⇒ 乘完都是 0.5）。正确答案只有一个：**中性灰**
/// gamma(0.5)≈188。老代码给 188/255/140（不灰）；把增益错配到任意一道也不灰。
/// 断言同时钉住数值 180..196，防止"整张全黑"也凑成三道相等。
///
/// ★ 只判**内部**像素（2..6 那一块）：双线性去马赛克在四边要把邻域钳到边上，
/// 于是边缘像素的某道色其实混进了别道样本——那与白平衡无关，混的是取样边界。
/// 8×8 才有这样的内部；用 4×2 会得到 188/188/255 这种"看着像失败"的边界值。
#[test]
fn white_balance_is_per_cfa_channel() {
    let (w, h) = (8usize, 8usize);
    // RGGB：偶行 R G R G…，奇行 G B G B…。乘完增益后三道都到 0.5 满量程。
    let vals: Vec<u16> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| match (y % 2, x % 2) {
                (0, 0) => 16384, // R
                (1, 1) => 8192,  // B
                _ => 32768,      // G（两道）
            })
        })
        .collect();
    let img = synthetic_rggb(w, h, [2.0, 1.0, 4.0, 0.0], 65535, 0, vals);
    let (rgba, _) = develop(&img).expect("develop");
    let mut checked = 0;
    for y in 2..6 {
        for x in 2..6 {
            let px = &rgba[(y * w + x) * 4..];
            let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
            assert!(
                (180..=196).contains(&r) && (180..=196).contains(&g) && (180..=196).contains(&b),
                "({x},{y}) 该是中性灰，实得 {r}/{g}/{b}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 16, "内部像素数不对，等于没判");
}

/// 真机文件里 rawler 交出来的两份 D65 标定矩阵（`probe_raw_wb` 从 `.temp/raw-samples`
/// 的 glarus.nef / Canon_EOS_30D.CR2 实读，各取前三行、行主序摊平）。
const CM_D80: [f32; 9] = [
    0.8628, -0.241, -0.0883, -0.9055, 1.694, 0.2171, -0.1491, 0.1363, 0.852,
];
const CM_30D: [f32; 9] = [
    0.6256, -0.0303, -0.1, -0.7879, 1.5621, 0.2396, -0.1713, 0.1905, 0.7047,
];

/// ★★ **中性灰过真机的色彩矩阵，出来还得是中性灰**（m85 的主判据，正本 §三-1）。
///
/// 白平衡增益乘过之后的相机 RGB 里，场景白就是 `(1,1,1)` 这一族向量；`cam_to_srgb_wb`
/// 里那次**行和归一**按构造保证它映到自己。所以这条不需要"参考答案"，它是链路的
/// 不变量：把归一去掉、或把 D50·Bradford 那步加回来，三道立刻分家 → 红。
///
/// ★ 两条腿一起判：`develop`（全尺寸）与 `develop_binned`（分箱档）**必须同源**，
/// 否则档位与 1:1 之间会跳色（m82/m83 一直在避免的那件事）。
#[test]
fn neutral_gray_survives_a_real_color_matrix() {
    let (w, h) = (8usize, 8usize);
    // 与白平衡那条同款：乘完 [2,1,4] 增益后三道都到 0.5 满量程。
    let vals: Vec<u16> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| match (y % 2, x % 2) {
                (0, 0) => 16384,
                (1, 1) => 8192,
                _ => 32768,
            })
        })
        .collect();
    for cm in [CM_D80, CM_30D] {
        let img = synthetic_rggb_cm(w, h, [2.0, 1.0, 4.0, 0.0], 65535, 0, vals.clone(), cm);
        let full = develop(&img).expect("develop").0;
        // ★ 只判内部：理由同 `white_balance_is_per_cfa_channel`（四边混的是取样边界）。
        for y in 2..6 {
            for x in 2..6 {
                let px = &full[(y * w + x) * 4..];
                let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
                assert!(
                    (180..=196).contains(&r)
                        && (180..=196).contains(&g)
                        && (180..=196).contains(&b),
                    "全尺寸腿 ({x},{y}) 该是中性灰，实得 {r}/{g}/{b}"
                );
            }
        }
        // 分箱档：step=2 恰好一个 2×2 周期一块，每块凑齐 1R2G1B ⇒ 整块都是同一份中性灰。
        let (binned, bw, bh) = develop_binned(&img, 2).expect("分箱");
        assert_eq!((bw, bh), (4, 4));
        for (i, px) in binned.chunks_exact(4).enumerate() {
            let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
            assert!(
                (180..=196).contains(&r) && (180..=196).contains(&g) && (180..=196).contains(&b),
                "分箱腿 #{i} 该是中性灰，实得 {r}/{g}/{b}"
            );
        }
    }
}

/// ★★ **渲染结果必须真的随色彩矩阵变**（这一条才是"矩阵有没有值进来"的探针）。
///
/// m85 的教训：`cam_to_xyz_matrix` 在、`inv3` 在、用例在、门禁全绿，而它读的
/// `img.xyz_to_cam` 在 rawler 0.7.2 里恒为全零 ⇒ 恒等兜底每次都触发 ⇒ **矩阵从来没生效**。
/// 中性灰那条抓不到这种退化（恒等也保中性），所以这里换判据：**同一份有彩度的像素，
/// 换一份矩阵结果就得变**。谁把来源改回那个恒零字段，三份渲染全成同一个恒等结果 → 红。
#[test]
fn render_depends_on_the_color_matrix() {
    let (w, h) = (8usize, 8usize);
    // 三道各给一档亮度（内部像素线性域 ≈ 0.79 / 0.31 / 0.12）：恒等与任何真标定矩阵
    // 在这种像素上必然分家，且两份真机矩阵之间也分家。
    let vals: Vec<u16> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| match (y % 2, x % 2) {
                (0, 0) => 52000, // R
                (1, 1) => 8000,  // B
                _ => 20000,      // G（两道）
            })
        })
        .collect();
    let mk =
        |cm: [f32; 9]| synthetic_rggb_cm(w, h, [1.0, 1.0, 1.0, 0.0], 65535, 0, vals.clone(), cm);
    let render = |img: &rawler::RawImage| -> Vec<u8> {
        let rgba = develop(img).expect("develop").0;
        let mut v = Vec::new();
        for y in 2..6 {
            for x in 2..6 {
                v.extend_from_slice(&rgba[(y * w + x) * 4..][..3]);
            }
        }
        v
    };
    let d80 = mk(CM_D80);
    let eos = mk(CM_30D);
    let mut none = mk(CM_D80);
    none.color_matrix = Default::default(); // 无标定数据 → 恒等兜底
    let (a, b, c) = (render(&d80), render(&eos), render(&none));
    assert_ne!(a, c, "D80 矩阵与恒等兜底渲染相同 ⇒ 矩阵没进链路");
    assert_ne!(b, c, "30D 矩阵与恒等兜底渲染相同 ⇒ 矩阵没进链路");
    assert_ne!(a, b, "两份真机矩阵渲染相同 ⇒ 矩阵没进链路");
    // 三份都必须仍然出画（判据⑤：无标定数据不许黑屏）。
    assert!(
        a.iter().any(|&v| v > 0) && c.iter().any(|&v| v > 0),
        "有矩阵/无矩阵两种渲染出了全黑：{a:?} / {c:?}"
    );
}

/// 色彩矩阵的选择口径：**优先 D65**，缺 D65 取枚举序最小者（`color_matrix` 是 `HashMap`，
/// 不排序的话同一张文件两次渲染可能取到不同 illuminant 的那份）。空/短数据 → `None`。
#[test]
fn xyz2cam_prefers_d65_and_is_deterministic() {
    use rawler::imgop::xyz::{FlatColorMatrix, Illuminant};
    let mut img = synthetic_rggb(4, 2, [1.0, 1.0, 1.0, 0.0], 65535, 0, vec![0u16; 8]);
    let mut pick = |entries: [(&'static str, [f32; 9]); 2]| {
        img.color_matrix = entries
            .iter()
            .map(|(k, v)| {
                let illu = if *k == "D65" {
                    Illuminant::D65
                } else {
                    Illuminant::A
                };
                (illu, FlatColorMatrix::from(v.to_vec()))
            })
            .collect();
        xyz2cam(&img)
    };
    // 两条都在，无论谁先插入都必须取 D65
    assert_eq!(
        pick([("A", CM_30D), ("D65", CM_D80)]).unwrap()[0].to_vec(),
        CM_D80[..3].to_vec()
    );
    assert_eq!(
        pick([("D65", CM_D80), ("A", CM_30D)]).unwrap()[0].to_vec(),
        CM_D80[..3].to_vec()
    );
    // 只有 A：取 A
    assert_eq!(
        pick([("A", CM_30D), ("A", CM_30D)]).unwrap()[2].to_vec(),
        CM_30D[6..9].to_vec()
    );
    // 空表 / 长度不足 9：None（调用方退回恒等）
    img.color_matrix = Default::default();
    assert!(xyz2cam(&img).is_none());
    img.color_matrix = [(Illuminant::D65, FlatColorMatrix::from(vec![1.0f32; 6]))]
        .into_iter()
        .collect();
    assert!(xyz2cam(&img).is_none(), "不足 3×3 的表不能当成矩阵用");
}

/// ★ **黑帧就该是黑的**（m82 删掉那条"元数据兜底"；把它加回来这条就红）。
///
/// 被删的规则是"观测峰值距黑电平不到声明量程的 5% ⇒ 按观测值重归一"。按它自己的触发
/// 条件，它**只可能在平场/黑帧上成立**（传感器噪声贴着黑电平），于是它把黑电平挪到
/// `峰值×0.1`、白电平挪到峰值，把一张黑帧刷成满量程亮场——实测 X100V 黑帧样本
/// （原样均值 996.7、黑电平 1019、声明白电平 16383，相机自己写下的预览也是黑的）
/// 在应用里渲染成一整块品红。查看器不该替照片做自动色阶。
#[test]
fn flat_black_frame_stays_black() {
    // 每个样本恰等于黑电平 = 最干净的黑帧；增益取一组不等的值，顺带盯住"黑不会被增益抬起来"。
    let img = synthetic_rggb(4, 2, [1.81, 1.0, 1.83, 0.0], 16383, 1019, vec![1019u16; 8]);
    let (rgba, _) = develop(&img).expect("develop");
    assert!(
        rgba.chunks_exact(4)
            .all(|px| px[..3].iter().all(|&v| v == 0)),
        "黑帧被刷亮了：{:?}",
        &rgba[..8]
    );
}

// ── m83：按视口档分箱显影的接线 ──────────────────────────────────────

/// 造一份渲染请求：目标框 + 是否点亮 [`REQ_ALLOW_SCALED`]。
fn req(tw: u32, th: u32, scaled: bool) -> IvRenderReq {
    IvRenderReq {
        target_w: tw,
        target_h: th,
        flags: if scaled { REQ_ALLOW_SCALED } else { 0 },
        ..Default::default()
    }
}

/// 中性平场用的合成图：12×9 的 RGGB，三道乘完增益后相等（⇒ 正确答案只有中性灰）。
fn flat_neutral(w: usize, h: usize) -> rawler::RawImage {
    let vals: Vec<u16> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| match (y % 2, x % 2) {
                (0, 0) => 16384, // R
                (1, 1) => 8192,  // B
                _ => 32768,      // G
            })
        })
        .collect();
    synthetic_rggb(w, h, [2.0, 1.0, 4.0, 0.0], 65535, 0, vals)
}

/// 位图必须如实（与 `tiff` 的同名用例同形）：这枚解码器对**能分箱的 CFA** 真降采样，
/// 其余回全尺寸——那是"白整解一次"，不是错图。
#[test]
fn caps_declare_scaled() {
    // rawpx 并入本档后多导出 `iv_render_raw`，能力位必须同时含
    // CAP_RAW_PARAM（宿主据此把参数化裸数据路由进来）。
    assert_eq!(iv_caps(), CAP_FULL | CAP_SCALED | CAP_RAW_PARAM);
}

/// 档位只在**点亮缩放意图、且这一档确实比原图小**时出得来。
/// 拔掉 `tier_plan` 的 flags 闸第一条就红（放大到 1:1 会被档位图顶替）；
/// 把 `bin_plan` 的"这一档本来就比原图大"闸写反，第三行就红。
#[test]
fn tier_plan_needs_the_flag_and_the_headroom() {
    let img = flat_neutral(12, 9);
    assert_eq!(
        tier_plan(&img, &req(4, 3, false)),
        None,
        "没点亮缩放意图也不许出档"
    );
    // 宿主"不限尺寸"的口径是 target=0，不许当成"缩到 0"。
    assert_eq!(tier_plan(&img, &req(0, 0, true)), None);
    assert_eq!(
        tier_plan(&img, &req(12, 9, true)),
        None,
        "目标框与原图一样大：没有可缩的余地"
    );
    assert_eq!(
        tier_plan(&img, &req(64, 64, true)),
        None,
        "目标比原图大还出档＝逼宿主放大"
    );
    // 正常一档：12×9 装进 4×3 ⇒ 步长 3、档尺寸 4×3。
    assert_eq!(tier_plan(&img, &req(4, 3, true)), Some((3, 4, 3)));
}

/// ★ **声称回全尺寸的每一种形态必须真的回来**（f32 原样 / cpp=3 的 Foveon / 非 CFA
/// 光度 / 行列周期不等 / 四色图案 / 图案缺一道色）。这是"位图不谎报"的另一半：
/// 判据写错一处，真机上一张 DNG 就会被分箱成偏色的低清档，而查询阶段已按档位报了字节。
#[test]
fn bin_plan_declines_the_forms_it_claims() {
    let mut img = flat_neutral(12, 9);
    img.data = rawler::RawImageData::Float(vec![0.5f32; 12 * 9]);
    assert!(bin_plan(&img, 4, 3).is_none(), "f32 原样必须回全尺寸");

    let mut img = flat_neutral(12, 9);
    img.cpp = 3;
    assert!(
        bin_plan(&img, 4, 3).is_none(),
        "cpp=3（Foveon 那种）必须回全尺寸"
    );

    let mut img = flat_neutral(12, 9);
    img.photometric = rawler::rawimage::RawPhotometricInterpretation::LinearRaw;
    assert!(bin_plan(&img, 4, 3).is_none(), "非 CFA 光度必须回全尺寸");

    // 四色图案（CYGM 那类）：青/黄/洋红在本仓口径里被压到蓝，分箱会污染蓝道均值。
    let mut img = flat_neutral(12, 9);
    img.photometric = cfa("GMYG", "RGB");
    assert!(bin_plan(&img, 4, 3).is_none(), "带第四色的图案必须回全尺寸");

    // 图案缺一道色：整张只有红绿，蓝道无从算起。
    let mut img = flat_neutral(12, 9);
    img.photometric = cfa("RRRR", "RGB");
    assert!(bin_plan(&img, 4, 3).is_none(), "缺蓝道的图案必须回全尺寸");

    // 行列周期不等：横向周期 12、纵向周期 3 的图案，一张 p×p 的相位表描述不了它。
    // （`CFA::new` 的 144 字符是 12×12；纵向按这三行重复、三行互不相同 ⇒ ph=3。）
    let mut img = flat_neutral(12, 9);
    img.photometric = cfa(
        &["RRGGRBRRGGBB", "BBRRGBRRGGBG", "GGRBBRBGGRGB"]
            .iter()
            .cycle()
            .take(12)
            .cloned()
            .collect::<String>(),
        "RGB",
    );
    let (ph, pw) = match &img.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) => cfa_period(&cfg.cfa),
        _ => panic!("夹具该是 Cfa"),
    };
    assert_eq!(
        (ph, pw),
        (3, 12),
        "夹具没造出周期不等的图案，等于没判这一条"
    );
    assert!(bin_plan(&img, 4, 3).is_none(), "行列周期不等必须回全尺寸");
}

/// 合成一个 CFA 光度（测试夹具用）。
fn cfa(pattern: &str, colors: &str) -> rawler::rawimage::RawPhotometricInterpretation {
    use rawler::cfa::{PlaneColor, CFA};
    use rawler::rawimage::CFAConfig;
    rawler::rawimage::RawPhotometricInterpretation::Cfa(CFAConfig {
        cfa: CFA::new(pattern),
        colors: PlaneColor::new(colors),
    })
}

/// ★ **缩完不许让宿主放大**（与 `tiff` 的 `scaled_step` 同一条立约，落成档位尺寸）。
///
/// 宿主拿到档位后要么直接 fit 到框（方向 1..4），要么先转 90° 再 fit（5..8）——解码器报的
/// 方向可能是转置的，所以**两种朝向都要成立**：按两种朝向各算一次显示尺寸，档位的两个
/// 边都必须还够得着它。判据是"边长 ≥"，不是"恰好等于"：等号成立时那条腿就没在管这事。
///
/// m84 起再乘一个**画幅维**：同一形状分别按整块、奇数起点画幅、非对称画幅各算一次。
/// 基准尺寸取画幅而不是整块——这一步专防"上限按整块算、输出按画幅算"这种口径分叉。
#[test]
fn tier_never_lets_the_host_upscale() {
    let boxes: [(u32, u32); 9] = [
        (4, 3),
        (3, 4),
        (4, 4),
        (8, 3),
        (3, 8),
        (2, 2),
        (1, 6),
        (6, 1),
        (5, 2),
    ];
    let shapes: [(usize, usize); 6] = [(12, 9), (9, 12), (16, 4), (4, 16), (11, 7), (7, 11)];
    let crops: [Option<(usize, usize, usize, usize)>; 3] =
        [None, Some((1, 1, 1, 1)), Some((2, 1, 1, 0))];
    let mut planned = 0usize;
    let mut cases = 0usize;
    for &(w, h) in &shapes {
        for crop in &crops {
            for &(bw, bh) in &boxes {
                let mut img = flat_neutral(w, h);
                if let Some((x, y, r, b)) = *crop {
                    with_crop(&mut img, x, y, w - x - r, h - y - b);
                }
                let f = frame(&img);
                let (fw, fh) = (f.w as f64, f.h as f64);
                cases += 1;
                let Some((step, ow, oh)) = bin_plan(&img, bw, bh) else {
                    continue;
                };
                planned += 1;
                // 朝向 1..4：显示尺寸 = 画幅装进 (bw,bh) 的等比短板。
                let s = (bw as f64 / fw).min(bh as f64 / fh);
                assert!(
                        (ow as f64) >= fw * s && (oh as f64) >= fh * s,
                        "正向：画幅 {fw}×{fh} 进 {bw}×{bh} 取步长 {step} 得 {ow}×{oh}，显示尺寸要 {}×{}",
                        fw * s,
                        fh * s
                    );
                // 朝向 5..8：宿主先把档位转 90°（档位的 h 边对应画幅的 w 边）再 fit 同一个框。
                let s2 = (bw as f64 / fh).min(bh as f64 / fw);
                assert!(
                        (oh as f64) >= fh * s2 && (ow as f64) >= fw * s2,
                        "转置：画幅 {fw}×{fh} 进 {bw}×{bh} 取步长 {step} 得 {ow}×{oh}，转正后显示尺寸要 {}×{}",
                        fw * s2,
                        fh * s2
                    );
            }
        }
    }
    assert!(
        planned >= cases / 2,
        "只有 {planned}/{cases} 组算出了档位，判据大半是空转"
    );
}

/// ★ **块均值必须按"该相位块里各道色的实际样本数"来除**。
///
/// Bayer 在步长 3 下是 4R4G1B，下一个相位是 3R4G2B——除数按 `step²` 或按固定值写死，
/// 每道的缩放就不一样，输出会按相位起一层彩色方格（真机上 X-Trans 那张正是步长 3）。
/// 夹具是"乘完增益后三道相等"的平场，所以正确答案只有一个中性灰 ≈188；除错就不灰。
/// 顺带钉住 `develop_binned` 的产出尺寸与 `bin_plan` 申报的一致——查询阶段已按那个尺寸
/// 报了 `bytes_needed`，宿主照它解释缓冲区。
#[test]
fn binned_tier_divides_by_per_phase_sample_counts() {
    let img = flat_neutral(12, 9);
    let (step, ow, oh) = bin_plan(&img, 4, 3).expect("12×9 进 4×3 该有档");
    assert_eq!((step, ow, oh), (3, 4, 3));
    let (rgba, bw, bh) = develop_binned(&img, step).expect("develop_binned");
    assert_eq!((bw, bh), (ow, oh), "分箱产出尺寸与申报尺寸不一致");
    assert_eq!(rgba.len(), bw * bh * 4);
    let mut checked = 0usize;
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        for (c, &v) in px[..3].iter().enumerate() {
            let v = v as i32;
            assert!(
                (180..=196).contains(&v),
                "第 {i} 像素第 {c} 道 = {v}，不在中性灰 188 附近（除数没按相位块的实际样本数？）"
            );
        }
        assert_eq!(px[3], 255);
        checked += 1;
    }
    assert_eq!(checked, 12, "输出像素数不对，等于没判");
}

// ── m84：画幅（遮光区不当画面交回宿主）───────────────────────────────

/// 给合成图装一个画幅矩形（绝对像素坐标），模拟相机的遮光区元数据。
fn with_crop(img: &mut rawler::RawImage, x: usize, y: usize, w: usize, h: usize) {
    img.crop_area = Some(rawler::imgop::Rect::new(
        rawler::imgop::Point::new(x, y),
        rawler::imgop::Dim2::new(w, h),
    ));
    img.active_area = None;
}

/// 画幅的优先级必须与 rawler 自己的 `CropDefault` 一致：`crop_area` 优先、回落 `active_area`、
/// 两者都没有才是整块原样。（写成"只认 active_area"或"两个取交集"，第二、三条就红。）
#[test]
fn frame_prefers_crop_then_active_then_whole() {
    let mut img = flat_neutral(12, 9);
    assert_eq!(
        frame(&img),
        Frame::whole(12, 9),
        "没有画幅元数据就该是整块原样"
    );
    img.active_area = Some(rawler::imgop::Rect::new(
        rawler::imgop::Point::new(2, 1),
        rawler::imgop::Dim2::new(8, 6),
    ));
    assert_eq!(
        frame(&img),
        Frame {
            x: 2,
            y: 1,
            w: 8,
            h: 6
        },
        "只有 active_area 时用它"
    );
    with_crop(&mut img, 1, 1, 10, 7);
    img.active_area = Some(rawler::imgop::Rect::new(
        rawler::imgop::Point::new(2, 1),
        rawler::imgop::Dim2::new(8, 6),
    ));
    assert_eq!(
        frame(&img),
        Frame {
            x: 1,
            y: 1,
            w: 10,
            h: 7
        },
        "crop_area 在场时优先（与 rawler 的 crop_area.or(active_area) 同序）"
    );
}

/// ★ **撒谎的元数据只能钳，不能崩、也不能裁空**（判据⑤）。这里不加任何启发式阈值：
/// 裁出 0 宽/0 高就当作没有画幅，宁可留一条黑边也不交回一张空白。
/// 第三条同时钉住溢出防护——`x + w` 直接相加会回绕成一个看起来很小的合法矩形。
#[test]
fn frame_clamps_lies_and_falls_back_instead_of_cropping_nothing() {
    let mut img = flat_neutral(12, 9);
    with_crop(&mut img, 8, 6, 20, 20);
    assert_eq!(
        frame(&img),
        Frame {
            x: 8,
            y: 6,
            w: 4,
            h: 3
        },
        "越界必须钳进原样，而不是报出比原样还大的画幅"
    );
    with_crop(&mut img, 12, 0, 5, 5);
    assert_eq!(
        frame(&img),
        Frame::whole(12, 9),
        "整块画幅在原样之外：裁空＝当作没有画幅"
    );
    with_crop(&mut img, 8, 6, usize::MAX, usize::MAX);
    assert_eq!(
        frame(&img),
        Frame {
            x: 8,
            y: 6,
            w: 4,
            h: 3
        },
        "饱和加也不许回绕：回绕成 7<8 就掉进上一条回落，看着合法其实错了"
    );
}

/// ★ **画幅内部逐字节等于"整块原样显影后按同一矩形裁出"**（判据③）。
/// 这条一次钉住两件事：给去马赛克留一圈**真实邻域**（不留 ⇒ 画幅外缘那一圈不等），
/// 和 CFA 图案按**绝对坐标**查（按局部查 ⇒ 奇数起点时认错道，整块都不等）。
/// 夹具用逐像素乱序的值：平场对"邻域钳位"完全钝感（邻域都一样），根本测不到第一条。
#[test]
fn cropped_develop_is_byte_identical_to_the_sliced_whole_frame() {
    let (w, h) = (12usize, 9usize);
    let vals: Vec<u16> = (0..w * h)
        .map(|i| {
            let mut s = (i as u32).wrapping_mul(2_654_435_761);
            s ^= s >> 13;
            ((s % 6000) + 300) as u16
        })
        .collect();
    let mut img = synthetic_rggb(w, h, [2.0, 1.0, 4.0, 0.0], 65535, 0, vals);
    let (whole, ww) = develop_in(&img, &Frame::whole(w, h)).expect("整块显影");
    assert_eq!(ww, w);
    // 五个矩形各钉一件事：(1,1,10,7) 奇数起点、(2,0,8,9) 上下都贴原样边（两个方向都不扩）、
    // (0,0,12,9) 整块、(3,2,5,4) 内部小块、(11,8,1,1) 右下角 1×1（横竖都贴边）。
    // 横向恒铺满整行 ⇒ 左右两个方向上这些矩形永远在展开矩形的真实邻域里。
    for (x, y, fw, fh) in [
        (1usize, 1usize, 10usize, 7usize),
        (2, 0, 8, 9),
        (0, 0, 12, 9),
        (3, 2, 5, 4),
        (11, 8, 1, 1),
    ] {
        with_crop(&mut img, x, y, fw, fh);
        let f = frame(&img);
        assert_eq!((f.x, f.y, f.w, f.h), (x, y, fw, fh));
        let (cut, cw) = develop(&img).expect("画幅显影");
        assert_eq!(cw, fw, "申报宽度该是画幅宽");
        assert_eq!(cut.len(), fw * fh * 4, "输出该只有画幅那一块");
        for ly in 0..fh {
            let from = ((y + ly) * w + x) * 4;
            let src = &whole[from..from + fw * 4];
            assert_eq!(
                &cut[ly * fw * 4..(ly + 1) * fw * 4],
                src,
                "画幅 ({x},{y}) 第 {ly} 行与整块裁出的不一致"
            );
        }
    }
}

/// ★ **平场 + 画幅**：全尺寸腿仍要出中性灰（判据④）。
/// 画幅取 (2,2,8,5) ⇒ 展开矩形起点 (1,1) 是**奇数**，去马赛克若按展开缓冲区的局部坐标
/// 查图案就把 R 与 B 认错道，平场立刻不灰。与 m82 的 `white_balance_is_per_cfa_channel`
/// 同一条方法论：**等价改写只在两种来源分歧的形态上才证明自己**——起点 (1,1) 的画幅
/// 展开后正好盖住整块（局部≡绝对），那种夹具对这条变异完全钝感（本轮实测过）。
#[test]
fn odd_frame_origin_keeps_neutral_flat_gray() {
    let mut img = flat_neutral(12, 9);
    with_crop(&mut img, 2, 2, 8, 5);
    let (rgba, cw) = develop(&img).expect("develop");
    assert_eq!((cw, rgba.len() / 4 / cw), (8, 5));
    let mut checked = 0usize;
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        for (c, &v) in px[..3].iter().enumerate() {
            let v = v as i32;
            assert!(
                (180..=196).contains(&v),
                "第 {i} 像素第 {c} 道 = {v}，画幅 (2,2) 下不灰＝CFA 相位没按绝对坐标查"
            );
        }
        assert_eq!(px[3], 255);
        checked += 1;
    }
    assert_eq!(checked, 40, "判到的像素数不对，等于没判");
}

/// ★ **分箱腿只准吃画幅里的样本**（判据④⑥同一条腿）。
/// 夹具在平场之外多画一圈**遮光区**：画幅外的样本一律写成黑值。于是"块起点忘了加画幅原点"
/// 这种写法（将画幅左缘外的带当画面累加）立刻把首行首列拉离中性灰——纯平场逮不住它，
/// 因为偏移一格之后仍然是"自洽的另一种取样"。真机上这一圈就是 m84 要裁掉的东西。
#[test]
fn binned_crop_keeps_blocks_inside_the_frame_and_uses_absolute_phase() {
    let (w, h) = (12usize, 9usize);
    let (fx, fy, fw, fh) = (2usize, 2usize, 8usize, 5usize);
    let vals: Vec<u16> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
                    0 // 遮光区：黑值，与画幅内的平场明显不同
                } else {
                    match (y % 2, x % 2) {
                        (0, 0) => 16384,
                        (1, 1) => 8192,
                        _ => 32768,
                    }
                }
            })
        })
        .collect();
    let mut img = synthetic_rggb(w, h, [2.0, 1.0, 4.0, 0.0], 65535, 0, vals);
    with_crop(&mut img, fx, fy, fw, fh);
    let (step, ow, oh) = bin_plan(&img, 4, 3).expect("8×5 进 4×3 该有档");
    assert!(
        ow * step >= fw && (ow - 1) * step < fw,
        "档宽没能盖住画幅宽：{ow}×步长{step} 对 {fw}"
    );
    let (rgba, bw, bh) = develop_binned(&img, step).expect("develop_binned");
    assert_eq!((bw, bh), (ow, oh), "分箱产出尺寸与申报尺寸不一致");
    assert_eq!(rgba.len(), bw * bh * 4);
    let mut checked = 0usize;
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        for (c, &v) in px[..3].iter().enumerate() {
            let v = v as i32;
            assert!(
                (180..=196).contains(&v),
                "第 {i} 像素第 {c} 道 = {v}，画幅外的黑带被累加进来了＝块起点/除数没按画幅算"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, bw * bh, "判到的像素数不对，等于没判");
}
