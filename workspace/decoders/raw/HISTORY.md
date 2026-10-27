# raw 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。
> 本档是 tier + 引擎：显影链在 `workspace/decoders/raw/src/lib.rs`，不在单库。

## 0.5.0 — 2026-10-27 — abi 5

- **变更**：DLL 自述增加可扩展 `iv_metadata()`；包格式改为 ZIP + 同版本许可证目录。
- **版本延续**：0.4.0 的 rawpx 并入、29→40 后缀、显影链/分箱/画幅/色彩矩阵改动随本版发布；
  细节见 git 提交记录。
- **认领**：40 个后缀、成员与能力位不变。
- **合规**：`rawler` 固定 **0.7.2**、LGPL-2.1，单 DLL 静态链接；发布 ZIP 含 LGPL-2.1 与
  `SOURCE-OFFER.md`（上游 commit、Cargo.lock 校验值、源码/重链接书面 offer），不内置 rawler 源码。

## 0.3.x 及更早

本仓未保留逐版记录；`Cargo.toml` 只留下 0.3.0 → 0.4.0 的版本延续说明。
更早的变更史以 git 提交记录为准，不在此追补。
