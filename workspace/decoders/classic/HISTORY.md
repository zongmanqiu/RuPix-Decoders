# classic 历史

> 只记 DLL 对外可见的变化。ABI 版本史见 `解码器开发规范.md` §九。

## 0.1.1 — 2026-11-25 — abi 6

- **可并发调用**：新增能力位 `CAP_PARALLEL`（v6 内加法）。全库审计：成员内无进程级共享态（解码按调用隔离，纯函数式），宿主据此跳过同一枚 DLL 的串行锁。并发压力验收（多线程 × 200 次迭代）像素与单线程逐字节一致。

## 0.1.0 — 2026-10-29 — abi 6

- **首版**：`classic` 收下 2026-10-28 所有权重排后无人认领的旧式位图后缀。
- **认领（20）**：`pcx`、`pcc`、`dcx`、`pnm`、`ppm`、`pgm`、`pbm`、`pam`、
  `wbmp`、`xbm`、`xpm`、`pict`、`pct`、`pic`、`ilbm`、`lbm`、`iff`、`sgi`、
  `iris`、`bw`。
- **实现**：纯 Rust——`image`（PNM/PAM）、`image-extras`（PCX、SGI、WBMP、XBM、XPM）、
  `oxideav-pict`、`oxideav-iff`；DCX 是本 crate 里的小型 PCX 容器读取器。
- **后缀边界**：`rgb` / `rgba` 仍归 `raw`；SGI 只经 `sgi` / `iris` / `bw` 认领，
  因此没有后缀重叠。
- **证据**：`tests:rupix-classic`（探针 + PNM 解码 + DCX 反例）。
- **升级建议**：新装。
