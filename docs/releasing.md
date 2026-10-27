# 发布

## 版本

| 事实 | 真源 |
|---|---|
| DLL 版本 | `decoders-rust/decoders/<id>/Cargo.toml` 的 `[package] version` |
| ABI 版本 | `abi/src/lib.rs` 的 `IV_ABI_VERSION`（当前 5） |
| 发布状态 | `index.json` 的 `released` / `gitee` / `github` |
| 对外变化 | `decoders-rust/decoders/<id>/HISTORY.md` |

- 产物名：`<id>_<version>.dll`。**文件名不写 ABI 版本**——ABI 由 DLL 内
  `iv_abi_version()` 自报，写进文件名就是第二份真源，还能手改错。
- 每枚 DLL 一个 release、一个 tag：`<id>_<version>`（与资产文件同名）。
- 版本号语义见 [`history-policy.md`](history-policy.md)；ABI 变更必须用户拍板，
  且宿主与全部解码器同步重发。

## 发布流程

完整步骤、对象核对和文档同步见根 [`开发指南.md`](../开发指南.md) §四/§五。
本文只记必须遵守的机制：

1. `index.json` 是 ZIP 内 `metadata.json` 的真源，不是“打包后再补”的附属品。
2. 因此**先**用 `make_index.py --set-url` 定稿 `gitee` / `github` / `released`，**再**跑
   `package_decoders.py`。`package_decoders.py` 会拒绝 `released` / 地址为空的条目。
3. 打包后如果又改了 `index.json`，必须重新打包；旧 ZIP 不能改名复用。
4. 每枚 ZIP 上传到自己的 release：tag 与资产名都用 `<id>_<version>`。
   - Gitee：`https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/<id>_<version>/<id>_<version>.zip`
   - GitHub：`https://github.com/zongmanqiu/RuPix-Decoder/releases/download/<id>_<version>/<id>_<version>.zip`
5. 上传后跑 `check_urls.py`；它会真下载 ZIP、解出 DLL 并核对 size/sha256。
6. 宿主侧刷新 `index.json` 镜像，验证「可下载」卡与安装。

> 清单可以先提交、资产后上传；那段时间该列地址会 404，宿主如实报下载失败。
> 反过来不行：`index.json` 不提交，宿主不知道有新版。

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
- 下载失败、字节数不符、ABI 越界都在宿主侧拒绝；本仓的 `check_urls.py` 只做发布前核对。

## 发布前清单

- [ ] `build.ps1` 全绿，打印 `done: N decoder(s) published ... (abi v5)`。
- [ ] 四枚 DLL 都在 `decoder-dist`，同 id 无旧版本。
- [ ] 每枚 DLL 的 Gitee / GitHub release 都已上传，tag 与资产名一致。
- [ ] `index.json` 的 `size` / `sha256` / `version` / `abi` / `formats` 与 DLL 自述一致。
- [ ] `released` 已填；`gitee` / `github` 都实测可下。
- [ ] ZIP 内 `metadata.json` 与 `index.json` 同一条目逐字段一致（含地址与日期）。
- [ ] 已记录最终 ZIP 的 size / SHA-256；重新打包后数字已更新。
- [ ] release 说明写明许可证为 AGPL-3.0，并链接 README / `THIRD-PARTY-NOTICES.md`；ZIP 内附 LICENSE、notices、licenses 与 `SOURCE-OFFER.md`。
- [ ] `check_urls.py` 绿。
- [ ] `decoders-rust/decoders/<id>/HISTORY.md` 已记录本次对外变化。
- [ ] 宿主 `docs_consistency` 与 smoke 已跑（并排开发时）。
