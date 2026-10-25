//! RAW 首开的**按段计时**探针（m78 归因）。
//!
//! 存在理由：m77 只量到端点总账（CR2 8.5 MP 728 ms、NEF 10.2 MP 846 ms、X100V RAF 26.7 MP 2525 ms，
//! 约 85–95 ms/MP），但"这笔花在 rawler 解 CFA、还是本仓库自写的显影链、还是宿主后处理"没有读数，
//! 而规矩立过：**没归因就不许谈优化**。这里把那一次首开拆成六段各自计时。
//!
//! 六段与归属：
//! 1. `read`     宿主把文件读进内存；
//! 2. `probe`    插件魔数嗅探（`iv_probe`）；
//! 3. `dummy`    查询阶段 `decode_dummy`（只读头与元数据，不读像素）——就是这里在量 rawler 的
//!    **机型库/元数据一次性初始化**，所以每个文件都必须是**进程里第一个**被解的（见下）；
//! 4. `unpack`   `rawler::decode` 真解出 CFA 原始像素（第三方，内部自带 rayon 并行）；
//! 5. `develop`  **本仓库**的显影链：线性化 → 双线性去马赛克 → 色彩矩阵 → sRGB gamma；
//! 6. `copy_out` 出口那一次全尺寸搬运（宿主给缓冲、插件 memcpy，与 `iv_render` 同一笔）。
//!
//! **一次进程只量一张**：raw 插件的 `develop_cached` 把结果留在解码线程 TLS（内容键），
//! 同进程里第二趟必然命中——m77 已经见过它把 728 ms 报成 19 ms（K101 同一枚陷阱咬过两次）。
//! 要多样本就重复起进程。
//!
//! 跑法（在 `RuPix-Decoder/` 下；一张一行，五个样本手记中位数）：
//! ```text
//! cargo run --release -p rupix-raw --example bench_raw_stages -- ..\.temp\raw-samples\Canon_EOS_30D.CR2
//! ```

use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: bench_raw_stages <one-raw-file>   （一次只量一张，保证冷解）");
        std::process::exit(2);
    }
    let path = Path::new(&args[0]);

    // 1. 读文件
    let t = Instant::now();
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读文件失败 {path:?}: {e}");
            std::process::exit(1);
        }
    };
    let read = t.elapsed();

    // 2. 魔数嗅探
    let t = Instant::now();
    let score = raw::iv_probe(data.as_ptr(), data.len(), std::ptr::null());
    let probe = t.elapsed();

    // 3. 查询阶段：decode_dummy（与 src/lib.rs 的 `probe_dims` 同一个调用）
    let t = Instant::now();
    let dummy_img = match rawler::decode_dummy(&rawler::rawsource::RawSource::new_from_slice(&data))
    {
        Ok(img) => img,
        Err(e) => {
            eprintln!("decode_dummy 失败: {e}");
            std::process::exit(1);
        }
    };
    let dummy = t.elapsed();

    // 4. 真解 CFA
    let t = Instant::now();
    let img = match rawler::decode(
        &rawler::rawsource::RawSource::new_from_slice(&data),
        &rawler::decoders::RawDecodeParams::default(),
    ) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("rawler::decode 失败: {e}");
            std::process::exit(1);
        }
    };
    let unpack = t.elapsed();

    // 5. 自写显影链
    let t = Instant::now();
    let (rgba, w) = match raw::develop(&img) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("develop 失败: {e}");
            std::process::exit(1);
        }
    };
    let develop = t.elapsed();

    // 6. 出口搬运（新分配一份同尺寸目标 + memcpy，与 iv_render 那笔同形）
    let t = Instant::now();
    let mut dst = vec![0u8; rgba.len()];
    dst.copy_from_slice(&rgba);
    let copy_out = t.elapsed();

    // 逐字节指纹（整遍 FNV-1a）：给"改法必须输出同一张图"当证据用。
    // 不是密码学哈希，但对逐字节差异足够敏感（同长度同前缀两张图相同时它才会失手）。
    let mut dig: u64 = 0xcbf29ce484222325;
    for &b in rgba.iter() {
        dig ^= b as u64;
        dig = dig.wrapping_mul(0x100000001b3);
    }
    println!(
        "digest=fnv1a64:{:016x} len={} sum_first_last={} {}",
        dig,
        rgba.len(),
        rgba[0] as u32 + rgba[1] as u32 + rgba[rgba.len() - 4] as u32,
        rgba[3]
    );

    // 7.（诊断，不改任何产出）develop 的 gamma 那一步每像素要调三次 `powf`，
    //    这里用同样的次数在真实像素值上量一次纯 powf 的价，好把 develop 内部拆开归因。
    let t = Instant::now();
    let mut acc = 0f32;
    for px in rgba.chunks_exact(4) {
        for v in &px[..3] {
            acc += (*v as f32 / 255.0).powf(1.0 / 2.4);
        }
    }
    let powf_cost = t.elapsed();
    println!(
        "powf_x{} = {:.1} ms（诊断臂，只算次数不产出像素；acc={:.3}）",
        rgba.len() / 4 * 3,
        powf_cost.as_secs_f64() * 1e3,
        acc
    );

    // 8.（诊断，不进产品）候选"gamma 查表"能省多少、代价多大：
    //    表 = 65536 项，按下标 `round(x*65535)` 取；对照逐点算的精确值。
    //    这一臂**只报数**，因为查表会把 8 位取值挪动 ≤1 级——那是**画面**上的改变，
    //    要用户拍板，不由测量轮自己落地。
    let gamma_exact = |x: f32| -> u8 {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.0031308 {
            x * 12.92
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0 + 0.5) as u8
    };
    let t = Instant::now();
    let lut: Vec<u8> = (0..=65535u32)
        .map(|i| gamma_exact(i as f32 / 65535.0))
        .collect();
    let lut_build = t.elapsed();

    // 代价：在 1,000,001 个均匀采样点上比两个 8 位取值。
    let mut diff_n = 0u64;
    let mut diff_max = 0i32;
    for k in 0..=1_000_000u32 {
        let x = k as f32 / 1_000_000.0;
        let a = gamma_exact(x) as i32;
        let b = lut[((x * 65535.0).round() as usize).min(65535)] as i32;
        let d = (a - b).abs();
        if d > 0 {
            diff_n += 1;
        }
        if d > diff_max {
            diff_max = d;
        }
    }

    // 收益：同样 3×像素次，查表 vs 逐点 powf。
    let t = Instant::now();
    let mut acc2 = 0u32;
    for px in rgba.chunks_exact(4) {
        for v in &px[..3] {
            acc2 += lut[(*v as usize) * 257] as u32;
        }
    }
    let lut_cost = t.elapsed();
    println!(
        "lut_build={:.1} ms lut_lookup_x{} = {:.1} ms | 代价：1e6 采样点上 8 位取值不同的比例 {:.3}%，最大 {} 级（acc={:.1}）",
        lut_build.as_secs_f64() * 1e3,
        rgba.len() / 4 * 3,
        lut_cost.as_secs_f64() * 1e3,
        diff_n as f64 / 1_000_001.0 * 100.0,
        diff_max,
        acc2 as f64
    );

    let h = rgba.len() / 4 / w;
    let mp = (w * h) as f64 / 1e6;
    let total = read + probe + unpack + develop + copy_out; // dummy 是查询阶段，另算
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;

    println!(
        "file={}  develop={}x{}  dummy={}x{}  score={}  = {:.1} MP",
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        w,
        h,
        dummy_img.width,
        dummy_img.height,
        score,
        mp
    );
    println!(
        "read={:.1} probe={:.2} dummy(query)={:.1} unpack={:.1} develop={:.1} copy_out={:.1}",
        ms(read),
        ms(probe),
        ms(dummy),
        ms(unpack),
        ms(develop),
        ms(copy_out)
    );
    println!(
        "render_total(unpack+develop+copy_out)={:.1} ms  = {:.1} ms/MP   first_request_total(read+dummy+render)={:.1} ms",
        ms(unpack + develop + copy_out),
        ms(unpack + develop + copy_out) / mp,
        ms(total + dummy)
    );
}
