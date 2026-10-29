# RuPix 解码器仓库规则（AGENTS.md）

> 对**本仓库根**（`RuPix-Decoder/`，远端同名仓库）生效的硬约束简版。
> 门面与解码器一览见 `README.md`；契约全文见 `解码器开发规范.md`（真源在此，宿主仓库那份 `docs/解码器开发规范.md` 只留指针）；
> 跨模块流程/设计见 `docs/`（地图 `docs/README.md`）；每个 DLL 的当前状态见
> `workspace/decoders/<id>/README.md`、对外变化见 `workspace/decoders/<id>/HISTORY.md`；
> **修改、发布与交付检查的操作顺序见根 [`开发指南.md`](开发指南.md)**（流程唯一正本）；本文件只写硬约束与特殊形态。
> **目录名铁律**：本目录叫 **`RuPix-Decoder/`**。这个名字改过三回：`extension/` → 2026-10-11 `decoders-src/`
> （当时作废旧名）→ **2026-10-16 用户拍板翻案：改回 `extension/`，`decoders-src/` 作废** →
> **2026-10-25 用户拍板改名 `RuPix-Decoder/`**：本仓要独立发布到
> `https://gitee.com/qiuzongman/RuPix-Decoders`；本地目录仍叫 `RuPix-Decoder/`，远端是复数 `RuPix-Decoders`。
> 旧名 `extension/` / `decoders-src/` 一律作废。
> 现口径：文档、脚本、链接一律写 `RuPix-Decoder/`；中间产物目录同步叫 `.temp/decoder-target`
> （构建期，落在**工作区根**——本仓库上一级——的 `.temp/`，独立 clone 时改设 `CARGO_BUILD_TARGET_DIR`）。
> 2026-10-16 起它是**独立仓库根**，不是宿主的子目录。
> **`<宿主>` 约定**：本仓库不假设 RuPix 宿主仓库在哪个盘、叫什么名。文中 `<宿主>` 一律指
> 宿主仓库根（`main/` 那一层，含 `vendor/rupix-abi`），需要落地时由维护者给出实际路径。

---

## 一、仓库边界（硬）

1. **禁引宿主类型**：不得依赖 `rupix` / `rupix_lib`、**不得引入 `windui` 与 `windows`**。
   机器判定 = 本仓库 `python tools/check_deps.py`（`violations: 0` 才算过）。
   窗口的 / 剪贴板 / DPI 能力属于宿主 `platform` 层，解码器侧一律不碰。
2. **跨 DLL 只走 `rupix-abi`**：`#[repr(C)]` 结构 + 导出函数；禁传 `String` / `Vec` / trait 对象 /
   闭包；**宿主分配像素缓冲、解码器只写入**，绝不跨 DLL 释放。
3. **本仓库只记自己写的代码**：编译中间产物**不落源码目录**，`.cargo/config.toml` 把
   `workspace/.cargo` 钉到 `../../.temp/decoder-target`（pro 档另有 `../../../../.temp/pro-target`）；第三方 C/C++ 源也不入库，放 `../.3rd/<name>-<ver>/`
   （溯源规则见 `.3rd/README.md`）。外部路径都有环境变量覆盖口：`CARGO_BUILD_TARGET_DIR`、
   `RUPIX_3RD_DIR`。**代价**：单独克隆本仓库不能直接构建形态 A 解码器，得连 `.3rd` 一起带上或设变量。
   发布产物**不进本仓库**（2026-10-25：本仓只放源码，DLL 走 release 资产发布）：`build.ps1`
   把发布暂存写到仓库外 `../.temp/decoder-dist`（`RUPIX_DIST_DIR` 可覆盖）。**调试符号不进发布目录**：`build.ps1` 只把 `<id>.pdb` 归档到
`../.temp/symbols/decoders/`（`<宿主>/build/dump_sym.py` 的默认查找点，一次性目录），
   因此 `workspace/Cargo.toml` 的 `[profile.release] debug = 1` **不许删**——删了解码器的 pdb 只剩公开符号，
   解码器里崩了只能停在 `模块+偏移`（宿主那边是同一条理由，实测见 `<宿主>/docs/踩坑记录.md` K137）。
   ★ 本仓库的 `.ps1` 一律 **ASCII**（`tools/check_docs.py` 硬查）：PowerShell 5.1 按 ANSI
   读无 BOM 的脚本，中文注释会被解错、且报错常常落在邻近那条无辜的语句上（宿主 K27；
   2026-10-25 写解码器符号归档时原样复发一次）。注释写英文，别写中文。
4. **禁止**把日志、草稿、一次性脚本写进本仓库；仓库内的长期文件只有
   `README/AGENTS/LICENSE/Cargo.toml`、`abi/`（契约 + kit + `abi/c/` 的 C 单头）、
   `workspace/shared/router/`（各档共用路由内核）、`workspace/decoders/`（**发布档**，一个文件夹一枚 DLL）、
   `workspace/singles/`（单库，**独立 cargo workspace**，见下）、
   `testdata/`（生成器 + `golden/` 证物）、`tools/`、`workspace/.cargo/`、`build.ps1`、
   `docs/`（**只放跨模块流程/设计**，地图 `docs/README.md`）、
   `解码器开发规范.md`、`THIRD-PARTY-NOTICES.md`、`decoders.json`、`DOCS_MANIFEST.md`。
   `dist/` 不在仓库里；发布暂存落 `../.temp/decoder-dist`。
   变更史写在 `workspace/decoders/<id>/HISTORY.md`（各档一份）；ABI 版本史写在
   `解码器开发规范.md` §九。本仓库不再另养无主的日志文件。
5. **七个构建图**（2026-10-29 现状）：
   - `workspace/decoders/<id>/` = **发布档**：只做装配（依赖表 + `MEMBERS` 路由表），
     一行解码逻辑都不写。默认加进根 workspace 的 `members`。
   - 根 workspace（`workspace/Cargo.toml`）成员 = `shared/router` + `shared/pro-support` +
     `decoders/{common,modern,raw}`（`abi` 是外部 path 依赖）。
   - `workspace/singles/<id>/` = **单库**（每格式一个参考实现，bench 横评/A-B 用）：
     同时是独立解码器（`standalone` feature 生成 `#[no_mangle] iv_*`），**不发布**；
     自己的 `[workspace]` / `.cargo/config.toml`（target-dir = `../../../.temp/singles-target`，
     **`+crt-static` 必须逐字复制**）/ `[profile.release]`（`z` + 逐包 `opt-level = 3`）。
   - **独立 workspace 档**（各自 `[workspace]` + `Cargo.lock` + `.cargo/config.toml`）：
     `pro`（`../../../../.temp/pro-target`；`image`/`exr` 会与根档 feature 统一：
     实测 common +685 KB、raw +834 KB）、`classic`、`texture`、`doc`（`opt-level = "s"`）、
     `doc-legacy`（`target = "x86_64-pc-windows-gnu"`，`[lib] name = doc_legacy`，
     链 libcdr/libwpg/libzmf 静态 C++）。
   同一 cargo 调用里不同形态并存时 feature 取**并集**，档会链进多份 `iv_abi_version`
   （实测 `LNK2005`）。根 workspace 的档 profile 用**全局 `opt-level = 3`**
   （不抄单库逐包清单：那份名单靠人记，会漂）。
   `build.ps1` 依次跑根、singles，以及每个自带 `[workspace]` 的档；**单库不发布**，只做测试门禁。
   覆盖闸：任何 `decoders/<id>` 既不在根 members、又没有自己的 `[workspace]` 即报错。
6. 单向关系：`宿主 → 契约（宿主侧快照）` 与 `本仓库发布目录 → 宿主解码器目录`
   （发布目录 = `../.temp/decoder-dist`，不在本仓库里）。
   解码器**永远不依赖宿主代码**；宿主缺席时本仓库必须照样能构建。

## 一·五、无密钥 / 无隐私（硬，本仓是公开仓）

本仓库 `https://gitee.com/qiuzongman/RuPix-Decoders` 是**公开**的。下列东西一条都不许进：

| 禁止 | 样子 | 正确做法 |
|---|---|---|
| 私钥 / 证书 | `-----BEGIN … PRIVATE KEY-----`、`.pfx` / `.pem` / `.p12` / `id_rsa*` | 放仓库外；运行时从环境变量读 |
| 访问令牌 | gitee / github 的 token（`glpat-` / `ghp_` / `AKIA` / `sk-` 开头的长串） | 只进本机 git 凭据管理器或环境变量 |
| URL 内嵌凭据 | 地址里写成「账号:口令@主机」的那种 | 远端只写 `https://host/owner/repo` |
| 口令字面量 | `password = "…"`、`api_key: "…"`（键名像凭据 + **带引号的值**） | 本机配置文件，且写进 `.gitignore` |
| 个人联系方式 | 手机号、QQ、微信号、私人邮箱 | 要署名就用仓库主页 |
| 含真名的本机路径 | `C:\Users\<真名>\…`（**机器判**）；文档里尽量别写 `E:\…` 这类本机构建路径（**人眼判**：它不泄密，但换台机器就指不到东西） | 相对路径 / `%LOCALAPPDATA%` / 环境变量 |

- 例外只有一处：三方署名里的上游作者邮箱（`THIRD-PARTY-NOTICES.md` / `LICENSE`），法律要求保留。
- 机器把关：`python tools/check_secrets.py`（`violations: 0` 才算过；已配负向验证——
  往 `README.md` 塞一个 `glpat-` 开头的长串，扫描器必须报红）。
- 代价与边界：扫描器**豁免它自己**（判据的正则与"应当命中的示例串"都写在它里面）。
  往那个文件里硬塞密钥是扫不出来的——它是最显眼的一个文件，评审看它比看门禁靠谱。
- **万一已经泄了**：先作废那把凭据，再清历史；删文件不算修。

## 二、ABI 版本冻结（改 = 事故，须用户拍板）

- **现状（实读）**：`IV_ABI_VERSION = 6`（`abi/src/lib.rs`），abi crate 版本 **0.6.0**；
  下界仍是 `MIN_ABI_VERSION = 5`（v5 是签名冻结下界，v6 只做加法 ⇒ v5 老 DLL 不重编照常收载）。
  发布产物 **8 档**（abi6）：`common` = **0.2.4**（24 后缀）、`modern` = **0.3.2**（9）、
  `raw` = **0.5.3**（40，含 rawpx 并入）、`pro` = **0.4.2**（16）、`classic` = **0.1.0**（20）、
  `texture` = **0.1.0**（8）、`doc` = **0.1.0**（30）、`doc-legacy` = **0.1.0**（5）。
  单库 **33 枚**住 `workspace/singles/`（不发布）；`pro-sci` / `pro-dicom` 暂无发布档认领。
  **七枚发布 DLL 纯 Rust**；`doc-legacy` 链静态 C++（libcdr/libwpg/libzmf + librevenge + ICU）。
  单库 `workspace/singles/core` 默认 `webp-c` 仍链 `libwebp-sys2`（C，仅 bench/A-B 用，不进发布档）。
- **宿主收载是区间判定**：`MIN_ABI_VERSION(=5) ≤ 解码器 abi ≤ IV_ABI_VERSION`，出界一律拒载。
- **★ 兼容性立约（永久条款，2026-10-28 用户拍板）**：**ABI 永远兼容旧版本 DLL；
  新 ABI 不得影响旧 DLL 的运行** ⇒ 宿主升级后，用户机器上的旧 DLL **不需要重编、
  不需要重传**。四条不可违反：(a) 已发布导出名/参数/返回类型、`#[repr(C)]` 结构体
  字段、`ERR_*`/`PF_*`/`CAP_*`/`REQ_*` 编号**只增不改号**；(b) 新能力只走「新可选导出 +
  新结构体 + 新位/新值」，宿主探测缺席 = 缺能力、不是错误；(c) `MIN_ABI_VERSION`
  **永不抬**（现 5），`IV_ABI_VERSION` 只增不减；(d) 宿主不得因升级改变既有导出的
  调用语义。**"改结构体然后 bump"这条路已禁止**（它会作废全部旧 DLL）。
  机器门禁：`tools/check_abi_freeze.py` + 冻结基线 `tools/abi_freeze.txt`
  （导出签名 / 结构体字段 / 布局 / 常量值 / 版本单调），已接进 `workspace/build.ps1`；
  **改既有条目即红灯**，新增条目绿灯并提示 `--update` 重新冻结。
- **冻结规则**：不改结构体布局、不改错误码编号、不改既有字段语义与像素格式取值；
  新增**可选导出** / 新结构体 / 新位值一律不 bump 版本（纯加法）；**「改既有条目 + bump
  重发」这条路已按上面的兼容性立约禁止**（唯一历史断点是 v5 改名）。
  布局靠 `size_of` 编译期断言钉死，**禁止任何手抄副本**。
- **第二条可选导出（2026-10-18）**：`iv_caps()` 返回**解码能力位图** `CAP_*`（规范 §十三）。
  宿主从此按 `(后缀, 解码类型)` 路由，`ext_claims_are_unique` 的归属判据也跟着升级——
  同一后缀由同一枚解码器按类型分派属正当，不是"重叠认领"。**结构体仍然一个字都不许加**
  （实测：往末尾追加字段会让宿主对旧 DLL 越界读，12/12 读到相邻 `.data` 的垃圾值）。
- **第三条可选导出（2026-10-26）**：`iv_metadata()` 返回可扩展键值表 `IvMetadata`；新字段只追加 key/value，不改 `IvDecoderInfo`、不 bump ABI。`version` / `author` / `license` / `released` 键分别给出 DLL 版本、作者、许可证与发布日期（规范 §十四）。
- **第四类能力：ABI v6 区域渲染（2026-10-28 起）**：新增可选导出 iv_render_region、IvRegionReq、CAP_REGION。iv_export_svg 只作为冻结历史符号保留，宿主已不再调用；CAP_VECTOR 不再与它绑定。区域语义、字体硬要求、默认接格式策略、验收清单见 docs/vector-decoder.md；**矢量解码器不需要改宿主**。
- **契约交付 = 快照机制**（拆仓后唯一的同步面，**没有同步脚本**）：改完 `abi/src/lib.rs`，
  把它**逐字节复制**到 RuPix 宿主仓库的 `main/vendor/rupix-abi/src/lib.rs`（覆盖，不编辑）。
  宿主仓库在哪里由维护者掌握，本仓库不写死路径，也不放任何往宿主里写的脚本。
  守卫：宿主侧 `core::abi::tests::abi_snapshot_matches_upstream_when_present`（两份都在时逐字节比对，
  不一致直接把宿主测试跑红）。**不许手改宿主那一份**，也不许改完契约不刷快照。
- **改 ABI 必须同步宿主注释**（两处，缺一即文档漂移）：`<宿主>/src/core/decode.rs` 的
  `DecodeBackend` trait 文档（消费方定义接口 / `decoders::Host` 实现 /
  `install_backend` 装入）与 `src/decoders/mod.rs` 的「DecodeBackend：core 调度接口的解码器层实现」节注释。
- 同一改动内还要同步：本仓库 `解码器开发规范.md`（§三～§五、§九版本策略）、
  `<宿主>/AGENTS.md` §五、`<宿主>/docs/架构总览.md` —— 对照表见 `<宿主>/开发指南.md` §二「改解码器 ABI」行。
- 验收：本仓库 `build.ps1` 全绿 + 宿主 `cargo test --lib core::abi` 全绿。

## 三、产物与发布（唯一落点）

- 一键：`powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1`
  = `cargo fmt --check` → `check_docs.py` + `check_history.py` + `check_abi_refs.py`
  + `check_workspace_config.py` + `check_deps.py` + `check_deps_direction.py`
  + `check_caps.py` + `check_claims.py` + `check_secrets.py` + `check_licenses.py`
  + `testdata/make_samples.py --check`
  → `cargo clippy --all-targets --workspace --release -- -D warnings`
  → `cargo build --release --workspace` → `cargo test --release --workspace -- --test-threads=1`
  → **任一门禁失败不发布**（fmt 检查 2026-10-17 补入：§七 早已列"格式化 exit 0"，但此前
  没有任何一步真的执行它，导致 `abi/src/lib.rs` 的漂移无人拦——而那个文件一漂移，
  宿主逐字节快照就脱钩。2026-10-20 m61 把 clippy 与两条 python 门禁也照同一办法接进
  build.ps1：**写在表里而没人执行的门禁不是门禁**——那天 clippy 全仓一条 deny-by-default
  的 error 都没过，文档锚点已经漂了好几轮）。
- 脚本入口先 dot-source `workspace/tools/toolchain.ps1`：固定 `RUSTUP_TOOLCHAIN`（本机 rustup
  无默认，K31）、`vswhere` 定位 `vcvars64.bat`、导出 `NASM_PREFIX` / `CMAKE_GENERATOR`、
  **拦 `CC=gcc`**（MSVC/MinGW 混链 = K06）；版本报告落 `<target-dir>/toolchain_report.txt`。
- **形态 B（原生 C/C++ DLL）识别**：`workspace/decoders/<id>/CMakeLists.txt` 存在 → 视为形态 B，
  走 `Import-VcVars64` + `cmake -G "NMake Makefiles"` 子构建（产物落 `<target-dir>/cbuild/<id>`），
  有 `ctest` 注册就跑 ctest 作发布门禁；版本从 `project(<id> VERSION x.y.z)` 读。
  形态 B 解码器在场时 vcvars64 / nasm / cmake 缺一即**立即报错**，不许退化到 MinGW 或静默跳过。
- **缺产物 = 硬错误**：`workspace/decoders/<id>/` 在 `<target-dir>/release` 里没有对应 DLL 时直接 `exit 1`
  （旧行为是 warning 后静默继续——静默丢一个 DLL 等于运行时静默丢一个格式）。
- **体积记账（无门禁）**：发布末尾打印逐 DLL 体积表与总量，**仅记账不拦**。
  D1 原为「60 MB 超限非零退出」，**2026-10-18 用户拍板推翻：解码器 DLL 没有总体积预算、无上限**。
- **单 DLL 构建（2026-10-28 用户口径）**：日常只改哪个档，就只构建/测试那个档，禁止用
  `--workspace` 或整包 `build.ps1` 代替单档构建；这样未改的 DLL 不会被重新链接、hash 不会漂。
  `common` / `modern` / `raw` 在 `workspace/` 下跑 `cargo build --release -p rupix-<id>`，
  `pro` 在 `workspace/decoders/pro/` 下跑 `cargo build --release -p rupix-pro`；
  测试把 `build` 换成 `test -- --test-threads=1`。`build.ps1` 只在用户明确要求全量发版时用。
  单档构建后只替换 `.build/decoders/<id>_<ver>.dll` 并只重打该档 ZIP；不要重打其他档。
- 发布目录**只有** DLL（在 `../.temp/decoder-dist`）；中间产物在 `<target-dir>/release/<id>.dll`（`<target-dir>` =
  `../../.temp/decoder-target`，由 `build.ps1` 从 `workspace/.cargo/config.toml` 解析，不在源码目录里），
  脚本改名后拷出。
  宿主 `.build/decoders/` 是**可选镜像**（两仓库并排的开发形态）：**先全部发布、最后统一
  prune** 同 id 旧文件（不在循环里删——删除失败不应中断一批产物已经正确的构建），
  宿主目录不存在时静默跳过——本仓库的构建不依赖它。
- 命名：`<id>_<ver>.dll`；`<id>` = **目录名 = `[lib] name`**，
  `version` 从各 `workspace/decoders/<id>/Cargo.toml` 正则实读（不走 workspace 继承）。
- **同 id 只留一份 DLL**：升版本后手工删发布目录里的旧文件——
  Windows 不允许覆盖已加载 DLL，留两份会让宿主重复加载同 id 解码器。
- 无清单文件随 DLL 走：宿主扫全部 `*.dll` 读 `iv_decoder_info()` 自述。唯一的清单是
  `decoders.json`（宿主「可下载」卡读的那份，由 `tools/make_index.py` **从发布目录里的 DLL 自述**
  生成，id/abi 与文件名不符即硬报错、不落盘；同 id 出现两份构建也硬报错）。
  **顶层就是一个数组，一项一枚 DLL**，十三个字段：`id` / `name`（自述里的解码器名，不是 DLL
  文件名）/ `version` / `abi` / `size` / `sha256` / `released` / `desc` / `formats`（分号分隔、不带点，
  与 ABI `extensions` 同形）/ `author`（来自 DLL `iv_metadata()`）/ `license`（同上）/ `gitee` / `github`。2026-10-26 精简掉的东西——`schema` /
  `updated` / `decoders` 包裹层、`file`（能从 id+version 算）、`homepage` /
  `minHost` / `tag` / `notes`。`sha256` 与 `size` 并用：size 挡截断/多收，sha256 挡
  同字节数替换；宿主下载后两者都核对。
  下载地址是 **ZIP release 资产**（`.../releases/download/<tag>/<id>_<version>.zip`），tag = `<id>_<version>`
  （每枚一个 release）。`gitee` / `released` 由人填、生成器**原样保留**；`github` 默认由
  `gitee` 推导（`derive_github_url`：**主机名与 owner 一起换**——Gitee `qiuzongman` /
  GitHub `zongmanqiu` 不是同一用户名），可用 `--set-github-url` 覆盖。GitHub 侧也要上传
  同一批 release；资产由人工上传，本地不做网络校验。缺 `gitee` 的项在收尾
  列成 `MISSING gitee`，`released` 也随之为空。
- **发布清单 = `workspace/decoders/` 下的目录，别的不算**：`build.ps1` 按
  `workspace/decoders/*` 枚举，`make_index.py` 也只收这里存在的 id。这里**只放档**
  （一个文件夹一枚发布 DLL）；单库住 `workspace/singles/`，**永远不进发布目录**。曾有的测试夹具 `test-decoders/testred`
  （认领 `.png` 渲 1×1 纯红，用于宿主首选路由端到端）已于 2026-10-16 按用户要求移除——
  首选路由的正向判定改由 `<宿主>/src/decoders/` 层（`select.rs` 的仲裁）的
  `preferred_candidate_overrides_auto_and_falls_back_on_stale` 单测承担。

## 四、基准样本

### gen_samples（3 MP，速度基线）

- 生成（单库 workspace）：`cd workspace/singles` → `cargo run --release --example gen_samples -p rupix-core -- <输出目录>`
- 2000×1500（3MP）程序化图 × `bench.{jpg,png,webp,tif,bmp,gif,ppm,tga}`，**样本不入库**。
- 默认输出目录指向**工作区根**（本仓库上一级）的 `.temp/samples`；消费方是宿主的真实打开路径
  （宿主 → 解码器 DLL → ABI，不是库函数直调）。
  基线与复测判据见本仓基准工具与各档 README（位图族 avg 不慢于基线 +5%）。
  **这是开发期联调路径，不是构建依赖**；独立克隆跑 bench 时显式传自己的目录。
- 性能结论必须按仓库口径来：**未测 ≠ PASS**，n ≥ 200、配对 run 内比较、发布配置与选型口径
  逐字一致（含 `opt-level`），尾部需倒序复跑。历史 runs 的台账留在本仓基准工具目录。

### testdata/make_samples.py（1/10/100 MiB，按后缀共享）

- 位置：`testdata/make_samples.py`。`testdata/` 有**两半**，划分标准是"能不能用脚本算出来"：
  `make_samples.py` 生成的 1/10/100 MiB 雪花样本 **不入库**（写 `../.temp/testdata/`）；
  `testdata/golden/` 里 **31 个** golden / 边界证物（**456 KB**）**入库**，一律 `include_bytes!` 编译期嵌入，
  引用路径见 `testdata/README.md`。入库判据不是"绝对造不出来"（实测：12 个真造不出——4 枚真机 RAW、
  5 枚 AVIF、2 枚 JXL、1 枚公版 DjVu；另 18 个能造 + 1 个 CC0 HEIC，只是需要冻结字节），而是**"最小样本 + 字节必须恒定"**：
  同一个字节永远对应同一个断言，换 Pillow 版本重生成就可能漂移、测试静默变松。
- **按后缀生成一份，不按解码器**：一个后缀只有一枚认领者（`check_deps.py` 守着），所以
  "JPEG 的测试图"对谁都是同一份字节；按解码器各存一份会让同一张 100 MB 图存在十几份并各自漂移。
  manifest 里仍记 `claimed_by`，只作"本该谁读"的元数据。别名后缀（`.jpe` 之于 `.jpg` 等）保留同一像素源，
  但每档用格式合法的尾注/元数据保证**字节长度唯一**（生成器末尾会硬检查同档大小冲突）。
- 从仓库根运行（本机 `python` 与 `py` 均为 3.11.3 + Pillow 11.1.0；仍用 `py` 保持口径）：
  ```text
  py testdata/make_samples.py                  # 全部后缀，默认 10M 档
  py testdata/make_samples.py --only .jpg,png  # 子集（后缀或解码器 id 都行）
  py testdata/make_samples.py --tier all       # 1M / 10M / 100M 三档
  py testdata/make_samples.py --list           # 打印计划表（不需要 Pillow）
  ```
- 产物：`<repo>/../.temp/testdata/<tier>/noise<ext>` + `manifest.json`（`RUPIX_SAMPLE_DIR` 可覆盖）。
  噪声（snow）是故意的——每种熵编码的最坏情况，用来压测解码器并暴露尺寸/内存门。
- **档位是下限**：装不下就写 `capped(...)` 给真实尺寸，不假装。物理上限包括
  ICO 256 px、PBM 1 bpp、BC1 0.5 B/px、GRAY8 1 B/px、GIF 8 bpp、4:2:x 1.5 B/px；
  各档实际数量以 `manifest.json` 为准，别在这里写死会过期的计数。
- **两个 gate**：解码器自身拒绝 >64 MP（各档 `const MAX_PIXELS`，由 `check_caps.py` 守），但宿主更早拒绝
  >41 MP 或 >164 MB RGBA（main/src/core/pixels.rs MAX_PIXELS = 41_000_000）。样本保持在宿主上限之下，
  保证每行都能被查看器真正打开；想探 64 MP 解码器门需用 ABI 直接调用（budget_bytes = 0）。
- 验证：宿主 `core_smoke.exe` 走真实用户路径（目录过滤 → 扩展名认领 → 两阶段渲染）。
  2026-10-25 实测：1M 档 **27 个非 rawpx 样本逐条 OK**（含改过尺寸的 100M 档 `.jpg` 5172²、
  `.webp` 5250²）。rawpx 的 11 个裸数据后缀没有文件头，走 `iv_render_raw` 另测；
  新增的 synthetic corpus（`testdata/generate_corpus.py`）另行验证。
  ★ 这类"当前全绿 N 个"的数字要么写实测日期、要么别写——它没有门禁守着，只写数字必然过期。

## 五、测试与红线

- **本机测试必须串行**：手动跑一律 `cargo test --release --workspace -- --test-threads=1`
  （杀软锁新链接二进制会假死；`build.ps1` 已带该参数——解码器的 `SESSIONS` 是进程内全局
  2 槽 LRU，并行跑会让两个用例互相挤掉会话，2026-10-18 就是这么把"顺序播放不填环"
  挤成假失败的。宿主侧同因，见 `<宿主>/build/5_test.bat` 注释）。
- `panic = "unwind"` **不许改 `abort`**：宿主靠 `catch_unwind` + C SEH 层隔离坏解码器，abort 会带走整个进程。
- 解码库热路径保持 `opt-level = 3`（singles workspace 默认 `z`，JPEG 3MP 会慢 4.6 倍），
  例外清单见 `workspace/singles/Cargo.toml`；发布 workspace 默认全局 `opt-level = 3`（`doc` 除外，用 `s`）。
  形态 A 的 C 侧由 `build.rs` 的 `cc .opt_level(3)` 单独管（Rust profile 管不到 cc）。
- 第三方 C 源一律放**仓库外**的 `../.3rd/<name>-<ver>/`，带 `LICENSE` + `SOURCE.md`
  （版本 / sha256 / 下载源），规则见 `.3rd/README.md`；
  接入新库要回写宿主 `<宿主>/THIRD-PARTY-NOTICES.md` §2.2。
- **对输入字节"推进且线性"**（防挂死，正本 `解码器开发规范.md` §六 第 10 条）：长度字段驱动的
  循环读到 0 长度必须推进或报错；递归 / 交叉引用用 visited 去重（只设深度上限不够，菱形重入是
  `N^层数`）；轮数上界取自文件长度而非文件内的计数。**挂死风险源是输入字节，不是解码器谁写的。**
- 测试代码与宿主隔离：解码器测试不打开宿主仓库；`expect` 属断言，不计入宿主「生产区零 panic 源」红线。

## 六、新增 / 改动一个解码器（顺序不可乱）

> **流程、发布顺序和提交前清单以根 [`开发指南.md`](开发指南.md) 为准**；本节保留目录落位、
> 单库/档接法与特殊形态的硬细节。
>
> **先回答"要不要动宿主"：不用**（三条代码级事实，非文档信念）——① 身份来自 DLL 自己：
> 宿主 `lib.get(b"iv_decoder_info")` 读自述拿 id / 名 / 扩展名（`main/src/decoders/` 里
> `Inspect` 装配那段 `lib.get(b"iv_decoder_info\0")`；按本文件 §六.8 只写名字锚点，不写行号——
> m117 复算时这里曾标 `:251`，实际代码早已搬到 291 行，正是那条规则要防的漂移）；
> ② **没有 id 白名单**：生产路径是 `read_dir` 扫目录 + 格式清单动态并集，硬编码解码器 id 只出现在
> `#[cfg(test)]` 断言里（`is_disabled(id)` 是**用户主动禁用集**，不在集合里就默认允许）；
> ③ 放进去不用重启：`decoders::request_rescan()`（同文件，符号名即锚点）由设置页扫描/刷新/卸载与
> `recovery_hint` 真实调用。⇒ **加格式 = 加一个 DLL，宿主一行不改、不重编。**

1. **单库住 `workspace/singles/<id>/`**：`cargo new --lib workspace/singles/<id>`，`[lib] name = "<id>"`、
   `crate-type = ["cdylib", "rlib"]`、`[package] version` **显式写出**；加进 `workspace/singles/Cargo.toml`
   的 `members`。目录名 = 解码器 id = lib name。★ id **不要带连字符**，且必须是无歧义短名。
2. 依赖 `rupix-abi`（bench workspace 继承）+ 解码库；bench 是单库口径，热路径在
   `workspace/singles/Cargo.toml` 加 `[profile.release.package.<依赖的 package 名>] opt-level = 3`。
   ★ profile 按 **`[package] name`** 匹配——`[lib] name` / 目录名只是解码器 id；写错时构建
   照样成功、只是**静默不生效**（K112）。
3. `src/lib.rs` 的形态是「薄壳 + 实现层」（改到哪一层看这里，别只改一半）：
   - `export_decoder! { id, name, extensions, description, is_vector, has_alpha }` 自述；
   - `pub fn probe_impl(..)` / `pub fn render_impl(..)`：业务全在这里，**`rupix_abi::guard(..)`
     留在 impl 内部**（档直接调 impl，不必再包一层）；
   - `#[cfg(feature = "standalone")] #[no_mangle] pub extern "C" fn iv_probe/iv_render`：
     只做转发。`[features] default = ["standalone"]`；档依赖时 `default-features = false`，
     否则多份 `#[no_mangle] iv_*` 在同一枚 DLL 里重名。
   无魔数 / 裸数据格式另提供 `iv_render_raw`（规范 §十二）。
4. **接进档**（发布物是档）：`workspace/decoders/<tier>/Cargo.toml` 加依赖
   `rupix-<id> = { path = "../../singles/<id>", default-features = false }`；
   `src/lib.rs` 的 `MEMBERS` 加一行；认领后缀写进该档 `export_decoder!` 的 `extensions`。
   若是新开一档，还要加进 `workspace/Cargo.toml` 的 `members` 并补 `workspace/decoders/<tier>/README.md`。
   ★ 档里**不写解码逻辑**——要换库、要改算法，去单库 crate 里改。
5. 每个单库带**盲点单测**：截断文件、畸形头、超 `budget_bytes`、`ext = NULL` 只按魔数认领
   （无魔数格式必须返回 0）、越界 / 菱形结构（见 §五）。**并且档里要有一条端到端**
   （探针 + 两阶段渲染 + 让位/路由，如 `workspace/decoders/common` 的
   `pro` 的 `psd_and_hdr_render_through_the_pro_entry_point`）——只验单库不知道档有没有接上。
   ★ 需要**样本文件**时放 `testdata/golden/`（全仓共享，按后缀共享），**不要**放进
   `workspace/singles/<id>/` 或 `workspace/decoders/<id>/`——同一后缀只有一枚认领者，换库时样本不该跟着丢；
   `include_bytes!` 用 `concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata/golden/<file>")` 引用。
6. `build.ps1` 全绿（并排开发时自动镜像宿主解码器目录）→ 宿主冒烟
   `cargo run --release --example decoder_smoke -- <dll>` / `core_smoke` / `--probe-decode`；
   单库自己的 workspace/singles/Example 从 `workspace/singles/` 目录跑。
7. 同一次改动内同步：`workspace/singles/<id>/README.md`（新增解码器就新写一份）、
   `workspace/singles/README.md` 总表、`workspace/decoders/<tier>/README.md`（成员表 + 认领后缀）、
   `workspace/decoders/<tier>/HISTORY.md`（对外可见变化一条）、`解码器开发规范.md` §八自查清单、
   根 `README.md` 的八档表、`docs/` 对应页（架构/清单/测试）、
   `testdata/claims.tsv`（新增认领必须登记成员与证据；没有证据就写 `unverified` + 原因）、
   宿主 `<宿主>/docs/架构总览.md` §四解码器矩阵、
   本文件第二节版本现状；最后 `py tools/make_index.py` 重生成清单。
8. **行锚怎么写才不会被搬漂**：`workspace/decoders/<id>/README.md` 里的 `:N-M` 只是给人导航的便利，**能站住的判据是名字**。
   写成 `` `fn_name`（`:120-135`）``，别裸写行号；改过某个 `src/lib.rs` 的行数，就把那一份的锚点重走一遍：
   - `python tools/check_docs.py` —— **硬门禁**（越界、整段空行、以及"文件里有裸 NUL 字节"）。
   - `python tools/check_anchors.py` —— **advisory，永远 exit 0**：它只交嫌疑名单。实测口径（m62，
     2026-10-20）：169 个可判锚报 48，其中 **26 个是真错**（已照地标改正）、22 个是"句子提到的符号
     定义在别处、锚指的是用它的那一行"这种假警报。收紧到"唯一符号 + 紧邻"后只剩 4 条报，4 条里仍有 2 条假。
     ★ 判不了就别装成判得了——把名单当结论会练出"忽略工具"的习惯，那比没有工具更糟。

### 特殊形态的额外步骤

| 形态 | 额外步骤 |
|---|---|
| **无魔数 / 裸数据**（`.raw`、`.yuv` 这类没有文件头的） | 提供 `iv_render_raw` **可选导出**，宽高 / 布局由宿主向用户收集后传入。规范 §十二，参考实现 `workspace/singles/rawpx`（现由 `raw` 档转发）。可选导出不 bump ABI 版本。 |
| **形态 A：内嵌单头 / 静态 C 库** | ① C 源放**仓库外** `../.3rd/<name>-<ver>/`，带 `LICENSE` + `SOURCE.md`（见 `.3rd/README.md`）；② `[build-dependencies] cc`；③ `build.rs` 里 **`opt_level(3)` 必须显式设**（Rust profile 管不到 cc）；④ 目标目录用 `ancestors()` 上溯到仓库上一级（`RUPIX_3RD_DIR` 可覆盖），**禁止 `canonicalize()`**（Windows verbatim `\\?\` 路径 cl.exe 不认，K34）；⑤ Rust 侧只写 `extern "C"` 薄封装。历史首例 `stb` 已于 2026-10-25 被纯 Rust 的 `psd`+`hdr` 替掉，当前无现役样本。 |
| **形态 B：原生 C/C++ DLL** | 目录里放 `CMakeLists.txt`（`project(<id> VERSION x.y.z)`），`build.ps1` 自动识别并走 CMake + ctest 子构建；版本从 `project()` 读。C 侧用 `abi/c/rupix_decoder_kit.h`（自带 `_Static_assert` 布局钉子，漂移直接编译失败），并跑 `abi/c/abi_probe.c` 复核（`cl /std:c11`，应打印 `info=48 req=24 out=48 abi=6`；
  探针 exe 落系统临时目录，别写进源码仓库）。 |
| **多帧动画**（GIF 类） | 实现 `frame_index` 帧选择 + `frame_count` / `frame_delay_ms` 回填（abi v3 语义）。 |
| **多页 / 子图**（TIFF 多页、ICO 尺寸档、MPO 双目、DDS mip） | 用 `page_index` 请求 + `page_count` 回填；ICC 走 `icc_len` 附加段（v4 引入，v5 保留）。 |

## 七、交付前核查（本仓库）

| 项 | 命令 | 判据 |
|---|---|---|
| 编译 + 测试 + 发布 | `build.ps1` | exit 0，打印 `done: N decoder(s) published to <path> (abi v5)`，末尾清单自检不报结构缺口 |
| 依赖门禁 | `python tools/check_deps.py` | `violations: 0` |
| 闸值门禁 | `python tools/check_caps.py` | `violations: 0`。管**等值**不管"哪个值才对"：① 各 crate 各自抄的 `const MAX_PIXELS`（64 MP，**分配闸**）必须同值——这条口径以前只靠注释互相引用（"与 core / png / jpeg / tiff 同口径"），没人读；② 宿主 `main/src/core/pixels.rs` 的 `MAX_PIXELS`（41 MP，**看图硬顶**）必须等于 `testdata/make_samples.py` 里那份镜像值（宿主不在这儿就打印 skip，不装成通过）。★ **同名两值是有意的**：两道闸管两件不同的事，改名统一会要重编 DLL 只换可读性，已判废（宿主 m120）；自证：`--selftest` |
| 认领门禁 | `python tools/check_claims.py` | `violations: 0`：各档的 `extensions:` 字面量两两不相交；每个后缀在 `testdata/claims.tsv` 恰好出现一次；`verified` 必须有 golden/测试证据，`unverified` 必须有原因 |
| 文档门禁 | `python tools/check_docs.py` | `missing: 0`（每个 `workspace/decoders/<id>/` 有 `README.md` + `HISTORY.md`、根 `解码器开发规范.md` 在、全仓相对链接可达、行数预算、`DOCS_MANIFEST` 覆盖、旧路径词、锚点不越界不落空行、**任何 .rs/.md/.ps1 文件里没有裸 NUL 字节、`.ps1` 全 ASCII**） |
| 历史门禁 | `python tools/check_history.py` | `violations: 0`（各档 HISTORY 存在；最新版本 = `Cargo.toml`；ABI = `IV_ABI_VERSION`；日期 = `decoders.json.released`；**禁 `状态`/待上传/发布包生成/未发布/release tag/仓库 URL**） |
| 依赖方向 | `python tools/check_deps_direction.py` | `violations: 0`（`cargo metadata` 判 abi 叶子、workspace/shared/router/workspace/shared/pro-support 只依赖 abi、单库不依赖档、档只组装单库） |
| 工作区配置 | `python tools/check_workspace_config.py` | `violations: 0`（三图 target-dir / `+crt-static` / release profile 不漂） |
| 秘密扫描 | `python tools/check_secrets.py` | `violations: 0`（公开仓无密钥/令牌/个人路径） |
| 许可证 | `python tools/check_licenses.py` | `violations: 0`（三图全部传递依赖都有许可，且 SPDX id 在允许表内） |
| 锚点嫌疑名单 | `python tools/check_anchors.py` | **advisory，恒 exit 0**；名单由人判，实测假警报 ~46%（见 §六 第 8 条） |
| 手动测试（档） | `cargo test --release --workspace -- --test-threads=1` | 全绿，用例数不低于上一版 |
| 手动测试（单库） | `cd workspace/singles` → `cargo test --release --workspace -- --test-threads=1` | 全绿；单库不发布，但档的成员全靠它们，必须同步验 |
| 产物 | 检查发布目录与宿主解码器目录 | 仅 `<id>_<ver>.dll`，同 id 一份，命名合规 |
| 清单 | `py tools\make_index.py` | 一项一枚 DLL（顶层就是数组）；id/abi/name/formats 与 DLL 自述逐一对齐、无重复 id，`decoders.json` 落盘；`gitee` 人工维护、`github` 由 gitee 推导（主机名 + owner）、缺 gitee 地址在收尾列成 `MISSING gitee` |
| 格式化 | `cargo fmt --check` | exit 0 |
| Lint | `cargo clippy --all-targets --workspace --release -- -D warnings` | exit 0。**必须带 `-D warnings`**：不带时 clippy 打完警告照样返回 0，等于没有门禁 |
| 宿主侧门禁（并排开发时） | `python <宿主>/build/layering_scan.py`、`python <宿主>/build/hygiene_scan.py` | 两条 `violations: 0` |

> 上表的格式化、文档/历史/依赖/方向/闸值/认领/秘密门禁、Lint **已由 `build.ps1` 自动执行**
> （文档与依赖门禁 2026-10-20 m61；历史/方向/秘密 2026-10-26）。单独跑也可以，它们是秒级/增量级的。
> `#[allow]` 只允许写在带理由的位置上，理由要么在该行注释里、要么在 `解码器开发规范.md` 有条文（现成的两例：§三《指针参数的所有权》
> 解释 DLL 边界导出为什么仍标 `#[allow(not_unsafe_ptr_arg_deref)]`，§六第 5 条解释
> rayon 的处置）。

> 依赖增删、ABI 变化、产物命名或落点变化 → 同步 `<宿主>/THIRD-PARTY-NOTICES.md`、
> `<宿主>/DOCS_MANIFEST.md` 登记行与本文件；ABI 语义变化另改 `解码器开发规范.md` 的版本史。
