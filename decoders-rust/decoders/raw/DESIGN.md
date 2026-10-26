# raw 引擎设计（当前状态）

> 本文件是 `raw` 的当前实现细节；对外变化见 [`HISTORY.md`](HISTORY.md)，
> 入口与限制见 [`README.md`](README.md)。行号会搬，引用用符号名。

## 0. 模块布局

| 文件 | 内容 |
|---|---|
| `src/lib.rs` | 导出、`MEMBERS`、常量、`iv_render_raw`、`fill_info`；只做装配与胶水 |
| `src/probe.rs` | `probe_impl` + RAW 魔数判据 |
| `src/frame.rs` | `Frame` + `frame()` 画幅裁剪 |
| `src/cache.rs` | rawler 调用、两阶段 `tier_plan`、线程本地解码缓存 |
| `src/develop.rs` | 线性化、demosaic、色彩矩阵、gamma、全尺寸显影 |
| `src/binning.rs` | `cfa_period`、`bin_plan`、`bin_phase_counts`、`develop_binned` |
| `src/tests.rs` | 29 条单测（`#[cfg(test)] mod tests;`） |

拆分只移动代码，不改变公开导出、字节输出或缓存语义；每一步都用
`cargo test --release -p rupix-raw -- --test-threads=1` 验过。

## 1. 分工

- `rawler 0.7.2`：容器分派、传感器解压，输出 CFA（多为 u16）。
- 本档：显影链（黑/白电平 → 白平衡 → demosaic → 色彩矩阵 → sRGB gamma → RGBA8）。
- `rawpx`：裸像素参数化数据；本档只做转发，不进 `MEMBERS`。

## 2. 认领

`looks_like_raw` 的魔数表：

| 魔数 | 格式 |
|---|---|
| `FUJIFILM` (8 B) | RAF |
| `FOVb` | X3F |
| `\0MRM` | IIQ |
| `IIII` | IIQ |
| `3ASH` | 3FR / FFF |
| 偏移 4 的 `ftyp` + `crx` / `c<digit>` | CR3 |
| `II*\0` / `MM\0*` | TIFF 族（含多数 RAW） |

`data.is_null()` → 0；`len < 12` → 不认；`ext` 完全忽略。

## 3. 画幅（frame）

`frame(img)` 只读元数据，优先级与 rawler 的 `CropDefault` 同序：

1. `crop_area`（相机建议画幅）；
2. `active_area`（有效区）；
3. 整块原样。

越界钳进原样；裁出 0 宽/高当作没有。**没有亮度启发式**——NEF 的遮光区可能写着
满量程亮，按"黑边"反推会裁出 0 像素。

全尺寸腿 `develop_in` 只出画幅那一块；线性化/去马赛克吃上下各扩 1 行的真实邻域
（贴边不扩），因此画幅内与"整块显影再裁出"逐字节相同。展开矩形横向铺满整行，
让线性化保持一整遍平铺，行宽保持传感器偶数。

**CFA 相位按绝对坐标查**：画幅起点常为奇数（实测 CR2 19、NEF 14、RAF 13），
按画幅内局部坐标会把 R/B 认错。分箱腿同样按绝对坐标，块起点 = 画幅原点 +
钳到画幅内的整块。

## 4. 显影（develop）

`develop_cached` 是唯一调用点：

- 点亮 `REQ_ALLOW_SCALED` 且 `bin_plan` 允许 → `develop_binned`；
- 否则 `develop` → `develop_in`。

`develop_in` 的顺序：

1. 黑/白电平线性化；
2. `cpp == 1 && Cfa` → 自写双线性 demosaic；`cpp == 3` 直通（Foveon）；
   否则灰度复制；
3. 色彩矩阵（`cam_to_srgb_wb`）一步带走白平衡；
4. sRGB gamma（精算基准 + 单值桶表快路）；
5. RGBA8，α 写死 255。

### 色彩矩阵

- 来源：`color_matrix`（`HashMap`），优先 D65；缺 D65 按 `Illuminant` 枚举序取最小
  （同一张文件两次运行不能取到不同 illuminant）。
- 组成：`inv3(normalize(xyz2cam · SRGB_TO_XYZ_D65)) · diag(wb_coeffs)`。
- **行和归一**是全部要害：归一后每行和 1，矩阵与逆都把 `(1,1,1)` 映成 `(1,1,1)`，
  中性进中性出按构造成立。
- 白平衡折进列，不在样本循环里逐样本乘；`cpp == 1` 时三道各用各的增益。
- 不再做 D50 Bradford 适配：相机矩阵按 D65 标定，当 D50 PCS 解是多余且错的白点适配。

### gamma

精算基准 + 65536 桶的单值桶表：本桶与左右邻桶 8 位取值一致才查表，跨输出边界的
逐桶回精算。穷举 65536 个桶与 20 万确定性伪随机点对撞，不是抽样。

## 5. 分箱档（m83）

`bin_plan` 给宿主那一框，直接在 CFA 上按 `step × step` 块同色平均，产出该档 RGBA8。

- 步长上限：`step ≤ ⌊max(w/tw, h/th)⌋`；方向可能转置，取两种朝向里更严的那个；
  缩完不许让宿主放大。
- 步长不必是 CFA 周期整数倍；`bin_phase_counts` 用积分图逐相位块数 R/G/B，
  任一块缺任一道色就不分箱（CYGM/QRBC、f32 原样、行列周期不成方回全尺寸）。
- X-Trans（6×6）在步长 3 下每块恰为 2R5G2B，能走分箱。
- 查询与渲染两阶段必须算出同一答案：判据只吃元数据（尺寸/cpp/图案/dtype/
  `crop_area`/`active_area`），不碰像素。
- 缓存键补目标框 + 缩放意图，防视口档与整图档串档。

画质判据不是"看着差不多"：A2 是同一条腿、同一份全尺寸像素，只把块起点平移
`step/2`；A−B 必须小于 A−A2，才判分箱可落地。

## 6. 缓存

thread-local 单槽；键 = 首 64 B + 尾 64 B + 长度 + 目标框 + 缩放意图的 FNV。
m80 起槽与返回值共享同一份像素（`Arc<Vec<u8>>`），消掉插入/命中各一次整图 clone。
槽会一直持有到下一次解码；宿主取消渲染时同样驻留。

## 7. 多帧 / 多页 / 元数据

- `target_w` / `target_h` / `flags` 从 m83 起真的读；`page_index` / `frame_index`
  仍忽略。
- `fill_info` 写 `page_count = 1`、`orientation`、`already_oriented = 0`；
  从不写 `frame_count` / `frame_delay_ms`，依赖宿主把 `IvRenderOut` 预清零。
- 只取尺寸、画幅、方向、显影输入（blacklevel/whitelevel/wb_coeffs/color_matrix）；
  EXIF 正文不导出，ICC 不导出（`icc_len = 0`）。
- `iv_render_raw` 用于裸像素；相机 RAW 走真魔数。

## 8. 错误码

- 1 / 4：入口参数错误，与其它档一致。
- 7：64 MP 或预算。
- 4：`out_cap` 不足。
- rawler 映射：`Unsupported` → 3；`DecoderFailed` → 2（CORRUPT），消息取 rawler
  `Display`。不认识的 TIFF 容器措辞保守，避免掐死宿主向 `core` 的兜底链。

## 9. 测试与证据

- 29 个 `#[test]`：探针、方向、矩阵、gamma 桶、白平衡、黑帧、分箱、画幅、
  缓存、两阶段尺寸。
- `examples/check_raw_tier.rs`：按宿主真实调用序列对账 dummy/真解的档位与画幅、
  申报与产出尺寸、串档、两种朝向的"不许放大"。
- `probe_raw_frame.rs`：`active_area`/`crop_area`/`blackareas` 逐字段对撞；
  整块 vs 画幅逐字节对撞。
- `bench_raw_stages.rs` / `bench_raw_bin.rs`：首开按段计时、像素指纹、三条腿对照。
- 真机样本在 `.temp/raw-samples`，不入库；golden 里的 4 枚 RAW 只用于嗅探。
- m85 的变异测试（M1 恒零字段 / M2 去行归一 / M3 加 D50 / M4 取错 illuminant）
  各自先红；证据在历史台账，不复制进仓库。
