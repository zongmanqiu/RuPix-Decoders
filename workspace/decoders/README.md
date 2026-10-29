# 发布档

`workspace/decoders/` 下**一个目录一枚发布 DLL**。目录名 = 解码器 id = DLL 文件主干名；
`[lib] name` 通常同 id（唯一例外 `doc-legacy` → `doc_legacy`）。

| 档 | 版本 | 后缀数 | 状态 | 成员 | 文档 |
|---|---|---:|---:|---|---:|---|
| `common` | 0.2.4 | 24 | 本地已构建；地址已写入 decoders.json | 6 | [README](common/README.md) · [HISTORY](common/HISTORY.md) |
| `modern` | 0.3.2 | 9 | 本地已构建；地址已写入 decoders.json | 5 | [README](modern/README.md) · [HISTORY](modern/HISTORY.md) |
| `raw` | 0.5.3 | 40 | 已发布（2026-10-29，见 decoders.json） | 1（rawpx 仅走 iv_render_raw） | [README](raw/README.md) · [HISTORY](raw/HISTORY.md) · [DESIGN](raw/DESIGN.md) |
| `pro` | 0.4.2 | 16 | 本地已构建；地址已写入 decoders.json | 7 | [README](pro/README.md) · [HISTORY](pro/HISTORY.md) |
| `classic` | 0.1.0 | 20 | 本地已构建；地址已写入 decoders.json；纯 Rust | 9 | [README](classic/README.md) · [HISTORY](classic/HISTORY.md) |
| `texture` | 0.1.0 | 8 | 本地已构建；地址已写入 decoders.json；纯 Rust | 8 | [README](texture/README.md) · [HISTORY](texture/HISTORY.md) |
| `doc` | 0.1.0 | 30 | 本地已构建；地址已写入 decoders.json；常见文档/CAD | 8 | [README](doc/README.md) · [HISTORY](doc/HISTORY.md) |
| `doc-legacy` | 0.1.0 | 5 | 本地已构建；地址已写入 decoders.json；CMX/WPG/ZMF/ZBR | 1 | [README](doc-legacy/README.md) · [HISTORY](doc-legacy/HISTORY.md) |

- 限制与已知空白见各档 `README.md`；对外变化见各档 `HISTORY.md`。
- 路由、workspace、ABI 快照见 [`../docs/architecture.md`](../../docs/architecture.md)。
- `raw` 是例外：它既是装配档，也含相机 RAW 显影引擎；不要按"档都只装配"理解。
- `pro` / `classic` / `texture` / `doc` / `doc-legacy` 是独立 workspace（各自
  `Cargo.toml` 带 `[workspace]`），构建顺序见 [`../docs/building.md`](../../docs/building.md)。

## 新增一档

1. 加进根 `Cargo.toml` 的 `members`；依赖会污染其它档时改为自带 `[workspace]`
   （`build.ps1` 会自动发现并单独构建）。
2. 目录名 = id；`[lib] name` 与 id 不同时 `build.ps1` 会按 `[lib] name` 找产物再改名；
   `crate-type = ["cdylib", "rlib"]`；版本显式写。
3. 写 `src/lib.rs` 的 `export_decoder!` + `MEMBERS` + `iv_*`。
4. 补 `README.md` + `HISTORY.md`，并登记到 `../DOCS_MANIFEST.md`。
5. 跑 `build.ps1`，补 `decoders.json` 与宿主 smoke。
