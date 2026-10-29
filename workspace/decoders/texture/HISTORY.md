# texture 历史

## 0.1.0 — 2026-10-29 — abi 6

- **首版**：DDS/KTX/KTX2/ASTC/PVR/Basis/BLP/VTF。
- **实现**：纯 Rust——`image-extras`/`dds`、`ktx2` + `texture2ddecoder`、
  `texture2ddecoder`、`basisu`、自写 BLP1/BLP2、`vtf`。
- **归属变化**：DDS/KTX/KTX2 离开 `pro`/`common` 后一度无人认领，现由本 DLL 认领。
- **新增认领**：ASTC/PVR/Basis/BLP/VTF。
- **已知限制**：KTX2 只有未压缩 + BC1/BC3/BC7；PVR 只有 v3 PVRTC；
  第三方畸形输入在 ABI 边界兜住。
- **升级建议**：新装。
