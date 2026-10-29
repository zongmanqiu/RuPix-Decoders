# doc-legacy — 旧版文档矢量档

一枚独立 DLL，只承接 **CMX/CCX、WPG、ZMF、ZBR**。这样把 `libcdr` / `libwpg` /
`libzmf` / `librevenge` 及 ICU/Boost 等 C++ 静态依赖从常用的 `doc` 里隔离出去。

## 成员

| 成员 | 后缀 | 库 | 语言 | 许可 |
|---|---|---|---|---|
| `legacy` | cmx/ccx/wpg/zmf/zbr | `libcdr` / `libwpg` / `libzmf` + `librevenge` | C++ 静态库 + C ABI | MPL-2.0 / LGPL-2.1+ / MPL-2.0 / MPL-2.0 |

## 构建

`doc-legacy` 固定使用 `x86_64-pc-windows-gnu`，依赖 MSYS2 UCRT64 的 g++/ICU/Boost
和 `.3rd` 下的 C++ 静态安装。构建：

```powershell
cd workspace/decoders/doc-legacy
$env:PATH = "C:\msys64\ucrt64\bin;" + $env:PATH
cargo test --release -- --test-threads=1
cargo build --release
```

产物为 `.temp/doc-legacy-target/x86_64-pc-windows-gnu/release/doc_legacy.dll`，
发布时重命名为 `doc-legacy_<ver>.dll`。

## 认领规则（iv_probe）

`iv_probe` 由装配层调用唯一成员 `legacy`；成员按后缀认领：
`cmx/ccx` → libcdr，`wpg` → libwpg，`zmf/zbr` → libzmf。
ZBR 使用文件内公开的 100×100 预览 DIB；ZBR 对象实体格式未公开，libzmf 上游也不输出。
认领失败返回 `0`；认领后解析失败返回 `ERR_CORRUPT`。

## 测试

真实样本在 `.temp/by_dll/doc-legacy/`（CMX/WPG/ZMF/ZBR）；可入库的安全样本见
`testdata/by_dll/doc-legacy/`。
