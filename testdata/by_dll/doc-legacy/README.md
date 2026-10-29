# doc-legacy 样本

`doc-legacy_0.1.0.dll` 只接 **CMX/CCX、WPG、ZMF、ZBR**。

| 格式 | 后缀 | 本地真实样本 |
|---|---|---|
| CMX | cmx/ccx | `../../../../.temp/by_dll/doc-legacy/cmx/test.cmx` |
| WPG | wpg | `../../../../.temp/by_dll/doc-legacy/wpg/AIRPLANE.WPG` |
| ZMF | zmf | `../../../../.temp/by_dll/doc-legacy/zmf/draw5.zmf` |
| ZBR | zbr | `../../../../.temp/by_dll/doc-legacy/zbr/zebra.zbr` |

这些样本目前只在本地 `.temp`，不进入仓库：CMX 来自公开样本站，WPG 来自
`libwpg-test`，ZMF/ZBR 来自 `libzmf` 测试集。

ZBR 解的是文件内 100×100 预览位图；ZBR 对象实体格式未公开，`libzmf` 上游
的 ZBR 解析器也不输出实体。

测试单枚 DLL：

```powershell
cd E:\tmp\AI.Agent\RuPix\main
$env:RUPIX_DECODERS_DIR='E:\tmp\AI.Agent\RuPix\.build\decoders'
cargo run --release --example decoder_ingest_probe -- "E:\tmp\AI.Agent\RuPix\.temp\by_dll\doc-legacy"
```
