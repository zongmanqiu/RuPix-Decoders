# classic history

> Only changes visible through the DLL are listed here. ABI history lives in
> `解码器开发规范.md` §九.

## 0.1.0 — 2026-10-29 — abi 6

- First release: `classic` owns the legacy bitmap suffixes that were left
  unclaimed after the 2026-10-28 ownership realignment.
- Claims (20): `pcx`, `pcc`, `dcx`, `pnm`, `ppm`, `pgm`, `pbm`, `pam`,
  `wbmp`, `xbm`, `xpm`, `pict`, `pct`, `pic`, `ilbm`, `lbm`, `iff`, `sgi`,
  `iris`, `bw`.
- Pure Rust: `image` (PNM/PAM), `image-extras` (PCX, SGI, WBMP, XBM, XPM),
  `oxideav-pict`, `oxideav-iff`; DCX is a small PCX container reader in this
  crate.
- `rgb` / `rgba` deliberately remain with `raw`; SGI is reachable only via
  `sgi` / `iris` / `bw`, so there is no suffix overlap.
- Evidence: `tests:rupix-classic` (probe + PNM decode + DCX negative case).
