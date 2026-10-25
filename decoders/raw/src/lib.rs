//! RuPix 解码插件：**相机 RAW**（raw）—— CR2/CR3/NEF/ARW/DNG/RAF/ORF/RW2/…。
//!
//! 解码库：[`rawler`]（dnglab 的解码核心，纯 Rust，~1000 种相机机型）。
//! RAW 文件里存的是**传感器拜耳原始数据**（CFA，每像素 1 个 12/14/16-bit 值），
//! 不是能直接显示的图像——必须做一遍"显影"（develop）才能看。本插件内置一条
//! 轻量显影管线（画质取向：看图器级别，不是 RAW 编辑器级别）：
//!
//! 1. **线性化**：减黑电平、按白电平归一到 0..1；
//! 2. **去马赛克**：CFA 双线性插值（bilinear），Foveon（cpp=3）与线性 RAW 跳过；
//! 3. **色彩矩阵（含白平衡）**：`color_matrix`（优先 D65）行归一求逆 → 线性 sRGB，
//!    `wb_coeffs` 的 R/G/B 乘子折进同一次列缩放；
//! 4. **Gamma**：sRGB 传递函数编码 → u8 RGBA。
//!
//! ★ 白平衡与色彩矩阵**不分开**：乘过增益的相机 RGB 里场景白就是 `(1,1,1)`，而行归一的
//! 标定矩阵按构造把 `(1,1,1)` 映成 `(1,1,1)` ⇒ 中性灰进、中性灰出。m85 之前这一步读的是
//! `img.xyz_to_cam`——那个字段在 rawler 0.7.2 里恒为全零，于是矩阵从来没生效过。
//!
//! # probe 策略
//!
//! RAW 家族没有统一魔数：RAF/CR3/X3F/MRW/IIQ 有自己的头，其余大多是 **TIFF 容器**
//! （CR2/NEF/ARW/DNG/PEF/RW2/ORF…），魔数与普通 TIFF 完全相同、内容层面不可区分。
//! 因此 TIFF 头一律放行（宿主按扩展名把 `.tif` 路由给 core 插件、把 RAW 扩展名
//! 路由到这里，不会互相抢）；真正分流交给 rawler 的解码器识别。
//!
//! # 按视口档分箱显影（m83，`CAP_SCALED`）
//!
//! 首屏与缩略图只要视口那一框，而 CFA 上每个输出像素要吃 `step²` 个原始样本——**这一档
//! 不必先把整张显影出来**。[`REQ_ALLOW_SCALED`] 点亮时 [`bin_plan`] 定步长、
//! [`develop_binned`] 直接在 CFA 上分箱出低清档：26.7 MP 那张到档 811→131 ms、
//! 交回宿主的字节 101.8 MB→11.9 MB（口径与判据见仓库 `.plan/m82` / `.plan/m83`）。
//!
//! 回全尺寸的情形（**不是谎报**：谎报的判据是"请求能否出画"，见规范 §十三）：
//! 原样不是 u16（部分 DNG 是 f32）、图案带第四色（CYGM/QRBC）、行列周期不等、
//! 或某个相位块凑不齐 R/G/B。缩完不许让宿主放大——步长上限由显示尺寸反推，
//! 与 `tiff` 的 `scaled_step` 同一口径。
//!
//! # 画幅：遮光区不当画面交回宿主（m84）
//!
//! 传感器原样四周有一圈**光学遮光区**（被封住不让进光，相机拿它估黑电平/噪声），rawler 把它
//! 留在原样里、只在元数据写下画幅矩形。本插件按 [`frame`]（`crop_area` 优先、回落 `active_area`、
//! 再回落整块）裁掉那一圈：两条腿（全尺寸、分箱档）与查询申报的尺寸都按它取样。实测三张真机
//! 的遮光区宽 6~84 像素，裁后的宽高比与**相机自己写进文件的 JPEG 预览**一致（NEF 那条预览的
//! 尺寸与 `crop_area` 逐像素相等）。
//! ★ 两个不易看的坑，各由一条用例钉住：去马赛克必须给画幅留**一圈 1 像素的真实邻域**，
//!   否则画幅外缘那一圈与"整块显影再裁出"不等；CFA 相位必须按**绝对坐标**查，因为画幅起点
//!   常常是奇数（实测 CR2 上边 19、NEF 左边 14、RAF 上边 13），按画幅内局部坐标查会认错道。
//!
//! # 档的形态：为什么本档含解码逻辑（2026-10-25 晚立此存照）
//!
//! 另外两个档（`common` / `modern`）是**纯装配**：依赖表 + `MEMBERS` 路由表，
//! 一行解码逻辑都不写（AGENTS.md §一.5）。本档是唯一例外——rawler 的集成
//! （下面整份 develop 管线）住在档里，原因是实测的硬约束：合并 crate 的
//! `[lib] name` 必须等于插件 id，而 `raw` 这个名字已被它要装配的那枚单库
//! 占着；两个 lib name 同为 `raw` 的 target 落进同一个 target 目录会互相
//! 覆盖（cargo 报 `output filename collision … 将来变成硬错`，2026-10-25
//! 复现过）。拆第三枚 crate（shell + impl）或给单库改名都能绕开，但要为
//! "档与单库同名"这一个命名巧合付长期成本。
//!
//! 所以这里的形态是：**raw 单库本身成为档**——原地升级，吸收 rawpx 为成员、
//! 补上 `iv_render_raw` 可选导出。代价：bench 侧少一枚 raw 单库（rawler 的
//! 选型结论早已归档在 `.plan/bench/`，要重横评时再拆不迟）；收益：零 cargo
//! 警告、零额外 crate、插件 id 与版本线（0.3.x → 0.4.0）连续。
//!
//! # 已知限制（有意而为）
//!
//! - 双线性去马赛克的边缘锐度低于 Malvar/AMaZE——看图器场景可接受，换来零额外依赖。
//! - 输出**未应用** orientation（`already_oriented=0`），宿主按 EXIF 方向补转；
//!   这与 core/av 插件对宿主职责的约定一致。
//! - 查询阶段用 rawler 的 dummy 解码（只读头不读像素），报的是**画幅**尺寸（[`frame`]，只吃元数据）。
//!   ★ 这里报出的宽高与档位**就是宿主解读缓冲区的依据**（它按查询申报的 `bytes_needed` 一次性分配、
//!   用查询阶段的尺寸解释渲染结果），所以两阶段的画幅与档位判断必须同出一个口径：判据只吃元数据，
//!   而 dummy 与真解拿到的是同一份元数据。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
use rupix_abi::{export_plugin, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

use std::cell::RefCell;
use std::sync::Arc;

export_plugin! {
    id: "raw",
    name: "Camera RAW & Raw Pixel Data",
    // 与 rawler::decoders::supported_extensions() 逐一对齐（大写转小写），
    // 再加上 rawpx 成员的 11 个裸数据后缀（`.raw` 两边都有——同档按请求类型
    // 分派：魔数命中走相机 RAW，没有魔数走参数化 `iv_render_raw`，见 §十二）。
    extensions: "3fr;ari;arw;cr2;cr3;crm;crw;dcr;dcs;dng;erf;fff;iiq;kdc;mef;mos;mrw;nef;nrw;orf;ori;pef;qtk;raf;raw;rw2;rwl;srw;x3f;rgb;rgba;gray;gry;bgra;yuv;i420;yv12;nv12;yuy2;uyvy",
    description: "相机 RAW（CR2/CR3/NEF/ARW/DNG/RAF 等）+ 裸像素/YUV 参数化解码（raw/rgba/gray/yuv…）",
    is_vector: 0,
    has_alpha: 0,
}

// 能力位图（规范 §十三）：
//   CAP_SCALED   = 认 `REQ_ALLOW_SCALED`：按视口档在 CFA 上分箱显影（m83）。
//                  少数文件点了也回全尺寸（f32 原样 / 四色图案 / 相位凑不齐
//                  三道色）——那是"白整解一次"，与 tiff 在未压缩条带之外的
//                  回落同一种性质，位图上不算谎报。
//   CAP_RAW_PARAM = 参数化裸像素（= 实现了 `iv_render_raw`）。宿主
//                  `candidate_for_ext(_, require_raw=true)` 按这个位筛插件，
//                  所以 `.raw` 的参数窗请求落得到本档；位缺席时宿主也会从
//                  导出存在性推导（`caps_of` 取"声明 ∪ 推导"），显式声明
//                  只是不依赖推导。
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_SCALED | rupix_abi::CAP_RAW_PARAM);

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB），与 core/av 插件同一道闸。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 导出：iv_probe / iv_render / iv_render_raw
// ---------------------------------------------------------------------------

/// 成员表。**顺序不影响路由**（按探针置信度排序），只影响同分时的先后。
///
/// 本档只有一名 probe/render 成员：相机 RAW 的实现就在本文件里（为什么这档
/// 含解码逻辑，见模块头的「档的形态」一节）。rawpx **不在这张表里**——它的
/// `render_impl` 是刻意的死路（恒报"需要宽/高/布局参数"），进了让位链只会给
/// 每个相机 RAW 解码失败添一行"已试：rawpx"的噪声；它唯一的入口是参数化的
/// `iv_render_raw`（下面直接转发，不过路由表）。
static MEMBERS: &[Member] = &[Member {
    id: "raw",
    probe: probe_impl,
    render: render_impl,
}];

/// `iv_probe`：走共用路由内核。本档虽只有一名成员，仍走 router——探针、两阶段
/// 与让位的边界条件只该有一份实现（`rupix_merge_router`，三个档共用）。
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| rupix_merge_router::probe(MEMBERS, data, len, ext))
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| rupix_merge_router::render(MEMBERS, data, len, req, out, out_cap, info, ext))
}

/// 裸像素参数化解码（ABI v4 可选导出，规范 §十二）：宽/高/布局由宿主向用户
/// 收集后传入，其余参数与 `iv_render` 同义（两阶段：`out=NULL` 查询回填）。
///
/// 宿主侧：`candidate_for_ext(_, require_raw=true)` 按 `CAP_RAW_PARAM` 筛到本档
/// （`.raw` 同时是相机 RAW 与裸像素 dump 的后缀，**请求类型**决定归属——
/// 常规打开走魔数，参数化走这个导出）。实现整体转发给 rawpx 成员，本档不写
/// 一行裸数据转换逻辑。
// 9 个参数是 ABI 契约本身定的形态（裸数据的宽/高/布局只能显式传），
// 减参数就得自己发明一个结构体——那等于给契约加第二份定义。
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref, clippy::too_many_arguments)]
pub extern "C" fn iv_render_raw(
    data: *const u8,
    len: usize,
    width: u32,
    height: u32,
    layout: u32,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
) -> i32 {
    rawpx::render_raw_impl(data, len, width, height, layout, req, out, out_cap, info)
}

/// 路由表（`MEMBERS`）直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if looks_like_raw(slice) {
            100
        } else {
            0
        }
    })
}
/// 路由表（`MEMBERS`）直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 {
            set_last_error("内部错误（宿主传入空数据）");
            return ERR_UNRECOGNIZED;
        }
        if info.is_null() {
            set_last_error("内部错误（宿主传入空输出指针）");
            return ERR_BUFFER_TOO_SMALL;
        }
        let input = unsafe { std::slice::from_raw_parts(data, len) };
        let r: IvRenderReq = if req.is_null() {
            IvRenderReq::default()
        } else {
            unsafe { *req }
        };

        // ── 阶段 1：dummy 解码（只读头/元数据，不读像素）。─────────────────
        if out.is_null() {
            let img = match dummy_decode(input) {
                Ok(v) => v,
                Err((code, msg)) => {
                    set_last_error(&msg);
                    return code;
                }
            };
            let (w, h) = (img.width as u32, img.height as u32);
            if w as u64 * h as u64 > MAX_PIXELS {
                set_last_error("图片超过 6400 万像素的插件上限");
                return ERR_TOO_LARGE;
            }
            // 出哪一档、走哪条腿。查询与渲染两阶段各算一次（一次 dummy、一次真解），
            // **必须算出同一个答案**——宿主按这里申报的 `bytes_needed` 一次性分配，
            // 渲染就得正好写那么多字节（与 `tiff` 的 `plan` 同一契约）。
            // ★ 全尺寸那一档报的是**画幅**尺寸（遮光区不交回宿主），与 `develop_in` 的产出同一个
            //   函数算出；上面那道 64 MP 闸仍按整块原样判，裁切只减不增，不因为它放宽。
            let full = frame(&img);
            let (dw, dh) = match tier_plan(&img, &r) {
                Some((_, tw, th)) => (tw, th),
                None => (full.w as u32, full.h as u32),
            };
            let bytes_needed = dw as u64 * dh as u64 * 4;
            if r.budget_bytes != 0 && bytes_needed > r.budget_bytes as u64 {
                set_last_error("图片太大，超出宿主给的内存预算");
                return ERR_TOO_LARGE;
            }
            fill_info(
                info,
                dw,
                dh,
                bytes_needed,
                orientation_to_exif(img.orientation),
            );
            return ERR_OK;
        }

        // ── 阶段 2：真解码 + 显影（按档位分箱，或全尺寸）。──────────────────
        let d = match develop_cached(input, &r) {
            Ok(d) => d,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };
        let (rgba, w, h, orientation) = (&d.0, d.1, d.2, d.3);
        let bytes_needed = rgba.len() as u64;
        if r.budget_bytes != 0 && bytes_needed > r.budget_bytes as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), out, bytes_needed as usize);
        }
        fill_info(info, w, h, bytes_needed, orientation);
        ERR_OK
    })
}
#[inline]
fn fill_info(info: *mut IvRenderOut, w: u32, h: u32, bytes_needed: u64, orientation: i32) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w;
    oi.actual_h = h;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    oi.bit_depth = BIT_DEPTH_8;
    oi.orientation = orientation;
    // 方向由宿主补转（与"旋转/镜像全在 exe"的边界一致）。
    oi.already_oriented = 0;
    // v4：单页；ICC 不透传。frame/delay 缺省 0 = 静态。
    oi.page_count = 1;
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// rawler 调用
// ---------------------------------------------------------------------------

type DecodeErr = (i32, String);

fn raw_source(input: &[u8]) -> rawler::rawsource::RawSource {
    rawler::rawsource::RawSource::new_from_slice(input)
}

/// 查询阶段的解码：dummy（只读头不读像素）。这里**保留整个 `RawImage`**而不是只拆尺寸——
/// 档位判据要与渲染阶段共用同一个函数、吃同一种输入，图案/cpp/dtype 都得在场。
fn dummy_decode(input: &[u8]) -> Result<rawler::RawImage, DecodeErr> {
    let src = raw_source(input);
    rawler::decode_dummy(&src).map_err(rawler_err)
}

/// 这一次请求该出哪一档：`Some((步长, 档宽, 档高))`，`None` = 全尺寸。
///
/// 判据全在元数据上（尺寸、图案、cpp、原样 dtype），所以 dummy 与真解两次调用**算出同一个
/// 答案**——rawler 的 `decode_*(…, dummy=true)` 只跳过填数据，dtype 与尺寸如实保留。
/// 点亮 [`REQ_ALLOW_SCALED`] 是前提：主画面的原图档升级、放大到 1:1、SVG 区域那几条路
/// 都不点这一位，走的仍是逐字节相同的全尺寸腿。
fn tier_plan(img: &rawler::RawImage, r: &IvRenderReq) -> Option<(usize, u32, u32)> {
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
/// * `Unsupported` = 「这份容器我不认识」（没有对应机型的解码器）。本插件对
///   **整个 TIFF 容器**报 100 置信度（相机 RAW 全是 TIFF 变体），而普通 TIFF 也走
///   同一份魔数：如果这里报 `ERR_CORRUPT`，宿主按"硬失败不换插件"的规矩就地放弃，
///   一个 CMYK / 调色板 TIFF 就会被判成坏文件，而 core 明明能解。报 `ERR_UNSUPPORTED`
///   才让"认得容器但没解码器"如实成为**让位**，兜底榜继续往下走。
/// * `DecoderFailed`（含 I/O 错误）= 真损坏 / 真读不下去 → 保持 `ERR_CORRUPT`，
///   换插件只是把同一句"已损坏"再花几秒重复一遍。
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
fn develop_cached(input: &[u8], r: &IvRenderReq) -> Result<Developed, DecodeErr> {
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
    let orientation = orientation_to_exif(img.orientation);
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
    if input.len() <= 128 {
        (fnv(input), input.len() as u64)
    } else {
        (
            fnv(&input[..64]).wrapping_mul(31) ^ fnv(&input[input.len() - 64..]),
            (input.len() as u64) << 1 | 1,
        )
    }
}

// ---------------------------------------------------------------------------
// 魔数嗅探
// ---------------------------------------------------------------------------

/// RAW 家族魔数：
/// - `FUJIFILM`（RAF）、`FOVb`（X3F/Sigma）、`\0MRM`（Minolta MRW）、
///   `IIII`（Phase One IIQ）、`3ASH`（Hasselblad 3FR/FFF）
/// - `ftyp` + `crx`（CR3）
/// - 其余（CR2/NEF/ARW/DNG/PEF/RW2/ORF…）都是 TIFF 容器：`II*\0` / `MM\0*`。
fn looks_like_raw(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    if &data[0..8] == b"FUJIFILM" {
        return true;
    }
    if &data[0..4] == b"FOVb" {
        return true;
    }
    if &data[0..4] == b"\0MRM" {
        return true;
    }
    if &data[0..4] == b"IIII" {
        return true;
    }
    if &data[0..4] == b"3ASH" {
        return true;
    }
    if &data[4..8] == b"ftyp" {
        let brand = &data[8..12];
        if &data[8..11] == b"crx" || &brand[0..3] == b"c" && brand[3].is_ascii_digit() {
            return true;
        }
    }
    // TIFF 头（II = 小端，MM = 大端）。
    &data[0..4] == b"II*\0" || &data[0..4] == b"MM\0*"
}

// ---------------------------------------------------------------------------
// 显影管线
// ---------------------------------------------------------------------------

/// 画幅矩形：整块传感器原样里**该交回宿主的那一块**（绝对 CFA 像素坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    fn whole(w: usize, h: usize) -> Self {
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

/// 把 [`rawler::RawImage`] 的**画幅**显影为 sRGB RGBA8（画幅外的光学遮光区不产出）。
///
/// 步骤：线性化 → 去马赛克（bilinear）→ 色彩矩阵（含白平衡）→ sRGB gamma。
/// 返回 (RGBA 缓冲, 宽)。
///
/// `pub` 只为按段计时探针（`examples/bench_raw_stages.rs`，m78 归因用）和画幅量具
/// （`examples/probe_raw_frame.rs`，m84）能单独量这一段；插件本体的调用点仍在下面的
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
    // 查看器不该替照片做自动色阶。
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
            d4[0] = gamma.encode(lin[0]);
            d4[1] = gamma.encode(lin[1]);
            d4[2] = gamma.encode(lin[2]);
            d4[3] = 255;
        }
    }
    Ok((out, f.w))
}

/// CFA 图案的**真实周期**（行、列）。rawler 把图案一律铺成 48×48 的表（`color_at` 内部
/// `(row+48)%48`），真实周期必然是 48 的约数；找最小满足周期性的那个。
///
/// 分箱必须按周期的整数倍切块：块内各色样本数不匀，均值就带系统性色偏——这是
/// `bin_plan` 把步长向下取到周期倍数的原因。
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
    // ★ 这里还要再防一层**方向**：插件报的 orientation 可能是转置的（竖构图 RAW 的
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
    let mut out = vec![0u8; ow * oh * 4];
    for oy in 0..oh {
        // 块起点的真实相位（绝对坐标）——右/下缘那几列/行被钳到画幅内最后一个整块，
        // 相位跟着钳后的起点算；画幅起点本身可能是奇数，相位也就不从 0 开始。
        let py = (f.y + (oy * step).min(h - step)) % p;
        for ox in 0..ow {
            let px = x0s[ox] % p;
            // 每道的样本数由 `counts` 给（该函数保证三道都 ≥1），不同相位可以不匀：
            // 例 3×3 的 Bayer 块是 4R4G1B，而 4×4 是 4R8G4B——除错就带系统性色偏。
            let cnt = &counts[py * p + px];
            let sums = acc[oy * ow + ox];
            let i = (oy * ow + ox) * 4;
            let mut cam = [0f32; 3];
            for c in 0..3 {
                let mean = (sums[c] / cnt[c] as f64) as f32;
                let den = (white[c] - black[c]).max(1.0);
                cam[c] = ((mean - black[c]) / den).clamp(0.0, 1.0);
            }
            let lin = mul3(&cam_to_srgb, cam);
            out[i] = gamma.encode(lin[0]);
            out[i + 1] = gamma.encode(lin[1]);
            out[i + 2] = gamma.encode(lin[2]);
            out[i + 3] = 255;
        }
    }
    Ok((out, ow, oh))
}
fn black_levels(img: &rawler::RawImage, cpp: usize) -> [f32; 3] {
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
fn white_level(img: &rawler::RawImage) -> [f32; 3] {
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
fn demosaic_bilinear(
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

/// 相机标定矩阵 `xyz→cam` 的**前三行**（RGBE 顺序；本插件的去马赛克只产出 R/G/B 三道，
/// 第四行 E 用不上）。★ 数据在 `img.color_matrix`，**优先 D65**（与 sRGB 同一白点），
/// 缺 D65 时按 `Illuminant` 的枚举序取最小者——`color_matrix` 是 `HashMap`，
/// 不排的话同一张文件两次渲染可能取到不同 illuminant 的那份矩阵。
///
/// ★★ **不要改回 `img.xyz_to_cam`**：那个字段在 rawler 0.7.2 里**恒为全零**
/// （`decoders/camera.rs:30` 声明、`:260` 初始化成零，之后再无人写；字段自己标着
/// `TODO: deprecated, use color_matrix`）。m85 之前读它 ⇒ 下面那道奇异兜底每次都触发 ⇒
/// **色彩矩阵从来没生效**，三张真机全部整张偏色（正本 `.plan/m85` §一、§2.3）。
/// 无标定数据时返回 `None`，由调用方退回恒等。
fn xyz2cam(img: &rawler::RawImage) -> Option<[[f32; 3]; 3]> {
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

fn mul3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn det3(m: &[[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inv3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
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
fn gamma_encode(x: f32) -> u8 {
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
const GAMMA_BUCKETS: usize = 65536;

/// gamma 的单值桶表（m79）。成立理由写在这里，别只看注释一半：
/// * 桶内 8 位取值恒定 ⇒ 该桶查表结果**等于**精算结果；
/// * 再要求**左右邻桶也恒定且同值** ⇒ `(x * 65535)` 那次 f32 乘法把 x 舍进邻桶时取值仍一样；
/// * 跨着输出边界（或紧邻边界）的桶不满足上面两条，逐个标记成"必须精算"，落回 [`gamma_encode`]。
///   注意边界**不只在暗调**：8 位一共 256 级，每级的交界处都有几个这种桶，只是暗调里它们密集成片
///   （sRGB 曲线在黑端陡）——所以这张表是"逐桶判定"，不是一条分界线。
///
/// 表本身 2×64 KB、建表 13 万次 `gamma_encode`（≈1 ms），相对省下的百毫秒级 `powf` 是零头。
struct GammaBuckets {
    value: Vec<u8>,
    /// true = 这个桶靠查表不安全，走精算。
    exact: Vec<bool>,
}

fn gamma_buckets() -> GammaBuckets {
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
    fn encode(&self, x: f32) -> u8 {
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
fn orientation_to_exif(o: rawler::decoders::Orientation) -> i32 {
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

// ---------------------------------------------------------------------------
// 测试：魔数嗅探 / 数学工具 / 显影数学（真实 RAW 样本见 examples/smoke_raw.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let info = unsafe { &*iv_plugin_info() };
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
                    (180..=196).contains(&r)
                        && (180..=196).contains(&g)
                        && (180..=196).contains(&b),
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
                    (180..=196).contains(&r)
                        && (180..=196).contains(&g)
                        && (180..=196).contains(&b),
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
        let mk = |cm: [f32; 9]| {
            synthetic_rggb_cm(w, h, [1.0, 1.0, 1.0, 0.0], 65535, 0, vals.clone(), cm)
        };
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

    /// 位图必须如实（与 `tiff` 的同名用例同形）：这枚插件对**能分箱的 CFA** 真降采样，
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
    /// 宿主拿到档位后要么直接 fit 到框（方向 1..4），要么先转 90° 再 fit（5..8）——插件报的
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
}
