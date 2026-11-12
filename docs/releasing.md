# 发布

## 版本

| 事实 | 真源 |
|---|---|
| DLL 版本 | `workspace/decoders/<id>/Cargo.toml` 的 `[package] version`（同一值也写在 `src/lib.rs` 的 `export_metadata!`，宿主读后者；见 `开发指南.md` §二） |
| ABI 版本 | `abi/src/lib.rs` 的 `IV_ABI_VERSION`（当前 6） |
| 发布状态 | `decoders.json` 的 `released` / `gitee` / `github` |
| 对外变化 | `workspace/decoders/<id>/HISTORY.md` |

- 产物名：`<id>_<version>.dll`。**文件名不写 ABI 版本**——ABI 由 DLL 内
  `iv_abi_version()` 自报，写进文件名就是第二份真源，还能手改错。
- 每枚 DLL 一个 release、一个 tag：`<id>_<version>`（与资产文件同名）。
- 版本号语义见 [`history-policy.md`](history-policy.md)；ABI 变更必须用户拍板，
  且宿主与全部解码器同步重发。

## 发布流程

完整步骤、对象核对和文档同步见根 [`开发指南.md`](../开发指南.md) §四/§五。
本文只记必须遵守的机制：

1. `decoders.json` 是 ZIP 内 `metadata.json` 的真源，不是“打包后再补”的附属品。
2. 因此**先**用 `make_index.py --set-url` 定稿 `gitee` / `github` / `released`，**再**跑
   `package_decoders.py`。`package_decoders.py` 会拒绝 `released` / 地址为空的条目。
3. 打包后如果又改了 `decoders.json`，必须重新打包；旧 ZIP 不能改名复用。
4. 每枚 ZIP 上传到自己的 release：tag 与资产名都用 `<id>_<version>`。
   - Gitee：`https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/<id>_<version>/<id>_<version>.zip`
   - GitHub：`https://github.com/zongmanqiu/RuPix-Decoders/releases/download/<id>_<version>/<id>_<version>.zip`
5. 资产由人工上传；本仓不做联网地址校验。★ **源码与资产分工不同**：源码、清单、tag
   只推 Gitee，GitHub 是自动镜像；镜像不复制 release 对象，所以 ZIP 附件与说明正文必须在
   两边分别上传，GitHub 侧在镜像过来的那个 tag 上新建或编辑 release，不另建 tag。
   完整规则见根 [`开发指南.md`](../开发指南.md) §四.9。
6. 宿主侧刷新 `decoders.json` 镜像，验证「可下载」卡与安装。

★ **发布 ZIP 一律由 7-Zip 生成**（2026-11-04 起）：`package_decoders.py` 调 7-Zip 命令行
（`7z` / `7za` / `7zz`，PATH 里找不到时可用 `RUPIX_7Z` 指到可执行文件），参数是
`a -tzip -mx=9 -mtm=off -mta=off -mtc=off`。产物仍是**标准 ZIP / Deflate**、文件名仍是
`<id>_<version>.zip`，不是 `.7z`，宿主解压链路一字未改。换工具的理由只有一个：同样内容，
最高级 Deflate 实测比原先的默认级别更小（851 KB DLL：7-Zip 371,876 B，Python 默认 387,583 B，
省约 4%）。找不到 7-Zip 时脚本**直接报错退出，不静默回退**，前置依赖见
[`building.md`](building.md)。

★ **打包仍是确定性的**：7-Zip 从磁盘 mtime 取条目时间，所以 `pack_zip()` 在打包前把暂存
目录（含目录自身）统一压到 `1980-01-01`，并关掉高精度时间戳字段；同样内容连打两次逐字节
一致（实测）。这条**只服务"可复现"**：上传前重打一次就能确认"改的到底是内容还是时间戳"。
**不再服务对账**——2026-11-02 起清单与宿主都不比哈希（见 `docs/catalog.md`
「清单里没有哈希」），所以"这一版到底是哪枚包"由 `id` + `version` 认定，而不是字节。

★ **2026-11-04 已全量重打并上架**：本地 `../.temp/decoder-packages/` 的八枚 ZIP 均由 7-Zip
生成，并经 Gitee OpenAPI（`releases` + `attach_files`）传到各自的 `<id>_<version>` tag：
已有 tag 的替换同名附件，没有 tag 的由服务端顺带创建；**release 说明统一取该档
`HISTORY.md` 里对应版本那一条**，不另写一份。清单 `size` 记的是 **DLL 字节数**，与 ZIP
用哪个压缩器无关，所以切换打包器不触发数字改动。清单里 `github` 那列是另一处镜像，
**两个镜像必须分别上传，Gitee 上架不代表镜像同步**；2026-11-04 晚 GitHub 镜像也已对齐：
八枚同名 ZIP 与说明逐一对上（`modern_0.3.4` / `doc_0.2.3` 在已有 tag 上新建 release，
其余六个覆盖同名附件）。

> 清单可以先提交、资产后上传；那段时间该列地址会 404，宿主如实报下载失败。
> 反过来不行：`decoders.json` 不提交，宿主不知道有新版。

## 改了哪一层材料 ⇒ 要不要 bump / 重传 / 改清单

判据只有一句：**`id` + `version` 是身份，包内 DLL 的字节是行为；清单只记身份，不记字节。**

| 这一轮动的是 | bump 版本号 | 重传 release 资产 | `decoders.json` 的 `size` |
|---|---|---|---|
| 包外材料（release 说明文字、本仓 README 门面表的数字、`patches/` 补丁登记） | 不 | 不 | 不动 |
| 包内人读材料（档 `README.md`、`THIRD-PARTY-NOTICES.md`、`licenses/`） | 不 | **要**——用户拿到的那几个文件变了 | 不动：`size` 算的是 DLL |
| DLL 本体（代码、依赖、认领后缀、能力位） | **要** | 要 | 重算（`make_index.py`），且**只影响列表上那个"1.2 MB"** |

- 常见误判有两个方向："改了包里的说明文字所以必须升版本"——不必，版本号服务的是**解码行为**；
  "改了包里的说明文字所以远端那份包不能算这一版"——对，所以重传，但 tag 与文件名照旧。
- ★ **重编不改行为的，不用升版本**（2026-11-02 起）：以前"同版本必须同字节"会逼着为一次纯重编
  bump 版本号，好让清单的 `size`/`sha256` 描述真实资产；现在清单不记字节，重编只要行为没变，
  版本号就可以不动——列表上的 `size` 会暂时是重编前的数字，那只是显示看着旧，不影响任何判定。
- 对照另一仓：`RuPix-Plugin/` 的 `plugins.json` 同样不再有哈希（同日改），那边"改一个字就得重传
  并同步清单"的老说法也作废了：改包内文件要重传（用户拿到的文件变了），但不必改清单数字。
- ★ **发布物必须用带 remap 的入口构建**（2026-11-08 实测，纠正同日的错误结论）：裸
  `cargo build --release -p rupix-<id>` 也能出 DLL，但它没有 `workspace/tools/toolchain.ps1`
  设的那份 RUSTFLAGS（`+crt-static` + 两条 `--remap-path-prefix`）⇒ 二进制里烙着本机
  `%USERPROFILE%\...` 与仓库绝对路径，宿主 `6_deploy` 的 host path gate 会当场拒绝（实测
  `modern` / `raw` 两枚一起被拦）。同源码的两种构建连字节数都不同（`modern` 7,313,408 B
  vs 7,754,752 B），所以"看着能跑"不等于合格。
  单档发布前先拿到那份 RUSTFLAGS（`Import-Module workspace\tools\toolchain.ps1`），或直接走
  `build.ps1`；拿不准时以 `decoder-dist` 里 `deploy` 不报错的那一枚为准。
- 清单 `size` 与远端包内 DLL 仍允许差几千字节（重编漂移，只影响列表显示，不重传、不改版本）。

## 发布目录规则

- 仓库**只放源码**：`dist/` 不在仓库里；发布暂存 = `../.temp/decoder-dist`。
- 发布目录里只有 `<id>_<version>.dll`；中间产物在各自的 target。
- 同 id 只留一份 DLL：升版本后旧文件由 `build.ps1` 统一 prune。
  Windows 不允许覆盖已加载 DLL，留两份会让宿主重复加载同 id 解码器。
- pdb 归档到 `../.temp/symbols/decoders/`，**不进发布目录**；每个 id 留最近 3 版。
- 宿主镜像目录 `../.build/decoders/` 是可选落点；不存在时静默跳过，本仓构建不依赖它。

## 宿主兼容

- 宿主收载是区间判定：`MIN_ABI_VERSION(=5) ≤ 解码器 abi ≤ IV_ABI_VERSION`；
  低于下界或高于宿主一律拒载。
- 宿主按 `(后缀, 能力位图)` 选解码器，不按解码器身份；能力位由 DLL 的 `iv_caps()` 自报。
- 下载失败、字节数不符、ABI 越界都在宿主侧拒绝；本仓不做联网地址校验。

## 发布前清单

- [ ] `build.ps1` 全绿，打印 `done: N decoder(s) published ... (abi v6)`。
- [ ] 八枚 DLL 都在 `decoder-dist`，同 id 无旧版本。
- [ ] 打包机能找到 7-Zip（`7z` / `7za` 在 PATH，或已设 `RUPIX_7Z`）；发布 ZIP 由它生成。
- [ ] 每枚 DLL 的 Gitee / GitHub release 都已上传，tag 与资产名一致。
- [ ] `decoders.json` 的 `version` / `abi` / `formats` 与 DLL 自述一致（`size` 只用于显示）。
- [ ] `released` 已填；`gitee` / `github` 已由人工上传。
- [ ] ZIP 内 `metadata.json` 与 `decoders.json` 同一条目逐字段一致（含地址与日期）。
- [ ] 需要可复现时重打一次：同内容应得同字节（7-Zip + 固定时间戳）。**不记录也不核对哈希**——
      2026-11-02 起清单与宿主都不比哈希，这一栏没有"要填的数字"。
- [ ] release 说明写明许可证为 AGPL-3.0，并链接 README / `THIRD-PARTY-NOTICES.md`；ZIP 内附 `README.md`（含「许可与源码」一节）、`LICENSE`、notices、`licenses/`。
- [ ] `workspace/decoders/<id>/HISTORY.md` 已记录本次对外变化；**没有状态/待上传/包生成/tag 行**。
- [ ] 宿主 `docs_consistency` 与 smoke 已跑（并排开发时）。
