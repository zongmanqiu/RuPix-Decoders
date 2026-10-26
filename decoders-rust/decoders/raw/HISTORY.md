# raw 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。
> 本档是 tier + 引擎：显影链在 `decoders-rust/decoders/raw/src/lib.rs`，不在单库。

## 0.5.0 — 2026-10-27 — abi 5

- **状态**：发布包已生成；等待 Gitee/GitHub 上传。
- **变更**：DLL 自述增加可扩展 `iv_metadata()`；包格式改为 ZIP + 同版本许可证目录。
- **认领**：40 个后缀、成员与能力位不变。
- **合规**：`rawler` 固定 **0.7.2**、LGPL-2.1，单 DLL 静态链接；发布 ZIP 含 LGPL-2.1 与 `SOURCE-OFFER.md`（上游 commit、Cargo.lock 校验值、源码/重链接书面 offer），不内置 rawler 源码。

## 0.4.0 — 未发布 — abi 5

- **状态**：未发布。`index.json` 的 `released` / `gitee` / `github` 为空；
  DLL 已可本地构建。
- **认领**：40 个后缀 = 29 种相机 RAW（含 `.raw`）+ 11 个裸像素后缀
  （`rgb/rgba/gray/gry/bgra/yuv/i420/yv12/nv12/yuy2/uyvy`；rawpx 另支持 `.raw`，
  按请求类型分派，唯一认领一次）。
- **成员**：`rawler 0.7.2`（相机 RAW 的 demux + 传感器解压）+ `rawpx`（裸像素，
  只走参数化 `iv_render_raw`，不进 `MEMBERS` 让位链）。
- **版本延续**：0.3.0 → 0.4.0（2026-10-25）：rawpx 并入，后缀 29 → 40，
  多一个可选导出 `iv_render_raw`。这不是新解码器重置，是 raw 自身功能增长。
- **能力位**：`CAP_FULL | CAP_SCALED | CAP_RAW_PARAM`；`CAP_SCALED` 只按
  `REQ_ALLOW_SCALED` 出档位。
- **显影链**：黑/白电平线性化 → 白平衡 → 自写双线性 demosaic → 色彩矩阵 →
  sRGB gamma → RGBA8。色彩矩阵的白平衡折进列，不在样本循环里乘。
- **m78 性能归因**：首开大部分时间在自写显影链，不在 rawler；gamma 的逐像素
  `powf` 是最大项。
- **m79 gamma 桶表**：单值桶表替代每像素 3 次 `powf`；端点首开
  −26%~−42%，像素指纹逐张不变。
- **m80 缓存共享**：缓存槽与返回值共享 `Arc<Vec<u8>>`，消掉整图 clone；
  峰值由宿主侧决定，解码器这次拷贝不在关键路径。
- **m82 白平衡 + 黑帧**：白平衡从样本循环折进色彩矩阵；删除"元数据撒谎兜底"
  （平场/黑帧会被刷成亮场）。用例 `white_balance_is_per_cfa_channel`、
  `flat_black_frame_stays_black` 钉住。
- **m83 分箱档**：`bin_plan` + `develop_binned` 直接在 CFA 上按 `step × step`
  同色平均；查询与渲染两阶段吃同一份元数据；缓存键补目标框与缩放意图，
  防视口档/整图档串档。
- **m84 画幅**：优先 `crop_area` → `active_area` → 整块；遮光区不再当画面；
  CFA 相位按绝对坐标查；展开矩形横向铺满整行。
- **m85 色彩矩阵**：rawler 0.7.2 的 `xyz_to_cam` 恒零，旧链实际没接矩阵；
  改读 `color_matrix`（优先 D65，缺则按枚举序取最小），行和归一，白平衡折进列。
  用例 `neutral_gray_survives_a_real_color_matrix`、
  `render_depends_on_the_color_matrix` 钉住。
- **证据**：29 个 `#[test]`；`examples/check_raw_tier.rs`、`probe_raw_frame.rs`、
  `bench_raw_stages.rs`、`bench_raw_bin.rs`；真机样本在 `.temp/raw-samples`。
- **已知空白**：compressed RAF 的真实白电平没有有内容的真机样本；
  超 64 MP 的 RAW 在档位之前就被拒绝；过曝处仍按道钳位。
- **升级建议**：未发布；发布后按新版本安装。

## 0.3.x 及更早

本仓未保留逐版记录；`Cargo.toml` 只留下 0.3.0 → 0.4.0 的版本延续说明。
更早的变更史以 git 提交记录为准，不在此追补。
