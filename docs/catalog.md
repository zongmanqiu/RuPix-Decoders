# `decoders.json`

## 形状

顶层就是**一个数组**，一项一枚 DLL。没有 `schema` / `updated` / `decoders` 包裹层：
清单里只有一种东西，就只写这一种。

```json
[
  {
    "id": "common",
    "name": "Common Raster Set",
    "version": "<version>",
    "abi": 6,
    "size": 1234567,
    "released": "YYYY-MM-DD",
    "desc": "常见位图合集：…",
    "formats": "jpg;jpeg;…",
    "author": "RuPix",
    "license": "AGPL-3.0",
    "gitee": "https://…/<id>_<version>.zip",
    "github": "https://…/<id>_<version>.zip"
  }
]
```

## 十二个字段

| 字段 | 例 | 说明 |
|---|---|---|
| `id` | `common` | 解码器标识；宿主按它认人，不从文件名反推 |
| `name` | `Common Raster Set` | DLL 自述名称，不是 DLL 文件名 |
| `version` | `<version>` | DLL 版本；来自 `workspace/decoders/<id>/Cargo.toml` |
| `abi` | `6` | ABI 大版本；宿主按区间决定收不收 |
| `size` | `1234567` | 字节数；**只用于列表显示**，宿主不拿它做任何判定 |
| `released` | `YYYY-MM-DD` | 发布时间；未发布为空串 |
| `desc` | `常见位图合集：…` | 一句话说明，来自 DLL 自述 |
| `formats` | `jpg;jpeg;…` | 认领的扩展名，分号分隔、不带点，与 ABI `extensions` 同形 |
| `author` | `RuPix` | 作者标识；来自 DLL 的 `iv_metadata()`；第三方可署自己的名字 |
| `license` | `AGPL-3.0` | 许可证标识；来自 DLL 可选导出 `iv_metadata()` 的 `license` 键 |
| `gitee` | `https://…/<id>_<version>.zip` | Gitee 主源（ZIP）；未发布为空串 |
| `github` | `https://…/<id>_<version>.zip` | GitHub 镜像（ZIP）；默认由 `gitee` 推导，可显式覆盖 |

## 清单里没有哈希（2026-11-02）

以前这里有个 `sha256`，2026-11-02 起删掉了（用户口径：**官方产物不需要指纹**）。

理由两条，一条是"它挡的东西不存在"，一条是"它自己的维护成本很实在"：

1. 这些 DLL 全部由本仓 `build.ps1` 产出，release 资产只有作者能发。清单里那串十六进制
   防的是"第三方替换 / 镜像篡改"，而这条链路上没有第三方写入的入口，它只是在给自己的
   产物抄一份副本。
2. MSVC 每次链接都生成新的 PDB GUID，**同源码重编出来的 DLL 字节必变**（尺寸常常一模一样）。
   于是每次发布都得重算清单、重传资产，"编 → 改清单 → 又编一次"任何一次顺序搞错，清单就
   与已上传资产脱节，宿主那边报"大小不符"。假警报远比它挡住的真问题多。

身份判据收敛到 **`id` + `version`**：`version` 由 `Cargo.toml` 的 `[package] version` 盖进
DLL 文件名与自述（两处一起 bump），装完宿主还要读 `iv_decoder_info().id` 核对。装错一枚、
或装上一枚"其实不是我们发的"，这三处就够拦。

`size` 保留但只作显示（列表那列"1.2 MB"），不参与任何判定——同样因为重编会让它漂。

★ 宿主侧是同一条约定：`src/decoders/remote.rs` 与 `src/extensions/mod.rs` 里也没有哈希校验。
**改清单结构必须两边一起改**，否则读侧会静默丢掉整项。

## 两个 raw 镜像

- Gitee：`https://raw.giteeusercontent.com/qiuzongman/RuPix-Decoders/raw/master/decoders.json`
- GitHub：`https://raw.githubusercontent.com/zongmanqiu/RuPix-Decoders/refs/heads/master/decoders.json`

宿主「解码器 / 可下载」卡读其中一份，按当前来源取 `gitee` 或 `github` 列。
两个 owner 不是同一个用户名（Gitee `qiuzongman` / GitHub `zongmanqiu`），
推导 GitHub 地址时**主机名与 owner 一起换**。

## 谁生成、谁维护

- `tools/make_index.py` 从 `../.temp/decoder-dist` 的 DLL 自述生成结构字段；`size` 是 DLL 的，下载 URL 指向 ZIP：
  `id` / `name` / `version` / `abi` / `size` / `desc` / `formats` / `author` / `license`。
- `gitee` 与 `released` 由人维护，生成器原样保留；缺 `gitee` 的项在收尾列成
  `MISSING gitee`，`released` 随之为空。
- `github` 默认由 `gitee` 推导；特殊地址用 `--set-github-url` 覆盖。
- 文件名不入库：由 `id` + `version` 算出 `<id>_<version>.dll`。

## 生成与核对

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1   # 先有 DLL
py tools/make_index.py                                        # 生成 decoders.json
py tools/make_index.py --set-url '<id>=https://…/<id>_<version>.zip'
py tools/make_index.py --set-github-url '<id>=https://…/<id>_<version>.zip'
```

发布 ZIP 由人工上传；本仓不检查下载地址是否可访问，也不把网络状态纳入门禁。

## 宿主怎么消费

1. 宿主从 raw 镜像拉清单，只读当前来源的地址列。
2. 下载后不按 `size` 比对：它只用于列表显示，包身份由 `id` + `version` 认定。
3. 安装到 `<exe>/decoders/<id>_<version>.dll`；宿主扫描目录，读 DLL 自述。
4. `released` 为空 = 未发布；对应列也为空，宿主不会给一个点了 404 的按钮。
5. 清单可以先提交、资产后上传；那段时间地址会 404，宿主如实报下载失败。
   反过来不行：`decoders.json` 不提交，宿主不知道有新版。

## 发布前核对

- [ ] `size` 与 `decoder-dist` 里实际 DLL 一致（重新链接会变，必须重跑生成器）。
- [ ] 八档 id 无重复；`id` 与文件名、DLL 自述一致。
- [ ] `abi` 在宿主接受区间内（`MIN_ABI_VERSION ≤ abi ≤ IV_ABI_VERSION`）。
- [ ] `released` 只在人工上传资产后才填。
- [ ] `gitee` / `github` 已由人工上传。
- [ ] 同 id 在发布目录与宿主镜像目录都只留一份。
