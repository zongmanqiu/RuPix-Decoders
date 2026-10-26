//! m83 端到端诚实性检查：按**宿主真实的调用序列**走 `iv_render`，逐张真机 RAW 对账。
//!
//! 存在理由：`CAP_SCALED` 一旦声明，宿主就按查询阶段申报的 `bytes_needed` 一次性分配，
//! 并**用查询阶段的尺寸解释渲染结果**（`main/src/decoders/mod.rs` 在渲染前就把 `actual_w/h`
//! 摘出来了）。所以这轮真正的风险不在分箱算得准不准，而在三件事：
//!
//! 1. **两阶段算出同一个答案**：查询用 `decode_dummy`、渲染用真解，`bin_plan` 吃的必须是
//!    同一份元数据（尺寸/cpp/图案/原样 dtype）。这里把两边的判据都打出来直接对比。
//! 2. **档位不许顶替整图、整图也不许顶替档位**：线程本地缓存只按内容键就会串档——
//!    于是这个探针按宿主那一串请求（①-a flags=0 问原图 → ① 查询档位 → ② 渲染档位 →
//!    升级：查询整图 → 渲染整图 → 再来一次档位请求验命中）走一遍，逐步核对尺寸。
//! 3. **缩完不许让宿主放大**：档位两维都得 ≥ 两种朝向下的显示尺寸。
//!
//! 跑法（在 `RuPix-Decoder/` 下）：
//! ```text
//! cargo run --release -p rupix-raw --example check_raw_tier -- \
//!   ..\.temp\raw-samples\X100V_compressed.RAF 1920 1080
//! ```

use rupix_abi::*;

fn query(data: &[u8], r: &IvRenderReq) -> (i32, IvRenderOut) {
    let mut info = IvRenderOut::default();
    let c = raw::iv_render(
        data.as_ptr(),
        data.len(),
        r,
        std::ptr::null_mut(),
        0,
        std::ptr::addr_of_mut!(info),
        std::ptr::null(),
    );
    (c, info)
}

/// 完整走一遍宿主的两阶段：先查询拿申报值，按申报值分配，再渲染。
fn round_trip(data: &[u8], r: &IvRenderReq, label: &str) -> (u32, u32, Vec<u8>, i32, u32) {
    let (cq, q) = query(data, r);
    assert_eq!(cq, ERR_OK, "{label} 查询失败 code={cq}");
    assert!(q.actual_w > 0 && q.actual_h > 0, "{label} 查询报 0 尺寸");
    assert_eq!(
        q.bytes_needed,
        q.actual_w as u64 * q.actual_h as u64 * 4,
        "{label} 申报字节与申报尺寸不自洽"
    );
    let mut buf = vec![0u8; q.bytes_needed as usize + q.icc_len as usize];
    let mut info = q;
    let cr = raw::iv_render(
        data.as_ptr(),
        data.len(),
        r,
        buf.as_mut_ptr(),
        buf.len(),
        std::ptr::addr_of_mut!(info),
        std::ptr::null(),
    );
    assert_eq!(cr, ERR_OK, "{label} 渲染失败 code={cr}");
    // ★ 这条就是整轮的主判据：渲染产出的尺寸必须等于查询申报的尺寸。
    assert_eq!(
        (info.actual_w, info.actual_h, info.bytes_needed as usize),
        (q.actual_w, q.actual_h, buf.len() - q.icc_len as usize),
        "{label} 渲染与查询的尺寸/字节不一致——宿主会按错误尺寸解释缓冲区"
    );
    buf.truncate(q.bytes_needed as usize);
    (q.actual_w, q.actual_h, buf, q.orientation, info.bit_depth)
}

fn mean_rgb(rgba: &[u8]) -> [f64; 3] {
    let n = (rgba.len() / 4).max(1) as f64;
    let mut s = [0f64; 3];
    for px in rgba.chunks_exact(4) {
        for c in 0..3 {
            s[c] += px[c] as f64;
        }
    }
    [s[0] / n, s[1] / n, s[2] / n]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: check_raw_tier <raw-file> <target_w> <target_h>");
        std::process::exit(2);
    }
    let path = std::path::Path::new(&args[0]);
    let (tw, th): (u32, u32) = match (args[1].parse(), args[2].parse()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            eprintln!("target_w/target_h 要填整数");
            std::process::exit(2);
        }
    };
    let data = std::fs::read(path).expect("读文件");
    let ms = |d: std::time::Instant| d.elapsed().as_secs_f64() * 1e3;

    // ①-a 宿主在点亮缩放时会先补问一次"原图多大"（flags=0，target 照传）。
    let plain = IvRenderReq {
        target_w: tw,
        target_h: th,
        flags: 0,
        ..Default::default()
    };
    let (cq, natural) = query(&data, &plain);
    assert_eq!(cq, ERR_OK, "①-a 查询失败");

    // 两阶段判据的直接对账：dummy 与真解喂进同一个 `bin_plan` 与同一个 `frame`。
    let src = rawler::rawsource::RawSource::new_from_slice(&data);
    let d_img = rawler::decode_dummy(&src).expect("dummy");
    let r_img = rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()).expect("真解");
    let plan_dummy = raw::bin_plan(&d_img, tw, th);
    let plan_real = raw::bin_plan(&r_img, tw, th);
    let (fd, fr) = (raw::frame(&d_img), raw::frame(&r_img));
    println!(
        "元数据对账 dummy={}x{} cpp={} dtype={} frame={fd:?} plan={:?} | real={}x{} cpp={} dtype={} frame={fr:?} plan={:?}",
        d_img.width,
        d_img.height,
        d_img.cpp,
        if matches!(&d_img.data, rawler::RawImageData::Integer(_)) {
            "u16"
        } else {
            "f32"
        },
        plan_dummy,
        r_img.width,
        r_img.height,
        r_img.cpp,
        if matches!(&r_img.data, rawler::RawImageData::Integer(_)) {
            "u16"
        } else {
            "f32"
        },
        plan_real,
    );
    assert_eq!(
        (d_img.width, d_img.height, d_img.cpp),
        (r_img.width, r_img.height, r_img.cpp),
        "dummy 与真解的尺寸/cpp 不一致——两阶段申报会分叉"
    );
    assert_eq!(
        fd, fr,
        "dummy 与真解算出的画幅不一致——查询按 A 报了尺寸，渲染按 B 出像素"
    );
    assert_eq!(
        plan_dummy, plan_real,
        "dummy 与真解算出的档位不一致——查询按 A 报了字节，渲染按 B 出像素"
    );

    // ①+② 档位一趟（首屏/prefetch 的形态）。
    let scaled = IvRenderReq {
        target_w: tw,
        target_h: th,
        flags: REQ_ALLOW_SCALED,
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let (sw, sh, tier, orient, _) = round_trip(&data, &scaled, "档位");
    let t_tier = ms(t);

    // 升级一趟：target=(0,0) 且不带缩放意图 = 原图档。
    let full = IvRenderReq::default();
    let t = std::time::Instant::now();
    let (fw, fh, fullpx, _, _) = round_trip(&data, &full, "整图");
    let t_full = ms(t);

    // ★ 串档检查：整图渲染之后缓存槽里是整图，这时再来一次档位请求必须拿到**档位**尺寸
    //   （缓存若只认内容键，这里就会把整图当档位交出去）。
    let (sw2, sh2, tier2, _, _) = round_trip(&data, &scaled, "档位·第二次");
    assert_eq!(
        (sw2, sh2, tier2.len()),
        (sw, sh, tier.len()),
        "整图之后请求档位，拿到的不是同一档"
    );

    // 尺寸账：档位必须真比原图小，且不许让宿主放大（两种朝向都算）。
    let s_fit = (tw as f64 / fw as f64).min(th as f64 / fh as f64);
    let s_rot = (tw as f64 / fh as f64).min(th as f64 / fw as f64);
    println!(
        "尺寸 原图={fw}x{fh} 档位={sw}x{sh} 朝向={orient} 申报(①-a)={}x{}",
        natural.actual_w, natural.actual_h
    );
    // ★ m84：交回宿主的"原图"必须是**画幅**而不是整块传感器原样——遮光区不进画面，
    //   而查询申报、整图产出、画幅元数据三处必须是同一个矩形。
    assert_eq!(
        (natural.actual_w, natural.actual_h),
        (fr.w as u32, fr.h as u32),
        "①-a 报的原图尺寸不是画幅尺寸——宿主会把遮光区当画面显示"
    );
    assert_eq!(
        (fw, fh),
        (fr.w as u32, fr.h as u32),
        "整图渲染产出的尺寸不是画幅尺寸"
    );
    if fr.w != r_img.width || fr.h != r_img.height {
        println!(
            "画幅生效 整块原样={}x{} → 画幅={}x{}（遮光区 LTRB {} / {} / {} / {}）",
            r_img.width,
            r_img.height,
            fr.w,
            fr.h,
            fr.x,
            fr.y,
            r_img.width - (fr.x + fr.w),
            r_img.height - (fr.y + fr.h)
        );
    } else {
        println!("画幅＝整块原样（这份文件没有可用的画幅元数据）");
    }
    println!(
        "不许放大 正向 显示={}x{} ≤ 档 {sw}x{sh}；转置 显示={}x{} ≤ 档 {sh}x{sw}",
        fw as f64 * s_fit,
        fh as f64 * s_fit,
        fw as f64 * s_rot,
        fh as f64 * s_rot,
    );
    assert!(
        (sw as f64) >= fw as f64 * s_fit && (sh as f64) >= fh as f64 * s_fit,
        "正向：档位比显示尺寸小，宿主会被迫放大"
    );
    assert!(
        (sw as f64) >= fw as f64 * s_rot && (sh as f64) >= fh as f64 * s_rot,
        "转置：档位转 90° 后比显示尺寸小，宿主会被迫放大"
    );

    let m_tier = mean_rgb(&tier);
    let m_full = mean_rgb(&fullpx);
    let m_tier2 = mean_rgb(&tier2);
    println!(
        "耗时 档位={t_tier:.1} ms 整图={t_full:.1} ms  字节 档={} 整图={} 少搬={}",
        tier.len(),
        fullpx.len(),
        fullpx.len() - tier.len()
    );
    println!(
        "均值 档位=[{:.1},{:.1},{:.1}] 整图=[{:.1},{:.1},{:.1}] 档位复用=[{:.1},{:.1},{:.1}]",
        m_tier[0],
        m_tier[1],
        m_tier[2],
        m_full[0],
        m_full[1],
        m_full[2],
        m_tier2[0],
        m_tier2[1],
        m_tier2[2]
    );
    // 缩略图那一档（240×180 的框）——胶片条与 prefetch 都会走它，尺寸账同样要成立。
    let thumb = IvRenderReq {
        target_w: 240,
        target_h: 180,
        flags: REQ_ALLOW_SCALED,
        ..Default::default()
    };
    let (uw, uh, upx, _, _) = round_trip(&data, &thumb, "缩略档");
    let s_t = (240.0 / fw as f64).min(180.0 / fh as f64);
    let s_tr = (240.0 / fh as f64).min(180.0 / fw as f64);
    assert!(
        (uw as f64) >= fw as f64 * s_t
            && (uh as f64) >= fh as f64 * s_t
            && (uw as f64) >= fw as f64 * s_tr
            && (uh as f64) >= fh as f64 * s_tr,
        "缩略档不许让宿主放大"
    );
    let m_thumb = mean_rgb(&upx);
    println!(
        "缩略档 {uw}x{uh} = {} B 均值=[{:.1},{:.1},{:.1}]",
        upx.len(),
        m_thumb[0],
        m_thumb[1],
        m_thumb[2]
    );
    println!("TIER-OK");
}
