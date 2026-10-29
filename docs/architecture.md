# 架构

## 一句话

宿主只认 C-ABI；本仓把"每格式一个实现"装进 **8 枚发布 DLL**。源码分 **7 个 cargo
workspace**，不是按目录美观分的，而是按 **feature 隔离**、**工具链** 与 **崩溃域** 分的。

## 七个 workspace

| workspace | 位置 | 成员 | 产物 | 发布 |
|---|---|---|---|---|
| 根档 | `workspace/Cargo.toml` | `shared/router`、`shared/pro-support`、`decoders/{common,modern,raw}`（`abi` 是外部 path 依赖） | `common.dll`、`modern.dll`、`raw.dll` | 是 |
| 单库 | `workspace/singles/Cargo.toml` | `workspace/singles/*`（清单见 `singles/README.md`） | 单库 DLL（bench/A-B 用） | 否 |
| 专业档 | `workspace/decoders/pro/Cargo.toml` | `rupix-pro` + `pro-*` 单库 | `pro.dll` | 是 |
| 经典位图档 | `workspace/decoders/classic/Cargo.toml` | `rupix-classic-dll` + `singles/classic` | `classic.dll` | 是 |
| 纹理档 | `workspace/decoders/texture/Cargo.toml` | `rupix-texture-dll` + `texture` / `texture-ktx2` 单库 | `texture.dll` | 是 |
| 文档档 | `workspace/decoders/doc/Cargo.toml` | `rupix-doc` + `doc-*` / `pro-wmf` / `pro-djvu` | `doc.dll` | 是 |
| 旧版文档档 | `workspace/decoders/doc-legacy/Cargo.toml` | `rupix-doc-legacy-tier` + `singles/doc-legacy`（C++ 静态库） | `doc-legacy.dll` | 是 |

七个 workspace 各有自己的 `Cargo.lock`、`.cargo/config.toml`（target-dir）与 release profile。
`build.ps1` 依次跑：根 → singles → pro → classic → texture → doc → doc-legacy；凡是
`decoders/<id>/Cargo.toml` 自带 `[workspace]` 的档都由同一段循环单独构建（从该档自己的
manifest 解析 target-dir、target triple 与 `[lib] name`）。

### 为什么不是一个 workspace

1. **`standalone` 是减性 feature**：单库默认导出 `#[no_mangle] iv_*`（`standalone`），
   档以 `default-features = false` 只取 `*_impl`。同一次 cargo 调用里 feature 取并集，
   档会链进多份 `iv_abi_version`，链接器报 `LNK2005`。
2. **pro 的依赖会污染根档**：`pro` 的 `image` / `exr` 依赖一旦与 common/raw 同图，
   feature 统一会把 EXR/AVIF 等后端编进每一枚 DLL（实测 common +685 KB、raw +834 KB）。
3. **doc-legacy 用另一条工具链**：它链 libcdr/libwpg/libzmf + librevenge + ICU 的静态 C++
   库，`[build] target = "x86_64-pc-windows-gnu"`；与 MSVC 档放进同一 workspace 会强行统一
   链接器和 CRT 口径。
4. **崩溃域**：每枚 DLL 各自独立；一枚坏了不拖累其它档。

### 档 vs 单库

- **单库**（`workspace/singles/<id>/`）：真正的解码实现，形态是"薄壳 + `probe_impl` / `render_impl`"。
  它也可以开 `standalone` 编成独立解码器，但不发布。
- **档**（`workspace/decoders/<id>/`）：装配件。`Cargo.toml` 只列成员依赖（`default-features = false`），
  `src/lib.rs` 是 `export_decoder!` + `MEMBERS` 路由表 + `iv_*` 导出，**不写解码逻辑**。
- **例外**：`raw` 档同时是 tier 和引擎——相机 RAW 的显影链（线性化、去马赛克、色彩矩阵、
  gamma）在 `workspace/decoders/raw/src/lib.rs` 里，不在单库。文档与评审必须按这个事实看它，
  不能套"档都只装配"。

## 数据流

```text
宿主 rupix.exe
  └─ 扫 <exe>/decoders/*.dll
       └─ iv_decoder_info() 读 id / 名称 / 后缀 / 能力
            └─ 按 (后缀, 解码类型, CAP_*) 选候选
                 └─ iv_probe() 置信度
                      └─ iv_render() 两阶段：out=NULL 查询，非 NULL 渲染
                           └─ 档内 MEMBERS 按置信度选成员
                                └─ 单库 *_impl() → RGBA8
```

- 像素缓冲由**宿主分配**，解码器只写；跨 DLL 不释放、不传 `String` / `Vec` / trait 对象。
- 查询阶段回填尺寸/格式/`bytes_needed`；渲染阶段写像素。两阶段必须给出同一答案。
- 档内允许让位：查询阶段按置信度降序试，软失败（`ERR_UNSUPPORTED` / `ERR_UNRECOGNIZED`）
  换下一家；渲染阶段也让位，但换人前重问尺寸，装不下就报 `ERR_BUFFER_TOO_SMALL`。
- 能力位（`CAP_*`）由 DLL 自报；宿主不维护名单。`CAP_SCALED` 永不从缺席推定。

## ABI 快照

- 契约真源：`abi/src/lib.rs`（`IV_ABI_VERSION`，当前 6；下界 `MIN_ABI_VERSION = 5`）。
- 宿主侧：`main/vendor/rupix-abi/src/lib.rs`，是**逐字节快照**，不是 path 依赖。
- 同步动作只有一个：改完上游后，把 `abi/src/lib.rs` **整份复制**覆盖宿主快照。
  两边都不放同步脚本；宿主守卫测试逐字节比对，漂移会红。
- 已发布导出名 / 参数 / `#[repr(C)]` 布局 / 错误码编号**只增不改**；新能力只走
  "新可选导出 + 新结构体 + 新位值"，宿主探测缺席 = 缺能力，不是错误。
- 可扩展信息走可选导出 `iv_metadata()` 的键值表（`version` / `author` / `license` /
  `released` 等）；加字段只追加 key/value，不改 `IvDecoderInfo`，不 bump ABI。
- **矢量能力走 ABI v6 区域渲染**：可选导出 `iv_render_region` + `IvRegionReq` + `CAP_REGION`，
  见 [`vector-decoder.md`](vector-decoder.md)。`iv_export_svg` 只作为**冻结历史符号**保留，
  宿主已不再调用；`CAP_VECTOR` 不再与它绑定。
- 结构体布局靠编译期 `size_of` 断言钉死；禁止手抄副本。

## 边界

- 解码器**永远不依赖宿主代码**；本仓不引 `windui` / `windows` / `rupix`。
- `doc-legacy` 是唯一含原生 C++ 的发布档（libcdr/libwpg/libzmf + librevenge + ICU，静态链接）；
  其余七枚发布 DLL 为纯 Rust。第三方 C/C++ 源放仓库外 `../.3rd/`，规则见 `.3rd/README.md`。
- 本仓只放源码：target、发布暂存、pdb 归档、一次性脚本都在仓库外
  `.temp/`（见 [`building.md`](building.md)）。
- 单库可以带 `examples/` 做探针；发布档不承担 bench 脚本。

## 文件地图

```text
abi/                 契约真源 + 解码器 kit + c/ 单头
workspace/shared/router/              各档共用的合并路由内核（rlib）
workspace/shared/pro-support/          尺寸闸 / RGBA 回填（rlib）
workspace/decoders/<id>/       发布档：装配 + 自述 + 路由；raw 例外含引擎
workspace/singles/<id>/  单库：每格式实现 + 测试 + README
testdata/            make_samples.py + golden/ 证物
tools/             工具链 + 门禁 + 清单生成
docs/                本目录：跨模块流程/设计
decoders.json           机器清单（宿主「可下载」卡读它）
解码器开发规范.md      ABI 契约正文
```
