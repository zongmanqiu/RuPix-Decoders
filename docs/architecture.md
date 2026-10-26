# 架构

## 一句话

宿主只认 C-ABI；本仓把"每格式一个实现"装进四枚发布 DLL。源码分三个 cargo workspace，
不是按目录美观分的，而是按 **feature 隔离** 和 **崩溃域** 分的。

## 三个 workspace

| workspace | 位置 | 成员 | 产物 | 发布 |
|---|---|---|---|---|
| 档 | `decoders-rust/Cargo.toml` | `shared/router`、`shared/pro-support`、`decoders/{common,modern,raw}`（`abi` 是外部 path 依赖） | `common.dll`、`modern.dll`、`raw.dll` | 是 |
| 单库 | `decoders-rust/singles/Cargo.toml` | `decoders-rust/singles/*` 共 22 个 | 单库 DLL（bench/A-B 用） | 否 |
| 专业档 | `decoders-rust/decoders/pro/Cargo.toml` | `rupix-pro` + 11 个 `pro-*` 单库 | `pro.dll` | 是 |

三个 workspace 各有自己的 `Cargo.lock`、`.cargo/config.toml`（target-dir）与 release profile。
`build.ps1` 依次跑：根 → singles → pro。

### 为什么不是一个 workspace

1. **`standalone` 是减性 feature**：单库默认导出 `#[no_mangle] iv_*`（`standalone`），
   档以 `default-features = false` 只取 `*_impl`。同一次 cargo 调用里 feature 取并集，
   档会链进多份 `iv_abi_version`，链接器报 `LNK2005`。
2. **pro 的依赖会污染前三档**：`pro` 的 `image` / `exr` / `resvg` 一旦与 common/raw 同图，
   feature 统一会把 EXR/AVIF 等后端编进每一枚 DLL（实测 common +685 KB、raw +834 KB）。
3. **崩溃域**：`common`、`modern`、`raw`、`pro` 各自是一枚 DLL；一枚坏了不拖累其它档。
   `pro` 的低频/专业格式尤其需要这个隔离。

### 档 vs 单库

- **单库**（`decoders-rust/singles/<id>/`）：真正的解码实现，形态是"薄壳 + `probe_impl` / `render_impl`"。
  它也可以开 `standalone` 编成独立解码器，但不发布。
- **档**（`decoders-rust/decoders/<id>/`）：装配件。`Cargo.toml` 只列成员依赖（`default-features = false`），
  `src/lib.rs` 是 `export_decoder!` + `MEMBERS` 路由表 + `iv_*` 导出，**不写解码逻辑**。
- **例外**：`raw` 档同时是 tier 和引擎——相机 RAW 的显影链（线性化、去马赛克、色彩矩阵、
  gamma）在 `decoders-rust/decoders/raw/src/lib.rs` 里，不在单库。文档与评审必须按这个事实看它，
  不能套"四档都只装配"。

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

- 契约真源：`abi/src/lib.rs`（`IV_ABI_VERSION`，当前 5）。
- 宿主侧：`main/vendor/rupix-abi/src/lib.rs`，是**逐字节快照**，不是 path 依赖。
- 同步动作只有一个：改完上游后，把 `abi/src/lib.rs` **整份复制**覆盖宿主快照。
  两边都不放同步脚本；宿主守卫测试逐字节比对，漂移会红。
- 改布局、错误码编号、既有字段语义 = bump，必须用户拍板；新增可选导出不 bump。
- 可扩展信息走可选导出 `iv_metadata()` 的键值表（`license` 等）；加字段只追加 key/value，
  不改 `IvDecoderInfo`，不 bump ABI。
- 结构体布局靠编译期 `size_of` 断言钉死；禁止手抄副本。

## 边界

- 解码器**永远不依赖宿主代码**；本仓不引 `windui` / `windows` / `rupix`。
- 本仓当前不引 `.3rd` 的 C 源；四档纯 Rust。
- 本仓只放源码：target、发布暂存、pdb 归档、一次性脚本都在仓库外
  `.temp/`（见 [`building.md`](building.md)）。
- 单库可以带 `examples/` 做探针；发布档不承担 bench 脚本。

## 文件地图

```text
abi/                 契约真源 + 解码器 kit + c/ 单头
decoders-rust/shared/router/              三档共用的合并路由内核（rlib）
decoders-rust/shared/pro-support/           pro 共用尺寸闸 / RGBA 回填（rlib）
decoders-rust/decoders/<id>/       发布档：装配 + 自述 + 路由；raw 例外含引擎
decoders-rust/singles/<id>/  单库：每格式实现 + 测试 + README
testdata/            make_samples.py + golden/ 证物
tools/             工具链 + 门禁 + 清单生成
docs/                本目录：跨模块流程/设计
index.json           机器清单（宿主「可下载」卡读它）
解码器开发规范.md      ABI 契约正文
```
