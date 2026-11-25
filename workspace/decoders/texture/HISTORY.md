# texture 历史

## 0.1.1 — 2026-11-25 — abi 6

- **可并发调用**：新增能力位 `CAP_PARALLEL`（v6 内加法）。全库审计：成员内无进程级
  共享态（解码按调用隔离，纯函数式），宿主据此跳过同一枚 DLL 的串行锁。并发压力
  验收（多线程 × 200 次迭代）像素与单线程逐字节一致。

## 0.1.0 — 2026-10-29 — abi 6

- **首版**：DDS/KTX/KTX2/ASTC/PVR/Basis/BLP/VTF。
- **实现**：纯 Rust——`image-extras`/`dds`、`ktx2` + `texture2ddecoder`、
  `texture2ddecoder`、`basisu`、自写 BLP1/BLP2、`vtf`。
- **归属变化**：DDS/KTX/KTX2 离开 `pro`/`common` 后一度无人认领，现由本 DLL 认领。
- **新增认领**：ASTC/PVR/Basis/BLP/VTF。
- **已知限制**：KTX2 只有未压缩 + BC1/BC3/BC7；PVR 只有 v3 PVRTC；
  第三方畸形输入在 ABI 边界兜住。
- **升级建议**：新装。
