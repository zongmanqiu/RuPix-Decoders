# emfsdk 0.2.0 — 补丁 E01（系统字体栈 + 逐字兜底）

> 本目录是**公开登记**：任何人拿上游 `emfsdk 0.2.0` 原文 + 这里的补丁，就能重建 `doc` 档
> 里 `doc-emf` 用的那份依赖。上游跟随规则见上一级 [`../README.md`](../README.md)。

## 基线

| 项 | 值 |
|---|---|
| 上游仓库 | <https://github.com/KaiserY/emfsdk> |
| crates.io | `emfsdk` |
| 版本 | 0.2.0（2025-10-10 发布） |
| `.crate` 文件 sha256 | `662afd8231566c47cb0ba3b38f5c4131a07ea68521e70917eeaee7d43db45f45` |
| 许可 | MIT OR Apache-2.0（crate 包内未带许可文件，按上游仓库 `main` 取回） |

## 谁在用

`workspace/singles/doc-emf`（`emfsdk = { path = "../../../../.3rd/emfsdk-0.2.0", features = ["render"] }`），
经 `doc` 档发布为 `doc_<ver>.dll` 的 EMF/EMZ 成员。构建 `doc` 之前必须把补丁副本放到
`<仓库上级>/.3rd/emfsdk-0.2.0/`（`docs/building.md` 的前置条件）。

## E01 — 做了什么

上游只准备了一款通用无衬线字体、并且忽略 LOGFONT 的字族名，中文整段掉进 5×7 点阵。
E01 把它换成"系统字体栈 + 逐字兜底"：

- `src/lib.rs`：`#![forbid(unsafe_code)]` → `#![deny(unsafe_code)]`，只为 `system_faces()`
  里那一次 `fontdb::make_shared_face_data`（把系统字库映射成可长期持有的只读共享数据），
  该函数就地 `#[allow(unsafe_code)]`；其余代码仍禁 unsafe。
- `src/render.rs`：字族列表 + `OnceLock` 进程内只扫一次系统字体；`render_text` 收 `family`
  参数，`LOGFONTW::lfFaceName` 点名的字族排最前，其余按回退链逐字查 `glyph_index`；
  `read_logfont_object` 读 `lfFaceName`。**只有所有字库都没有这个字时才整段退回 5×7 点阵**。

为什么必须打补丁而不是在解码器里绕：`emfsdk` 的栅格器不暴露视口、也不暴露字体回调，
字形绘制全在库内完成，外部只能改写二进制或改库。取舍记录见 `docs/design-adoption.md` A14。

## 重放

```bash
# 在上游 0.2.0 解包目录里执行
git -c core.autocrlf=false apply /path/to/patches/emfsdk-0.2.0/E01-system-fonts-and-logfont.patch
```

★ `core.autocrlf=false` 不是可选项：Windows 上仓库常设 `autocrlf=true`，`git apply` 会写出
CRLF，下面这张表的哈希立刻对不上（本机实测差 583 字节）。

| 文件 | 上游 sha256 | 打完后 sha256 | 打完后 git blob |
|---|---|---|---|
| `src/lib.rs` | `a4cb2a871db54dc23d358da8fa19ab100613ee115945dc96f31d05de5df72e8f` | `2627754e4074f4182245d98bda0ee10b225eeacf528e845bcd347ed247fdf560` | `1dd42752bed85b12f2adecf8835bf371a029ec65` |
| `src/render.rs` | `001c6b58dfd2a7e2a68a3bae2dc1ec2edbf8c9854433461eb97f613998e9d3af` | `88229161069ee3d34ef76cedb2e0ec3411e9d0d63e57f79c63d253cb5fa7610c` | `e65c1f13b10caa67ee047443159ec48d4546a2f0` |

逐字节核对（可选，本机有上游原文时）：

```bash
sha256sum src/lib.rs src/render.rs          # 应与「打完后 sha256」一致
python tools/check_patches.py               # 门禁：核对本机 ../.3rd 副本与登记的哈希
```

## 上游核对（每 90 天一次，过期门禁判红）

- **上次核对：2026-11-01**，上游最新仍是 `0.2.0`。
- 核对方法（任选其一）：

  ```bash
  cargo search emfsdk --limit 1
  curl -s https://crates.io/api/v1/crates/emfsdk | python -c "import json,sys;print(json.load(sys.stdin)['crate']['max_stable_version'])"
  ```

  也可让门禁代查：`python tools/check_patches.py --online`。
- 核对完**无论上游变没变**，把 `manifest.json` 的 `upstream_check.checked_at` 改成当天。
  没变就只改日期；变了就按下一节升级。

## 升级上游（重放，不搬迁）

1. 取新版本解包到工作区 `.temp/`，与本机 `.3rd` 副本逐文件 diff，重点看
   `RenderFontCache` / `render_text` / `read_logfont_object` / `draw_text` 四段是否被上游改签名。
2. 把 E01 重新叠到新版本上（形状照抄；上游若已改签名，按新签名改写而不是回退旧代码）。
3. 同步三处，缺一处都算没升级完：
   - 工作区 `.3rd/emfsdk-<新版本>/`（含 `SOURCE.md` 的版本 / 校验和 / 补丁段）、
   - 本目录：`manifest.json`（version、crate_sha256、post_sha256、checked_at）、本 README 的基线表与哈希表、
   - `Cargo.toml` 里的 path 依赖与新文件名。
4. `python tools/check_patches.py` 通过后，重编 `doc` 并重发。
