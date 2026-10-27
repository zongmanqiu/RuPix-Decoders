//! RuPix 解码器：**相机 RAW**（raw）—— CR2/CR3/NEF/ARW/DNG/RAF/ORF/RW2/…。
//!
//! 解码库：[`rawler`]（dnglab 的解码核心，纯 Rust，~1000 种相机机型）。
//! RAW 文件里存的是**传感器拜耳原始数据**（CFA，每像素 1 个 12/14/16-bit 值），
//! 不是能直接显示的图像——必须做一遍"显影"（develop）才能看。本解码器内置一条
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
//! 因此 TIFF 头一律放行（宿主按扩展名把 `.tif` 路由给 core 解码器、把 RAW 扩展名
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
//! 留在原样里、只在元数据写下画幅矩形。本解码器按 [`frame`]（`crop_area` 优先、回落 `active_area`、
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
//! `[lib] name` 必须等于解码器 id，而 `raw` 这个名字已被它要装配的那枚单库
//! 占着；两个 lib name 同为 `raw` 的 target 落进同一个 target 目录会互相
//! 覆盖（cargo 报 `output filename collision … 将来变成硬错`，2026-10-25
//! 复现过）。拆第三枚 crate（shell + impl）或给单库改名都能绕开，但要为
//! "档与单库同名"这一个命名巧合付长期成本。
//!
//! 所以这里的形态是：**raw 单库本身成为档**——原地升级，吸收 rawpx 为成员、
//! 补上 `iv_render_raw` 可选导出。代价：bench 侧少一枚 raw 单库（rawler 的
//! 选型结论早已归档在 `.plan/bench/`，要重横评时再拆不迟）；收益：零 cargo
//! 警告、零额外 crate、解码器 id 与版本线（0.3.x → 0.4.0）连续。
//!
//! # 已知限制（有意而为）
//!
//! - 双线性去马赛克的边缘锐度低于 Malvar/AMaZE——看图器场景可接受，换来零额外依赖。
//! - 输出**未应用** orientation（`already_oriented=0`），宿主按 EXIF 方向补转；
//!   这与 core/av 解码器对宿主职责的约定一致。
//! - 查询阶段用 rawler 的 dummy 解码（只读头不读像素），报的是**画幅**尺寸（[`frame`]，只吃元数据）。
//!   ★ 这里报出的宽高与档位**就是宿主解读缓冲区的依据**（它按查询申报的 `bytes_needed` 一次性分配、
//!   用查询阶段的尺寸解释渲染结果），所以两阶段的画幅与档位判断必须同出一个口径：判据只吃元数据，
//!   而 dummy 与真解拿到的是同一份元数据。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
use rupix_abi::{export_decoder, guard};
use rupix_merge_router::Member;
use std::ffi::c_char;

mod probe;
use probe::probe_impl;

mod frame;
pub use frame::{frame, Frame};

mod cache;
pub(crate) use cache::{develop_cached, dummy_decode, tier_plan};

mod flash;
mod orient;
export_decoder! {
    id: "raw",
    name: "Camera RAW & Raw Pixel Data",
    // 与 rawler::decoders::supported_extensions() 逐一对齐（大写转小写），
    // 再加上 rawpx 成员的 11 个裸数据后缀（`.raw` 两边都有——同档按请求类型
    // 分派：魔数命中走相机 RAW，没有魔数走参数化 `iv_render_raw`，见 §十二）。
    extensions: "3fr;ari;arw;cr2;cr3;crm;crw;dcr;dcs;dng;erf;fff;iiq;kdc;mef;mos;mrw;nef;nrw;orf;ori;pef;qtk;raf;raw;rw2;rwl;srw;x3f;rgb;rgba;gray;gry;bgra;yuv;i420;yv12;nv12;yuy2;uyvy",
    description: "相机 RAW 与裸像素/YUV 数据",
    is_vector: 0,
    has_alpha: 0,
}

// 能力位图（规范 §十三）：
//   CAP_SCALED   = 认 `REQ_ALLOW_SCALED`：按视口档在 CFA 上分箱显影（m83）。
//                  少数文件点了也回全尺寸（f32 原样 / 四色图案 / 相位凑不齐
//                  三道色）——那是"白整解一次"，与 tiff 在未压缩条带之外的
//                  回落同一种性质，位图上不算谎报。
//   CAP_RAW_PARAM = 参数化裸像素（= 实现了 `iv_render_raw`）。宿主
//                  `candidate_for_ext(_, require_raw=true)` 按这个位筛解码器，
//                  所以 `.raw` 的参数窗请求落得到本档；位缺席时宿主也会从
//                  导出存在性推导（`caps_of` 取"声明 ∪ 推导"），显式声明
//                  只是不依赖推导。
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_SCALED | rupix_abi::CAP_RAW_PARAM);

rupix_abi::export_metadata! {
    "version" => "0.5.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-27",
}

const BIT_DEPTH_8: u32 = 8;

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB），与 core/av 解码器同一道闸。
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
pub fn render_impl(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
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

        let x3f = flash::is_x3f(ext);

        // ── 阶段 1：dummy 解码（只读头/元数据，不读像素）。─────────────────
        if out.is_null() {
            let (img, fallback) = match dummy_decode(input) {
                Ok(img) => (Some(img), None),
                Err((code, msg)) if x3f => match flash::dummy(input) {
                    Ok(v) => (None, Some(v)),
                    Err(_) => {
                        set_last_error(&msg);
                        return code;
                    }
                },
                Err((code, msg)) => {
                    set_last_error(&msg);
                    return code;
                }
            };
            let (w, h) = match (&img, fallback) {
                (Some(img), _) => (img.width as u32, img.height as u32),
                (None, Some(v)) => v,
                (None, None) => {
                    set_last_error("内部错误（没有解码结果）");
                    return ERR_CORRUPT;
                }
            };
            if w as u64 * h as u64 > MAX_PIXELS {
                set_last_error("图片超过 6400 万像素的解码器上限");
                return ERR_TOO_LARGE;
            }
            // 出哪一档、走哪条腿。查询与渲染两阶段各算一次（一次 dummy、一次真解），
            // **必须算出同一个答案**——宿主按这里申报的 `bytes_needed` 一次性分配，
            // 渲染就得正好写那么多字节（与 `tiff` 的 `plan` 同一契约）。
            // ★ 全尺寸那一档报的是**画幅**尺寸（遮光区不交回宿主），与 `develop_in` 的产出同一个
            //   函数算出；上面那道 64 MP 闸仍按整块原样判，裁切只减不增，不因为它放宽。
            let (dw, dh, orientation) = match &img {
                Some(img) => {
                    let full = frame(img);
                    let (dw, dh) = match tier_plan(img, &r) {
                        Some((_, tw, th)) => (tw, th),
                        None => (full.w as u32, full.h as u32),
                    };
                    (dw, dh, raw_orientation(img, input, x3f))
                }
                None => (w, h, 1),
            };
            let bytes_needed = dw as u64 * dh as u64 * 4;
            if r.budget_bytes != 0 && bytes_needed > r.budget_bytes as u64 {
                set_last_error("图片太大，超出宿主给的内存预算");
                return ERR_TOO_LARGE;
            }
            fill_info(info, dw, dh, bytes_needed, orientation);
            if x3f {
                unsafe { (*info).already_oriented = 1 };
            }
            return ERR_OK;
        }

        // ── 阶段 2：真解码 + 显影（按档位分箱，或全尺寸）。──────────────────
        let d = match develop_cached(input, &r) {
            Ok(d) => d,
            Err((code, msg)) if x3f => match flash::develop(input) {
                Ok((rgba, w, h)) => (rgba, w, h, 1),
                Err(_) => {
                    set_last_error(&msg);
                    return code;
                }
            },
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
        if x3f {
            unsafe { (*info).already_oriented = 1 };
        }
        ERR_OK
    })
}
pub(crate) fn raw_orientation(img: &rawler::RawImage, input: &[u8], x3f: bool) -> i32 {
    if x3f {
        return 1;
    }
    let o = orientation_to_exif(img.orientation);
    if o != 1 {
        return o;
    }
    if let Some(v) = orient::from_bytes(input) {
        if v != 1 {
            return v;
        }
    }
    // Phase One H 25 stores the sensor buffer in the rotated orientation and
    // does not put tag 274 in the TIFF IFD that rawler exposes. LibRaw/camera
    // previews use 6 for this back; keep the exception limited to that model.
    if img.model.trim() == "H 25" {
        return 6;
    }
    1
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

// ---------------------------------------------------------------------------
// 魔数嗅探
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 显影管线
// ---------------------------------------------------------------------------

mod develop;
use develop::orientation_to_exif;
pub use develop::{cam_to_srgb_wb, develop, develop_in};

mod binning;
pub use binning::{bin_plan, cfa_period, develop_binned};

// ---------------------------------------------------------------------------
// 测试：魔数嗅探 / 数学工具 / 显影数学（真实 RAW 样本见 examples/smoke_raw.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
