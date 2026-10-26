//! m85 诊断量具：**我们的显影链把"中性灰"映到哪儿去了**。
//!
//! 起因（正本 §一）：`probe_raw_preview.rs` 的三联对照图上，两条我方腿（全尺寸显影 / 分箱）
//! 相对相机自己写进文件的预览**整张偏青、高光炸成品红**（实测档位均值
//! CR2 100/123/140 vs 预览 155/165/177；NEF 90/134/148 vs 123/141/153）。
//! 这不是"风格差"，是白平衡/色矩阵用错了。
//!
//! 判据不必等真图：**中性灰必须映成中性灰**。白平衡增益乘过之后的相机 RGB 里，场景白是
//! `(1,1,1)`（rawler 自己就按这个约定写 `map_3ch_to_rgb`）。于是把 `(1,1,1)` 喂进两条链的
//! 3×3 矩阵（增益已含在向量里，矩阵两边都不再乘 wb），看输出的三道：
//!   现路 = `XYZ(D50·Bradford)→linear sRGB · inv3(xyz_to_cam)`
//!   候选 = `pseudo_inverse(normalize(xyz_to_cam · SRGB_TO_XYZ_D65))`（rawler 自己的标定步）
//! 候选按构造恒等于 `(1,1,1)`（行和归一）；现路偏多少，就是整张图的色偏多少——逐像素零成本。
//!
//! 跑法：`cargo run --release -p rupix-raw --example probe_raw_wb`

use std::path::PathBuf;

/// rawler `imgop::xyz::SRGB_TO_XYZ_D65`（线性 sRGB→XYZ，D65）。
#[allow(clippy::excessive_precision)] // 上游常数按原样抄录，7 位有效数字不是笔误
const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
    [0.4124564, 0.3575761, 0.1804375],
    [0.2126729, 0.7151522, 0.0721750],
    [0.0193339, 0.1191920, 0.9503041],
];

/// XYZ(D50, PCS) → linear sRGB。**m85 之前本解码器现路的那一步**（当时叫
/// `xyz_to_linear_srgb`，矩阵来源是恒为零的 `img.xyz_to_cam` ⇒ 实际走的是恒等兜底）。
/// 本轮把矩阵接到 `color_matrix` 之后这一步整个删掉了，这里留着只为把"生产当时算什么"量出来。
#[allow(clippy::excessive_precision)] // 上游常数按原样抄录，7 位有效数字不是笔误
const XYZ_D50_TO_SRGB: [[f32; 3]; 3] = [
    [3.1338561, -1.6168667, -0.4906146],
    [-0.9787684, 1.9161415, 0.0334540],
    [0.0719453, -0.2289914, 1.4052427],
];

fn mul3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut r = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                r[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    r
}

fn det(m: &[[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inv(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let d = det(m);
    let i = 1.0 / d;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * i,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * i,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * i,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * i,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * i,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * i,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * i,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * i,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * i,
        ],
    ]
}

fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// 行归一（rawler `imgop::matrix::normalize`）：每行加起来等于 1 ⇒ 中性进、中性出。
fn normalize_rows(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut r = *m;
    for row in r.iter_mut() {
        let s: f32 = row.iter().sum();
        if s.abs() != 0.0 {
            for v in row.iter_mut() {
                *v /= s;
            }
        }
    }
    r
}

/// 3×3 的伪逆：满秩时就是普通逆，这里显式写出来是为了跟 rawler 的 `pseudo_inverse`
/// 在**降秩**时的行为对齐（4 通道传感器取前三行可能奇异）。
fn pseudo_inverse_3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    if det(m).abs() < 1e-9 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    inv(m)
}

fn main() {
    let dir: PathBuf = match std::env::args().nth(1) {
        Some(p) => p.into(),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".temp")
            .join("raw-samples"),
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("样本目录")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| ["cr2", "nef", "raf", "dng", "cr3"].contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let data = std::fs::read(&path).expect("读文件");
        let src = rawler::rawsource::RawSource::new_from_slice(&data);
        let Ok(img) = rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()) else {
            println!("== {stem}: 解不动，跳过");
            continue;
        };
        // 本解码器现路的 wb 取法：>0 才用，再归一到 G=1
        let mut wb = [1f32; 3];
        wb.iter_mut().zip(img.wb_coeffs.iter()).for_each(|(o, &v)| {
            if v > 0.0 {
                *o = v;
            }
        });
        let g = wb[1].max(1e-6);
        for v in &mut wb {
            *v /= g;
        }
        // 生产读的那份：`img.xyz_to_cam`（rawler 结构体里标着 "TODO: deprecated, use color_matrix"）
        let zero = img.xyz_to_cam.iter().flatten().all(|&v| v == 0.0);
        let x2c: [[f32; 3]; 3] = [
            [
                img.xyz_to_cam[0][0],
                img.xyz_to_cam[0][1],
                img.xyz_to_cam[0][2],
            ],
            [
                img.xyz_to_cam[1][0],
                img.xyz_to_cam[1][1],
                img.xyz_to_cam[1][2],
            ],
            [
                img.xyz_to_cam[2][0],
                img.xyz_to_cam[2][1],
                img.xyz_to_cam[2][2],
            ],
        ];
        // 真数据在 color_matrix 里：**优先 D65**（与 sRGB 同一白点），缺则按枚举序取最小者
        // ——与生产 `xyz2cam` 同口径（`color_matrix` 是 HashMap，不排序的话同一张文件两次
        // 运行可能取到不同 illuminant 的那份）。rawler 自己的标定步 `imgop/develop.rs` 同序。
        let flat = img
            .color_matrix
            .get(&rawler::imgop::xyz::Illuminant::D65)
            .or_else(|| {
                img.color_matrix
                    .iter()
                    .min_by_key(|(k, _)| *k)
                    .map(|(_, v)| v)
            })
            .expect("无 color_matrix");
        let n = flat.len() / 3;
        let mut cm = [[0f32; 3]; 3];
        for (i, row) in cm.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = flat[i * 3 + j];
            }
        }

        // 四条链的"相机→线性 sRGB"。测试向量：前三条**不含 wb**，喂 `(1,1,1)`（= 乘过增益
        // 的相机 RGB 里的场景白）；第四条是本轮生产的真函数，它把增益折进了列，所以喂
        // **乘之前**的 `(1/wb)`——两者在同一个坐标上，可直接并排比。
        let prod = mul3(
            &XYZ_D50_TO_SRGB,
            &if det(&x2c).abs() < 1e-9 {
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] // ← m85 之前生产实际走的兜底
            } else {
                inv(&x2c)
            },
        );
        let cur_real = mul3(&XYZ_D50_TO_SRGB, &inv(&cm));
        let cand = pseudo_inverse_3(&normalize_rows(&mul3(&cm, &SRGB_TO_XYZ_D65)));
        let now = raw::cam_to_srgb_wb(&img);

        let enc = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as i32;
        let gray = |m: &[[f32; 3]; 3], v: [f32; 3]| {
            let r = apply(m, v);
            [enc(r[0]), enc(r[1]), enc(r[2])]
        };
        let neutral = [1.0f32; 3];
        let pre_wb = [1.0 / wb[0], 1.0 / wb[1], 1.0 / wb[2]];
        println!(
            "== {stem}  {} {}  wb={:.3}/{:.3}/{:.3}",
            img.make, img.model, wb[0], wb[1], wb[2]
        );
        println!(
            "   xyz_to_cam（m85 之前读的那份）全零 = {}｜color_matrix 条目 {:?}，取用 {} 行里的前三行 = {:?}",
            zero,
            img.color_matrix
                .iter()
                .map(|(k, v)| (format!("{k:?}"), v.len()))
                .collect::<Vec<_>>(),
            n, cm
        );
        println!(
            "   中性灰编码后 → m85 前 {:?}｜现路公式＋真矩阵 {:?}｜rawler 标定步＋真矩阵 {:?}｜★ 本轮生产 {:?}",
            gray(&prod, neutral),
            gray(&cur_real, neutral),
            gray(&cand, neutral),
            gray(&now, pre_wb)
        );
        let lin = |m: &[[f32; 3]; 3], v: [f32; 3]| apply(m, v);
        println!(
            "   线性域同一向量 → m85 前 {:?}｜现路＋真矩阵 {:?}｜rawler 标定步＋真矩阵 {:?}｜★ 本轮生产 {:?}",
            lin(&prod, neutral),
            lin(&cur_real, neutral),
            lin(&cand, neutral),
            lin(&now, pre_wb)
        );
    }
}
