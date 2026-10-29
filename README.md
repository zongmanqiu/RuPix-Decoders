# RuPix 解码器仓库

> **仓库入口**：[**Gitee**](https://gitee.com/qiuzongman/RuPix-Decoders) | [**GitHub**](https://github.com/zongmanqiu/RuPix-Decoders)

## 发布 DLL

| DLL | 版本 | 认领后缀 | ZIP 大小 | 发行日期 | Gitee | GitHub |
|---|---:|---:|---:|---:|---|---|
| `common` | 0.2.4 | 24 | 0.96 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/common_0.2.4/common_0.2.4.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/common_0.2.4/common_0.2.4.zip) |
| `modern` | 0.3.2 | 9 | 2.90 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/modern_0.3.2/modern_0.3.2.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/modern_0.3.2/modern_0.3.2.zip) |
| `raw` | 0.5.3 | 40 | 3.74 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/raw_0.5.3/raw_0.5.3.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/raw_0.5.3/raw_0.5.3.zip) |
| `pro` | 0.4.2 | 16 | 0.79 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/pro_0.4.2/pro_0.4.2.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/pro_0.4.2/pro_0.4.2.zip) |
| `classic` | 0.1.0 | 20 | 0.39 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/classic_0.1.0/classic_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/classic_0.1.0/classic_0.1.0.zip) |
| `texture` | 0.1.0 | 8 | 1.12 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/texture_0.1.0/texture_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/texture_0.1.0/texture_0.1.0.zip) |
| `doc` | 0.1.0 | 30 | 12.86 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/doc_0.1.0/doc_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/doc_0.1.0/doc_0.1.0.zip) |
| `doc-legacy` | 0.1.0 | 5 | 16.06 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/doc-legacy_0.1.0/doc-legacy_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/doc-legacy_0.1.0/doc-legacy_0.1.0.zip) |

- 表中 ZIP 大小为约值（1 MB = 1024×1024 B，四舍五入两位）；精确字节以实际资产为准，
  发行日期 = `decoders.json.released`，版本与发布状态以 `decoders.json` 为准。
- 八枚合计 152 个后缀；ABI 冻结 **v6**（v5 兼容：区间收载）。
- **兼容性立约**：ABI 永远兼容旧版本 DLL——宿主升级后，已装好的旧 DLL **不需要重新编译、
  也不需要重新上传**，照常加载工作；新能力只以「新可选导出 + 新结构体」加法形式出现。
- 安装：下载 `<id>_<version>.zip`，解压后把 DLL 和 `<id>_<version>/` 一起放进 `<exe>/decoders/`；
  `decoders/` 是用户目录，宿主只加载顶层 `*.dll`，许可证目录和其它文件不参与加载。
  ZIP 内的同版本目录含 `metadata.json`、`LICENSE`、`THIRD-PARTY-NOTICES.md`、`licenses/` 和 `SOURCE-OFFER.md`；
  源码获取/重链接说明见 `SOURCE-OFFER.md`。
- 下载：release 资产，每枚 ZIP 一个 `<id>_<version>` tag。机器可读清单是 [`decoders.json`](decoders.json)，
  `size`/`sha256` 描述 DLL，URL 指向 ZIP；字段、镜像与校验规则见 [`docs/catalog.md`](docs/catalog.md)。
- 不发布：`workspace/singles/` 的单库只作 bench 对照、档的装配来源与 A/B 排查（总表见 [`workspace/singles/README.md`](workspace/singles/README.md)）。

## 文档地图

| 想知道 | 去哪 |
|---|---|
| 修改、发版、提交前检查的完整顺序 | [`开发指南.md`](开发指南.md) |
| 解码器 ABI、错误码、像素格式、能力位 | [`解码器开发规范.md`](解码器开发规范.md) |
| 七个 workspace、档与单库、路由、ABI 快照 | [`docs/architecture.md`](docs/architecture.md) |
| 本机构建、门禁、profile、target 目录 | [`docs/building.md`](docs/building.md) |
| 版本、tag、双镜像上传、清单 | [`docs/releasing.md`](docs/releasing.md) |
| 测试分层、串行原因、样本与证据 | [`docs/testing.md`](docs/testing.md) |
| `decoders.json` 十三个字段与宿主消费 | [`docs/catalog.md`](docs/catalog.md) |
| 新增 / 修改一个解码器 | [`docs/contributing.md`](docs/contributing.md) |
| 每档历史怎么写 | [`docs/history-policy.md`](docs/history-policy.md) |
| 本仓专属坑 | [`docs/pitfalls.md`](docs/pitfalls.md) |
| 已采纳 / 已拒绝的架构决策 | [`docs/design-adoption.md`](docs/design-adoption.md) |
| 每档的成员、限制与历史 | [`workspace/decoders/`](workspace/decoders/README.md) |
| 未来格式规划、目标 DLL 划分 | [`解码器能力规划.md`](解码器能力规划.md) |
| 单库总表 | [`workspace/singles/README.md`](workspace/singles/README.md) |
| 仓库硬规则 | [`AGENTS.md`](AGENTS.md) |

## 构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

构建中间产物落仓库外 `../.temp/decoder-target`（`singles` / `pro` 各有自己的 target），
发布暂存落 `../.temp/decoder-dist`，并排开发时自动镜像到宿主 `../.build/decoders`。
命令、门禁顺序与故障排查见 [`docs/building.md`](docs/building.md)。

## 版本与历史

- DLL 版本看各档 `Cargo.toml` 的 `[package] version`；ABI 版本看 `abi/src/lib.rs` 的 `IV_ABI_VERSION`。
- 每档的对外变化看 `workspace/decoders/<id>/HISTORY.md`；ABI 版本史在 `解码器开发规范.md` §九。
- 版本、后缀数、发布状态以 `decoders.json` 为准（版本源头是各档 `Cargo.toml`）；README 的表只是门面。

## 许可证

本仓库 **AGPL-3.0**。第三方库与许可证见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

解码器由宿主联网下载 ZIP；ZIP 内附 DLL、同版本目录中的 `LICENSE`、`THIRD-PARTY-NOTICES.md`、`licenses/` 和 `SOURCE-OFFER.md`。
仓库根 `LICENSE` 是 AGPL-3.0 全文，供源码使用者查阅。
DLL 通过可选导出 `iv_metadata()` 自带 `version` / `author` / `license` / `released`（键值表，不是许可证文件）；
第三方解码器可以把自己的 `author` 写成自己的名字。
