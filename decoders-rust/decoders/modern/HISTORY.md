# modern 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.2.0 — 2026-10-27 — abi 5

- **变更**：DLL 自述增加可扩展 `iv_metadata()`；包格式改为 ZIP + 同版本许可证目录。
- **认领**：6 个后缀、成员与能力位不变。

## 0.1.0 — 2026-10-26 — abi 5

- **认领**：6 个后缀（heic/heif/hif/avif/avifs/jxl）。
- **成员**：`av`（HEIC/HEIF + AVIF，`heic` 开 `av1` feature）与 `jxl`（jxl-oxide）。
- **崩溃域**：与现代格式的年轻解码库隔离，坏 HEIC/AVIF/JXL 不拖累 common 的 29 个格式。
- **能力位**：只声明 `CAP_FULL`；预测编码没有解码期降采样入口，缩放归宿主基座。
- **让位**：查询阶段按置信度换成员；渲染阶段换人前重问尺寸。
- **证据**：`claims_only_its_own_six_suffixes`、
  `both_members_render_through_one_entry_point`、
  `unknown_bytes_are_not_claimed_and_not_rendered`。
- **已知空白**：`heic/heif/hif` 当前无 golden 夹具；AVIF/JXL 有样本。
- **升级建议**：首个发布版本；无更早版本可升级。
