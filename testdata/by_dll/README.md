# 按 DLL 归档的测试样本

`testdata/by_dll/<dll-id>/` 是**单枚 DLL 的冒烟样本视图**，不是第二套语料：

- 原始生成器与 `testdata/golden/` 仍按**后缀**共享（一个后缀只有一个 DLL 认领，见 `tools/check_claims.py`）；
- `by_dll/` 按运行时 DLL 分目录，方便只拿一枚 DLL 做接入/出图测试；
- 可入库的样本放本目录；来源不明或只在本地可用的真实样本放 `<repo>/../.temp/by_dll/<dll-id>/`，
  不进入仓库。

命名约定：

```
testdata/by_dll/<dll-id>/<格式族>/<文件>
```

`doc` 当前样本见 [`doc/README.md`](doc/README.md)，旧版矢量见
[`doc-legacy/README.md`](doc-legacy/README.md)。其他 DLL 的生成产物由
`testdata/make_samples.py` 生成到 `.temp/testdata/`；运行下面的脚本即可得到按 DLL 的视图：

```powershell
cd E:\tmp\AI.Agent\RuPix\RuPix-Decoder
python testdata/make_samples.py
python testdata/by_dll.py          # 默认输出 .temp/by_dll
```

脚本不复制字节：同盘优先硬链接；缺失文件只统计，不伪造。
