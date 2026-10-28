# RAW / rawpx 样本记录

日期：2026-10-28。只记录**样本来源、许可、本地案例位置和验证结果**；
大体积样本不提交进仓库，`.temp/` 是本地语料区。

## 结论

- 相机 RAW：**29/29** 个后缀都有本地 seed，且冒烟 `decode=ok`。
- rawpx：**11/11** 个裸像素后缀都有生成样本；`rawpx` 另有 18 个单测。
- 29 个相机 seed 中，**26 个 manifest 标为 CC0**；`crm`/`dcs`/`qtk` 没有明确再分发许可。
- 仓库只提交了 31 个 golden；其中 4 个 RAW golden 被 exiftool 改过，只能嗅探，不能当解码证据。
- 逐文件 source URL、字节数和 SHA256 的正本是本地 `.temp/corpus/raw/manifest.tsv`；本文件是便于阅读的提交版记录。
- 一个后缀目录里可能有多份 seed；下表记录实际拿来做冒烟的那一份，来源列是 manifest 为该后缀登记的原始来源。

## 相机 RAW（29）

| 后缀 | 本地案例文件 | 来源（manifest 登记） | 许可 | 本地冒烟 | 尺寸 |
|---|---|---|---|---|---|
| `3fr` | `.temp/corpus/raw/3fr/Hasselblad_H3D_A9391120.3FR` | https://raw.pixls.us/data/Hasselblad/X1D/B9999910.3FR | CC0-1.0 | `ok` / 7216x5412 / 非零 100.0% | 53594112 B |
| `ari` | `.temp/corpus/raw/ari/A004C002_190722_R20I.478.ari` | https://raw.pixls.us/data/Arri/Alexa%20Mini/A004C002_190722_R20I.478.ari | CC0-1.0 | `ok` / 3424x2202 / 非零 100.0% | 11529396 B |
| `arw` | `.temp/corpus/raw/arw/small_sony_A300.arw` | https://raw.pixls.us/data/Sony/ILCE-7RM5/7RM5-LosslessUncompressed.ARW | CC0-1.0 | `ok` / 3872x2592 / 非零 100.0% | 10519152 B |
| `cr2` | `.temp/corpus/raw/cr2/B13A0729.CR2` | https://raw.pixls.us/data/Canon/EOS%205D%20Mark%20IV/B13A0729.CR2 | CC0-1.0 | `ok` / 6720x4480 / 非零 100.0% | 61940429 B |
| `cr3` | `.temp/corpus/raw/cr3/Canon_EOS_M200__MG_2231.CR3` | https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_RAW_ISO_100_nocrop_dual.CR3 | CC0-1.0 | `ok` / 6000x4000 / 非零 100.0% | 28778121 B |
| `crm` | `.temp/corpus/raw/crm/2A5A2953_00.CRM` | https://github.com/dnglab/dnglab/files/7974699/2A5A2953_00.zip | 无明确再分发许可 | `ok` / 8192x4320 / 非零 99.8% | 14518550 B |
| `crw` | `.temp/corpus/raw/crw/RAW_CANON_PRO1_ARGB.CRW` | https://raw.pixls.us/data/Canon/PowerShot%20Pro1/RAW_CANON_PRO1_ARGB.CRW | CC0-1.0 | `ok` / 3344x2484 / 非零 98.8% | 8163966 B |
| `dcr` | `.temp/corpus/raw/dcr/D4C50031.DCR` | https://raw.pixls.us/data/Kodak/DCS%20Pro%2014N/D4C50031.DCR | CC0-1.0 | `ok` / 4500x3000 / 非零 100.0% | 15913984 B |
| `dcs` | `.temp/corpus/raw/dcs/RAW_KODAK_DCS460D_FILEVERSION_3.DCS` | http://www.rawsamples.ch/raws/kodak/RAW_KODAK_DCS460D_FILEVERSION_3.TIF | 无明确再分发许可 | `ok` / 3066x2048 / 非零 100.0% | 6367232 B |
| `dng` | `.temp/corpus/raw/dng/IMGP0771.DNG` | https://raw.pixls.us/data/Pentax/K-70/IMGP0771.DNG | CC0-1.0 | `ok` / 6000x4000 / 非零 100.0% | 116873685 B |
| `erf` | `.temp/corpus/raw/erf/EPSN3575.ERF` | https://raw.pixls.us/data/Epson/R-D1s/EPSN3575.ERF | CC0-1.0 | `ok` / 3040x2024 / 非零 100.0% | 10092985 B |
| `fff` | `.temp/corpus/raw/fff/Hasselblad_H5D-40_20151114_CCSG.fff` | https://raw.pixls.us/data/Hasselblad/H4D-50/2-9-2017_street_0012.fff | CC0-1.0 | `ok` / 7304x5478 / 非零 100.0% | 69826305 B |
| `iiq` | `.temp/corpus/raw/iiq/PhaseOne_H25_H25_Outdoor_.IIQ` | https://raw.pixls.us/data/Phase%20One/P65%2B/CF027310.IIQ | CC0-1.0 | `ok` / 4090x5450 / 非零 100.0% | 17843198 B |
| `kdc` | `.temp/corpus/raw/kdc/100_1548.KDC` | https://raw.pixls.us/data/Kodak/EasyShare%20Z981/100_1548.KDC | CC0-1.0 | `ok` / 4288x3217 / 非零 100.0% | 25702448 B |
| `mef` | `.temp/corpus/raw/mef/RAW_MAMIYA_ZD.MEF` | https://raw.pixls.us/data/Mamiya/ZD/RAW_MAMIYA_ZD.MEF | CC0-1.0 | `ok` / 4016x5344 / 非零 100.0% | 36575308 B |
| `mos` | `.temp/corpus/raw/mos/L_003173.mos` | https://raw.pixls.us/data/Leaf/Aptus%2022/L_003173.mos | CC0-1.0 | `ok` / 4000x5336 / 非零 100.0% | 43365202 B |
| `mrw` | `.temp/corpus/raw/mrw/PICT0881.MRW` | https://raw.pixls.us/data/Minolta/DiMAGE%20A2/PICT0881.MRW | CC0-1.0 | `ok` / 3272x2456 / 非零 100.0% | 12150304 B |
| `nef` | `.temp/corpus/raw/nef/Nikon_1_J1__DSC0355.NEF` | https://raw.pixls.us/data/Nikon/D850/Nikon-D850-14bit-uncompressed.NEF | CC0-1.0 | `ok` / 3872x2592 / 非零 100.0% | 11254575 B |
| `nrw` | `.temp/corpus/raw/nrw/DSCN0417.NRW` | https://raw.pixls.us/data/Nikon/Coolpix%20A1000/DSCN0417.NRW | CC0-1.0 | `ok` / 4608x3456 / 非零 100.0% | 35241855 B |
| `orf` | `.temp/corpus/raw/orf/Olympus_EM1mk2_Standard_20MP.ORF` | https://raw.pixls.us/data/Olympus/E-M1MarkII/Olympus_EM1mk2_Standard_20MP.ORF | CC0-1.0 | `ok` / 5184x3888 / 非零 100.0% | 17272835 B |
| `ori` | `.temp/corpus/raw/ori/50mp-14bit-handheld.ori` | https://raw.pixls.us/data/OM%20System/OM-1%20Mark%20II/50mp-14bit-handheld.ori | CC0-1.0 | `ok` / 5184x3888 / 非零 100.0% | 22529506 B |
| `pef` | `.temp/corpus/raw/pef/IMGP9856.PEF` | https://raw.pixls.us/data/Pentax/K-3%20Mark%20III/IMGP9856.PEF | CC0-1.0 | `ok` / 6192x4128 / 非零 100.0% | 121355115 B |
| `qtk` | `.temp/corpus/raw/qtk/QT150_1.QTK` | https://raw.githubusercontent.com/colinleroy/qtkn_decoder/main/QT150/QT150_1.QTK | 无明确再分发许可 | `ok` / 640x480 / 非零 100.0% | 43890 B |
| `raf` | `.temp/corpus/raw/raf/DSCF0733.RAF` | https://raw.pixls.us/data/Fujifilm/X100V/DSCF0733.RAF | CC0-1.0 | `ok` / 6240x4160 / 非零 100.0% | 59201344 B |
| `raw` | `.temp/corpus/raw/raw/RAW_LEICA_DLUX3.RAW` | https://raw.pixls.us/data/Leica/Leica%20D-LUX%203/RAW_LEICA_DLUX3.RAW | CC0-1.0 | `ok` / 4224x2376 / 非零 100.0% | 21123276 B |
| `rw2` | `.temp/corpus/raw/rw2/Panasonic_DC-FZ45_P1000336.RW2` | https://raw.pixls.us/data/Panasonic/DC-S1R/P1000077.RW2 | CC0-1.0 | `ok` / 4896x3672 / 非零 100.0% | 21934080 B |
| `rwl` | `.temp/corpus/raw/rwl/Leica_-_V-LUX_(typ114)_-_3_2.RWL` | https://raw.pixls.us/data/Leica/V-LUX%20%28Typ%20114%29/Leica_-_V-LUX_%28typ114%29_-_3%3A2.RWL | CC0-1.0 | `ok` / 5472x3648 / 非零 100.0% | 23589376 B |
| `srw` | `.temp/corpus/raw/srw/03194133.SRW` | https://raw.pixls.us/data/Samsung/NX20/03194133.SRW | CC0-1.0 | `ok` / 5472x3648 / 非零 100.0% | 38192032 B |
| `x3f` | `.temp/corpus/raw/x3f/SDIM0061.X3F` | https://raw.pixls.us/data/Sigma/sd%20Quattro%20H/SDIM0061.X3F | CC0-1.0 | `ok` / 6192x4128 / 非零 100.0% | 65875572 B |

## rawpx 裸像素（11）

rawpx 没有文件头，下载真实照片没有意义；案例由 `testdata/make_samples.py` 生成，
宽高和布局由测试显式传入。文件位于 `.temp/testdata/10M/`。

| 后缀 | 本地生成案例 | 布局 | 证据 |
|---|---|---|---|
| `rgb` | `.temp/testdata/10M/noise.rgb`（存在） | RGB8 | `rupix-rawpx` 单测 + 生成器检查 |
| `rgba` | `.temp/testdata/10M/noise.rgba`（存在） | RGBA8 | `rupix-rawpx` 单测 + 生成器检查 |
| `gray` | `.temp/testdata/10M/noise.gray`（存在） | GRAY8 | `rupix-rawpx` 单测 + 生成器检查 |
| `gry` | `.temp/testdata/10M/noise.gry`（存在） | GRAY8 | `rupix-rawpx` 单测 + 生成器检查 |
| `bgra` | `.temp/testdata/10M/noise.bgra`（存在） | BGRA8 | `rupix-rawpx` 单测 + 生成器检查 |
| `yuv` | `.temp/testdata/10M/noise.yuv`（存在） | I420 | `rupix-rawpx` 单测 + 生成器检查 |
| `i420` | `.temp/testdata/10M/noise.i420`（存在） | I420 | `rupix-rawpx` 单测 + 生成器检查 |
| `yv12` | `.temp/testdata/10M/noise.yv12`（存在） | YV12 | `rupix-rawpx` 单测 + 生成器检查 |
| `nv12` | `.temp/testdata/10M/noise.nv12`（存在） | NV12 | `rupix-rawpx` 单测 + 生成器检查 |
| `yuy2` | `.temp/testdata/10M/noise.yuy2`（存在） | YUY2 | `rupix-rawpx` 单测 + 生成器检查 |
| `uyvy` | `.temp/testdata/10M/noise.uyvy`（存在） | UYVY | `rupix-rawpx` 单测 + 生成器检查 |

`.raw` 同时属于相机 RAW 和 rawpx：有魔数走相机 RAW，参数化请求走 `iv_render_raw`；
相机 RAW 的 `.raw` 案例是 `.temp/corpus/raw/raw/RAW_LEICA_DLUX3.RAW`。

## 快速样本

`.temp/raw-samples/` 另有 10 张来自 `immich-app/test-assets` 的快速样本（Canon/Nikon/Fuji/Panasonic/Ricoh/Sony），
只用于本地冒烟；不是上表 29 份 seed 的替代证据。

## 只有嗅探证据的 golden

| 文件 | 原因 |
|---|---|
| `testdata/golden/CanonRaw.cr2` | exiftool 改写容器，查询失败 |
| `testdata/golden/CanonRaw.cr3` | exiftool 改写容器，查询失败 |
| `testdata/golden/DNG.dng` | exiftool 改写容器，查询失败 |
| `testdata/golden/FujiFilm.raf` | exiftool 改写容器，渲染失败 |

## 复现

```powershell
cd E:\tmp\AI.Agent\RuPix\RuPix-Decoder\workspace
$env:RUPIX_RAW_SAMPLES_DIR='E:\tmp\AI.Agent\RuPix\.temp\raw-samples-all'
cargo run --release -p rupix-raw --example smoke_raw
cargo test --release -p rupix-rawpx -- --test-threads=1
```

`.temp/raw-samples-all/` 是从 `.temp/corpus/raw/<ext>/` 每个后缀取一份 seed 的汇总目录；
它只是本地测试目录，不提交。

