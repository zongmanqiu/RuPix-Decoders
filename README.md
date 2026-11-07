# RuPix 解码器仓库

> RuPix 看图器的**格式解码 DLL** 源码与发布清单。
> [**Gitee**](https://gitee.com/qiuzongman/RuPix-Decoders) | [**GitHub**](https://github.com/zongmanqiu/RuPix-Decoders)
>
> 普通用户不用碰这个仓库：宿主「设置 → 解码器 → 可下载」点「下载」即可。

## 发布 DLL

| DLL | 版本 | 认领后缀 | ZIP 大小 | 发行日期 | Gitee | GitHub |
|---|---:|---:|---:|---:|---|---|
| `common` | 0.2.6 | 24 | 0.95 MB | 2026-11-02 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/common_0.2.6/common_0.2.6.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/common_0.2.6/common_0.2.6.zip) |
| `modern` | 0.3.4 | 9 | 2.79 MB | 2026-11-04 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/modern_0.3.4/modern_0.3.4.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/modern_0.3.4/modern_0.3.4.zip) |
| `raw` | 0.5.4 | 40 | 3.61 MB | 2026-11-08 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/raw_0.5.4/raw_0.5.4.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/raw_0.5.4/raw_0.5.4.zip) |
| `pro` | 0.4.3 | 16 | 0.76 MB | 2026-11-02 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/pro_0.4.3/pro_0.4.3.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/pro_0.4.3/pro_0.4.3.zip) |
| `classic` | 0.1.0 | 20 | 0.37 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/classic_0.1.0/classic_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/classic_0.1.0/classic_0.1.0.zip) |
| `texture` | 0.1.0 | 8 | 1.07 MB | 2026-10-29 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/texture_0.1.0/texture_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/texture_0.1.0/texture_0.1.0.zip) |
| `doc` | 0.2.3 | 30 | 12.57 MB | 2026-11-04 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/doc_0.2.3/doc_0.2.3.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/doc_0.2.3/doc_0.2.3.zip) |
| `doc-legacy` | 0.2.0 | 5 | 14.91 MB | 2026-11-01 | [下载](https://gitee.com/qiuzongman/RuPix-Decoders/releases/download/doc-legacy_0.2.0/doc-legacy_0.2.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Decoders/releases/download/doc-legacy_0.2.0/doc-legacy_0.2.0.zip) |

表中 ZIP 大小为约值（1 MB = 1024² B，两位小数），版本与发行日期以
[`decoders.json`](decoders.json) 为准；字段与宿主消费见 [`docs/catalog.md`](docs/catalog.md)。
ABI 冻结 **v6**（v5 兼容：区间收载），旧 DLL 无需重编或重传。

## 仓库结构

```text
RuPix-Decoder/
├─ workspace/decoders/   八枚发布档（一览见 workspace/decoders/README.md）
├─ workspace/singles/    单库（装配来源，不发布）
├─ abi/                  C ABI 契约、kit 与 C 单头
├─ docs/                 贡献者长期文档（入口 docs/README.md）
├─ patches/              第三方补丁公开登记
├─ tools/                门禁 + 清单生成器 + 打包
├─ testdata/             认领登记与测试样本
├─ build.ps1             根构建入口
├─ decoders.json         可下载清单
└─ AGENTS.md             本仓库硬规则
```

## 文档

| 想知道 | 去哪 |
|---|---|
| ABI / 错误码 / 像素格式 / 能力位 | [`解码器开发规范.md`](解码器开发规范.md) |
| 新增、修改、发版、提交前检查的顺序 | [`开发指南.md`](开发指南.md) |
| 架构 / 构建 / 测试 / 发布 / 清单 / 许可 | [`docs/README.md`](docs/README.md) |
| 八档成员、认领后缀与历史 | [`workspace/decoders/README.md`](workspace/decoders/README.md) |
| 第三方补丁登记 | [`patches/README.md`](patches/README.md) |
| 全部文档的登记簿 | [`DOCS_MANIFEST.md`](DOCS_MANIFEST.md) |

## 构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

细节见 [`docs/building.md`](docs/building.md)。

## 许可证

本仓库 **AGPL-3.0**（[`LICENSE`](LICENSE)）；第三方归属见
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) 与 [`docs/licensing.md`](docs/licensing.md)。
