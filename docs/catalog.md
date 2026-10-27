# `index.json`

## 形状

顶层就是**一个数组**，一项一枚 DLL。没有 `schema` / `updated` / `decoders` 包裹层：
清单里只有一种东西，就只写这一种。

```json
[
  {
    "id": "common",
    "name": "Common Raster Set",
    "version": "0.2.0",
    "abi": 5,
    "size": 1968128,
    "released": "2026-10-26",
    "desc": "常见位图合集：…",
    "formats": "jpg;jpeg;…",
    "author": "RuPix",
    "license": "AGPL-3.0",
    "gitee": "https://…/common_0.2.1.zip",
    "github": "https://…/common_0.2.1.zip"
  }
]
```

## 十三个字段

| 字段 | 例 | 说明 |
|---|---|---|
| `id` | `common` | 解码器标识；宿主按它认人，不从文件名反推 |
| `name` | `Common Raster Set` | DLL 自述名称，不是 DLL 文件名 |
| `version` | `0.2.0` | DLL 版本；来自 `decoders-rust/decoders/<id>/Cargo.toml` |
| `abi` | `5` | ABI 大版本；宿主按区间决定收不收 |
| `size` | `1967104` | 字节数；下载后先按它挡截断/多收 |
| `sha256` | `e3b0…` | DLL 的 SHA-256；与实收字节逐项核对，挡同字节数替换 |
| `released` | `2026-10-26` | 发布时间；未发布为空串 |
| `desc` | `常见位图合集：…` | 一句话说明，来自 DLL 自述 |
| `formats` | `jpg;jpeg;…` | 认领的扩展名，分号分隔、不带点，与 ABI `extensions` 同形 |
| `author` | `RuPix` | 作者标识；来自 DLL 的 `iv_metadata()`；第三方可署自己的名字 |
| `license` | `AGPL-3.0` | 许可证标识；来自 DLL 可选导出 `iv_metadata()` 的 `license` 键 |
| `gitee` | `https://…/common_0.2.1.zip` | Gitee 主源（ZIP）；未发布为空串 |
| `github` | `https://…/common_0.2.1.zip` | GitHub 镜像（ZIP）；默认由 `gitee` 推导，可显式覆盖 |

## 两个 raw 镜像

- Gitee：`https://raw.giteeusercontent.com/qiuzongman/RuPix-Decoder/raw/master/index.json`
- GitHub：`https://raw.githubusercontent.com/zongmanqiu/RuPix-Decoder/refs/heads/master/index.json`

宿主「解码器 / 可下载」卡读其中一份，按当前来源取 `gitee` 或 `github` 列。
两个 owner 不是同一个用户名（Gitee `qiuzongman` / GitHub `zongmanqiu`），
推导 GitHub 地址时**主机名与 owner 一起换**。

## 谁生成、谁维护

- `tools/make_index.py` 从 `../.temp/decoder-dist` 的 DLL 自述生成结构字段；`size`/`sha256` 是 DLL，下载 URL 指向 ZIP：
  `id` / `name` / `version` / `abi` / `size` / `sha256` / `desc` / `formats` / `author` / `license`。
- `gitee` 与 `released` 由人维护，生成器原样保留；缺 `gitee` 的项在收尾列成
  `MISSING gitee`，`released` 随之为空。
- `github` 默认由 `gitee` 推导；特殊地址用 `--set-github-url` 覆盖。
- 文件名不入库：由 `id` + `version` 算出 `<id>_<version>.dll`。
- `size` 与 `sha256` 并用：size 挡截断/多收，sha256 挡同字节数替换；宿主下载后两者都核对。
  `file` 仍不存：文件名由 `id` + `version` 算出。

## 生成与核对

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1   # 先有 DLL
py tools/make_index.py                                        # 生成 index.json
py tools/make_index.py --set-url common=https://…/common_0.2.1.zip
py tools/make_index.py --set-github-url common=https://…/common_0.2.1.zip
py tools/check_urls.py                                        # 有网时逐条下载核对
```

`check_urls.py` 把清单里的地址真下载一遍，核对 `Content-Length` 与实收字节数是否等于
清单 `size`；404 或对不上即红。未发布条目只提示，不判红。它是**发布门禁**，不在
`build.ps1` 的离线路径里。

## 宿主怎么消费

1. 宿主从 raw 镜像拉清单，只读当前来源的地址列。
2. 下载后按 `size` 比对；不符即拒绝安装。
3. 安装到 `<exe>/decoders/<id>_<version>.dll`；宿主扫描目录，读 DLL 自述。
4. `released` 为空 = 未发布；对应列也为空，宿主不会给一个点了 404 的按钮。
5. 清单可以先提交、资产后上传；那段时间地址会 404，宿主如实报下载失败。
   反过来不行：`index.json` 不提交，宿主不知道有新版。

## 发布前核对

- [ ] `size` 与 `decoder-dist` 里实际 DLL 一致（重新链接会变，必须重跑生成器）。
- [ ] 四档 id 无重复；`id` 与文件名、DLL 自述一致。
- [ ] `abi` 在宿主接受区间内（`MIN_ABI_VERSION ≤ abi ≤ IV_ABI_VERSION`）。
- [ ] `released` 只在资产真的可下后才填。
- [ ] `gitee` / `github` 都实测可下；`check_urls.py` 绿。
- [ ] 同 id 在发布目录与宿主镜像目录都只留一份。
