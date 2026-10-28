# 测试

## 必须串行

```powershell
cargo test --release --workspace -- --test-threads=1
```

三个构建图各跑一次（根 / `workspace/singles/` / `workspace/decoders/pro/`）。原因不是洁癖：
解码器持有进程级会话池（GIF/APNG 的 `SESSIONS` 是 2 槽 LRU），并行跑会让两条用例
互相挤掉缓存，把"顺序播放"断言挤成假失败。杀软锁新链接二进制也会让并行 runner
看起来像挂了。

## 分层

| 层 | 位置 | 验什么 |
|---|---|---|
| 单库单测 | `workspace/singles/<id>/src/lib.rs` 的 `mod tests` | 截断文件、畸形头、超预算、`ext=NULL` 只按魔数、越界/菱形结构 |
| 档端到端 | `workspace/decoders/<id>/src/lib.rs` 的 `mod tests` | 探针 + 两阶段渲染 + 让位/路由，证明成员真的接进档 |
| 门禁 | `tools/check_*.py` + `cargo fmt` / `clippy` | 文档、依赖、闸值、后缀认领、格式、lint |
| 真机 smoke | 宿主 `decoder_smoke` / `core_smoke` / `--probe-decode` | 真实用户路径：目录过滤 → 后缀认领 → 两阶段渲染 |

只有单库测试不够：它不知道档有没有接上。只有档测试也不够：它覆盖不到单库的
边界与畸形输入。两者都要。

## 样本

- `testdata/golden/`：31 个 golden / 边界证物（456 KB）**入库**，以编译期
  `include_bytes!` 为主；`av` 与 `raw` 的少数用例运行时读取。引用路径：
  `concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata/golden/<file>")`。
- `testdata/make_samples.py`：生成 1/10/100 MiB 雪花样本，**不入库**，写
  `../.temp/testdata/`；按后缀共享；别名后缀保留同一像素源，但每档字节长度唯一。
- **任何 `workspace/decoders/` 或 `workspace/singles/` 下不放图片**：同一后缀只有一枚认领者，
  换解码库时样本不该跟着丢。
- 生成样本需要 Pillow；本机 `python` 与 `py` 都是 3.11.3 + Pillow 11.1.0。换机器时用带 Pillow 的
  解释器，或先 `python -m pip install pillow`。`make_samples.py --check` 只验覆盖，不需要 Pillow。

## 门禁与证据

`build.ps1` 当前执行：`fmt` → `check_docs` → `check_history` → `check_abi_refs` → `check_deps` →
`check_deps_direction` → `check_workspace_config` → `check_caps` → `check_claims` →
`make_samples.py --check` → `check_secrets` → `check_licenses` → `clippy -D warnings` → 三图 build/test。
`check_anchors` 是 advisory，恒 exit 0；本仓不做联网下载校验。

证据规则：

- 性能结论必须写清配置、样本、n、配对方式；未测 ≠ PASS。
- "当前全绿 N 个"这类数字要么写实测日期，要么别写；没有门禁守着的数字必然过期。
- 认领后缀要有"成员 probe/魔数 + 至少一个夹具或测试"的证据；没有证据的格式
  在 `testdata/claims.tsv` 里标 `unverified` + 原因，不要写成已覆盖。

## 认领 → 证据

`testdata/claims.tsv` 把每个后缀落到 `tier` / `member` / `evidence`；`check_claims.py`
在 `build.ps1` 里执行：

- `verified` 的证据是 `golden:<file>`（文件存在且被 `.rs` 引用）或
  `tests:<workspace/singles package>`（该包至少一个 `#[test]`）；
- `unverified` 必须写原因；当前缺口是相机 RAW 的 29 个后缀（含 `.raw`）、`.ff`；
- 新增后缀不在台账里 = 门禁红；台账里多出未认领后缀 = 门禁红。

## bench 口径

- 单库 bench 是选型依据，比较的是**单库自己的形态**；发布档的全局 `opt-level = 3`
  与单库的 `z + 逐包 3` 是刻意不同，读数不能混。
- 同一次 run 内配对，正序 + 倒序复跑；尾部漂移大于两臂之差就判"无可测差异"。
- 宿主侧端点测试用 `.build/decoders` 里换 DLL、交替配对，不拿库函数直调代替
  真实用户路径。

## 已知空白

- `heic/heif/hif` 已有 CC0 HEIC golden；31 个 golden 均至少被一处 `.rs` 引用。先补最少证物，或在
  文档中明确标为未验证（现已登记在 `claims.tsv`）。
- `raw` 的 64 MP 闸只在查询阶段检查；宿主跳过查询直接渲染可绕过。没有真机样本
  可验，登记而不改。
- 单库 `hdr` / `psd` / 11 个 `pro-*` 没有独立 README；由 `workspace/singles/README.md`
  总表覆盖，详见 [`contributing.md`](contributing.md) 的文档规则。
