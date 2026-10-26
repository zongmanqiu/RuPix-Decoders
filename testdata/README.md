# testdata/ —— 测试输入的两半：生成器 + 入库的 golden

| 里面是什么 | 在仓库里吗 | 量级 | 引用方式 |
|---|---|---|---|
| `make_samples.py`：1/10/100 MiB 雪花样本生成器 | **是**（只有脚本） | 32 KB | — |
| `golden/`：27 个 golden / 边界证物（1×1 红点、隔行 PNG、CMYK TIFF、4 枚相机 RAW、AVIF 旋转样本、2 个最小 JXL、2 帧最小 GIF、最小 PSD/HDR…） | **是** | 137 KB | `include_bytes!` **编译期**进测试二进制 |
| 生成出来的 `<tier>/noise<ext>` | **否** → `../.temp/testdata/` | 186 个 / 逻辑 5.8 GB（别名是硬链接，实际占用更小） | 运行时读文件 |

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
文件名不带尺寸：尺寸/字节数在 manifest 里。别名后缀是**硬链接**，100 MB 档也只占一份：

- `.jpeg/.jpe/.jfif/.jif/.jfi/.exif` ← `.jpg`
- `.tiff` ← `.tif`；`.dib/.rle` ← `.bmp`；`.icon` ← `.ico`；`.pnm` ← `.ppm`
- `.j2k/.j2c/.jpc/.jpx/.jpf/.jpt` ← `.jp2`；`.pcc` ← `.pcx`
- `.mrcs/.ccp4/.map` ← `.mrc`；`.dicom` ← `.dcm`

## 跑法

```text
python testdata/make_samples.py                   # 全部后缀，三档
python testdata/make_samples.py --only .jpg,.png  # 子集（后缀或成员 id 都行：--only jpeg）
python testdata/make_samples.py --only common     # 四个发布 DLL 之一（common/modern/raw/pro）
python testdata/make_samples.py --only pro-jp2 --tier 100M
python testdata/make_samples.py --tier 10M        # 单档
python testdata/make_samples.py --list            # 只打印计划，不写文件（不需要 Pillow）
python testdata/make_samples.py --check           # 只验 claims 覆盖，不写文件
```

`--only common|modern|raw|pro` 对应四个发布 DLL。`modern` 的 HEIC/AVIF/JXL 全在 SKIP，
所以这条命令只打印“没有可编码后缀”，不会误生成整套样本。

`--list` 与 `--check` 都会先对 `claims.tsv` 做覆盖检查：每个已登记后缀必须出现在 `PLAN`
（会生成）或 `SKIP`（有明确理由），否则直接非 0 退出。**新增认领不补覆盖，门就红。**
`--check` 已接进 `build.ps1`，只输出计数，不生成样本。

本机 `python` 与 `py` 都是 3.11.3 + Pillow 11.1.0；换机器时只要装 Pillow。
`--list` 和手工编码器不需要 Pillow。

## 档位是下限，不是精确值

边长一路长到文件**超过**该档为止。长不过去就在 manifest 里写 `capped(...)` 并给出真实尺寸——
不假装达标。**当前实测**：62 个后缀 × 3 档 = 186 个样本；100 MiB 档 **42/62 达标**，
其余 20 个是物理上限或容器上限：

| 后缀 | 卡在哪一档 | 实际 | 为什么到不了 |
|---|---|---|---|
| `.ico` | 三档都卡 | 0.35 MB / 256×256 | ICO 规范单图最大 256×256 |
| `.pbm` | 10M / 100M | 5.0 MB / 6324×6324 | 1 bpp，40 MP 上限下最多 5 MB |
| `.wbmp` | 10M / 100M | 5.0 MB / 6324×6324 | 1 bpp，40 MP 上限下最多 5 MB |
| `.pgm` | 100M | 40.0 MB / 6324×6324 | GRAY8 = 1 B/px |
| `.gray .gry .raw` | 100M | 40.0 MB / 4096×9764 | rawpx GRAY8 = 1 B/px |
| `.nii .nrrd .mrc` | 100M | 40.0 MB / 6324×6324 | 单通道科学格式 = 1 B/px |
| `.i420 .nv12 .yuv .yv12` | 100M | 60.0 MB / 4096×9764 | 4:2:0 = 1.5 B/px |
| `.uyvy .yuy2` | 100M | 80.0 MB / 4096×9764 | 4:2:2 = 2 B/px |
| `.dds` | 100M | 20.0 MB / 6324×6324 | BC1（DXT1）固定 0.5 B/px，是 core 唯一能解的 DDS 形态 |
| `.gif` | 100M | 54.4 MB / 6324×6324 | 8 bpp 调色板 + LZW，噪声几乎不可压 |
| `.icns` | 10M / 100M | 6.65 MB / 1024×1024 | Pillow 的 ICNS writer 最大 1024 |
| `.xbm` | 100M | 25.3 MB / 6324×6324 | 1 bpp 文本，40 MP 上限下约 25 MB |

两处**不是**物理上限、已按需顶格：

- `.webp` 的维度上限原写 4096（我们自己设的，不是规范），改到 WebP 规范上限 16383 后
  实测 5250×5250 / 110 MB 达标。
- `.jp2` 原先被 4096 的保守上限卡在 72.9 MB；放开到 65535 后实测 5028×5028 / 109.9 MB 达标。

**40 MP 是故意的上限**：样本要能被查看器真正打开（`main/src/core/pixels.rs` 的 `MAX_PIXELS`；
解码器自己的闸是 64 MP）。超过 40 MP 的样本只能用来测"拒绝打开"，测不了渲染。
想探 64 MP 解码器闸，直接调 ABI 并令 `budget_bytes = 0`。

## 造不出来的格式（如实声明，不造假样本）

| 谁 | 后缀 | 为什么 |
|---|---|---|
| jxl | `.jxl` | 本机没有 `cjxl` / imagecodecs |
| av | `.heic .heif .hif .avif .avifs` | 需要 heif-enc / SVT-AV1 / x265；`.avif/.avifs` 已有 golden，不生成 |
| raw | `.3fr .ari .arw .cr2 .cr3 .crm .crw .dcr .dcs .dng .erf .fff .iiq .kdc .mef .mos .mrw .nef .nrw .orf .ori .pef .qtk .raf .rw2 .rwl .srw .x3f` | 相机 RAW 是真传感器马赛克 + 厂商元数据，合成字节只能测"解不出来" |
| pro | `.exr .jxr .wdp .hdp .xcf .wmf .ase .aseprite .djvu .djv .ktx2` | 本机没有对应编码器；KTX2 的 DFD/超压缩头未实现，只生成 KTX1（`.ktx`） |
| core | `.ff` | image-rs 认不出魔数，core 也没给 `.ff` 留 ext 通道，造出来是死样本 |

## 后缀认领台账 `claims.tsv`

`testdata/claims.tsv` 是机器可读的 **后缀 → 成员 → 证据** 台账，和 golden 是两件事：

- 每一行：`tier` / `member` / `suffixes`（分号分隔）/ `status` / `evidence` / `note`；
- `verified` 的证据只能是 `golden:<file>`（文件存在且被某个 `.rs` 引用）或
  `tests:<decoders-rust/singles package>`（该包至少有一个 `#[test]`）；
- `unverified` 必须写 `-` 和原因；
- `decoders-rust/decoders/*/src/lib.rs` 声明的每个后缀必须**恰好出现一次**；新增认领不补台账，
  `tools/check_claims.py` 直接红。

当前 `check_claims.py` 读数：**108 个后缀，verified 75，unverified 33**。未验证的主要缺口是
`.heic/.heif/.hif`（无 HEIC/HEIF 夹具）、相机 RAW（只有 `.temp/raw-samples` 真机样本，
入库 golden 仅用于嗅探）和 `.ff`（无魔数/无样本）。别名后缀共用同一个成员证据。

## 与 `golden/` 的分工

| | `golden/`（入库，137 KB，27 个） | 生成器产物（仓库外，5.8 GB，186 个） |
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
