# texture history

## 0.1.0 — 2026-10-29 — abi 6

- First release: DDS/KTX/KTX2/ASTC/PVR/Basis/BLP/VTF.
- Pure Rust implementations: `image-extras`/`dds`, `ktx2` + `texture2ddecoder`,
  `texture2ddecoder`, `basisu`, self-written BLP1/BLP2, `vtf`.
- DDS/KTX/KTX2 were previously unowned after leaving `pro`/`common`; they are
  now claimed by this DLL.
- ASTC/PVR/Basis/BLP/VTF are new claims.
- Known limits: KTX2 only uncompressed + BC1/BC3/BC7; PVR v3 PVRTC only;
  malformed third-party inputs are guarded at the ABI boundary.
