# doc 样本

`doc_0.1.0.dll` 当前支持 **30 个后缀 / 11 个格式组**：

| 格式族 | 后缀 | 可入库样本 | 本地真实样本 |
|---|---|---|---|
| CAD | dwg/dwt/dxf | `cad/simple.dxf` | `../../../../.temp/by_dll/doc/cad/QuickViewLogo.DWG` |
| WMF | wmf/wmz | `wmf/sample.wmf` | `../../../../.temp/by_dll/doc/wmf/clock.wmf`、`clock.wmz` |
| DjVu | djvu/djv | `djvu/djvu_pd_scan.djvu` | 同左 |
| EMF | emf/emz | — | — |
| HPGL | hp/hpg/plo/hg/hgl/plt/hp1/hp2/hpgl/hpgl2/gl2 | `hpgl/sample.hpgl` | 同左 |
| PDF | pdf | — | `../../../../.temp/by_dll/doc/pdf/Categorization.pdf` |
| PostScript | ps/ps1/ps2/ps3/prn/eps/ai | `print/sample.ps`、`print/sample.eps` | 同左 |
| XPS | xps/oxps | `xps/sample.xps` | 同左 |

- 生成样本：`simple.dxf`、`sample.ps`、`sample.eps`、`sample.hpgl`、`sample.xps`、`sample.wmf`；
- 公版扫描：`djvu_pd_scan.djvu`（来自 `testdata/golden/`）；
- 本地真实样本：`Categorization.pdf`、`QuickViewLogo.DWG`、`clock.wmf/clock.wmz`，只在 `.temp`；
- EMF/EMZ 目前只有生成器单测里的最小 EMF。

旧版矢量样本不在这里，见 [`doc-legacy`](../doc-legacy/README.md)。

测试单枚 DLL：

```powershell
cd E:\tmp\AI.Agent\RuPix\main
$env:RUPIX_DECODERS_DIR='E:\tmp\AI.Agent\RuPix\.build\decoders'
cargo run --release --example decoder_ingest_probe -- "E:\tmp\AI.Agent\RuPix\.temp\by_dll\doc"
```
