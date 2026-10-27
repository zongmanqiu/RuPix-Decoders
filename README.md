# RuPix 解码器仓库

> **仓库入口**：[**Gitee**](https://gitee.com/qiuzongman/RuPix-Decoder) | [**GitHub**](https://github.com/zongmanqiu/RuPix-Decoder)

RuPix 的位图解码全部在 DLL 里：宿主 `rupix.exe` 只做窗口、交互、几何变换、路由与缓存，
启动时扫描 exe 同级 `decoders/` 目录，按每个 DLL 的 `iv_decoder_info()` 自述认领后缀。
本仓库是这些 DLL 的源码、契约与构建入口；当前四档**纯 Rust**，无 C 编译依赖。

## 发布 DLL

| DLL | 版本 | 认领后缀 | Gitee | GitHub |
|---|---:|---:|---|---|
| `common` | 0.2.1 | 29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/common_0.2.1/common_0.2.1.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoder/releases/download/common_0.2.1/common_0.2.1.zip) |
| `modern` | 0.2.0 | 6 | [下载](https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/modern_0.2.0/modern_0.2.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoder/releases/download/modern_0.2.0/modern_0.2.0.zip) |
| `raw` | 0.5.0 | 40 | [下载](https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/raw_0.5.0/raw_0.5.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoder/releases/download/raw_0.5.0/raw_0.5.0.zip) |
| `pro` | 0.2.1 | 33 | [下载](https://gitee.com/qiuzongman/RuPix-Decoder/releases/download/pro_0.2.1/pro_0.2.1.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoder/releases/download/pro_0.2.1/pro_0.2.1.zip) |

- 四档合计 108 个后缀；ABI 冻结 **v5**。
- 安装：下载 `<id>_<version>.zip`，解压后把 DLL 和 `<id>_<version>/` 一起放进 `<exe>/decoders/`；
  `decoders/` 是用户目录，宿主只加载顶层 `*.dll`，许可证目录和其它文件不参与加载。
  ZIP 内的同版本目录含 `metadata.json`、`LICENSE`、`THIRD-PARTY-NOTICES.md`、`licenses/` 和 `SOURCE-OFFER.md`；
  源码获取/重链接说明见 `SOURCE-OFFER.md`。
- 下载：release 资产，每枚 ZIP 一个 `<id>_<version>` tag。机器可读清单是 [`index.json`](index.json)，
  `size`/`sha256` 描述 DLL，URL 指向 ZIP；字段、镜像与校验规则见 [`docs/catalog.md`](docs/catalog.md)。
- 不发布：`decoders-rust/singles/` 的 22 个单库只作 bench 对照、档的装配来源与 A/B 排查。

## 文档地图

| 想知道 | 去哪 |
|---|---|
| 修改、发版、提交前检查的完整顺序 | [`开发指南.md`](开发指南.md) |
| 解码器 ABI、错误码、像素格式、能力位 | [`解码器开发规范.md`](解码器开发规范.md) |
| 三个 workspace、档与单库、路由、ABI 快照 | [`docs/architecture.md`](docs/architecture.md) |
| 本机构建、门禁、profile、target 目录 | [`docs/building.md`](docs/building.md) |
| 版本、tag、双镜像上传、清单 | [`docs/releasing.md`](docs/releasing.md) |
| 测试分层、串行原因、样本与证据 | [`docs/testing.md`](docs/testing.md) |
| `index.json` 十三个字段与宿主消费 | [`docs/catalog.md`](docs/catalog.md) |
| 新增 / 修改一个解码器 | [`docs/contributing.md`](docs/contributing.md) |
| 每档历史怎么写 | [`docs/history-policy.md`](docs/history-policy.md) |
| 本仓专属坑 | [`docs/pitfalls.md`](docs/pitfalls.md) |
| 已采纳 / 已拒绝的架构决策 | [`docs/design-adoption.md`](docs/design-adoption.md) |
| 每档的成员、限制与历史 | [`decoders-rust/decoders/`](decoders-rust/decoders/README.md) |
| 22 个单库总表 | [`decoders-rust/singles/README.md`](decoders-rust/singles/README.md) |
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
- 每档的对外变化看 `decoders-rust/decoders/<id>/HISTORY.md`；ABI 版本史在 `解码器开发规范.md` §九。
- 发布状态以 `index.json` 的 `released` / `gitee` / `github` 为准，README 的表只是门面。

## 许可证

本仓库 **AGPL-3.0**。第三方库与许可证见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

解码器由宿主联网下载 ZIP；ZIP 内附 DLL、同版本目录中的 `LICENSE`、`THIRD-PARTY-NOTICES.md`、`licenses/` 和 `SOURCE-OFFER.md`。
仓库根 `LICENSE` 是 AGPL-3.0 全文，供源码使用者查阅。
DLL 通过可选导出 `iv_metadata()` 自带 `version` / `author` / `license` / `released`（键值表，不是许可证文件）；
第三方解码器可以把自己的 `author` 写成自己的名字。
