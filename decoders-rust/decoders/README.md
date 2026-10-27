# 发布档

`decoders-rust/decoders/` 下**一个目录一枚发布 DLL**。目录名 = 解码器 id = `[lib] name` = DLL 文件主干名。

| 档 | 版本 | 后缀数 | 状态 | 成员 | 文档 |
|---|---:|---:|---|---:|---|
| `common` | 0.2.1 | 29 | 发布包已生成（待上传） | 8 | [README](common/README.md) · [HISTORY](common/HISTORY.md) |
| `modern` | 0.2.0 | 6 | 发布包已生成（待上传） | 2 | [README](modern/README.md) · [HISTORY](modern/HISTORY.md) |
| `raw` | 0.5.0 | 40 | 发布包已生成（待上传） | 2（含 rawpx） | [README](raw/README.md) · [HISTORY](raw/HISTORY.md) · [DESIGN](raw/DESIGN.md) |
| `pro` | 0.2.0 | 33 | 发布包已生成（待上传） | 11 | [README](pro/README.md) · [HISTORY](pro/HISTORY.md) |

- 限制与已知空白见各档 `README.md`；对外变化见各档 `HISTORY.md`。
- 路由、workspace、ABI 快照见 [`../docs/architecture.md`](../../docs/architecture.md)。
- `raw` 是例外：它既是装配档，也含相机 RAW 显影引擎；不要按"四档都只装配"理解。
- `pro` 是独立 workspace（`decoders-rust/decoders/pro/Cargo.toml`），构建顺序见
  [`../docs/building.md`](../../docs/building.md)。

## 新增一档

1. 加进根 `Cargo.toml` 的 `members`（pro 除外，它自带 `[workspace]`）。
2. 目录名 = id = `[lib] name`；`crate-type = ["cdylib", "rlib"]`；版本显式写。
3. 写 `src/lib.rs` 的 `export_decoder!` + `MEMBERS` + `iv_*`。
4. 补 `README.md` + `HISTORY.md`，并登记到 `../DOCS_MANIFEST.md`。
5. 跑 `build.ps1`，补 `index.json` 与宿主 smoke。
