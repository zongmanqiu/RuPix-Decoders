# testdata/ —— 测试输入的两半：生成器 + 入库的 golden

| 里面是什么 | 在仓库里吗 | 量级 | 引用方式 |
|---|---|---|---|
| `make_samples.py` + `samples.json`：1/10/100 MiB 雪花/真实 seed 生成器 | **是**（只有脚本和配置） | — | — |
| `golden/`：27 个 golden / 边界证物（1×1 红点、隔行 PNG、CMYK TIFF、4 枚相机 RAW、AVIF 旋转样本、2 个最小 JXL、2 帧最小 GIF、最小 PSD/HDR…） | **是** | 137 KB | `include_bytes!` **编译期**进测试二进制 |
| 生成出来的 `<tier>/noise<ext>` | **否** → `../.temp/testdata/` | 324 个（108 后缀 × 3 档；别名/seed 保证每档大小唯一） | 运行时读文件 |

划分标准是两条，缺一不可：**能不能用脚本算出来** + **算出来的字节能不能保证永远不变**。
- 能算且字节恒定（雪花噪声、按尺寸增长的 1/10/100 MB）→ **不进仓库**，一条命令随时重建；
- 算不出来，**或**算出来也会随工具链漂移的（真实相机 RAW、某个具体 bug 的复现字节、
  `golden` 参考输出）→ 入库，且放在**全仓共享**的 `golden/`，不放进任何一个 crate 里。

★ 为什么第二条也要算：27 个证物里 **11 个真造不出来**（4 枚真机 RAW、5 枚 AVIF、2 枚 JXL——本机
Pillow 无 AVIF/JXL 编码器），另 **16 个能造**（6 TIFF、4 JPEG、2 PNG、1 APNG、1 GIF、1 PSD、1 HDR），
只是需要冻结字节。这 16 个**也不该改成现生成**：同一个字节永远对应同一个断言，换个 Pillow
版本重生成就可能漂移，测试会静默变松——留档比"能重建"更重要。

## 为什么都不跟着解码库走

1. **一个后缀只有一枚认领者**（`tools/check_claims.py` 守着 ext → 单一解码器），所以"JPEG 的测试图"
   对谁都是同一份字节。按解码器各存一份的后果我们真的有过：同一张 100 MB 的图存在十几份，
   而且会漂移——两个解码器对着两份不同的"10 MB png"互相以为对方过了。
2. **解码库会换**：同一后缀今天用 A 库、明天换 B 库（`.jxl`、`.dds` 都换过）。样本绑在库目录里，
   换库时要么丢样本、要么把样本再拷一遍。

⇒ 两半都按**后缀**共享：生成器一份、`golden/` 一份。生成器产物的 manifest 里仍记 `claimed_by`，
只是用来查"这枚样本本该由谁读"。

## `golden/` 的规矩

- **新增样本放这里，不要放进 `decoders-rust/decoders/<tier>/` 或 `decoders-rust/singles/<id>/`**：
  文件名自带信息（`grey16_4x4.tif`、`prog_64x48.jpg`），后缀就是格式；两个 crate 想看同一份样本时
  直接引用同一路径，不需要拷贝。
- 引用方式（`include_bytes!` 要编译期常量，所以路径得从 crate 里拼出来）：
  ```rust
  const GOLDEN: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),
      "/../../../testdata/golden/golden_16x12.jpg"));
  ```
  运行时读文件的那几处（`av` 的测试、`raw` 的 `smoke_raw`）同理，只是换成
  `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../testdata/golden")`。
- **删样本前先 grep 文件名**：2026-10-25 搬家时揪出两个零引用的死文件
  （`gif/src/testdata/red_1x1.gif` 43 B、`tiff/src/testdata/red_1x1.tif` 144 B），
  它们从来没被任何测试读过——只是被"放在那儿"。

## 生成器产物在哪

`<repo>/../.temp/testdata/<tier>/noise<ext>` + `manifest.json`（`--out` 或 `RUPIX_SAMPLE_DIR` 可改）。
文件名不带尺寸：尺寸/字节数在 manifest 里。别名后缀仍是同一张图，但**不再共享字节长度**：
每档内每个文件都有唯一字节数，方便按大小排序时直接看出不同文件：

- `.jpeg/.jpe/.jfif/.jif/.jfi/.exif` ← `.jpg`
- `.tiff` ← `.tif`；`.dib/.rle` ← `.bmp`；`.icon` ← `.ico`；`.pnm` ← `.ppm`
- `.j2k/.j2c/.jpc/.jpx/.jpf/.jpt` ← `.jp2`；`.pcc` ← `.pcx`
- `.mrcs/.ccp4/.map` ← `.mrc`；`.dicom` ← `.dcm`

裸像素后缀（`.bgra/.rgb/.rgba/.gray/.gry/.i420/.yuv/.yv12/.nv12/.yuy2/.uyvy`）也不共享字节：
每个后缀按自己的布局、形状和像素预算生成，1M 档例如 `.bgra` 4:3、`.rgba` 3:2。
生成器最后会检查「同档内大小唯一」，发现撞车直接失败，不会静默产出两个同尺寸文件。

## 跑法

默认总开关是 [`samples.json`](samples.json)：

```json
{
  "output": "../.temp/testdata",
  "seed_dir": "../.temp/corpus",
  "tiers": { "1M": 1048576, "10M": 10485760, "100M": 104857600 },
  "strict": true
}
```

改 `output` 就换落点，改 `tiers` 就换目标字节数；`strict` 要求真实 seed 齐全。
相对路径按解码器仓库根解析。命令行可以覆盖：

```text
python testdata/make_samples.py                   # 按 samples.json 一次生成全部
python testdata/make_samples.py --config FILE     # 换一个总开关文件
python testdata/make_samples.py --out DIR         # 只覆盖输出位置
python testdata/make_samples.py --seed-dir DIR    # 只覆盖 seed 根目录
python testdata/make_samples.py --only .jpg,.png  # 子集（后缀或成员 id 都行：--only jpeg）
python testdata/make_samples.py --tier 10M        # 单档
python testdata/make_samples.py --list            # 只打印计划，不写文件
python testdata/make_samples.py --check           # 只验覆盖，不写文件
```

seed 按档位选：1M/10M 优先选**最接近目标**且能打开的小样本，100M 选大样本；
所以 `1M/noise.pef` 现在是 11 MB 的 PEF，不是 121 MB 的 100M 档文件。
仍超过档位或小于档位的真实格式会在 manifest 写 `capped(SEED ...)`。

真实 seed 的目录约定：

```text
.temp/corpus/raw/<ext>/<file>    # 29 个相机 RAW
.temp/corpus/real/<ext>/<file>   # HEIC/AVIF/JXL/EXR/JXR/XCF/WMF/ASE/DjVu/KTX2/FF
```

`--check` 仍只验清单覆盖（61 generated + 47 seeds = 108）；`--strict` 才会检查
每个 seed 文件真的存在。`build.ps1` 只跑 `--check`，不把外部语料变成构建依赖。

`--only modern` 现在会生成/复制 JXL、HEIC、AVIF 的真实 seed；`raw` 会复制 29 个
相机的真实 RAW；`pro` 会复制 EXR/JXR/XCF/WMF/ASE/DjVu/KTX2。

## 档位是下限，不是精确值

边长一路长到文件**超过**该档为止；格式/像素/seed 上限不足以达到该档时，manifest
写 `capped(...)` 并给出真实尺寸，不填充、不伪造。当前实测：**61 个可编码后缀 +
47 个真实 seed，共 108 个后缀 × 3 档 = 324 个样本**。

典型物理上限：

| 后缀 | 卡在哪一档 | 原因 |
|---|---|---|
| `.ico` | 三档都卡 | ICO 单图最大 256×256 |
| `.pbm` / `.wbmp` | 10M / 100M | 1 bpp，41 MP 上限下最多约 5 MB |
| `.pgm` / `.gray` / `.nii` / `.nrrd` / `.mrc` | 100M | 单通道 1 B/px |
| `.dds` | 100M | 唯一能解的 BC1 固定 0.5 B/px |
| `.gif` | 100M | 8 bpp 调色板 + LZW |
| `.icns` | 10M / 100M | Pillow ICNS writer 最大 1024 |
| 真实 RAW / 小样 pro 格式 | 视 seed 大小 | seed 是原图字节，不能凭空放大 |

**41 MP 是故意的上限**：样本要能被查看器真正打开（`main/src/core/pixels.rs` 的
`MAX_PIXELS`；解码器自己的闸是 64 MP）。

## 真实 seed 来源

| 谁 | 后缀 | 来源 |
|---|---|---|
| raw | `.3fr .ari .arw .cr2 .cr3 .crm .crw .dcr .dcs .dng .erf .fff .iiq .kdc .mef .mos .mrw .nef .nrw .orf .ori .pef .qtk .raf .raw .rw2 .rwl .srw .x3f` | raw.pixls.us CC0；`.dcs` raw.pixls.us CC0（DCS460D）；`.crm` dnglab test media 与 `.qtk` qtkn_decoder 仅 local-only |
| modern | `.heic .heif .hif .avif .avifs .jxl` | `.plan/bench/corpus-real` 真实样本 + BandiSoft 的 Canon HIF |
| pro | `.exr .jxr .wdp .hdp .xcf .wmf .ase .aseprite .djvu .djv .ktx2` | OpenEXR-images、kimageformats、xcf-rs、ImageMagick、Aseprite、Wikimedia、KTX-Software |
| core | `.ff` | 按 farbfeld 规范手写真实 RGBA16 样本 |

seed 的逐文件 URL、字节数、sha256、许可见
`.temp/corpus/raw/manifest.tsv` 与 `.temp/corpus/real/manifest.tsv`。X3F 由
`raw` DLL 内新增的 flashraw 回退解码；其余格式走 rawler 或对应 DLL。

### 不生成 / 不公开样本

`.crm`（Canon Cinema RAW Light，视频容器）和 `.qtk`（Apple QuickTake 150
静态 RAW）保留解码支持，但目前没有可公开再分发的样本，因此**不进入 public
corpus**，只在 local-only 目录测试。不得用 0 字节文件、改名 DNG 或其它假样本
占位。以后新增任何无法生成/无法公开的格式，必须在
[`GENERATION_EXCEPTIONS.md`](GENERATION_EXCEPTIONS.md) 和
`corpus/registry.py::GENERATION_EXCEPTIONS` 同时登记；测试会检查两边一致。

## 后缀认领台账 `claims.tsv`

`testdata/claims.tsv` 是机器可读的 **后缀 → 成员 → 证据** 台账，和 golden 是两件事：

- 每一行：`tier` / `member` / `suffixes`（分号分隔）/ `status` / `evidence` / `note`；
- `verified` 的证据只能是 `golden:<file>`（文件存在且被某个 `.rs` 引用）或
  `tests:<decoders-rust/singles package>`（该包至少有一个 `#[test]`）；
- `unverified` 必须写 `-` 和原因；
- `decoders-rust/decoders/*/src/lib.rs` 声明的每个后缀必须**恰好出现一次**；新增认领不补台账，
  `tools/check_claims.py` 直接红。

当前 `check_claims.py` 读数：**108 个后缀，verified 75，unverified 33**。未验证的主要缺口是
`.heic/.heif/.hif`、相机 RAW、`.ff` 等无本机编码器的格式全部使用 `.temp/corpus`
真实 seed（见 `make_samples.py --seed-dir`）；别名后缀共用同一个成员证据。

## 与 `golden/` 的分工

| | `golden/`（入库，137 KB，27 个） | 生成器产物（仓库外，逻辑 8.7 GB，324 个 = 108 后缀 × 3 档） |
|---|---|---|
| 用途 | **断言正确性**：能不能解、值对不对、该不该拒绝 | **压性能与尺寸/内存门** |
| 入口 | `include_bytes!` 编译期嵌入测试二进制 | 运行时读文件（`main/examples/core_smoke.rs`、`main/examples/decoder_smoke.rs`） |
| 尺寸 | 24 B – 52 KB，全是"最小可复现" | 1 / 10 / 100 MiB 三档 |
| 能否重建 | 11 个不能、16 个能但需要冻结字节（见上） | 能，一条命令 |

噪声是每种熵编码的最坏情况，所以同一个后缀在三档下的解码耗时、内存峰值才有可比性；
而 golden 只关心"这条路径通不通、这个值对不对"，用 100 MB 图去断言"CMYK 必须报
`ERR_UNSUPPORTED`"不会多出任何信息。

★ **`decoders-rust/decoders/<tier>/` 与 `decoders-rust/singles/<id>/` 下都不放任何图片**
（2026-10-25 迁完）：需要样本就引用 `/../../../testdata/golden/<file>`。同一后缀只有一枚认领者，
换解码库时样本不该跟着丢。
