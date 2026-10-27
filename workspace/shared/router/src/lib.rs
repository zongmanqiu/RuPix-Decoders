//! 合并档共用的路由内核。
//!
//! 一枚合并 DLL 里装了几个「每格式冠军」，对外只有一个身份；宿主看到的是
//! "这个解码器认领 N 个后缀、能整解"，**内部分给谁、失败后换谁，全在这里决定**。
//!
//! 三件事说清楚，因为每个合并成员都要靠它们：
//!
//! 1. **探针取最高分**。合并解码器只有一个 `iv_probe`，答案是各成员里最确定的那个
//!    （专项冠军 100、通用兜底 50、不认 0）。低于最高分的成员仍然留在候选表里——
//!    它们就是让位链的后半段。
//! 2. **查询阶段让位**：软失败（`ERR_UNSUPPORTED` / `ERR_UNRECOGNIZED`）换下一家；
//!    硬错误（坏文件 / 超预算 / OOM）立即返回，那是对这份文件的判断，换一家也是
//!    同一个结论，继续试只会把错误文本换掉。
//! 3. **渲染阶段也让位，但换人前必须重问尺寸**。宿主是按上一个赢家申报的
//!    `bytes_needed` 分配缓冲的，换人后尺寸可能更大——直接写就是越界。
//!
//! 第 3 条是实测踩出来的（2026-10-25）：`gif` 只服务第 0 帧、`apng` 在查询阶段
//! 就让位，动图后续帧要靠 `core` 的会话缓存。合并前那是"宿主换解码器"，合并后宿主
//! 只看得到合并档这一枚——让位不在这里走完，动图从第 2 帧起每次都报"解码失败"。
//!
//! 这段逻辑**只有这一份**：合并档已经有四五个，抄一份就是四个地方要同步修同一个
//! 边界条件，而它改一次只花十分钟、漏一次要用户报"动图不动了"。

use rupix_abi::set_last_error;
use rupix_abi::*;
use std::ffi::c_char;

/// 成员的 `probe_impl`：魔数 + 扩展名 → 置信度（0 = 不认）。
pub type ProbeFn = fn(*const u8, usize, *const c_char) -> i32;

/// 成员的 `render_impl`：与 `iv_render` 逐字同签名，`out == NULL` 为查询阶段。
pub type RenderFn = fn(
    *const u8,
    usize,
    *const IvRenderReq,
    *mut u8,
    usize,
    *mut IvRenderOut,
    *const c_char,
) -> i32;

/// 合并档里的一个成员。
pub struct Member {
    /// 解码器 id（只用于排查与错误信息，路由靠下面两个函数）。
    pub id: &'static str,
    pub probe: ProbeFn,
    pub render: RenderFn,
}

/// 只有这两个码算"我解不了这个文件，换一家问"，其余都是对这份文件的最终判断。
pub fn is_soft_fail(code: i32) -> bool {
    code == ERR_UNSUPPORTED || code == ERR_UNRECOGNIZED
}

/// 探针认得的成员下标，按置信度从高到低（稳定排序：同分保持传入顺序）。
///
/// 置信度只算一次：`probe_impl` 要解文件头，放进排序比较器里反复调等于把首开路径翻倍。
fn candidates(members: &[Member], data: *const u8, len: usize, ext: *const c_char) -> Vec<usize> {
    let conf: Vec<i32> = members.iter().map(|m| (m.probe)(data, len, ext)).collect();
    let mut order: Vec<usize> = (0..members.len()).filter(|&i| conf[i] > 0).collect();
    order.sort_by(|&a, &b| conf[b].cmp(&conf[a]));
    order
}

/// 「都没成」时的错误文本：**最后一家的原话留在前面**，试过谁只作排查尾巴。
///
/// 用户要的是"为什么打不开"；只报"已试：a、b、c"等于把问题推回给他。
fn set_chain_error(members: &[Member], tried: &[usize]) {
    if tried.is_empty() {
        set_last_error("这个文件没有可用的解码库（合并档里没有成员认领它）");
        return;
    }
    let names: Vec<&str> = tried.iter().map(|&i| members[i].id).collect();
    let names = names.join("、");
    let detail = rupix_abi::last_error_text();
    if detail.is_empty() {
        set_last_error(&format!("这个文件没有可用的解码库（已试：{names}）"));
    } else {
        set_last_error(&format!("{detail}（已试：{names}）"));
    }
}

/// 查询阶段：按候选顺序逐个问尺寸，返回**第一个能出尺寸**的成员下标。
///
/// 返回 `Err(code)` 时 `info` 里留着最后一次的尺寸申报，宿主据此仍能说清
/// "多大、为什么不行"。
fn pick_first_able(
    members: &[Member],
    order: &[usize],
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> Result<usize, i32> {
    let mut tried: Vec<usize> = Vec::with_capacity(order.len());
    let mut last = ERR_UNRECOGNIZED;
    for &i in order {
        tried.push(i);
        let code = (members[i].render)(data, len, req, std::ptr::null_mut(), 0, info, ext);
        if code == ERR_OK {
            return Ok(i);
        }
        if !is_soft_fail(code) {
            return Err(code);
        }
        last = code;
    }
    set_chain_error(members, &tried);
    Err(last)
}

/// 渲染阶段：从查询选中的赢家开始，软失败就往下一家让——**换人前重问一次尺寸**。
///
/// 装不进宿主给的缓冲就不写，记下来最后如实报 `ERR_BUFFER_TOO_SMALL`（宿主不重试，
/// 但报错比静默失败可查）。
#[allow(clippy::too_many_arguments)]
fn render_from(
    members: &[Member],
    order: &[usize],
    first: usize,
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    let start = order.iter().position(|&i| i == first).unwrap_or(0);
    let mut tried: Vec<usize> = Vec::with_capacity(order.len());
    let mut last = ERR_UNSUPPORTED;
    let mut too_small = false;
    for (n, &i) in order.iter().enumerate().skip(start) {
        tried.push(i);
        if n != start {
            // 换人：先问尺寸。`info` 只在确认装得下之后才改，免得给宿主留下一份
            // "申报了却没写"的错配。
            let mut q = IvRenderOut::default();
            let code = (members[i].render)(data, len, req, std::ptr::null_mut(), 0, &mut q, ext);
            if code != ERR_OK {
                if !is_soft_fail(code) {
                    return code;
                }
                last = code;
                continue;
            }
            if q.bytes_needed as usize > out_cap {
                too_small = true;
                continue;
            }
            unsafe { *info = q };
        }
        let code = (members[i].render)(data, len, req, out, out_cap, info, ext);
        if code == ERR_OK {
            return ERR_OK;
        }
        if !is_soft_fail(code) {
            return code;
        }
        last = code;
    }
    if too_small {
        set_last_error("内部错误（让位后的解码库需要更大的缓冲，请反馈）");
        return ERR_BUFFER_TOO_SMALL;
    }
    set_chain_error(members, &tried);
    last
}

/// `iv_probe` 的实现：所有成员各跑一遍，报**最高**的那个置信度。
///
/// 合并解码器对外只有一个身份，"我有多确定"必须取最确定的那个——否则一个报 100 的
/// 冠军会被同表里报 0 的成员拉平，宿主就可能把它排到别的解码器后面去。
pub fn probe(members: &[Member], data: *const u8, len: usize, ext: *const c_char) -> i32 {
    let mut top = 0i32;
    for m in members {
        let c = (m.probe)(data, len, ext);
        if c > top {
            top = c;
        }
        if top >= 100 {
            break; // 没有比 100 更高的置信度了
        }
    }
    top
}

/// `iv_render` 的实现：两阶段共用同一张候选表，查询选人、渲染让位。
#[allow(clippy::too_many_arguments)] // C ABI 形状：7 个参数由宿主契约固定，不能再打包
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn render(
    members: &[Member],
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    if data.is_null() || len == 0 {
        set_last_error("内部错误（宿主传入空数据）");
        return ERR_UNRECOGNIZED;
    }
    if info.is_null() {
        set_last_error("内部错误（宿主传入空输出指针）");
        return ERR_BUFFER_TOO_SMALL;
    }
    // 一次探针，两个阶段共用：候选顺序只由内容 + 扩展名决定，与 req 无关，
    // 所以查询和渲染看到的是同一张表（这一点是"让位"能成立的前提）。
    let order = candidates(members, data, len, ext);
    match pick_first_able(members, &order, data, len, req, info, ext) {
        Ok(_) if out.is_null() => ERR_OK,
        Ok(first) => {
            // 渲染前核一遍宿主给的缓冲：查询阶段申报的是这个数，宿主就该按它分配。
            let need = unsafe { (*info).bytes_needed } as usize;
            if need > out_cap {
                set_last_error("内部错误（宿主给的缓冲小于申报值）");
                return ERR_BUFFER_TOO_SMALL;
            }
            render_from(
                members, &order, first, data, len, req, out, out_cap, info, ext,
            )
        }
        Err(code) => code,
    }
}
