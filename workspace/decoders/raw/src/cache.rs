use std::cell::RefCell;
use std::sync::Arc;

use rupix_abi::{IvRenderReq, ERR_CORRUPT, ERR_UNSUPPORTED, REQ_ALLOW_SCALED};

use super::{bin_plan, develop, develop_binned, raw_orientation};

type DecodeErr = (i32, String);

fn raw_source(input: &[u8]) -> rawler::rawsource::RawSource {
    rawler::rawsource::RawSource::new_from_slice(input)
}

/// 查询阶段的解码：dummy（只读头不读像素）。这里**保留整个 `RawImage`**而不是只拆尺寸——
/// 档位判据要与渲染阶段共用同一个函数、吃同一种输入，图案/cpp/dtype 都得在场。
pub(crate) fn dummy_decode(input: &[u8]) -> Result<rawler::RawImage, DecodeErr> {
    let src = raw_source(input);
    rawler::decode_dummy(&src).map_err(rawler_err)
}

/// 这一次请求该出哪一档：`Some((步长, 档宽, 档高))`，`None` = 全尺寸。
///
/// 判据全在元数据上（尺寸、图案、cpp、原样 dtype），所以 dummy 与真解两次调用**算出同一个
/// 答案**——rawler 的 `decode_*(…, dummy=true)` 只跳过填数据，dtype 与尺寸如实保留。
/// 点亮 [`REQ_ALLOW_SCALED`] 是前提：主画面的原图档升级、放大到 1:1、SVG 区域那几条路
/// 都不点这一位，走的仍是逐字节相同的全尺寸腿。
pub(crate) fn tier_plan(img: &rawler::RawImage, r: &IvRenderReq) -> Option<(usize, u32, u32)> {
    if r.flags & REQ_ALLOW_SCALED == 0 {
        return None;
    }
    let (step, w, h) = bin_plan(img, r.target_w, r.target_h)?;
    Some((step, w as u32, h as u32))
}

fn box_msg(e: rawler::RawlerError) -> String {
    format!("{}", e)
}

/// rawler 的错误分两类，**必须分开映射**——混成"损坏"会关掉宿主的兜底链。
///
/// * `Unsupported` = 「这份容器我不认识」（没有对应机型的解码器）。本解码器对
///   **整个 TIFF 容器**报 100 置信度（相机 RAW 全是 TIFF 变体），而普通 TIFF 也走
///   同一份魔数：如果这里报 `ERR_CORRUPT`，宿主按"硬失败不换解码器"的规矩就地放弃，
///   一个 CMYK / 调色板 TIFF 就会被判成坏文件，而 core 明明能解。报 `ERR_UNSUPPORTED`
///   才让"认得容器但没解码器"如实成为**让位**，兜底榜继续往下走。
/// * `DecoderFailed`（含 I/O 错误）= 真损坏 / 真读不下去 → 保持 `ERR_CORRUPT`，
///   换解码器只是把同一句"已损坏"再花几秒重复一遍。
fn rawler_err(e: rawler::RawlerError) -> DecodeErr {
    match e {
        rawler::RawlerError::Unsupported { .. } => (ERR_UNSUPPORTED, box_msg(e)),
        rawler::RawlerError::DecoderFailed(_) => (ERR_CORRUPT, box_msg(e)),
    }
}

/// 渲染阶段的产物：这一档的像素（与线程本地缓存共享同一块缓冲）、宽、高、EXIF 方向。
type Developed = (Arc<Vec<u8>>, u32, u32, i32);

/// 渲染阶段：真解码 + 显影（点亮缩放意图时在 CFA 上分箱到那一档，否则全尺寸），
/// 缓存到线程本地（紧随查询的渲染调用零重复解码）。
///
/// m80：缓存槽与返回值**共享同一份像素**（`Arc<Vec<u8>>`），命中与插入都不再搬整图。
/// 形态取 `Arc<Vec<u8>>` 而不是 `Arc<[u8]>`——后者从 `Vec` 建出来必然另开一块带 Arc 头的
/// 分配再整份搬一次（宿主 m73 实测 146.7 MB / 32.9 ms 就栽在这）。
///
/// m83：缓存键补上**请求档位**。同一份文件的视口档与整图档是两块不同的像素，谁也不能
/// 顶替谁——只按内容键就会把"整图"当成"缩略图"交上去，宿主按查询申报的尺寸解读缓冲区，
/// 结果是花屏或整条升级闸永不触发（`core/decode.rs` 的 `scaled_of` 注释记着同一个坑）。
pub(crate) fn develop_cached(input: &[u8], r: &IvRenderReq) -> Result<Developed, DecodeErr> {
    struct Cache {
        key: (u64, u64),
        /// 这份像素是为哪一次请求出的：目标框 + 缩放意图。
        req_key: (u32, u32, bool),
        rgba: Arc<Vec<u8>>,
        w: u32,
        h: u32,
        orientation: i32,
    }
    thread_local! {
        static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) };
    }
    let key = fingerprint(input);
    let req_key = (r.target_w, r.target_h, r.flags & REQ_ALLOW_SCALED != 0);
    let hit = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|s| s.key == key && s.req_key == req_key)
            .map(|s| (s.rgba.clone(), s.w, s.h, s.orientation))
    });
    if let Some((rgba, w, h, orientation)) = hit {
        return Ok((rgba, w, h, orientation));
    }

    let src = raw_source(input);
    let img =
        rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()).map_err(rawler_err)?;
    // Use the same full chain as the query phase (rawler orientation → TIFF tag 274
    // → known model fallback). Reading only `img.orientation` made Phase One H 25
    // report 1 while its dummy/query phase reported 6.
    let orientation = raw_orientation(&img, input, false);
    let (rgba, w, h) = match tier_plan(&img, r) {
        // 档尺寸由 `develop_binned` 自己算（与 `bin_plan` 同一个 `div_ceil` 式子），
        // 这里只取步长——两处若有一天分叉，查询/渲染的尺寸对不上会在出口 `out_cap` 闸上炸出来。
        Some((step, ..)) => {
            let (rgba, ow, oh) = develop_binned(&img, step).map_err(rawler_err)?;
            (rgba, ow as u32, oh as u32)
        }
        None => {
            let (rgba, dw) = develop(&img).map_err(rawler_err)?;
            let h = (rgba.len() / 4 / dw) as u32;
            (rgba, dw as u32, h)
        }
    };
    // 整份像素从显影到出口只有这一份：包住它，缓存与返回值各持一个计数。
    let rgba = Arc::new(rgba);
    CACHE.with(|c| {
        *c.borrow_mut() = Some(Cache {
            key,
            req_key,
            // Arc 计数，不是像素搬运。
            rgba: rgba.clone(),
            w,
            h,
            orientation,
        })
    });
    Ok((rgba, w, h, orientation))
}

/// 缓存指纹：长度 + 首/尾 64 字节的 FNV-1a（避免全量哈希大文件）。
fn fingerprint(input: &[u8]) -> (u64, u64) {
    fn fnv(slice: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in slice {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
    if input.len() <= 192 {
        (fnv(input), input.len() as u64)
    } else {
        // Sample a middle block too: same-length files from one camera share
        // the TIFF header and trailer, so ends-only keys collided and the
        // cache served the previous file's pixels.
        let mid = input.len() / 2 - 32;
        (
            fnv(&input[..64]).wrapping_mul(31)
                ^ fnv(&input[mid..mid + 64]).wrapping_mul(131)
                ^ fnv(&input[input.len() - 64..]),
            (input.len() as u64) << 1 | 1,
        )
    }
}
