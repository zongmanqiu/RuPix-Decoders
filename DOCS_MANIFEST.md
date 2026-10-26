# 文档清单（DOCS_MANIFEST）

> 每份长期文档一行：**什么时候必须改**、**谁验证**。没有机器门禁的写 `manual`，
> 不假装它被自动检查。新增/删除 `.md` 时同步本表。

| 文件 | 何时更新 | 验证 |
|---|---|---|
| `README.md` | 四档版本/后缀数/发布状态、安装方式、文档入口变化 | `check_docs.py` 链接；四档表与 `index.json` / `Cargo.toml` 人工核对 |
| `AGENTS.md` | 仓库边界、ABI 冻结、构建/发布落点、测试红线、新增解码器顺序变化 | `build.ps1` + `check_docs.py`；人工核对 |
| `解码器开发规范.md` | ABI 结构/错误码/导出/像素格式/能力位/版本策略变化 | `build.ps1` + 宿主 `cargo test --lib core::abi` |
| `docs/README.md` | 文档增删或职责变化 | `check_docs.py` 链接 |
| `docs/architecture.md` | workspace、档 vs 单库、路由、ABI 快照、边界变化 | `manual`（评审） |
| `docs/building.md` | 工具链、target、`build.ps1` 顺序、profile、形态 A/B 变化 | `build.ps1` 实跑 + `manual` |
| `docs/releasing.md` | 版本/tag/镜像/清单回填流程变化 | `manual`（发布演练） |
| `docs/testing.md` | 测试分层、串行原因、样本、bench 口径变化 | `build.ps1` + `manual` |
| `docs/catalog.md` | `index.json` 字段、镜像 URL、生成器行为变化 | `make_index.py` + `check_urls.py`（有网） |
| `docs/contributing.md` | 新增/修改解码器步骤、特殊形态、文档同步清单变化 | `manual`（评审） |
| `docs/history-policy.md` | 历史粒度、格式、版本语义变化 | `check_history.py` |
| `docs/pitfalls.md` | 新增本仓专属坑；编号只增不改 | `manual`（评审） |
| `docs/design-adoption.md` | 采纳/拒绝/待评估的决策变化 | `manual`（评审） |
| `docs/licensing.md` | 许可证允许表、依赖许可策略、供应链门禁变化 | `check_licenses.py` |
| `decoders-rust/decoders/README.md` | 四档矩阵、状态、链接变化 | `check_docs.py` + 人工核对 |
| `decoders-rust/decoders/<id>/README.md` | 该档成员、后缀、能力位、限制变化 | `check_docs.py` 必需文件/链接；人工核对 |
| `decoders-rust/decoders/<id>/HISTORY.md` | 该档每次对外可见变化 | `check_history.py` |
| `decoders-rust/decoders/raw/DESIGN.md` | raw 显影/画幅/分箱/缓存/错误码的设计变化 | `manual`（评审）+ raw 测试 |
| `decoders-rust/singles/README.md` | 单库增删、归属档、README 状态变化 | `manual`（评审） |
| `decoders-rust/singles/<id>/README.md` | 有独立价值的单库实现细节变化 | `manual`；总表必须登记 |
| `testdata/README.md` | golden / 生成样本的分工、引用方式变化 | `manual` |
| `testdata/claims.tsv` | 认领后缀增删、成员归属、证据状态变化 | `check_claims.py` |"}
| `THIRD-PARTY-NOTICES.md` | 依赖/许可证/版本变化 | `manual` + `cargo metadata` |
| `index.json` | 版本、字节数、发布状态、地址变化 | `make_index.py` + `build.ps1` 新鲜度检查 + `check_urls.py` |
| `.cargo/config.toml` / `decoders-rust/singles/.cargo/config.toml` / `decoders-rust/decoders/pro/.cargo/config.toml` | target-dir 变化 | `build.ps1` 实跑 |
| `build.ps1` / `tools/*` | 流水线、门禁、工具链变化 | 自跑 + 故意注入违规验证 |

## 验证等级

- `check_docs.py`：文件存在、README 链接、锚点、NUL、`.ps1` ASCII。
- `check_history.py`：版本/日期对齐（已接 `build.ps1`）。
- `check_deps.py` / `check_caps.py` / `check_claims.py`：依赖、闸值、后缀认领。
- `manual`：需要人判断的叙事/架构/发布演练；不写成自动门禁。
