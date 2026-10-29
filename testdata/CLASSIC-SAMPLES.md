# classic 案例图（20 个后缀）

## 结论

**全部可以生成，不需要外部下载。** 这一套是自制的 16×16 几何图案，覆盖
`classic` 的 20 个后缀；每个字节都由本仓脚本/示例编码，不含第三方图片。
生成图本身按 **CC0-1.0** 释放（代码仍按本仓 AGPL-3.0）。

## 生成

```powershell
cd RuPix-Decoder
python testdata/make_classic_samples.py --zip
```

默认输出 `.temp/classic-samples/`，同时写 `manifest.json`（大小 / sha256 /
尺寸）和 `README.md`。`--out <dir>` 可改目录。

- Python 侧：PCX/PCC、DCX、PNM/PPM、PGM、PBM、PAM、WBMP、XBM
- Rust `singles/classic/examples/make_legacy.rs`：PICT/PCT/PIC、ILBM/LBM/IFF、SGI/IRIS/BW、XPM

20 个文件都已在 `classic_0.1.0.dll` 上逐个验证：probe=100、render=0、16×16。

## 为什么不用“没版权”的外链做默认

“没有版权声明”不等于 CC0/公版，外链也不等于再分发授权。案例包默认用自制
样本，避免把来源不明的图片打进发布物。若以后确实要补真实世界样本，只收
CC0/明确公版，并在 manifest 里逐文件登记来源与许可证；不把“看起来没版权”
当作默认许可。

## 可选：真实世界样本

需要验证真实编码器怪癖时，优先从 Wikimedia Commons 等公共档案检索，并**逐
文件**核对许可证；不要按整个分类假定 CC0。真实样本不必入库，下载到
`.temp/` 做本地验证即可。
