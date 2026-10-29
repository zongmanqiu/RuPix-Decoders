# 文档清单（DOCS_MANIFEST）

> 每份长期文档一行：**什么时候必须改**、**谁验证**。没有机器门禁的写 `manual`，
> 不假装它被自动检查。新增/删除 `.md` 时同步本表。

| 文件 | 何时更新 | 验证 |
|---|---|---|
| `开发指南.md` | 修改/发布流程、交付检查表、版本与地址真源变化 | `manual`（发布演练）；本文件是流程唯一正本 |
| `README.md` | 八档版本/后缀数/发布状态、安装方式、文档入口变化 | `check_docs.py` 链接；八档表与 `decoders.json` / `Cargo.toml` 人工核对 |
| `AGENTS.md` | 仓库边界、ABI 冻结、构建/发布落点、测试红线、新增解码器顺序变化 | `build.ps1` + `check_docs.py`；人工核对 |
| `解码器开发规范.md` | ABI 结构/错误码/导出/像素格式/能力位/版本策略变化 | `build.ps1` + 宿主 `cargo test --lib core::abi` |
| `docs/README.md` | 文档增删或职责变化 | `check_docs.py` 链接 |
| `docs/architecture.md` | workspace、档 vs 单库、路由、ABI 快照、边界变化 | `manual`（评审） |
| `docs/building.md` | 工具链、target、`build.ps1` 顺序、profile、形态 A/B 变化 | `build.ps1` 实跑 + `manual` |
| `解码器能力规划.md` | 目标 DLL/方向/未来格式/当前后缀数变化 | `manual`（评审） |
| `testdata/GENERATION_EXCEPTIONS.md` | 无法生成/公开的格式增删 | `manual` + `testdata/tests` |
| `docs/releasing.md` | 版本/tag/镜像/清单回填机制变化 | `manual`（发布演练）；步骤以 `开发指南.md` §四为准 |
| `docs/testing.md` | 测试分层、串行原因、样本、bench 口径变化 | `build.ps1` + `manual` |
| `docs/catalog.md` | `decoders.json` 字段、镜像 URL、生成器行为变化 | `make_index.py` |
| `docs/contributing.md` | 新增/修改解码器步骤、特殊形态、文档同步清单变化 | `manual`（评审） |
| `docs/history-policy.md` | 历史粒度、格式、版本语义、禁写状态/上传/tag 规则变化 | `check_history.py` |
| `docs/pitfalls.md` | 新增本仓专属坑；编号只增不改 | `manual`（评审） |
| `docs/design-adoption.md` | 采纳/拒绝/待评估的决策变化 | `manual`（评审） |
| `docs/licensing.md` | 许可证允许表、依赖许可策略、供应链门禁变化 | `check_licenses.py` |
| `workspace/decoders/README.md` | 八档矩阵、状态、链接变化 | `check_docs.py` + 人工核对 |
| `workspace/decoders/<id>/README.md` | 该档成员、后缀、能力位、限制变化 | `check_docs.py` 必需文件/链接；人工核对 |
| `workspace/decoders/<id>/HISTORY.md` | 该档每次对外可见变化 | `check_history.py` |
| `workspace/decoders/raw/DESIGN.md` | raw 显影/画幅/分箱/缓存/错误码的设计变化 | `manual`（评审）+ raw 测试 |
| `workspace/singles/README.md` | 单库增删、归属档、README 状态变化 | `manual`（评审） |
| `workspace/singles/<id>/README.md` | 有独立价值的单库实现细节变化 | `manual`；总表必须登记 |
| `testdata/README.md` | golden / 生成样本的分工、引用方式变化 | `manual` |
| `testdata/claims.tsv` | 认领后缀增删、成员归属、证据状态变化 | `check_claims.py` |
| `THIRD-PARTY-NOTICES.md` | 依赖/许可证/版本变化 | `manual` + `cargo metadata` |
| `decoders.json` | 版本、字节数、发布状态、地址变化 | `make_index.py` + `build.ps1` 新鲜度检查 |
| `workspace/.cargo/config.toml` / `workspace/singles/.cargo/config.toml` / `workspace/decoders/pro/.cargo/config.toml` | target-dir 变化 | `build.ps1` 实跑 |
| `build.ps1` / `tools/*` | 流水线、门禁、工具链变化 | 自跑 + 故意注入违规验证 |

## 验证等级

- `check_docs.py`：文件存在、README 链接、锚点、NUL、`.ps1` ASCII。
- `check_history.py`：版本/日期对齐（已接 `build.ps1`）。
- `check_deps.py` / `check_caps.py` / `check_claims.py`：依赖、闸值、后缀认领。
- `manual`：需要人判断的叙事/架构/发布演练；不写成自动门禁。
