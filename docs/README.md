# RuPix-Decoder 文档地图

本目录只放**跨模块**的流程与设计；某个 DLL 的当前状态和历史留在它自己的目录里。
根目录的 [`README.md`](../README.md) 是门面，[`AGENTS.md`](../AGENTS.md) 是硬规则。

## 按读者选入口

| 你是 | 先读 | 再看 |
|---|---|---|
| 只想装解码器 | 根 [`README.md`](../README.md) | [`decoders-rust/decoders/README.md`](../decoders-rust/decoders/README.md)、各档 `README.md` |
| 要改代码 / 发版 / 提交 | 根 [`开发指南.md`](../开发指南.md) | [`contributing.md`](contributing.md)、[`building.md`](building.md)、[`releasing.md`](releasing.md) |
| 要写新解码器 / 改 ABI | [`解码器开发规范.md`](../解码器开发规范.md) | [`contributing.md`](contributing.md)、[`architecture.md`](architecture.md) |
| 要构建 / 发布 | 根 [`开发指南.md`](../开发指南.md) | [`building.md`](building.md)、[`releasing.md`](releasing.md)、[`catalog.md`](catalog.md) |
| 要查测试 / 证据 | [`testing.md`](testing.md) | [`pitfalls.md`](pitfalls.md) |
| 要维护仓库 | [`architecture.md`](architecture.md) | [`history-policy.md`](history-policy.md)、[`design-adoption.md`](design-adoption.md)、[`DOCS_MANIFEST.md`](../DOCS_MANIFEST.md) |

## 文件职责

| 文件 | 职责 |
|---|---|
| [`architecture.md`](architecture.md) | 三个 workspace、档 vs 单库、路由、ABI 快照、边界 |
| [`building.md`](building.md) | 工具链、target 目录、`build.ps1` 顺序、profile、形态 A/B |
| [`releasing.md`](releasing.md) | 版本、tag、双镜像上传、清单回填、发布顺序 |
| [`testing.md`](testing.md) | 串行测试、单测/端到端、golden、bench 口径、证据规则 |
| [`licensing.md`](licensing.md) | 许可证允许表、供应链门禁、未覆盖项 |
| [`catalog.md`](catalog.md) | `index.json` 十三个字段、镜像 URL、宿主消费、完整性判据 |
| [`contributing.md`](contributing.md) | 新增/修改解码器的完整步骤与文档同步清单 |
| [`history-policy.md`](history-policy.md) | 四档 `HISTORY.md` 的格式、版本语义、记录边界 |
| [`pitfalls.md`](pitfalls.md) | 本仓踩过的坑；编号 D，不与宿主 K 号混用 |
| [`design-adoption.md`](design-adoption.md) | 已采纳/已拒绝/待评估的架构决策 |
