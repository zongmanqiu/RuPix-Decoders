//! RuPix pro 档成员：WMF（`.wmf`）与 WMZ（`.wmz`，gzip 包装的 WMF）。
//!
//! `wmf-core` 把 WMF 转成 SVG，再由 `resvg` 光栅化；输出 **非预乘** RGBA8。
//! 矢量语义（规范 §十五）：`render_impl` 按 `req.target_w/target_h` 把 usvg 场景
//! **等比缩放**装进目标框（0 = 原始尺寸）；可选导出 `iv_export_svg` 把中间产物
//! SVG 文本字节直接交出（宿主缓存后用自家 resvg 按视口重渲）。`.wmz` 在转换前
//! 先解 gzip 包。文本记录保留为 `<text>` + 通用 font-family，宿主 resvg 用系统
//! 字体渲染；本解码器不承诺内嵌字体或把文本转路径。
//!
//! ★ **区域渲染（ABI v6，规范 §十六）**：`export_region_impl` 把文档坐标里的
//! **指定区域**按密度**直接光栅化**——不是"渲大图再缩"，而是把变换矩阵直接
//! 挪到那一小块上重画。这是"矢量图放大到 4000% 也无损"的解码器侧实现：
//! 画布恒为一屏大小（倍率无上限），线条是矢量重画（不是位图插值）。
//! 解析结果按字节指纹**进程内缓存**（同一份字节会连着换区域请求，契约 §十六
//! 第 4 条硬要求"不许每次重新解析文档"）。

#![allow(non_snake_case)]
#![allow(linker_messages)]

use pro_support as support;
use rupix_abi::*;
#[cfg(feature = "standalone")]
use rupix_abi::{export_decoder, guard};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_char;
use std::io::Read;
use std::sync::Arc;

#[cfg(feature = "standalone")]
export_decoder! {
    id: "pro-wmf",
    name: "Windows Metafile (WMF)",
    extensions: "wmf",
    description: "WMF（转 SVG 后光栅化）",
    is_vector: 1,
    has_alpha: 1,
}

#[cfg(feature = "standalone")]
rupix_abi::export_caps!(rupix_abi::CAP_FULL | rupix_abi::CAP_VECTOR | rupix_abi::CAP_REGION);

#[cfg(feature = "standalone")]
rupix_abi::export_metadata! {
    "version" => "0.1.0",
    "author" => "RuPix",
    "license" => "AGPL-3.0",
    "released" => "2026-10-28",
    "vector_exts" => "wmf;wmz",
}

/// `.wmz` 解包上限（解压后的 WMF 字节数）。gzip 的压缩比没有数学下界（全零流
/// 1032:1 起），不设闸的话一枚手造 `.wmz` 能把 64 MP 闸后面的内存闸整个绕过——
/// 必须在解包这一步卡死总量。64 MiB 的 WMF 记录流已是天文数字（真实剪贴美术
/// 文件普遍 < 1 MiB），卡不住真文件、拦得住炸弹。
const WMZ_MAX_BYTES: usize = 64 * 1024 * 1024;

/// WMF 字节特征：可放置头（D7 CD C6 9A）或标准头（01/02 00 09 00）。
fn is_wmf_magic(slice: &[u8]) -> bool {
    slice.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
        || slice.starts_with(&[0x01, 0x00, 0x09, 0x00])
        || slice.starts_with(&[0x02, 0x00, 0x09, 0x00])
}

/// gzip 魔数（`.wmz` 的包装层）。魔数本身太通用——任何 gzip 文件都长这样——
/// 所以它只在**点名路径**上作证据（`iv_probe` 要求 ext 点名 `.wmz`；
/// `iv_export_svg` 只服务 vector_exts 声明过的文件），内容嗅探永不认领。
fn is_gzip_magic(slice: &[u8]) -> bool {
    slice.starts_with(&[0x1F, 0x8B])
}

/// `.wmz` = gzip 包装的 WMF：解包到内存，总量封顶 [`WMZ_MAX_BYTES`]。
fn gunzip_wmf(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    let mut out = Vec::new();
    // take(上限+1)：解包超过上限时 read_to_end 停在帽上，由下面的长度检查报
    // TOO_LARGE，而不是把炸弹先整个解进内存再量。
    let mut dec = flate2::read::GzDecoder::new(data).take(WMZ_MAX_BYTES as u64 + 1);
    dec.read_to_end(&mut out)
        .map_err(|_| (ERR_CORRUPT, "WMZ 的 gzip 包装损坏，解包失败"))?;
    if out.len() > WMZ_MAX_BYTES {
        return Err((ERR_TOO_LARGE, "WMZ 解包后超过 64 MiB 的解码上限"));
    }
    Ok(out)
}

/// WMF/WMZ 字节 → 自包含 SVG 文本字节。这正是喂给 resvg 的那份中间产物，
/// `iv_export_svg` 交出的就是它——两边看到的是同一个渲染语义。
fn svg_bytes(data: &[u8]) -> Result<Vec<u8>, (i32, &'static str)> {
    // 普通 WMF 不需要复制输入；只有 .wmz 解包后才有 owned payload。
    let payload = if is_gzip_magic(data) {
        Cow::Owned(gunzip_wmf(data)?)
    } else {
        Cow::Borrowed(data)
    };
    wmf_core::converter::convert_to_svg(payload.as_ref())
        .map_err(|_| (ERR_CORRUPT, "WMF 解析/转换 SVG 失败"))
}

// ── 字体（规范 §十六 的硬要求：文字必然要渲染）────────────────────────
//
// 三件事，缺一件就等于"这张图没渲染出来"：
//   1. 字体库必须**装上系统字体**——usvg 的默认 `Options` 带的是**空** fontdb，
//      空库 ⇒ `<text>` 展不出路径 ⇒ 字直接消失（不是画歪，是没了）。
//   2. 文档指定的字体族本机没有时，**照样渲染**（用兜底字体），绝不丢字/留白。
//   3. 用了兜底字体就在 `iv_last_error` 记一句，宿主把它转成日志行给用户看。
//
// 系统字体扫描（`C:\Windows\Fonts`）只做一次，全进程共享。

/// 本机字体库 + 可用字体族名集合（供"文档字体是否缺失"的判定）。
struct FontSet {
    db: Arc<resvg::usvg::fontdb::Database>,
    names: HashSet<String>,
}

/// 通用字体族（`sans-serif` 之类）：不是"某个具体字体"，永远解析得出，
/// 不算"字体缺失"。
fn is_generic_family(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "serif" | "sans-serif" | "sans serif" | "cursive" | "fantasy" | "monospace"
    )
}

static FONTS: std::sync::OnceLock<FontSet> = std::sync::OnceLock::new();

/// 本机字体库（进程内一次）。
fn fonts() -> &'static FontSet {
    FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        // Windows：扫描 %SYSTEMROOT%\Fonts（fontdb 内建，不需要额外依赖）。
        db.load_system_fonts();
        let mut names = HashSet::new();
        for face in db.faces() {
            for (name, _lang) in &face.families {
                names.insert(name.clone());
            }
        }
        FontSet {
            db: Arc::new(db),
            names,
        }
    })
}

/// 解析用的 usvg 选项（**带系统字体**）。
///
/// 选项里装着 `Arc<Database>`，克隆只是引用计数——所以整份选项每个线程建一次，
/// 之后一直用（字体库不再重复扫描）。
fn svg_options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        // 选项里装着 `Arc<Database>`，克隆只是引用计数——所以整份选项每个线程建一次，
        // 之后一直用（字体库不再重复扫描）。
        fontdb: fonts().db.clone(),
        ..resvg::usvg::Options::default()
    }
}

// 线程内的解析选项 + 场景缓存（宿主单解码线程 ⇒ 线程局部即可，不必上锁）。
thread_local! {
    static OPTS: resvg::usvg::Options<'static> = svg_options();
    /// 按 SVG 字节指纹缓存已解析的场景（契约 §十六 第 4 条：同一份字节会连着
    /// 换区域请求，不许每次重新解析文档）。容量 4，够覆盖"当前图 + 前后各一张"。
    static SCENE_CACHE: RefCell<Vec<(u64, Arc<resvg::usvg::Tree>)>> =
        const { RefCell::new(Vec::new()) };
}

/// FNV-1a 指纹（只当缓存键用，不当安全哈希）。
fn fingerprint(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^ (data.len() as u64)
}

/// 解析 SVG 场景（带缓存）。
fn parse_scene(svg: &[u8]) -> Result<Arc<resvg::usvg::Tree>, (i32, &'static str)> {
    let key = fingerprint(svg);
    if let Some(hit) = SCENE_CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, t)| Arc::clone(t))
    }) {
        return Ok(hit);
    }
    let tree = OPTS
        .with(|o| resvg::usvg::Tree::from_data(svg, o))
        .map_err(|_| (ERR_CORRUPT, "WMF 生成的 SVG 无效"))?;
    let tree = Arc::new(tree);
    SCENE_CACHE.with(|c| {
        let mut v = c.borrow_mut();
        v.push((key, Arc::clone(&tree)));
        if v.len() > 4 {
            v.remove(0);
        }
    });
    Ok(tree)
}

/// 扫出 SVG 里声明的字体族名（`font-family="A, B"` 形态）。
fn scan_font_families(svg: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(svg);
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while let Some(p) = text[i..].find("font-family") {
        let mut j = i + p + "font-family".len();
        // 跳过空白与 '='
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            i = j;
            continue;
        }
        j += 1;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let quote = bytes[j];
        if quote != b'"' && quote != b'\'' {
            i = j;
            continue;
        }
        j += 1;
        let start = j;
        while j < bytes.len() && bytes[j] != quote {
            j += 1;
        }
        let body = &text[start..j.min(text.len())];
        for fam in body.split(',') {
            let f = fam.trim().trim_matches('"').trim_matches('\'').trim();
            if f.is_empty() || is_generic_family(f) {
                continue;
            }
            if !out.iter().any(|x| x == f) {
                out.push(f.to_string());
            }
        }
        i = j + 1;
    }
    out
}

/// 字体缺失时在 `iv_last_error` 记一句（宿主会把这句话转成 `[vector-note]` 日志行）。
///
/// **只在真缺字体时记**：文档用的是通用族（`sans-serif`）不算缺——那种情况下
/// 解析器本来就按系统通用族取，渲染结果是"设计者意图的通用字体"。
fn note_missing_fonts(svg: &[u8]) {
    let missing: Vec<String> = scan_font_families(svg)
        .into_iter()
        .filter(|f| !fonts().names.contains(f))
        .collect();
    if !missing.is_empty() {
        set_last_error(&format!(
            "文档字体 {} 缺失，已用兜底字体渲染",
            missing.join("、")
        ));
    }
}

/// WMF 字节 → 解析好的场景（带缓存）。
fn to_tree(data: &[u8]) -> Result<Arc<resvg::usvg::Tree>, (i32, &'static str)> {
    let svg = svg_bytes(data)?;
    parse_scene(&svg)
}

/// 树的自然尺寸（f32 → u32；取整到 0 的画布没有意义）。
fn positive_size(w: f32, h: f32) -> Option<(u32, u32)> {
    let (w, h) = (w.round() as u32, h.round() as u32);
    (w > 0 && h > 0).then_some((w, h))
}

/// 读 `req` 的目标框（`req` 可为 NULL，视同 0 = 原始尺寸）。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
fn targets_of(req: *const IvRenderReq) -> (u32, u32) {
    if req.is_null() {
        (0, 0)
    } else {
        unsafe { ((*req).target_w, (*req).target_h) }
    }
}

/// 等比 contain：`scale = min(tw/nw, th/nh)`，任一 target 为 0 = 原始尺寸。
/// 返回 `(实际宽, 实际高, 统一缩放系数)`；实际边 = round(自然边 × scale)，
/// 恒 ≥ 内容实际覆盖（round ≥ 值），所以渲染永不裁边；缩到 0 的边至少 1 px。
fn target_size(nw: u32, nh: u32, (tw, th): (u32, u32)) -> (u32, u32, f32) {
    if tw == 0 || th == 0 {
        return (nw, nh, 1.0);
    }
    let s = (tw as f32 / nw as f32).min(th as f32 / nh as f32);
    let w = ((nw as f32 * s).round() as u32).max(1);
    let h = ((nh as f32 * s).round() as u32).max(1);
    (w, h, s)
}

/// 按 `(w, h)` 与变换矩阵渲染，输出**非预乘** RGBA8（宿主 ABI 口径）。
///
/// `transform` 是 usvg 用户坐标 → 画布像素的变换：整图路线是纯缩放，
/// 区域路线是"平移到那一小块 + 按密度缩放"。
fn rasterize_into(
    tree: &resvg::usvg::Tree,
    w: u32,
    h: u32,
    transform: resvg::tiny_skia::Transform,
    out: *mut u8,
    out_cap: usize,
    bytes: u64,
) -> Result<(), (i32, &'static str)> {
    if (out_cap as u64) < bytes {
        return Err((ERR_BUFFER_TOO_SMALL, "内部错误（宿主给的缓冲小于申报值）"));
    }
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or((ERR_TOO_LARGE, "WMF 画布分配失败"))?;
    resvg::render(tree, transform, &mut pixmap.as_mut());
    let src = pixmap.data();
    // 直接解预乘写入宿主缓冲：不再先分配一枚全尺寸 RGBA Vec 再整块拷贝。
    let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes as usize) };
    for (d, c) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let a = c[3] as u16;
        if a == 0 || a == 255 {
            d.copy_from_slice(c);
        } else {
            // tiny-skia 是预乘；宿主 ABI 要非预乘。
            let un = |v: u8| ((v as u16 * 255 + a / 2) / a).min(255) as u8;
            d[0] = un(c[0]);
            d[1] = un(c[1]);
            d[2] = un(c[2]);
            d[3] = c[3];
        }
    }
    Ok(())
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_probe(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    probe_impl(data, len, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn probe_impl(data: *const u8, len: usize, ext: *const c_char) -> i32 {
    guard(|| {
        if data.is_null() || len < 4 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        if is_wmf_magic(slice) {
            return 100;
        }
        // .wmz = gzip 包装的 WMF。gzip 魔数太通用（任何 gzip 文件都长这样），
        // ext=NULL 的内容嗅探不认领——只有宿主按扩展名点名（vector_exts）时才接，
        // 与 TGA「无可靠魔数 → 只按扩展名认领」同一口径。
        if is_gzip_magic(slice) && unsafe { ext_str(ext) } == Some("wmz") {
            return 100;
        }
        0
    })
}

#[cfg(feature = "standalone")]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn iv_render(
    data: *const u8,
    len: usize,
    req: *const IvRenderReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    render_impl(data, len, req, out, out_cap, info, ext)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
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
        if data.is_null() || len == 0 || info.is_null() {
            set_last_error("内部错误（空数据或空输出指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        let tree = match to_tree(slice) {
            Ok(v) => v,
            Err((code, msg)) => {
                // 01/02 00 09 00 这类魔数很弱（小端数据常见这个开头），解析失败时
                // 保持「不认领」而不是「文件损坏」：让位链还能往下问，别把一份
                // 只是长得像 WMF 的无关文件判死。
                if code == ERR_TOO_LARGE {
                    set_last_error(msg);
                    return code;
                }
                set_last_error("不是合法的 WMF 字节特征");
                return ERR_UNRECOGNIZED;
            }
        };
        let s = tree.size();
        let Some((nw, nh)) = positive_size(s.width(), s.height()) else {
            set_last_error("WMF 画布尺寸为 0");
            return ERR_CORRUPT;
        };
        // 矢量按需重渲（CAP_VECTOR）：target 非 0 时等比缩进目标框（放大无损），
        // 0 = 原始尺寸。actual_* 回填缩放后的实际画布，允许 ≠ target_*。
        let (w, h, scale) = target_size(nw, nh, targets_of(req));
        let (_, _, bytes) = match support::check_dims(w, h, support::budget_of(req)) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        if out.is_null() {
            support::fill_info(info, w, h, bytes, 1, 0);
            return ERR_OK;
        }
        if let Err((code, msg)) = rasterize_into(
            &tree,
            w,
            h,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            out,
            out_cap,
            bytes,
        ) {
            set_last_error(msg);
            return code;
        }
        support::fill_info(info, w, h, bytes, 1, 0);
        ERR_OK
    })
}

// ── 区域渲染（ABI v6，规范 §十六）─────────────────────────────────────

/// 区域请求的**画布与密度**。
///
/// 宿主给的 `target_w/target_h` 是"区域 × 密度"算出来的（两个方向同一个密度），
/// 所以正常情况下比例天然一致 ⇒ 直接按目标画布出图、密度取 `target_w / w`。
/// 比例真的不一致时（第三方宿主 / 将来别的调用方）**等比缩进**并把实际画布
/// 回填进 `actual_w/actual_h`——绝不拉伸变形（契约 §十六 硬约定第 2 条）。
fn region_canvas(r: &IvRegionReq) -> (u32, u32, f32) {
    let (tw, th) = (r.target_w, r.target_h);
    if tw == 0 || th == 0 {
        // 1:1：画布就是区域本身。
        let cw = r.w.round().max(1.0) as u32;
        let ch = r.h.round().max(1.0) as u32;
        return (cw, ch, 1.0);
    }
    let dx = tw as f32 / r.w;
    let dy = th as f32 / r.h;
    if (dx - dy).abs() <= 1e-3 * dx.max(dy) {
        (tw, th, dx)
    } else {
        let d = dx.min(dy);
        (
            (r.w * d).round().max(1.0) as u32,
            (r.h * d).round().max(1.0) as u32,
            d,
        )
    }
}

/// 区域渲染的核心：**SVG 字节 + 区域请求 → 画布**。
///
/// 拆出这一层是为了能**不用 WMF 文件**就测区域语义（测试里直接喂一段合成
/// SVG）：契约的关键几条——画布尺寸、锐利边缘（重画而非插值）、越界透明——
/// 全在这一层可断。
///
/// `out == NULL` 是查询阶段（只算尺寸，不写像素）；否则渲染并写进宿主给的缓冲。
fn region_render_svg(
    svg: &[u8],
    req: &IvRegionReq,
    out: *mut u8,
    out_cap: usize,
) -> Result<(u32, u32, u64), (i32, &'static str)> {
    let tree = parse_scene(svg)?;
    if !(req.w > 0.0 && req.h > 0.0) {
        return Err((ERR_CORRUPT, "区域请求的宽或高不为正"));
    }
    let (cw, ch, density) = region_canvas(req);
    let (_, _, bytes) = support::check_dims(cw, ch, req.budget_bytes)?;
    if out.is_null() {
        // 查询阶段：只报尺寸，不写像素（契约的两阶段形状）。
        return Ok((cw, ch, bytes));
    }
    // 变换 = 缩放(density) + 平移(-x,-y)：把文档坐标系的区域左上角挪到画布原点。
    let transform = resvg::tiny_skia::Transform::from_row(
        density,
        0.0,
        0.0,
        density,
        -req.x * density,
        -req.y * density,
    );
    rasterize_into(&tree, cw, ch, transform, out, out_cap, bytes)?;
    Ok((cw, ch, bytes))
}

/// `iv_render_region` 的业务实现（规范 §十六）。
///
/// 语义要点（逐条对应契约的硬约定）：
/// - **按密度重画**，不是"渲大图再缩"（第 1 条）：变换矩阵直接把用户坐标挪到
///   那一小块上，画布多大就画多大。
/// - **不拉伸变形**（第 2 条）：比例不一致时等比缩进，实际画布回填 `actual_*`。
/// - **越界画透明**（第 3 条）：画布初始全透明，文档外的内容压根没画。
/// - **解析结果进程内缓存**（第 4 条）：见 `parse_scene`。
/// - **字体照常渲染**（字体硬要求）：`svg_options()` 装的是系统字体库，缺字体
///   时用兜底并记一句（`note_missing_fonts`）。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 规范 §三「指针参数的所有权」
pub fn export_region_impl(
    data: *const u8,
    len: usize,
    req: *const IvRegionReq,
    out: *mut u8,
    out_cap: usize,
    info: *mut IvRenderOut,
    ext: *const c_char,
) -> i32 {
    guard(|| {
        if data.is_null() || len == 0 || info.is_null() || req.is_null() {
            set_last_error("内部错误（空数据或空请求指针）");
            return ERR_UNRECOGNIZED;
        }
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        // ★ **认领闸**：不是 WMF/WMZ 字节 → `ERR_UNRECOGNIZED`（让位语义），
        // 让宿主换下一家解码器；只有"认领了却解不出来"才是 `ERR_CORRUPT`。
        // （与 `probe_impl` 同一口径：gzip 魔数太通用，只有 ext 点名 `.wmz` 才算认领。）
        let is_wmz = is_gzip_magic(slice) && unsafe { ext_str(ext) } == Some("wmz");
        if !is_wmf_magic(slice) && !is_wmz {
            set_last_error("不是 WMF/WMZ 字节特征");
            return ERR_UNRECOGNIZED;
        }
        let svg = match svg_bytes(slice) {
            Ok(s) => s,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        let r = unsafe { *req };
        let (cw, ch, bytes) = match region_render_svg(&svg, &r, out, out_cap) {
            Ok(v) => v,
            Err((code, msg)) => {
                set_last_error(msg);
                return code;
            }
        };
        support::fill_info(info, cw, ch, bytes, 1, 0);
        // 成功之后才记字体提示（宿主把这句话转成 `[vector-note]` 日志行）。
        note_missing_fonts(&svg);
        ERR_OK
    })
}

#[cfg(feature = "standalone")]
rupix_abi::export_region!(export_region_impl);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_requires_wmf_magic() {
        assert_eq!(
            iv_probe([0xD7, 0xCD, 0xC6, 0x9A].as_ptr(), 4, std::ptr::null()),
            100
        );
        assert_eq!(iv_probe(b"nope".as_ptr(), 4, std::ptr::null()), 0);
    }

    /// gzip 魔数太通用：只有 ext 点名 `.wmz` 才认领（vector_exts 的点名路径）；
    /// ext=NULL / 别的后缀 / 非 gzip 字节一律 0。
    #[test]
    fn probe_claims_wmz_only_when_named() {
        let gz = [0x1Fu8, 0x8B, 0x08, 0x00];
        let wmz = b"wmz\0";
        let bin = b"bin\0";
        assert_eq!(
            probe_impl(gz.as_ptr(), gz.len(), wmz.as_ptr() as *const c_char),
            100
        );
        assert_eq!(probe_impl(gz.as_ptr(), gz.len(), std::ptr::null()), 0);
        assert_eq!(
            probe_impl(gz.as_ptr(), gz.len(), bin.as_ptr() as *const c_char),
            0
        );
        // 点名 .wmz 但不是 gzip：不认领。
        assert_eq!(
            probe_impl(b"nope".as_ptr(), 4, wmz.as_ptr() as *const c_char),
            0
        );
    }

    /// 区域通道的**认领闸**：不是 WMF/WMZ 字节 → `ERR_UNRECOGNIZED`（让位语义），
    /// 不是"文件损坏"——宿主据此换下一家解码器，而不是把这份文件判死。
    #[test]
    fn region_rejects_bytes_outside_the_wmf_family() {
        let req = IvRegionReq {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            target_w: 10,
            target_h: 10,
            ..Default::default()
        };
        let mut info = IvRenderOut::default();
        assert_eq!(
            export_region_impl(
                b"not an image".as_ptr(),
                12,
                &req,
                std::ptr::null_mut(),
                0,
                &mut info,
                std::ptr::null(),
            ),
            ERR_UNRECOGNIZED
        );
        assert_eq!(info.actual_w, 0);
        assert_eq!(info.actual_h, 0);
    }

    /// ★ 区域渲染的核心判据：**按密度重画**，不是"渲大图再插值放大"。
    ///
    /// 合成一张 100×100 的 SVG：白底 + 一条文档坐标 x=50 处宽 1 的黑竖线。
    /// 请求区域 (40,0,20,20)、密度 4（画布 80×80）：
    /// - 画布必须恰为 80×80（区域 × 密度）；
    /// - 黑线落在画布 x=(50-40)×4=40 ⇒ 像素 39 亮、40 暗，**边界锐利**。
    ///
    /// 这条就是"无损放大"与"位图放大"的**分界**：位图插值会把这一条边糊成
    /// 几像素的灰坡，而矢量重画就是一刀切。
    #[test]
    fn region_render_is_rasterized_at_density_not_upscaled() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/><rect x="50" y="0" width="1" height="100" fill="black"/></svg>"#;
        let req = IvRegionReq {
            x: 40.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        // 查询阶段：不写像素，只报尺寸。
        let (w, h, bytes) =
            region_render_svg(svg, &req, std::ptr::null_mut(), 0).expect("查询阶段");
        assert_eq!((w, h), (80, 80));
        assert_eq!(bytes, 80 * 80 * 4);

        // 写入阶段。
        let mut px = vec![0u8; bytes as usize];
        let (w2, h2, _) =
            region_render_svg(svg, &req, px.as_mut_ptr(), px.len()).expect("渲染阶段");
        assert_eq!((w2, h2), (80, 80));

        let luma = |x: usize, y: usize| {
            let i = (y * 80 + x) * 4;
            px[i]
        };
        assert!(
            luma(39, 10) > 200,
            "黑线左侧必须是白（实际 {}）",
            luma(39, 10)
        );
        assert!(
            luma(40, 10) < 60,
            "黑线必须锐利地落在 x=40（实际 {}）——糊成灰坡就是插值放大",
            luma(40, 10)
        );
    }

    /// 区域**越出文档**的部分画透明（契约 §十六 第 3 条），不报错、不裁错。
    #[test]
    fn region_outside_the_document_is_transparent() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/></svg>"#;
        // 区域 (90,90,40,40) 有一半在文档之外。
        let req = IvRegionReq {
            x: 90.0,
            y: 90.0,
            w: 40.0,
            h: 40.0,
            target_w: 80,
            target_h: 80,
            ..Default::default()
        };
        let (_, _, bytes) =
            region_render_svg(svg, &req, std::ptr::null_mut(), 0).expect("查询阶段");
        let mut px = vec![0u8; bytes as usize];
        region_render_svg(svg, &req, px.as_mut_ptr(), px.len()).expect("渲染阶段");
        let alpha = |x: usize, y: usize| {
            let i = (y * 80 + x) * 4;
            px[i + 3]
        };
        // 左上角在文档内 → 不透明。
        assert!(alpha(10, 10) > 200, "文档内的部分必须画出来");
        // 右下角在文档外 → 透明。
        assert_eq!(
            alpha(70, 70),
            0,
            "文档外的部分必须是透明，不是错误也不是乱画"
        );
    }
}
