# texture

GPU/game texture formats, pure Rust.

`texture 0.1.0` owns the eight suffixes that have a usable Rust decoder:

| Member | Suffixes | Implementation |
|---|---|---|
| `dds` | `dds` | `image-extras` / `dds` (MIT/Apache) |
| `ktx2` | `ktx` / `ktx2` | existing KTX member: `ktx2` (Apache) + `texture2ddecoder` (MIT/Apache) |
| `astc` | `astc` | raw ASTC container + `texture2ddecoder` |
| `pvr` | `pvr` | PVR v3 PVRTC container + `texture2ddecoder` |
| `basis` | `basis` | `basisu` (Apache, pure Rust); also accepts Basis KTX2 |
| `blp` | `blp` | 自写 BLP1/BLP2：JPEG、调色板、DXT1/3/5、BGRA |
| `vtf` | `vtf` | `vtf` (MIT, pure Rust) |

## Claim rule

`iv_probe` uses the format magic/header. DDS/KTX/KTX2/ASTC/PVR/BLP/VTF have
fixed signatures; Basis additionally accepts a Basis KTX2 payload. The merged
router tries the KTX2 member first and falls back to Basis when the KTX2
payload is supercompressed.

## Scope

- `texture 0.1.0` covers the eight formats above. Other texture candidates
  (ACE/FSH/GAF/LIF/M8/MTX/RSB/SYJ/TIL/TIM/TM2/WAD/WAL and the ambiguous BPR)
  remain future work; they have no mature pure-Rust library yet.
- DDS/ImageMagick malformed inputs are caught by the ABI `guard`; PVR only
  supports PVR v3 PVRTC RGB/RGBA 2/4 bpp in this release.
- BLP is deliberately self-written: the `blp` crate pulls `self_cell` under
  GPL-2.0-only, which is not compatible with this AGPL-3.0 DLL merely by adding
  a notice file. The local parser covers BLP1/BLP2 JPEG, palette, DXT1/3/5 and
  BLP2 BGRA; BLP0 external mipmaps are not supported.
- KTX/KTX2 currently inherit the KTX member's limits: KTX1 uncompressed,
  KTX2 uncompressed + BC1/BC3/BC7. Other block formats are future work.

## Build

```powershell
cd RuPix-Decoder\workspace\decoders\texture
cargo test --release -- --test-threads=1
cargo build --release
```

The DLL lands in `.temp/texture-target/release/texture.dll`; the shared build
publishes it as `texture_0.1.0.dll`.
