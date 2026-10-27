# 新增 / 修改一个解码器

> **流程正本**：修改、发版、提交前的完整顺序见根 [`开发指南.md`](../开发指南.md)。
> 本文只讲“文件放哪、怎么接、同步哪些文档”的操作细节。

先回答一个问题：**要不要动宿主？** 不用。身份来自 DLL 自己的 `iv_decoder_info()`；
宿主没有 id 白名单，生产路径是扫描目录 + 动态并集；加格式 = 加一个 DLL，宿主一行不改。

## 一、放哪

| 你要做的事 | 放哪 |
|---|---|
| 新增一种格式的参考实现 | `decoders-rust/singles/<id>/` |
| 把它接进发布档 | `decoders-rust/decoders/<tier>/Cargo.toml` + `MEMBERS` + `extensions` |
| 改解码算法 | 改单库 crate，不在档里写解码逻辑 |
| 改 ABI | `abi/src/lib.rs`，然后整份复制到宿主快照；必须用户拍板 |

目录名 = 解码器 id = `[lib] name`；id 不带连字符，必须无歧义短名。
`[package] version` 显式写出，不从 workspace 继承。

## 二、单库

1. `cargo new --lib decoders-rust/singles/<id>`；`[lib] name = "<id>"`，
   `crate-type = ["cdylib", "rlib"]`，版本显式写。
2. 加进 `decoders-rust/singles/Cargo.toml` 的 `members`。
3. 依赖 `rupix-abi`（workspace 继承）+ 解码库；热路径依赖在
   `decoders-rust/singles/Cargo.toml` 的 `[profile.release.package.<package-name>]` 提 `opt-level = 3`。
   注意按 **package name** 匹配，不是 `[lib] name`。
4. `src/lib.rs` 形态：
   - `export_decoder! { id, name, extensions, description, is_vector, has_alpha }`；
   - `probe_impl(..)` / `render_impl(..)` 业务全在这里，`rupix_abi::guard(..)` 留在 impl 内；
   - `#[cfg(feature = "standalone")] #[no_mangle] pub extern "C" fn iv_probe/iv_render`
     只做转发；`[features] default = ["standalone"]`；
   - 无魔数 / 裸数据格式另提供 `iv_render_raw`（规范 §十二）。
5. 自测至少覆盖：截断文件、畸形头、超 `budget_bytes`、`ext = NULL` 只按魔数
   （无魔数格式必须返回 0）、越界 / 菱形结构。
6. 在 `testdata/claims.tsv` 登记这个成员认领的每个后缀：有 golden/测试就写
   `verified`，暂时没有就写 `unverified` + 原因。

## 三、接进档

1. `decoders-rust/decoders/<tier>/Cargo.toml` 加依赖：
   `rupix-<id> = { path = "../../decoders-rust/singles/<id>", default-features = false }`。
2. `src/lib.rs` 的 `MEMBERS` 加一行。
3. 认领后缀写进该档 `export_decoder!` 的 `extensions`；四档后缀两两不相交
   （`check_claims.py` 守着）。
4. 新开一档时，还要加进根 `Cargo.toml` 的 `members`，并补
   `decoders-rust/decoders/<tier>/README.md` + `HISTORY.md`。
5. 档里**不写解码逻辑**；要换库、改算法，去单库 crate 里改。

## 四、档端到端测试

档里至少一条：探针 + 两阶段渲染 + 让位/路由。只验单库不知道档有没有接上。
需要样本时放 `testdata/golden/`，不要放进单库或档目录。
改完后 `python tools/check_claims.py` 必须 `violations: 0`。

## 五、文档同步（同一次改动）

- `decoders-rust/singles/<id>/README.md`：新增单库就写；不是每个单库都必须有，
  但 `decoders-rust/singles/README.md` 的总表必须登记它。
- `decoders-rust/decoders/<tier>/README.md`：成员表 + 认领后缀 + 限制。
- `decoders-rust/decoders/<tier>/HISTORY.md`：对外可见变化一条；版本号变才起新条目；
  只写版本/日期/ABI/变更，不写上传状态、ZIP 生成、release tag（见 `history-policy.md`）。
- `解码器开发规范.md` §八：自查清单（若 ABI 语义变化）。
- `testdata/claims.tsv`：认领后缀 / 成员 / 证据状态变化时。
- 根 `README.md`：只改四档表的版本/后缀数/发布状态，不抄细节。
- `index.json`：`py tools/make_index.py` 重生成。
- `docs/architecture.md`：workspace / 路由 / 能力位变化时。
- `docs/catalog.md`：字段或镜像规则变化时。
- 宿主侧：`docs/架构总览.md` §四解码器矩阵、`docs/图片格式与后缀名.md` 归属列、
  `README.md` 解码器表（并排开发时）。

## 六、验证

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
# 宿主并排开发时：
cargo run --release --example decoder_smoke -- <dll>
cargo run --release --example core_smoke
cargo run --release -- --probe-decode <file>
```

`build.ps1` 必须全绿；单库与档都要过；测试失败不发布。

## 七、特殊形态

| 形态 | 额外步骤 |
|---|---|
| 无魔数 / 裸数据 | 提供 `iv_render_raw`，宽高/布局由宿主收集；`ext = NULL` 时返回 0；参考 `decoders-rust/singles/rawpx` |
| 形态 A：内嵌 C / 静态库 | C 源放仓库外 `.3rd/<name>-<ver>/`；`cc` 的 `opt_level(3)` 显式设；`RUPIX_3RD_DIR` 可覆盖；目标路径不要 `canonicalize()` |
| 形态 B：原生 C/C++ DLL | 放 `CMakeLists.txt`（`project(<id> VERSION x.y.z)`）；`build.ps1` 自动走 CMake + ctest；C 侧用 `abi/c/rupix_decoder_kit.h` |
| 多帧动画 | 实现 `frame_index` + `frame_count` / `frame_delay_ms`（abi v3 语义） |
| 多页 / 子图 | 用 `page_index` + `page_count`；ICC 走 `icc_len`（v4 引入，v5 保留） |

## 八、代码纪律

- 不依赖宿主：禁 `rupix` / `rupix_lib` / `windui` / `windows`。
- 生产代码禁 `unwrap` / `expect` / `panic!` / `todo!`；测试断言例外。
- 跨 DLL 不传 `String` / `Vec` / trait 对象 / 闭包；宿主分配像素，解码器只写。
- 输入字节"推进且线性"：长度字段驱动的循环读到 0 长度必须推进或报错；
  递归/交叉引用用 visited 去重；轮数上界取自文件长度。
- 本仓 `.ps1` 一律 ASCII；日志/草稿/一次性脚本不进仓库。
- 行锚只写符号名，不写裸行号。
