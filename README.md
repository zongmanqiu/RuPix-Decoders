# RuPix 解码插件仓库

RuPix 看图器的**解码插件**源码工程：三个 cargo workspace（档 / `pro` / 单库），发布产物是
**三枚合并档 DLL**（`common` / `modern` / `raw`）+ 一枚**专业档**（`pro`）。宿主 `rupix.exe` 本体不含任何位图解码实现，
运行时把 exe 同级 `decoders/` 目录里的 DLL 全扫一遍、按各自自述认领扩展名。

> 本仓库**只放源码**。单库（每格式一枚的参考实现）住在 `bench/singles/`，是**第二个
> workspace**、不发布；`decoders/<id>/` 下只有档——一个文件夹一枚发布 DLL。

宿主仓库（`main/`）在何处、如何与本仓库对接，见下文「与宿主的关系」。

---

## 一、发布产物（abi v4）

| 档 | 版本 | 认领的后缀 | 体积（实测） | 下载 |
|---|---|---|---|---|
| [`common`](decoders/common/README.md) | 0.1.0 | **29**：jpg jpeg jpe jfif jif jfi exif png apng gif tif tiff qoi webp bmp dib rle ico icon pnm ppm pgm pbm pam dds ff tga psd hdr | 1,967,104 B | [已发布][common-dl] |
| [`modern`](decoders/modern/README.md) | 0.1.0 | **6**：heic heif hif avif avifs jxl | 7,465,472 B | [已发布][modern-dl] |
| [`raw`](decoders/raw/README.md) | 0.4.0 | **40**：29 种相机 RAW（cr2 cr3 nef arw dng …）+ raw rgb rgba gray gry bgra yuv i420 yv12 nv12 yuy2 uyvy | 7,542,272 B | 未发布（本地已构建，release 还没上传） |
| [`pro`](decoders/pro/README.md) | 0.1.0 | **33**：jp2 j2k j2c jpc jpx jpf jpt jxr wdp hdp exr ktx ktx2 dcm dicom nii nrrd mrc mrcs ccp4 map pcx pcc dcx icns wbmp xbm xcf wmf ase aseprite djvu djv | 8,266,240 B | 未发布（本地已构建，release 还没上传） |

**下载地址**（release 资产，tag 与 `<id>_<version>` 同名；2026-10-26 逐条实测可下，
实收字节数与清单里的 `size` 一致）：

```text
[common-dl]: https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/common_0.1.0/common_0.1.0.dll
[modern-dl]: https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/modern_0.1.0/modern_0.1.0.dll
```

| 文件 | 大小 |
|---|---|
| `common_0.1.0.dll` | 1,967,104 B |
| `modern_0.1.0.dll` | 7,465,472 B |

★ **清单里的地址是人工维护的**：`scripts/make_index.py` 没有网络，无从判断某个 release 到底存不存在，
所以它**不臆造地址**——缺地址的条目会在跑完时列成 `MISSING url`，`index.json` 里该条目 `url` 为空。
上传完一枚 DLL 就把释放资产地址填进对应条目，再跑一次生成器（地址会被原样保留）。
`scripts/check_urls.py` 负责把清单里的地址真下载一遍核对。

### `index.json` 长什么样

**顶层就是一个数组，一项一枚 DLL。** 没有 `schema` / `updated` / `plugins` 包裹层——清单里只有
一种东西（DLL），那就只写这一种东西；早先每项还带 `file` / `sha256` / `author` / `homepage` /
`minHost` / `tag` / `notes` / `url{gitee,github}`，2026-10-26 一并砍掉（理由见
`scripts/make_index.py` 头部：文件名能从 `id`+`version` 算出来，而 `size` 给出的完整性判据
比一串 64 位哈希更好懂）。九个字段，顺序固定：

| 字段 | 例 | 说明 |
|---|---|---|
| `id` | `common` | 解码器标识；宿主按它认人（**不从文件名反推**） |
| `name` | `Common Raster Set` | 解码器名称（DLL 自述，**不是 DLL 文件名**） |
| `version` | `0.1.0` | 版本号 |
| `abi` | `4` | ABI 大版本，宿主按它决定收不收 |
| `size` | `1967104` | 字节数；下载后按它比对——**这就是完整性校验** |
| `released` | `2026-10-26` | 发布时间；未发布为空串 |
| `desc` | `常见位图合集：…` | 一句话说明（DLL 自述） |
| `formats` | `jpg;jpeg;jpe;…` | 认领的扩展名，**分号分隔、不带点**（与 ABI `extensions` 同形） |
| `url` | `https://…/common_0.1.0.dll` | 下载地址；未发布为空串 |

宿主的「解码器 / 可下载」卡读的就是这份：
`https://raw.giteeusercontent.com/qiuzongman/RuPix-Decoder/raw/master/index.json`。

合并的动机只有一个：**省体积**。每枚独立插件自带一份底座（空壳 + Rust 运行时，按 `qoi`
那枚 192 KB 反推约 150 KB/枚）；合成一枚后底座只剩一份，库本体一分不少。合并**不改变
速度**——1.80× 来自"绕开 `image` 包装层、由冠军库原生直出 RGBA8"，与 DLL 粒度无关。

四档都是**纯 Rust**（依赖树里无针对解码的 `-sys`、无 `cc`、无 `bindgen`，构建期零 C 编译）。
`common` 里 PSD 的像素解码是插件自写的（`zune-psd 0.5.1` 的像素路径有三处实测缺陷，
详见 [decoders/common/README.md](decoders/common/README.md) 与 `bench/singles/psd/src/lib.rs` 模块头）。

### 单库（`bench/singles/`，不发布）

22 个单库（11 个通用 + 11 个 pro）是 bench 横评的比对目标、档的装配来源，也是 A/B 排查的现成实现：

| 单库 | 库 | 归属档 |
|---|---|---|
| `jpeg` / `png` / `gif` / `tiff` / `qoi` / `core` | zune-jpeg / png / gif / tiff / 自研 / image+image-webp | `common` |
| `psd` / `hdr` | zune-psd（头）+ 自解像素 / image 的 `hdr` feature | `common` |
| `av` / `jxl` | heif-oxide + zenavif / jxl-oxide | `modern` |
| `rawpx` | 零依赖参数化裸数据 | `raw` |
| `pro-jp2` / `pro-jxr` / `pro-exr` / `pro-ktx2` | hayro-jpeg2000 / jxr / exr / ktx2+texture2ddecoder | `pro` |
| `pro-dicom` / `pro-sci` / `pro-small` | dicom-pixeldata / 自写 NIfTI/NRRD/MRC / pcx+icns+wbmp+xbm | `pro` |
| `pro-xcf` / `pro-ase` / `pro-djvu` / `pro-wmf` | xcf / aseprite-loader / djvu-rs / wmf-core+resvg | `pro` |

单库的 `[lib] name` 与档同名（如 `core`、`raw`）时，三个构建图必须用**三个 target
目录**（`../../.temp/bench-target`、`../.temp/decoder-target`、`../.temp/pro-target`），否则 `raw.dll` / `raw.pdb`
会互相覆盖（cargo 的 output filename collision）。

## 二、目录

```
.
├── Cargo.toml              档 workspace 根（common/modern/raw；profile：全局 opt-level = 3 + LTO + strip + panic=unwind）
├── .cargo/config.toml      target-dir 钉到 ../.temp/decoder-target（编译中间产物不进源码仓库）
├── build.ps1               一键：门禁 + 编译 + 测试 + 发布/镜像（产物落仓库外 ../.temp/decoder-dist）
├── abi/                    rupix-abi —— 契约真源 + 插件样板 kit（同一 crate）
│   └── c/                  C/C++ 插件单头 rupix_plugin_kit.h + abi_probe.c 自检
├── router/                 四档共用的路由内核（rlib，不是插件）
├── pro-support/            pro 档共用的尺寸闸 / RGBA 回填（rlib，不是插件）
├── decoders/<id>/          ★ 一个发布 DLL 一个目录（common / modern / raw + pro）
│   ├── Cargo.toml          只做装配：成员依赖（default-features = false）+ 版本
│   ├── src/lib.rs          export_plugin! + MEMBERS 路由表 + iv_* 导出，无解码逻辑
│   ├── README.md           这一档的说明、体积账、扩张规则
│   └── pro/                ★ 第三个 workspace（独立 Cargo.lock；防 feature 统一污染前三档）
├── bench/                  ★ 第二个 workspace（单库，不发布）
│   ├── Cargo.toml          members = singles/<id>；核心库 opt-level = 3
│   ├── .cargo/config.toml  target-dir 钉到 ../../.temp/bench-target
│   └── singles/<id>/       每格式一枚的参考实现 + 各自的 README / examples
├── testdata/               测试输入的两半（划分标准：能不能用脚本算出来）
│   ├── make_samples.py     生成 1/10/100 MiB 雪花样本 → 落仓库外（按后缀共享，别名走硬链接）
│   ├── golden/             27 个 golden / 边界证物（134 KB，入库）：真 RAW、隔行 PNG、CMYK TIFF、PSD/HDR、最小 JXL…
│   └── README.md           两半的分工、档位怎么定、哪些格式造不出来、`include_bytes!` 怎么引用
├── scripts/
│   ├── toolchain.ps1       工具链固定与体检（rustup / vcvars64 / nasm / cmake / CC 护栏）
│   ├── check_deps.py       依赖门禁：插件不得引 windui / windows / rupix
│   ├── check_docs.py       文档门禁：每个 decoders/<id>/ 必有 README.md，根规范必须在，链接可达
│   ├── check_caps.py       闸值一致性：MAX_PIXELS 同值 + 与宿主/make_samples 的镜像值对齐
│   ├── check_claims.py     后缀认领门禁：四档的 extensions 两两不相交
│   ├── check_anchors.py    行锚嫌疑名单（advisory，恒 exit 0；名单给人判，不当结论）
│   └── make_index.py       生成 index.json（可下载清单：一项一枚 DLL，读 DLL 自述；url 人工维护）
├── 插件开发规范.md          契约正文（C-ABI 规格、认领规则、发布约定）
├── THIRD-PARTY-NOTICES.md  发布 DLL 的第三方库与许可证（随 release 分发）
├── index.json              可下载清单（顶层数组，一项一枚 DLL；宿主从 raw 读它）
├── AGENTS.md               本仓库硬约束 + 新增解码器操作顺序
└── LICENSE                 AGPL-3.0
```

**本仓库只放自己写的代码**，几样东西刻意留在外面（都在上一级 `RuPix/`）：

| 外部目录 | 装什么 | 谁在用 |
|---|---|---|
| `../.temp/decoder-target/` | 档的 cargo 中间产物、形态 B 的 `cbuild/<id>`、`toolchain_report.txt` | `build.ps1` 从 `.cargo/config.toml` 读出这个路径；可用 `CARGO_BUILD_TARGET_DIR` 覆盖 |
| `../../.temp/bench-target/`（相对 bench/） | 单库的 cargo 中间产物 | `bench/.cargo/config.toml` 钉死 |
| `../.temp/pro-target/` | pro 独立 workspace 的 cargo 中间产物 | `decoders/pro/.cargo/config.toml` 钉死；`build.ps1` 把 `pro.dll` 拷回根 target 再发布 |
| `../.temp/decoder-dist/` | 发布暂存：`<id>_<ver>.dll`（对外走 gitee release 资产，不进仓库） | `build.ps1` 写、`scripts/make_index.py` 读；可用 `RUPIX_DIST_DIR` 覆盖 |
| `../.temp/testdata/` | 按后缀生成的 1/10/100 MiB 雪花样本 + `manifest.json` | `testdata/make_samples.py` 写；可用 `RUPIX_SAMPLE_DIR` 覆盖 |

当前构建**不依赖 `../.3rd/`**（纯 Rust 化后没有 C 源）。`../.3rd/` 里剩的
`stb-2.30` / `libde265` / `libheif` / `jxrlib` 是历史与宿主侧保留，本仓库不引用。

**`decoders/<id>/` 里放什么**（档是装配件，单库才有完整实现）：

```
decoders/<id>/
├── Cargo.toml              [lib] name = 目录名 = 插件 id；crate-type = ["cdylib","rlib"]；version 显式写
├── src/lib.rs              export_plugin! 自述 + MEMBERS 路由表 + iv_probe / iv_render
└── README.md               这一档的说明、每个成员为什么被选、限制与已知空白
```

★ **样本不在这里**：任何 `decoders/<id>/` 与 `bench/singles/<id>/` 下都不放图片，一律用
`..\..\testdata\golden\<file>`（`include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),
"/../../testdata/golden/<file>"))`）。理由：同一后缀只有一枚认领者，换解码库时样本不该跟着丢。
规则见 `testdata/README.md`。

## 三、构建与发布

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

它做的事，一步都不许跳过：

1. dot-source `scripts/toolchain.ps1`：固定 `RUSTUP_TOOLCHAIN`（本机 rustup 无默认，K31）、
   `vswhere` 定位 `vcvars64.bat`、导出 `NASM_PREFIX` / `CMAKE_GENERATOR`、**拦 `CC=gcc`**
   （MSVC/MinGW 混链 = K06）；版本报告落 `<target-dir>/toolchain_report.txt`。
2. 门禁先跑：`cargo fmt --check` → `python scripts/check_docs.py` +
   `python scripts/check_deps.py` + `python scripts/check_caps.py`
   → `cargo clippy --all-targets --workspace --release -- -D warnings`
   （clippy **必须带 `-D warnings`**；这几步 2026-10-20 才真正接进流水线——此前只写在
   `AGENTS.md` §七 的表里，没有任何一步执行）。
3. `cargo build --release --workspace` → `cargo test --release --workspace -- --test-threads=1`
   （**必须串行**：解码插件持有进程级会话池，并行会让两条用例互相挤掉缓存），**测试失败不发布**。
4. 形态 B 识别：`decoders/<id>/CMakeLists.txt` 存在 → 走 CMake + ctest 子构建
   （`<target-dir>/cbuild/<id>`），ctest 不过不发布。当前四档全是形态 A（Rust cdylib）。
5. 改名发布到 `<id>_<ver>.dll`（落 `../.temp/decoder-dist`，**不进仓库**）；
   **缺任何一个插件产物 = 硬错误**（静默丢一个 DLL 等于运行时静默丢一个格式）。
6. 若宿主构建目录 `../.build/decoders` 存在（两仓库并排的开发形态），把同一批 DLL
   镜像过去，并在最后**统一 prune 同 id 的旧文件**（Windows 不允许覆盖已加载 DLL，
   同 id 留两份会让宿主重复加载）。
7. 逐 DLL 体积表 + 总量（仅记账；D1 于 2026-10-18 拍板为**无总体积预算**，不拦）。
8. 调试符号：把 `<id>.pdb` 归档到 `../.temp/symbols/plugins/`（**不进发布目录**），
   供宿主 `build/dump_sym.py` 把插件崩溃解到源码行。

产物命名就是版本策略：`<id>_<[package] version>.dll`；ABI 版本由 DLL 里的
`iv_abi_version()` 自报，**文件名不写 abi**（写进文件名就是第二份真源，还能手工改错）。

```powershell
python scripts/check_deps.py            # 依赖门禁，violations: 0 才算过
python scripts/check_docs.py            # 文档门禁，missing: 0 才算过
py scripts/make_index.py                # 生成清单 index.json（一项一枚 DLL，url 保留手填值）
python scripts/check_urls.py            # 清单里的地址真下一遍，核对字节数
```

下载地址是 **release 资产**，形如
`https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/<id>_<version>/<id>_<version>.dll`，
**每枚 DLL 一个 release、一个 tag**（2026-10-26 改：以前四档共用一个日期 tag，结果只有被上传的
那几枚能下，其余全是 404）。

生成器只覆盖它自己从 DLL 里读出来的字段（id/name/version/abi/formats/desc/file/sha256/size/tag），
**`url`、`notes`、`lang` / `pureRust` 这类手工键原样保留**。`-Tag <共享 tag>` 仍可用，仅限
"一次发布把所有 DLL 都带上"的批式发布——那时清单会按那个 tag 重算全部地址。

## 四、与宿主的关系（只有两条线）

```
发布目录 *.dll  ──(build.ps1 可选镜像)──►  宿主构建目录的 decoders/  ──► rupix.exe 扫描加载
本仓库 abi/src/lib.rs ──(人工整份复制)─────────►  宿主 vendor/rupix-abi/src/lib.rs（逐字节快照）
```

* **单向**：插件永远不依赖宿主代码；宿主只认 C-ABI 契约，不认插件用什么语言写、快不快。
  宿主的**基座文档不记录解码器清单**（不写“有几个插件、每个认领什么”）：那是本仓的唯一真源，
  宿主运行时只读 DLL 自述。因此本仓改档、合并、换库都不需要同步宿主文档。
* **契约同步是机械动作，没有脚本**：宿主持有快照而不是 path 依赖，所以纯宿主克隆也能构建。
  代价是改完 `abi/src/lib.rs` 要**人工把它整份复制**到宿主的
  `vendor/rupix-abi/src/lib.rs`（覆盖、不编辑）。宿主侧守卫测试
  `core::abi::tests::abi_snapshot_matches_upstream_when_present` 逐字节比对，两份不一致就把
  宿主 `cargo test --lib` 跑红——**手改宿主快照会被测试抓住，不会被静默合并**。
* 宿主文档里的《插件开发规范》已退化成一个指路指针，规范正文以本仓库
  `插件开发规范.md` 为准。

## 五、约定速查（细节看 AGENTS.md）

| 约定 | 内容 |
|---|---|
| 目录名铁律 | `decoders/<id>/` 目录名 = 插件 id = `[lib] name` = DLL 文件主干名 |
| 版本号 | 各档 `Cargo.toml` 的 `[package] version` **显式写出**，不走 workspace 继承（脚本从这里读） |
| ABI | 冻结 v4；改布局/改错误码含义/改字段语义 = bump，**必须用户拍板**；新增可选导出不 bump（版本史见 `插件开发规范.md` §九） |
| panic | `panic = "unwind"` 不许改 `abort`：宿主靠 `catch_unwind` + SEH 隔离坏插件 |
| 热路径 | 档 workspace 全局 `opt-level = 3`；单库 workspace 默认 `z` + 逐包 3 |
| 分发 | 一个 DLL 自带全部自述，`plugins/` 目录里**只有 DLL**，没有清单/指针文件 |
| 测试样本 | 分两半（判据=能不能用脚本算出来）：`py testdata/make_samples.py` 生成 1/10/100 MiB 雪花样本**落仓库外**；`testdata/golden/` 的 27 个边界证物**入库**、编译期 `include_bytes!`。**任何 decoders/ 或 bench/singles/ 下不放图片** |

新增一种格式的完整顺序（含形态 A/B、裸数据、多帧、多页的额外步骤）在
`AGENTS.md` §六；交付前核查表在 §七。

## 六、许可证

本仓库 **AGPL-3.0**（与宿主一致）。将来重新引入第三方 C 库时，源码放 `../.3rd/<name>-<ver>/`
并回写本仓 `THIRD-PARTY-NOTICES.md`；当前四档无 C 依赖。

## 七、发布与开放事项

* **发布清单走 release 资产**（2026-10-25 拍板，已落地）。本仓**只放源码**：`dist/` 不在仓库里，
  发布暂存 = `../.temp/decoder-dist`。发布一次的动作顺序：

  1. `build.ps1` 编出 4 枚 DLL 并镜像到宿主 `../.build/decoders`；
  2. 每枚 DLL 传成**自己的一个 release**，tag 用 `<id>_<version>`（与资产文件名同名）；
  3. 把资产地址填进 `index.json` 对应条目的 `url`（或 `py scripts/make_index.py --set-url <id>=<地址>`），提交；
  4. 宿主拉 `raw/master/index.json`，按 `url` 去 release 下 DLL。

  ⇒ **清单可以先提交、资产后上传**（那段时间 url 会 404，宿主如实报下载失败）；
  反过来不行：`index.json` 不提交，宿主不知道有新版。
* **还没有 GitHub 镜像**。`url` 是单个字符串（指向 gitee release 资产）；真要加镜像，
  那是清单结构的一次改动（加第二份地址），不是给生成器塞个参数就算完。
* 仓库名：目录现在叫 `RuPix-Decoder/`，与 gitee 远端同名（clone 前后路径一致）。
* 尚未做：`pro`（jp2/jxr/exr/ktx2/dicom…）、`rust-full`（并集安装档）、
  `jpegturbo` 加速包（只声明 `CAP_SCALED`）。分层与后缀路由表见宿主 `.plan/m138`。
