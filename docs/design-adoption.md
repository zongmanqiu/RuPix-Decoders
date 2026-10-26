# 架构决策账

> 只记"为什么现在是这样"和"为什么不那样"。已拒绝的方案保留，避免下次重新讨论。
> 状态：`accepted` / `rejected` / `measured-not-adopted` / `open`。

## 已采纳

| ID | 决策 | 理由 | 落点 |
|---|---|---|---|
| A01 | 三个 cargo workspace（根 / bench / pro） | feature 并集导致 `LNK2005`；pro 依赖会污染前三档 | 根 `Cargo.toml`、`decoders-rust/singles/Cargo.toml`、`decoders-rust/decoders/pro/Cargo.toml` |
| A02 | 档 workspace 全局 `opt-level = 3` | 逐包清单靠人记会漂；实测代价约 +97 KB | 根 `[profile.release]` |
| A03 | 四档纯 Rust | 无 C 工具链、无 `-sys`/`cc`/`bindgen`；分发简单 | 各档依赖与 `THIRD-PARTY-NOTICES.md` |
| A04 | 合并档（common/modern/raw/pro） | 每枚独立解码器自带底座；合并省体积，不改变速度 | `decoders-rust/decoders/*` |
| A05 | `pro` 独立 workspace | 防 EXR/AVIF 后端被 feature 统一编进每一枚 DLL | `decoders-rust/decoders/pro/Cargo.toml` |
| A06 | `index.json` 扁平数组 + `size` 完整性 | 只有一种东西；文件名可算；`size` 比哈希更好懂 | `index.json`、`make_index.py` |
| A07 | 每档 `README.md` + `HISTORY.md` | 用户/维护者能按 DLL 判断是否升级 | `decoders-rust/decoders/<id>/` |
| A08 | `解码器开发规范.md` 留仓库根 | 宿主已有指针；移动会牵动 `main/DOCS_MANIFEST`、`abi.rs`、j1 | 根规范 |
| A09 | `docs/` 只放跨模块流程/设计 | 不造第二棵逐文件目录树；DLL 状态贴代码 | `docs/` |
| A10 | 每枚 DLL 一个 release/tag | 旧的四档共用一个日期 tag 会让未上传的条目 404 | `docs/releasing.md` |
| A11 | `raw` 拆成 `probe/frame/cache/develop/binning/tests` 模块，不拆 crate | 行为保持、单测逐步验证；避免拆 crate 带来 target/命名成本 | `decoders-rust/decoders/raw/src/` |
| A12 | 清单加 `sha256`，宿主下载后校验 | `size` 只能挡截断/多收；同字节数替换必须靠哈希。端到端已接通，宿主 exe +9,728 B | `tools/make_index.py`、`core::hash`、`decoders::install` |\n| A13 | ABI v5：把自述导出/结构体/样板宏统一改为 decoder 命名 | 当前只有自家解码器、无第三方 DLL，是唯一无兼容包袱的窗口；旧 v4 拒载 | `abi/src/lib.rs`、C kit、宿主 DLL 加载器 |"}]

## 已拒绝

| ID | 方案 | 为什么拒绝 |
|---|---|---|
| R01 | `rust-full` 并集安装档 | 体积与维护成本换不来用户价值；已按用户要求撤除 |
| R02 | `opt-level = s` 作为默认 | 四 DLL 体积 −15.89%，但 JPEG/PNG 各慢约 4–5 ms；默认仍用 3 |
| R03 | `opt-level = z` 用于解码热路径 | JPEG 3 MP 慢约 4.6 倍；热循环吃不了体积优化 |
| R04 | 22 个单库各写 HISTORY | 单库不是独立发布物，22 份会腐；由所属档覆盖 |
| R05 | `docs/` 恢复成逐 DLL 一份详细文档 | 与"文档跟着代码走"冲突；每档 README 已承担 |
| R06 | 把单库和档塞进一个 workspace | feature 并集直接链接失败（D01） |

## 已测量但未采纳

| ID | 方案 | 读数 | 结论 |
|---|---|---|---|
| M01 | `opt-level = s` | 四 DLL 25,241,088 → 21,229,056 B（−15.89%）；common JPEG 24.4→28.7 ms、PNG 23.7→28.7 ms | 体积收益明确，速度代价也在；默认不改，保留为可选发布档实验 |
| M02 | 给 `jxl` 解码器本体提 `opt-level = 3` | 2.46 MP：400.1/443.5 vs 406.3/409.4 ms；8.3 MP：947.0/900.5 → 895.7 ms | 差异在噪声内；不加 |
| M03 | `#[inline]` 标注 / 把展开矩形左右裁进去 | 无可复现差异；后者行宽变 3874 更慢 | 撤回 |

## 待评估

| ID | 事项 | 现状 |
|---|---|---|
| O01 | `index.json` 加 `sha256` | 2026-10-26 已落地，见 A12；清单 13 字段，宿主解析可选、下载后强制核对 |
| O03 | `raw` 拆模块 | 2026-10-26 已完成：`probe/frame/cache/develop/binning/tests`，见 A11 |
| O04 | 共享 `MAX_PIXELS` / confidence 常量 | 2026-10-26 评估：13 份 `MAX_PIXELS` 是各 crate 的独立安全闸，`check_caps.py` 靠字面量做等值检查；抽 policy crate 会改依赖图与门禁口径，本轮不做，保留待评估 |
| O05 | 依赖版本收敛 | 2026-10-26 评估：`png`/`miniz_oxide`/`hayro-jpeg2000` 双版本来自 `djvu-rs`/`icns`/`image` 等上游传递依赖，`syn` 三版本含旧 proc-macro；强行统一会改上游 feature/ABI，本轮不做 |
| O06 | `cargo-deny` / `cargo-audit` | 本机未安装；2026-10-26 以 `check_licenses.py` 落地许可证允许表（三图 402 包，0 违规）。漏洞公告不是本地构建门禁；有 `cargo audit` 时按需跑，不列为待办 |
