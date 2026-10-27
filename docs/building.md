# 构建

## 前置

- Windows + MSVC（`vcvars64.bat`）；`rustup` 工具链由 `workspace/tools/toolchain.ps1` 固定。
- `python` 在 PATH（门禁脚本用）；`py` 启动器仅用于 `testdata/make_samples.py`（它需要 Pillow）。
- 当前四档是纯 Rust，不需要 NASM / CMake；形态 B（原生 C/C++ DLL）在场时，
  `workspace/tools/toolchain.ps1 -RequireNative` 会强制要求它们。

## 一条命令

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

`build.ps1` 是唯一入口；不要在根目录直接 `cargo build` 当作发布（会漏 singles / pro 的测试，
也不会发布/镜像/生成清单）。

## 顺序（脚本实际做的事）

1. dot-source `workspace/tools/toolchain.ps1`：固定 `RUSTUP_TOOLCHAIN`、定位 `vcvars64`、
   导出 `NASM_PREFIX` / `CMAKE_GENERATOR`、拦 `CC=gcc`；版本报告落
   `<target-dir>/toolchain_report.txt`。
2. 根 workspace：`cargo fmt --check` → `check_docs.py` → `check_history.py` →
   `check_deps.py` → `check_deps_direction.py` → `check_workspace_config.py` →
   `check_caps.py` → `check_claims.py` → `check_secrets.py` → `check_licenses.py` →
   `cargo clippy --all-targets --workspace --release -- -D warnings`
   → `cargo build --release --workspace` → `cargo test --release --workspace -- --test-threads=1`。
3. singles workspace（`workspace/singles/`）：同样的 fmt / clippy / build / test；单库不发布，但
   档的成员全靠它们，测试失败同样阻断发布。
4. pro workspace（`workspace/decoders/pro/`）：同样的 fmt / clippy / build / test；产物
   `pro.dll` / `pro.pdb` 拷回根 target 供发布。
5. 形态 B：`workspace/decoders/<id>/CMakeLists.txt` 存在 → CMake 子构建 + ctest；缺 DLL = 硬错误。
6. 发布：按 `workspace/decoders/*` 枚举，改名为 `<id>_<version>.dll`，写
   `../.temp/decoder-dist`；缺任何一枚 DLL = 硬错误。
7. 可选宿主镜像：`../.build/decoders` 存在时把同一批 DLL 拷过去，最后统一 prune
   同 id 的旧文件。`RUPIX_NO_HOST_SYNC=1` 可跳过。
8. pdb 归档：`../.temp/symbols/decoders/<id>.pdb` + 版本硬链接，每个 id 留最近 3 版；
   `RUPIX_NO_SYMBOLS=1` 可跳过。pdb **不进发布目录**。
9. 体积记账：逐 DLL + 总量，仅打印，无预算。
10. 清单新鲜度：`decoders.json` 缺项/多项/字节数漂移只警告，不阻断；发布前必须重跑
    `py tools/make_index.py`。

## target 与产物目录

| 目录 | 内容 | 谁定 |
|---|---|---|
| `../../.temp/decoder-target/`（相对 `workspace/`） | 根 workspace 中间产物、形态 B 的 `cbuild/<id>`、工具链报告 | `workspace/.cargo/config.toml`；`CARGO_BUILD_TARGET_DIR` 可覆盖 |
| `../../../.temp/singles-target/`（相对 `workspace/singles/`） | 单库中间产物 | `workspace/singles/.cargo/config.toml` |
| `../../../../.temp/pro-target/`（相对 `workspace/decoders/pro/`） | pro 中间产物 | `workspace/decoders/pro/.cargo/config.toml` |
| `../.temp/decoder-dist/` | 发布暂存 `<id>_<ver>.dll` | `build.ps1`；`RUPIX_DIST_DIR` 可覆盖 |
| `../.temp/symbols/decoders/` | pdb 取证归档 | `build.ps1` |
| `../.build/decoders/` | 宿主运行目录（并排开发镜像） | 可选；`RUPIX_NO_HOST_SYNC=1` 跳过 |

## profile

| workspace | opt-level | LTO | codegen-units | strip | panic | debug |
|---|---|---|---:|---|---|---|
| 档 | 3（全局） | true | 1 | true | unwind | 1 |
| singles | `z` + 逐包 3 | true | 1 | true | unwind | 1 |
| pro | 3（全局） | true | 1 | true | unwind | 1 |

- `panic = "unwind"` 不许改 `abort`：宿主靠 `catch_unwind` + SEH 隔离坏解码器。
- `debug = 1` 不许删：MSVC 行表只进 pdb、不进 dll；删了崩溃只能停在模块+偏移。
- 档不抄 singles 的"逐包 3"清单：全局 3 的代价只有约 +97 KB，换来新增成员不必补 profile。
- singles 的逐包条目按 **`[package] name`** 匹配，不是 `[lib] name`；写错会静默不生效。

## 手动命令

```powershell
# 根 workspace
cargo fmt --check
cargo clippy --all-targets --workspace --release -- -D warnings
cargo build --release --workspace
cargo test --release --workspace -- --test-threads=1

# 单库
cd workspace/singles
cargo fmt --check
cargo clippy --all-targets --release --workspace -- -D warnings
cargo test --release --workspace -- --test-threads=1

# pro
cd workspace/decoders/pro
cargo test --release --workspace -- --test-threads=1
```

## 形态 A / B

| 形态 | 判据 | 构建 |
|---|---|---|
| A：Rust cdylib | 默认 | 根 workspace 正常 cargo 构建 |
| B：原生 C/C++ DLL | `workspace/decoders/<id>/CMakeLists.txt` 存在 | `cmake -S . -B <target>/cbuild/<id> -G "NMake Makefiles"` → `cmake --build` → 有 ctest 就跑 |

形态 B 的版本从 `project(<id> VERSION x.y.z)` 读；DLL 必须落 `<build-dir>/<id>.dll`，
否则硬错误。C 侧用 `abi/c/rupix_decoder_kit.h`，`abi/c/abi_probe.c` 复核布局。

## 故障排查

| 症状 | 原因 / 做法 |
|---|---|
| `LNK2005: iv_abi_version` | 把单库和档放进同一次 cargo 调用；分 workspace（见 [`architecture.md`](architecture.md)） |
| `raw.dll` / `raw.pdb` 互相覆盖 | 三个构建图共用 target 目录；各自钉自己的 target-dir |
| 改了 profile 没生效 | 条目按 `[package] name` 匹配，不是 `[lib] name`；用 `cargo tree -e features` 复核 |
| PowerShell 脚本中文注释报错 | 本仓 `.ps1` 必须 ASCII；PowerShell 5.1 按 ANSI 读无 BOM 脚本 |
| 宿主镜像目录出现两份同 id | 发布后统一 prune；Windows 不允许覆盖已加载 DLL |
| `check_docs.py` 报锚点越界 | 文档里只写符号名，不写裸行号；行号会搬 |
| `cargo test` 假死 | 必须 `--test-threads=1`；并行会让会话池互相挤掉 |
