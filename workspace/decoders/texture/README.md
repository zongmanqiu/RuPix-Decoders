# texture（GPU / 游戏纹理档）

GPU / 游戏纹理格式，纯 Rust。

`texture 0.1.0` 认领 8 个有可用 Rust 解码器的后缀：

| 成员 | 后缀 | 实现 |
|---|---|---|
| `dds` | `dds` | `image-extras` / `dds`（MIT/Apache） |
| `ktx2` | `ktx` / `ktx2` | 既有 KTX 成员：`ktx2`（Apache）+ `texture2ddecoder`（MIT/Apache） |
| `astc` | `astc` | 裸 ASTC 容器 + `texture2ddecoder` |
| `pvr` | `pvr` | PVR v3 PVRTC 容器 + `texture2ddecoder` |
| `basis` | `basis` | `basisu`（Apache，纯 Rust）；也接受 Basis KTX2 |
| `blp` | `blp` | 自写 BLP1/BLP2：JPEG、调色板、DXT1/3/5、BGRA |
| `vtf` | `vtf` | `vtf`（MIT，纯 Rust） |

## 认领规则

`iv_probe` 按格式魔数/文件头判断。DDS/KTX/KTX2/ASTC/PVR/BLP/VTF 有固定签名；
Basis 额外接受 Basis KTX2 载荷。合并路由先试 KTX2 成员，KTX2 载荷是超压缩时回退到 Basis。

## 范围

- `texture 0.1.0` 覆盖上表 8 个格式。其余纹理候选
  （ACE/FSH/GAF/LIF/M8/MTX/RSB/SYJ/TIL/TIM/TM2/WAD/WAL 以及存在歧义的 BPR）
  留待以后；它们还没有成熟的纯 Rust 库。
- DDS/ImageMagick 的畸形输入由 ABI `guard` 兜住；本版 PVR 只支持 PVR v3 PVRTC
  RGB/RGBA 2/4 bpp。
- BLP 是刻意自写的：`blp` crate 会通过 `self_cell` 引入 GPL-2.0-only，与本 DLL 的
  AGPL-3.0 不兼容（加一份声明文件也不能解决）。本仓解析器覆盖 BLP1/BLP2 JPEG、
  调色板、DXT1/3/5 与 BLP2 BGRA；不支持 BLP0 外部 mipmap。
- KTX/KTX2 目前沿用 KTX 成员的限制：KTX1 未压缩、KTX2 未压缩 + BC1/BC3/BC7。
  其它块格式留待以后。

## 构建

```powershell
cd RuPix-Decoder\workspace\decoders\texture
cargo test --release -- --test-threads=1
cargo build --release
```

DLL 落 `.temp/texture-target/release/texture.dll`；统一构建脚本发布为 `texture_0.1.0.dll`。
