# 不生成声明（Generation exceptions）

本文件是「RuPix 能解码、但合成语料生成器无法（或不应）生成」的格式的**唯一登记处**。
`corpus/registry.py::GENERATION_EXCEPTIONS` 镜像这些条目；有一条测试保证两边一致。

## 规则

- **不要**用 0 字节文件、改名的容器或伪造样本来补一个缺失的后缀。
- 没有公开样本的格式，查看器照样保持支持。
- 公开语料只收「生成出来的字节」或「有明确再分发许可的样本」，优先 CC0。
- 仅本地样本可以用于测试，但不得提交进公开语料，也不得被公开下载器引用。
- 每新增一条例外，必须同时写进本文件与 `corpus/registry.py::GENERATION_EXCEPTIONS`；
  生成器随后会报告原因，而不是去调用一个不存在的写入器。

## 当前例外

### `.crm` — Canon Cinema RAW Light

- **类别：** RAW 视频容器，不是静态图片格式。
- **查看器支持：** 保留——解码器与 CR3 共用 Canon CRAW 代码，可以输出第一帧。
- **为什么不生成：** 没有公开写入器；Canon Cinema RAW Light 是视频编解码/容器。
- **为什么没有公开样本：** 可拿到的 Canon/CineD/Korro/dnglab 样本仅限评估使用，
  或没有明确的再分发许可。
- **语料策略：** 仅本地；不进公开语料。

### `.qtk` — Apple QuickTake 100/150 RAW 静态图片

- **类别：** 旧式静态 RAW 格式。
- **查看器支持：** 保留，供仍持有原版 QuickTake 文件的用户使用。
- **为什么不生成：** 现有工具链里没有公开的 QTK 写入器。
- **为什么没有公开样本：** `qtkn_decoder` 的 QT150 样本没有明确的再分发许可；
  也没有找到 CC0 的 QTK 样本。
- **语料策略：** 仅本地；不进公开语料。

## 以后的格式

新增一个无法生成的格式时：

1. 把后缀与原因写进本文件。
2. 把同一后缀与原因写进 `corpus/registry.py::GENERATION_EXCEPTIONS`。
3. 在对应的 source manifest 里把该行标为 local-only。
4. 保留解码支持，并说明不随包提供公开样本。
5. 跑 `python -m unittest discover -s testdata/tests -v`，让「文档/登记表一致性」测试看到新条目。

这样公开语料才靠得住：每个发布文件要么是生成的，要么有可再分发的来源；
每条例外都写明理由。
