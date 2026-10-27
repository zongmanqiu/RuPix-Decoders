//! RuPix 外挂解码器：GIF → RGBA8（库：gif crate）。
//!
//! # 为什么是这个库（G1 实测，不是"全网都说"）
//!
//! 2026-10-15 本机实测（本仓历史记录，run **r1792006432**，n=200 取 median，同一 run 内配对）：
//! gif crate 直调 **1.18×** 于 core 现走的 `image` 路线（GIF 只有这一个独立 Rust 实现，
//! 收益全部来自拆掉 DynamicImage → into_rgba8() 的包装税——与 JPEG/PNG/TIFF 同一结论：
//! **快的不是库，是调用方式**）。WIC 照测但按宪章 §5.1"钉入不复活"，不作否决项。
//!
//! **发布配置复测（2026-10-15，run r1792052003，n=200 × photo/snowflake × S/M/L，6/6 bit-exact）**：
//! 选型 run 的 1.18× 是**裸帧**口径；发布路径（首帧合成 + 探帧）首测反而**比 image 慢 2.2%**
//! （r1792051563：86.5 vs 84.6）——整屏画布的 memset + blit（3MP ≈ 12MB）把裸帧优势全吃掉了。
//! 加「**整屏帧零拷贝快路径**」（见 [`decode_first_frame`]）后复测 **74.2 vs 86.6 = 1.17×**，
//! 与选型倍率复现。**教训：GIF 的"快"不在裸帧解码，在"首帧合成不许有第二份画布"**——
//! image 在 `帧矩形 == 屏幕` 时走 in-place 分支，冠军必须对齐这条路径。
//!
//! # 动画语义（与 core 的 `gif_anim` 会话逐条对齐）
//!
//! GIF 与静态图不同：宿主要拿 `frame_count` / `frame_delay_ms` 排播放计划。core 的会话
//! 缓存语义是：**查询阶段**报逻辑屏幕尺寸、帧数未知报 0（数帧 ≈ 全帧 LZW 解码，绝不
//! 放进首开路径）；**渲染阶段**逐帧服务、EOF 惰性发现总数、单帧判定为静态（delay=0）、
//! 多帧 0-delay 钳到 ≥10ms 防忙转。本解码器的对应取舍：
//!
//! * **只服务第 0 帧**（`frame_index != 0` → `ERR_UNSUPPORTED` 主动让位）。dispose 合成
//!   天然串行（第 N 帧必须顺序解完 0..N），core 已有游标续接 + 会话缓存，冠军重建一套
//!   是纯重复；宿主递增探帧时经让位链落到 core，播放语义不受影响。
//! * **首帧合成两条路径，均与 core 的 `image` into_frames 首帧逐字节一致**（parity 测试锁定，
//!   整屏帧与局部帧各一条）——frame 0 由冠军出、frame ≥1 由 core 出，两边不同像素会在播放
//!   第一跳时闪帧：
//!   ① **整屏帧（绝大多数 GIF）走零拷贝快路径**：直接解进宿主缓冲，透明像素清零点
//!   （等价"保留画布原值"），无第二份画布——这是发布配置能赢的关键（见上）；
//!   ② 局部帧矩形才分配透明画布 + blit，透明像素保留画布原值（**不是**写 gif crate
//!   输出的 (调色板色, 0)，那是与 image 差一字节的坑）。
//! * **单帧/多帧的判定用 `next_frame_info()` 轻探**：它只解析块结构、不解 LZW 像素，
//!   成本是 O(流长度) 的字节跳读。探不到第二帧 → 单帧静态（count=1、delay=0，与 core
//!   `count==Some(1)` 分支同语义）；探到了 → **顺势把表头跳完，总数当场数出**
//!   （count=帧数、delay=raw×10 钳 ≥10ms）。旧行为在这里报 count=0（未知），宿主刚打开
//!   就暂停只能写 `帧 1/?`。数中途出错退回 count=0（未知），不因此把图判成坏图。
//! * **逻辑屏幕尺寸自解析**（魔数 6 + LSD 7 = 13 字节，O(1)）：core 走会话缓存查尺寸
//!   是因为它是"全家桶"；冠军解码器为一项格式服务，直接读头即可，且与库自报的屏幕尺寸
//!   互为校验（不一致 = 结构坏，宁可报 CORRUPT 也不猜）。
//!
//! # 不接的部分
//!
//! * `frame_index != 0`：动画播放让位 core（见上）。
//! * 帧矩形超出逻辑屏幕、屏幕宽高为 0：结构坏，报 `ERR_CORRUPT`，不猜不救。
//! * "我不会解"与"文件坏了"必须分开报（解码器规范 §六第 10 条）：库的错误映射见
//!   [`dec_err`]——内存/限制类报 TOO_LARGE，结构/LZW/截断类报 CORRUPT，
//!   non_exhaustive 的未知变体报 UNSUPPORTED（宁可多走一步让位，不误判"文件坏了"）。

#![allow(non_snake_case)]
// cdylib 链接时 MSVC link.exe 会打印"正在创建库 …"，rustc 默认当成 lint 报出，这里静音。
#![allow(linker_messages)]

use rupix_abi::set_last_error;
use rupix_abi::*;
// 只有「独立解码器形态」才生成 iv_* 与自述宏；合并库不导出这些符号。
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::ffi::c_char;
use std::io::Cursor;
use std::slice;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "gif",
    name: "GIF (gif crate)",
    // GIF 后缀随冠军解码器迁出 core（照 png 的做法）：宿主 ext_claims_are_unique 门禁
    // 保证一个后缀只有一个认领者。动画帧 ≥1 的播放仍归 core（本解码器让位，见模块头）。
    extensions: "gif",
    description: "解码 GIF：gif crate 直调首帧画布；动画帧 ≥1 让位 core 会话",
    is_vector: 0,
    has_alpha: 1,
}

/// 解码上限：64 兆像素（RGBA8 ≈ 256 MiB）。与 core / png / jpeg / tiff 同口径。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// 动画帧最小展示时长（毫秒）：多帧 0-delay 防忙转。与 core 的 `gif_anim::MIN_DELAY_MS` 同值。
const MIN_DELAY_MS: u32 = 10;

/// 表头数帧的迭代上限（与 core `gif_anim::count_frames` 同值）：正常 GIF 到不了这个量级，
/// 它只挡"每次迭代都不消费字节"的畸形输入，免得数帧的循环自己变成挂死点。
const FRAME_SCAN_CAP: u32 = 100_000;

/// 最小合法长度：魔数 6 + 逻辑屏幕描述符 7 = 13。
const MIN_LEGAL_LEN: usize = 13;

/// gif 0.14.2 的读取器类型叫 `Decoder`（`DecodeOptions::read_info` 的返回），
/// 不是旧文档里的 `Reader`——以本地缓存源码为准。
type Reader<'d> = gif_crate::Decoder<Cursor<&'d [u8]>>;

// ---------------------------------------------------------------------------
// iv_probe / iv_render
// ---------------------------------------------------------------------------

/// GIF87a / GIF89a 魔数 + 逻辑屏幕校验。返回 100 = 本解码器是该格式的实测冠军。
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_probe(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    probe_impl(data, len, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn probe_impl(data: *const u8, len: usize, _ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < MIN_LEGAL_LEN {
            return 0;
        }
        let s = unsafe { slice::from_raw_parts(data, len) };
        if &s[..6] != b"GIF87a" && &s[..6] != b"GIF89a" {
            return 0;
        }
        // 宽或高为 0 的"屏幕"是无意义文件：挡在 probe，别让垃圾进渲染阶段。
        if u16::from_le_bytes([s[6], s[7]]) == 0 || u16::from_le_bytes([s[8], s[9]]) == 0 {
            return 0;
        }
        100
    })
}
#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    _ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, _ext)
}

/// 合并库直接调这一层：`guard` 在内部，调用方不必再包一层。
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
        let input = unsafe { slice::from_raw_parts(data, len) };
        let frame_index = if req.is_null() {
            0
        } else {
            unsafe { (*req).frame_index }
        };

        // ── 阶段 1：逻辑屏幕描述符自解析（O(1)，只碰头 13 字节，不触碰 LZW）。──
        let (sw, sh) = match screen_size(input) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(&msg);
                return code;
            }
        };
        let npixels = sw as u64 * sh as u64;
        if npixels > MAX_PIXELS {
            set_last_error("图片超过 6400 万像素的解码器上限");
            return ERR_TOO_LARGE;
        }
        let bytes_needed = npixels * 4;
        let budget: u32 = if req.is_null() {
            0
        } else {
            unsafe { (*req).budget_bytes }
        };
        if budget != 0 && bytes_needed > budget as u64 {
            set_last_error("图片太大，超出宿主给的内存预算");
            return ERR_TOO_LARGE;
        }

        if out.is_null() {
            // 查询阶段：帧数未知报 0（契约允许；数帧 ≈ 全帧 LZW，绝不放首开路径），
            // delay 也还不知道。宿主以渲染阶段回填为准、递增探帧发现总数。
            fill_info(info, sw, sh, bytes_needed, 0, 0);
            return ERR_OK;
        }

        // ── 渲染阶段：只服务第 0 帧，后续帧让位 core 的会话缓存。──────────────
        if frame_index != 0 {
            set_last_error("动图播放由 core 解码器负责，本冠军解码器让位");
            return ERR_UNSUPPORTED;
        }
        if (out_cap as u64) < bytes_needed {
            set_last_error("内部错误（宿主给的缓冲小于申报值）");
            return ERR_BUFFER_TOO_SMALL;
        }

        match decode_first_frame(input, sw, sh, out, bytes_needed) {
            Ok((count, delay)) => {
                fill_info(info, sw, sh, bytes_needed, count, delay);
                ERR_OK
            }
            Err((code, msg)) => {
                set_last_error(&msg);
                code
            }
        }
    })
}
#[inline]
fn fill_info(info: *mut IvRenderOut, w: u16, h: u16, bytes_needed: u64, count: u32, delay: u32) {
    let oi = unsafe { &mut *info };
    oi.actual_w = w as u32;
    oi.actual_h = h as u32;
    oi.bytes_needed = bytes_needed;
    oi.pixel_format = PF_RGBA8;
    // 调色板索引经 ColorOutput::RGBA 展开为 8bit，恒 8。
    oi.bit_depth = 8;
    oi.orientation = 1;
    // GIF 没有方向语义，声明"已定向"以免宿主二次旋转（与 core 一致）。
    oi.already_oriented = 1;
    oi.frame_count = count;
    oi.frame_delay_ms = delay;
    // GIF 单文件单页。
    oi.page_count = 1;
    // 不做色彩管理（与 core 的 image 路线同口径）。
    oi.icc_len = 0;
}

// ---------------------------------------------------------------------------
// gif crate 薄封装
// ---------------------------------------------------------------------------

/// 冠军配置：RGBA 直出（索引 → 调色板色展开，透明索引带 alpha=0）。
fn open(data: &[u8]) -> Result<Reader<'_>, gif_crate::DecodingError> {
    let mut opts = gif_crate::DecodeOptions::new();
    opts.set_color_output(gif_crate::ColorOutput::RGBA);
    opts.read_info(Cursor::new(data))
}

/// 逻辑屏幕描述符自解析：魔数 6 字节 + 宽高各 LE u16（偏移 6/8）。
fn screen_size(data: &[u8]) -> Result<(u16, u16), (i32, String)> {
    if data.len() < MIN_LEGAL_LEN {
        return Err((ERR_CORRUPT, "文件比逻辑屏幕描述符还短".into()));
    }
    if &data[..6] != b"GIF87a" && &data[..6] != b"GIF89a" {
        return Err((ERR_UNRECOGNIZED, "GIF 魔数不对".into()));
    }
    Ok((
        u16::from_le_bytes([data[6], data[7]]),
        u16::from_le_bytes([data[8], data[9]]),
    ))
}

/// 表头数帧（**另开一个解码器**，纯跳块、不解像素）：第一帧就报得出总帧数，
/// 宿主的信息条才不会写 `帧 1/?`。数不出来回 `None` → 宿主只写 `帧 1`，不写分母。
///
/// 这一趟是 O(文件字节) 的扫块，在本解码器的构建配置下不便宜（8.29 MB / 100 帧实测几毫秒），
/// 所以外面必须套 [`cached_count`] 那一层，别让它每次回到第 0 帧都跑。
fn count_frames(data: &[u8]) -> Option<u32> {
    let mut dec = gif_crate::Decoder::new(data).ok()?;
    let mut n = 0u32;
    loop {
        match dec.next_frame_info() {
            Ok(Some(_)) => n += 1,
            Ok(None) => break,
            Err(_) => return None,
        }
        if n >= FRAME_SCAN_CAP {
            return None;
        }
    }
    (n > 0).then_some(n)
}

/// 缓存槽的内容：`(文件长度, 头 32 字节, 尾 32 字节) -> 总帧数`。起名是为了过 `type_complexity`。
type CountSlot = ((u64, [u8; 32], [u8; 32]), u32);

/// 一槽总数缓存，键 = `(len, head32, tail32)` 指纹（与 core 的会话键同一个套路）。
///
/// ★ 为什么必须缓存：回到第 0 帧是常态操作——循环播放每绕一圈要一次、后退退到头也要一次，
/// 而本解码器是无会话的"只服务第 0 帧"设计，不缓存就每次都重付一趟全文件扫块。
/// 换图 = 指纹变 = 重算；`0`（数不出来）也照缓存，免得畸形文件每次都被重走。
static COUNT_CACHE: std::sync::Mutex<Option<CountSlot>> = std::sync::Mutex::new(None);

fn fingerprint(data: &[u8]) -> (u64, [u8; 32], [u8; 32]) {
    let n = data.len();
    let mut head = [0u8; 32];
    let mut tail = [0u8; 32];
    head.copy_from_slice(&data[..n.min(32)]);
    let t = &data[n.saturating_sub(32)..];
    tail[..t.len()].copy_from_slice(t);
    (n as u64, head, tail)
}

fn cached_count(data: &[u8]) -> u32 {
    let fp = fingerprint(data);
    let mut slot = COUNT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, c)) = slot.as_ref() {
        if *k == fp {
            return *c;
        }
    }
    let c = count_frames(data).unwrap_or_default();
    *slot = Some((fp, c));
    c
}

/// 解第 0 帧进宿主缓冲：透明画布 + 帧矩形 blit；返回 (frame_count, frame_delay_ms)。
fn decode_first_frame(
    data: &[u8],
    sw: u16,
    sh: u16,
    out: *mut u8,
    bytes_needed: u64,
) -> Result<(u32, u32), (i32, String)> {
    let mut dec = open(data).map_err(dec_err)?;
    // 库自报的屏幕尺寸必须与 LSD 自解析一致：两套解析漂移说明我们对格式的理解有洞，
    // 宁可报坏也不把图解成错色。
    let (rw, rh) = (dec.width(), dec.height());
    if (rw, rh) != (sw, sh) {
        return Err((
            ERR_CORRUPT,
            format!("全局色板尺寸 {rw}x{rh} 与文件头 {sw}x{sh} 不符"),
        ));
    }
    let hdr = dec
        .next_frame_info()
        .map_err(dec_err)?
        .ok_or_else(|| (ERR_CORRUPT, "结束标记之前没有任何帧".to_string()))?;
    // 帧矩形越出逻辑屏幕 = 结构坏（解入/blit 会越界写，必须挡在拷贝前）。
    if hdr.left as u64 + hdr.width as u64 > sw as u64
        || hdr.top as u64 + hdr.height as u64 > sh as u64
    {
        return Err((
            ERR_CORRUPT,
            format!(
                "帧区域 {}x{}+{},{} 越出画面 {sw}x{sh}",
                hdr.width, hdr.height, hdr.left, hdr.top
            ),
        ));
    }
    // 帧元数据一次拷成标量：下面不再碰 `hdr`。★ 它是 `dec` 的**借用**（`next_frame_info`
    // 返回 `Option<&Frame>`），所以没有"释放 header"这回事——借用按 NLL 在最后一次使用后
    // 自动结束，`dec` 随即可以接着 `read_into_buffer`。原先那句 `drop(hdr)` 丢弃一个引用，
    // 是空操作（编译器 `dropping_references` 警告），留着只会误导读者以为做了释放。
    let raw_delay_ms = hdr.delay as u32 * 10;
    let has_trns = hdr.transparent.is_some();
    let (fl, ft) = (hdr.left as usize, hdr.top as usize);
    let (fw, fh) = (hdr.width as usize, hdr.height as usize);
    let full_frame = fl == 0 && ft == 0 && hdr.width == sw && hdr.height == sh;

    let dst = unsafe { slice::from_raw_parts_mut(out, bytes_needed as usize) };
    if full_frame {
        // ★ 快路径（G1 复测逼出来的）：帧覆盖整屏时不建独立画布，直接解到宿主缓冲——
        //   等价于 image 在 `frame rect == screen` 时的 in-place 分支，省掉一次
        //   w×h×4 的画布分配 + 整屏 blit（3MP 就是 12MB 的 memset + 拷贝，足以吃掉
        //   裸帧解码的全部优势：r1792051563 复测，不优化时发布配置 86.5ms 反比
        //   image 路线的 84.6ms 慢 2.2%）。
        //   透明像素的"保留画布原值"（画布 = 全透明）等价于把 alpha==0 像素清零。
        dec.read_into_buffer(dst).map_err(dec_err)?;
        if has_trns {
            for px in dst.chunks_exact_mut(4) {
                if px[3] == 0 {
                    px[0] = 0;
                    px[1] = 0;
                    px[2] = 0;
                }
            }
        }
    } else {
        // 慢路径：局部帧矩形 → 独立画布 + blit。
        // 透明像素**保留画布原值**（(0,0,0,0)），而不是写 gif crate 输出的
        // (调色板色, 0)——与 core 的 `image` 路线（`blend_and_dispose_pixel` 里
        // "alpha==0 → 取 previous"）逐字节对齐。让位链上 frame 0 冠军出、
        // frame ≥1 core 出，差这一处播放第一跳就会闪帧。
        dst.fill(0);
        let fbuf_size = dec.buffer_size();
        if fbuf_size != fw * fh * 4 {
            return Err((
                ERR_CORRUPT,
                format!("帧缓冲 {fbuf_size} 与帧区域 {fw}x{fh} 不符"),
            ));
        }
        let mut fbuf = vec![0u8; fbuf_size];
        dec.read_into_buffer(&mut fbuf).map_err(dec_err)?;
        let sw_us = sw as usize;
        for row in 0..fh {
            let src_row = &fbuf[row * fw * 4..][..fw * 4];
            let dst_row = &mut dst[((ft + row) * sw_us + fl) * 4..][..fw * 4];
            if has_trns {
                for (s, d) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    if s[3] != 0 {
                        d.copy_from_slice(s);
                    }
                }
            } else {
                // 无透明索引：整行直拷（绝大多数 GIF 的路径）。
                dst_row.copy_from_slice(src_row);
            }
        }
    }

    // 轻探第二帧（旧契约原样保留：这一步报错就是文件坏了）：探不到 → 单帧静态。
    if dec.next_frame_info().map_err(dec_err)?.is_none() {
        return Ok((1, 0));
    }
    // 多帧：总数走表头数出来（`cached_count`：另开解码器扫一遍并缓存一槽）。旧行为在这里
    // 报 count=0（未知），于是宿主刚打开就暂停只能写 `帧 1/?`。数不出来仍报 0——
    // "总数没数清"不是"文件坏了"，不许因此让一张本来能开的图打不开。
    let count = cached_count(data);
    Ok((count, raw_delay_ms.max(MIN_DELAY_MS)))
}

/// 库的错误 → ABI 错误码。**"我不会解"与"文件坏了"必须分开报**（解码器规范 §六第 10 条）。
/// 枚举是 `#[non_exhaustive]`：未知变体不当"坏文件"误报，诚实让位（core 重试一次代价极小，
/// 误报"文件已损坏"的代价是用户永远打不开这张图）。
fn dec_err(e: gif_crate::DecodingError) -> (i32, String) {
    use gif_crate::DecodingError as E;
    let code = match &e {
        // 解码器内部内存/限制类 = 资源不够，不是文件坏。
        E::OutOfMemory | E::MemoryLimit => ERR_TOO_LARGE,
        // 结构坏 / LZW 坏 / 截断 / 流读不动 = "文件坏了"。
        E::Format(_)
        | E::LzwError(_)
        | E::Io(_)
        | E::UnexpectedEof
        | E::DecoderNotFound
        | E::EndCodeNotFound => ERR_CORRUPT,
        _ => ERR_UNSUPPORTED,
    };
    (code, format!("GIF 库报错 \"{e}\""))
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gif_crate::{Encoder, Frame};
    use std::borrow::Cow;
    use std::mem::size_of;

    fn req() -> IvRenderReq {
        IvRenderReq {
            target_w: 0,
            target_h: 0,
            budget_bytes: 0,
            flags: 0,
            frame_index: 0,
            page_index: 0,
        }
    }

    fn req_frame(index: u32) -> IvRenderReq {
        IvRenderReq {
            frame_index: index,
            ..req()
        }
    }

    /// 走宿主的两阶段协议，拿回 (info, RGBA8)。
    fn decode_via_decoder(
        data: &[u8],
        r: &IvRenderReq,
    ) -> Result<(IvRenderOut, Vec<u8>), (i32, String)> {
        let mut info = IvRenderOut::default();
        let q = iv_render(
            data.as_ptr(),
            data.len(),
            r,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        if q != ERR_OK {
            return Err((q, last_err()));
        }
        let mut buf = vec![0u8; info.bytes_needed as usize];
        let rr = iv_render(
            data.as_ptr(),
            data.len(),
            r,
            buf.as_mut_ptr(),
            buf.len(),
            &mut info,
            std::ptr::null(),
        );
        if rr != ERR_OK {
            return Err((rr, last_err()));
        }
        Ok((info, buf))
    }

    fn last_err() -> String {
        let mut e = [0i8; 512];
        let need = unsafe { rupix_abi::write_last_error(e.as_mut_ptr(), e.len()) } as usize;
        let n = need.saturating_sub(1).min(e.len());
        String::from_utf8_lossy(&e[..n].iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }

    /// 造 GIF：每帧整屏纯色（调色板索引 = 帧序号），无 disposal 纠缠。
    /// `delay` 为 GIF 单位（10ms）。调色板 0 填充到 8 项（GIF 要求 2 的幂）。
    fn encode_gif(colors: &[[u8; 3]], delay: u16) -> Vec<u8> {
        let mut palette = vec![0u8; 8 * 3];
        for (i, c) in colors.iter().enumerate() {
            palette[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let mut out = Vec::new();
        {
            let mut enc = Encoder::new(&mut out, 4, 3, &palette).expect("写头");
            for (i, _) in colors.iter().enumerate() {
                let frame = Frame {
                    width: 4,
                    height: 3,
                    delay,
                    buffer: Cow::Owned(vec![i as u8; 4 * 3]),
                    ..Default::default()
                };
                enc.write_frame(&frame).expect("写帧");
            }
        }
        out
    }

    /// 造带透明的多帧 GIF：帧 0 的第 0 像素用透明索引（其余不透明），
    /// 用来验证画布合成与 image 逐字节一致（透明像素的颜色处理是易错点）。
    fn encode_gif_transparent() -> Vec<u8> {
        // 调色板：0 = 透明位（色值无所谓），1 = 红，2 = 绿。
        let palette = [
            255u8, 255, 255, // idx0（被 transparent 指认）
            255, 0, 0, // idx1 红
            0, 255, 0, // idx2 绿
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0 填充到 8 项
        ];
        let mut out = Vec::new();
        {
            let mut enc = Encoder::new(&mut out, 4, 3, &palette).expect("写头");
            // 帧 0：红，左上角像素透明。
            let mut px = vec![1u8; 4 * 3];
            px[0] = 0;
            let f0 = Frame {
                width: 4,
                height: 3,
                delay: 10,
                transparent: Some(0),
                buffer: Cow::Owned(px),
                ..Default::default()
            };
            enc.write_frame(&f0).expect("写帧 0");
            // 帧 1：绿。
            let f1 = Frame {
                width: 4,
                height: 3,
                delay: 10,
                buffer: Cow::Owned(vec![2u8; 4 * 3]),
                ..Default::default()
            };
            enc.write_frame(&f1).expect("写帧 1");
        }
        out
    }

    /// core 的 image 路线（into_frames）第 index 帧的 (画布尺寸, RGBA, delay_ms)。
    fn image_first_frame(data: &[u8]) -> ((u32, u32), Vec<u8>, u32) {
        let dec =
            image::codecs::gif::GifDecoder::new(Cursor::new(data.to_vec())).expect("image 打开");
        let mut frames = image::AnimationDecoder::into_frames(dec);
        let f = frames.next().expect("有帧").expect("帧有效");
        (
            (f.buffer().width(), f.buffer().height()),
            f.buffer().as_raw().clone(),
            f.delay().numer_denom_ms().0,
        )
    }

    // ── ABI 布局 ──────────────────────────────────────────────────────────────

    /// 与 abi crate 的编译期断言同口径：跨 DLL 布局漂移在这里现形。
    #[test]
    fn abi_layout_asserts_hold() {
        assert_eq!(size_of::<IvRenderReq>(), 24);
        assert_eq!(size_of::<IvRenderOut>(), 48);
        assert_eq!(IV_ABI_VERSION, 5);
    }

    // ── probe ────────────────────────────────────────────────────────────────

    #[test]
    fn probe_accepts_gif_magic_and_rejects_lookalikes() {
        let real = encode_gif(&[[255, 0, 0]], 0);
        assert_eq!(iv_probe(real.as_ptr(), real.len(), std::ptr::null()), 100);
        // 87a / 89a 都认领（"GIF89a" 的 '9'→'7'，注意 8 在 index 3、9 在 index 4）。
        let mut v87 = real.clone();
        v87[4] = b'7';
        assert_eq!(iv_probe(v87.as_ptr(), v87.len(), std::ptr::null()), 100);
        // 魔数改一字节 → 不认领。
        let mut bad = real.clone();
        bad[1] = b'X';
        assert_eq!(iv_probe(bad.as_ptr(), bad.len(), std::ptr::null()), 0);
        // 短于 LSD → 不认领。
        assert_eq!(
            iv_probe(c"GIF89a\x01".as_ptr().cast::<u8>(), 8, std::ptr::null()),
            0
        );
        // 屏幕宽/高为 0 → 无意义文件，不认领。
        let mut zero = real.clone();
        zero[6] = 0;
        zero[7] = 0;
        assert_eq!(iv_probe(zero.as_ptr(), zero.len(), std::ptr::null()), 0);
        assert_eq!(iv_probe(std::ptr::null(), 0, std::ptr::null()), 0);
    }

    // ── 查询阶段 ─────────────────────────────────────────────────────────────

    /// 查询阶段只读 LSD：尺寸/字节数准确，帧数报 0（未知，契约允许），delay 未知报 0。
    #[test]
    fn query_phase_reports_screen_without_decoding() {
        let data = encode_gif(&[[255, 0, 0]], 10);
        let mut info = IvRenderOut::default();
        let rc = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(rc, ERR_OK, "查询失败: {}", last_err());
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        assert_eq!(info.bytes_needed, 4 * 3 * 4);
        assert_eq!(info.pixel_format, PF_RGBA8);
        assert_eq!(info.frame_count, 0, "查询阶段帧数未知必须报 0");
        assert_eq!(info.frame_delay_ms, 0, "查询阶段 delay 未知报 0");
    }

    // ── 单帧静态 ─────────────────────────────────────────────────────────────

    /// 单帧 GIF = 静态图：count=1、delay=0（与 core `count==Some(1)` 分支同语义），
    /// 像素与 core 的 image 路线逐字节相等。
    #[test]
    fn single_frame_is_static_and_matches_image() {
        let data = encode_gif(&[[255, 0, 0]], 10); // 编码端给 100ms 也要报静态
        let (info, buf) = decode_via_decoder(&data, &req()).expect("解码失败");
        assert_eq!(info.frame_count, 1, "单帧必须报 1");
        assert_eq!(info.frame_delay_ms, 0, "单帧必须报静态 delay=0");
        let ((iw, ih), ipx, _) = image_first_frame(&data);
        assert_eq!((info.actual_w, info.actual_h), (iw, ih));
        assert_eq!(buf, ipx, "与 image 路线逐字节不一致");
        // 语义抽查：整屏纯红。
        assert!(buf.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    // ── 多帧动画 ─────────────────────────────────────────────────────────────

    /// 多帧首帧：总数由表头跳读当场给出（3 帧就报 3）、delay = GCE×10ms；后续帧让位 core。
    #[test]
    fn multi_frame_reports_exact_count_and_delay() {
        let data = encode_gif(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]], 10); // 100ms
        let (info, buf) = decode_via_decoder(&data, &req()).expect("解码失败");
        assert_eq!(info.frame_count, 3, "多帧总数第一帧就该报得出");
        assert_eq!(info.frame_delay_ms, 100, "delay = GCE(10) × 10ms");
        // 首帧 = 帧 0 纯红（整帧 blit，画布无残留）。
        assert!(buf.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        // frame_index=1 必须让位（UNSUPPORTED），不是"文件坏了"。
        let e = decode_via_decoder(&data, &req_frame(1)).unwrap_err();
        assert_eq!(e.0, ERR_UNSUPPORTED, "frame 1 应让位 core，实际: {e:?}");
    }

    /// 尾字节被改成非法块类型：首帧必须照样解得出来——"总数没数清"不是"文件已损坏"。
    /// 数得出就报真数，数不出就报 0（宿主于是只写 `帧 1`，不写分母、更不写问号）。
    #[test]
    fn uncountable_tail_still_opens_and_never_fakes_a_total() {
        let mut data = encode_gif(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]], 10);
        let last = data.len() - 1;
        data[last] = 0xFF; // GIF 结束块 0x3B → 非法块类型
        let (info, buf) = decode_via_decoder(&data, &req()).expect("首帧必须照样解出来");
        assert!(
            info.frame_count == 0 || info.frame_count == 3,
            "总数只能「要么数得出、要么老实说未知」，实际 {}",
            info.frame_count
        );
        assert!(
            buf.chunks_exact(4).all(|p| p == [255, 0, 0, 255]),
            "首帧像素不受影响"
        );
    }

    /// 多帧 0-delay 钳到 ≥10ms 防忙转（与 core 的 MIN_DELAY_MS 同规矩）。
    #[test]
    fn multi_frame_zero_delay_is_clamped() {
        let data = encode_gif(&[[255, 0, 0], [0, 255, 0]], 0);
        let (info, _) = decode_via_decoder(&data, &req()).expect("解码失败");
        assert_eq!(info.frame_delay_ms, MIN_DELAY_MS);
    }

    // ── parity（让位链衔接的根基）───────────────────────────────────────────

    /// 首帧画布与 core 的 image into_frames 逐字节一致——含透明像素。
    /// 透明索引的颜色处理是 GIF 合成最易错处，让位链上 frame0/≥1 分属两家，
    /// 这里差一字节播放第一跳就会闪帧。
    #[test]
    fn transparent_first_frame_parity_with_image() {
        let data = encode_gif_transparent();
        let (info, buf) = decode_via_decoder(&data, &req()).expect("解码失败");
        let ((iw, ih), ipx, idelay) = image_first_frame(&data);
        assert_eq!((info.actual_w, info.actual_h), (iw, ih));
        assert_eq!(buf, ipx, "透明帧画布与 image 逐字节不一致");
        // delay 语义对齐：两家都把 GCE(10) 报成 100ms。
        assert_eq!(info.frame_delay_ms, idelay);
        assert_eq!(info.frame_count, 2, "样本是 2 帧，总数要由表头跳读数出来");
        // 语义抽查：透明像素 = 画布透明原值（不是 gif crate 输出的调色板色），
        // 不透明像素 = 调色板红 + alpha 255。
        assert_eq!(&buf[0..4], &[0, 0, 0, 0], "idx0 透明像素应取画布原值");
        assert_eq!(&buf[4..8], &[255, 0, 0, 255], "idx1 红像素");
    }

    /// 造局部帧矩形样本：逻辑屏幕 4×3，帧只画 2×2 于 (1,1)。
    /// 专门覆盖慢路径（画布 + blit）与 image 的"边界外取 previous"行为一致性。
    fn encode_gif_partial_rect() -> Vec<u8> {
        let palette = [
            255u8, 0, 0, // idx0 红
            0, 255, 0, // idx1 绿
            0, 0, 255, // idx2 蓝
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0 填充到 8 项
        ];
        let mut out = Vec::new();
        {
            let mut enc = Encoder::new(&mut out, 4, 3, &palette).expect("写头");
            let f = Frame {
                left: 1,
                top: 1,
                width: 2,
                height: 2,
                delay: 10,
                buffer: Cow::Owned(vec![2u8; 2 * 2]), // 2×2 全蓝
                ..Default::default()
            };
            enc.write_frame(&f).expect("写局部帧");
        }
        out
    }

    /// 局部帧矩形：慢路径（画布 + blit）与 image 逐字节一致——帧外像素是画布透明原值。
    #[test]
    fn partial_rect_first_frame_parity_with_image() {
        let data = encode_gif_partial_rect();
        let (info, buf) = decode_via_decoder(&data, &req()).expect("解码失败");
        assert_eq!((info.actual_w, info.actual_h), (4, 3));
        let ((iw, ih), ipx, _) = image_first_frame(&data);
        assert_eq!((info.actual_w, info.actual_h), (iw, ih));
        assert_eq!(buf, ipx, "局部帧矩形画布与 image 逐字节不一致");
        // 语义抽查：帧外 = 透明 (0,0,0,0)；(1,1) 起 2×2 = 蓝。
        assert_eq!(&buf[0..4], &[0, 0, 0, 0], "帧外像素应透明");
        assert_eq!(&buf[(4 + 1) * 4..][..4], &[0, 0, 255, 255], "(1,1) 应为蓝");
        assert_eq!(
            &buf[(2 * 4 + 2) * 4..][..4],
            &[0, 0, 255, 255],
            "(2,2) 应为蓝"
        );
        assert_eq!(&buf[(4 + 3) * 4..][..4], &[0, 0, 0, 0], "(3,1) 帧外应透明");
    }

    // ── 防御性 ───────────────────────────────────────────────────────────────

    #[test]
    fn buffer_too_small_is_reported_not_overrun() {
        let data = encode_gif(&[[255, 0, 0]], 0);
        let mut info = IvRenderOut::default();
        let mut tiny = [0u8; 8];
        let rc = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            tiny.as_mut_ptr(),
            tiny.len(),
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(rc, ERR_BUFFER_TOO_SMALL);
    }

    /// 巨屏幕（65535×65535 ≈ 4G 像素）挡在分配前。GIF 无 CRC，LSD 直接改写即可。
    #[test]
    fn huge_screen_rejected_before_alloc() {
        let mut data = encode_gif(&[[255, 0, 0]], 0);
        data[6] = 0xFF;
        data[7] = 0xFF;
        data[8] = 0xFF;
        data[9] = 0xFF;
        assert_eq!(
            iv_probe(data.as_ptr(), data.len(), std::ptr::null()),
            100,
            "probe 只验魔数与非零尺寸，巨屏照样认领（渲染阶段才拒）"
        );
        let mut info = IvRenderOut::default();
        let rc = iv_render(
            data.as_ptr(),
            data.len(),
            &req(),
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(rc, ERR_TOO_LARGE, "巨屏必须在分配前被拒");
    }

    /// budget 生效：4×3 只要 48 字节，给 8 字节预算必须报太大。
    #[test]
    fn budget_is_honoured() {
        let data = encode_gif(&[[255, 0, 0]], 0);
        let mut info = IvRenderOut::default();
        let req = IvRenderReq {
            budget_bytes: 8,
            ..req()
        };
        let rc = iv_render(
            data.as_ptr(),
            data.len(),
            &req,
            std::ptr::null_mut(),
            0,
            &mut info,
            std::ptr::null(),
        );
        assert_eq!(rc, ERR_TOO_LARGE);
    }

    /// 截断流：撞 LZW 中途 EOF 必须报 CORRUPT，绝不挂死。
    #[test]
    fn truncated_stream_is_corrupt_not_hung() {
        let mut data = encode_gif(&[[255, 0, 0]], 0);
        data.truncate(data.len() - 6); // 掐掉尾部（含块终止），LZW 流不完整
        let e = decode_via_decoder(&data, &req()).unwrap_err();
        assert_eq!(e.0, ERR_CORRUPT, "截断应报文件坏了，实际: {e:?}");
    }

    // ── 测试脚手架 ───────────────────────────────────────────────────────────

    // （IvRenderReq 是 abi crate 的外部类型，按孤儿规则不能本地 impl——
    //   带变体的构造一律用结构体更新语法 `IvRenderReq { ..req() }`。）
}
