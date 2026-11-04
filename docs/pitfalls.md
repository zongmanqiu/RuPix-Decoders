# 本仓专属坑（D 编号）

> 宿主坑用 K 编号，见 `<宿主>/docs/踩坑记录.md`。这里是解码器仓自己的坑；
> 编号只增不改。每条写"现象 / 原因 / 做法"。

## D01 单库与档同图构建 → `LNK2005: iv_abi_version`

**现象**：链接器报 `iv_abi_version` 已在 `common...rcgu.o` 中定义。

**原因**：单库默认 feature `standalone` 会导出 `#[no_mangle] iv_*`；档以
`default-features = false` 只取 `*_impl`。同一次 cargo 调用里 feature 取并集，
档里出现多份同名导出。

**做法**：单库与档分 workspace（`workspace/singles/` 独立，`workspace/decoders/pro/` 独立），
各自 target-dir；不要为了"少跑一次 cargo"合并构建图。

## D02 多个构建图共用一个 target → `raw.dll` / `raw.pdb` 互相覆盖

**现象**：cargo 报 output filename collision，或发布出来的 DLL 是另一个 workspace 的。

**原因**：多个 workspace 里有同 `[lib] name` 的 crate（如 `raw` / `core`），
产物文件名相同。

**做法**：`workspace/.cargo` → `../../.temp/decoder-target`；`workspace/singles/.cargo` →
`../../../.temp/singles-target`；`workspace/decoders/pro/.cargo` → `../../../../.temp/pro-target`。
不要手动合并 target 目录。

## D03 profile 条目写 `[lib] name` → 静默不生效

**现象**：给热路径包加了 `opt-level = 3`，速度没变。

**原因**：`[profile.release.package.<name>]` 按 **package name** 匹配；
`[lib] name` / 目录名只是解码器 id。写错时 cargo 不报错，只是不生效。

**做法**：用 `cargo tree -e features` 或 `cargo metadata` 核对 package 名；
单库清单里 `rupix-tiff` 与上游 `tiff` 是两行，别只写一行。

## D04 `opt-level = z` 用在逐像素热循环

**现象**：JPEG 3 MP 从约 25 ms 掉到约 125 ms（约 4.6 倍）。

**原因**：`z` 为体积优化；IDCT、熵解码、预测、逐字节循环上就是纯损失。

**做法**：档 workspace 全局 `opt-level = 3`；singles 用 `z` + 逐包 3；
热路径包必须显式提 3。不要用 `z` 做发布档的默认。

## D05 PowerShell 脚本写中文 → 解析错位

**现象**：报错落在邻近的无辜语句上，或字符串被截断。

**原因**：PowerShell 5.1 按 ANSI 读无 BOM 脚本；中文注释会被解错。

**做法**：本仓 `.ps1` 一律 ASCII（`check_docs.py` 硬查）；注释写英文。

## D06 样本放进单库/档目录 → 换库时样本丢失

**现象**：换解码库后测试样本跟着旧 crate 一起被删/改，别的实现没有同一份字节。

**原因**：同一后缀只有一枚认领者；样本应属于"后缀"，不属于某个实现。

**做法**：样本只放 `testdata/golden/`（入库）或 `../.temp/testdata/`（生成）；
任何 `workspace/decoders/` / `workspace/singles/` 下不放图片。

## D07 改 ABI 只改宿主快照 → 上游漂移

**现象**：宿主守卫测试红，或宿主与解码器对同一结构体理解不一致。

**原因**：宿主持有的是逐字节快照，不是 path 依赖；两边都可编辑。

**做法**：改 `abi/src/lib.rs` → 整份复制到 `main/vendor/rupix-abi/src/lib.rs`
→ 跑宿主 `cargo test --lib core::abi`。不要手改宿主快照。

## D08 跨 DLL 释放/传所有权 → 崩溃或 UB

**现象**：解码器返回后宿主释放像素崩溃；或解码器释放宿主缓冲。

**原因**：跨 DLL 的分配器/运行时不一定相同。

**做法**：宿主分配像素缓冲，解码器只写入；不传 `String` / `Vec` / trait 对象 /
闭包；错误文本走 `iv_last_error` 的线程局部缓冲。

## D09 `ext = NULL` 时按后缀认领 → 违反嗅探约定

**现象**：宿主内容嗅探时解码器误认领；或无魔数格式被错误接受。

**原因**：`ext == NULL` 的契约是"只能按内容（魔数）认领"。

**做法**：探针里先判 `ext`；无魔数格式（如 TGA）在 `ext == NULL` 时返回 0。
有魔数的格式才允许忽略 `ext`。

## D10 由文件长度字段驱动的循环不推进 → 挂死

**现象**：某个畸形文件让解码线程卡住，宿主无法取消。

**原因**：读到 0 长度不推进、递归/交叉引用没有 visited 去重、轮数上界取自
文件内计数。

**做法**：0 长度必须推进或报错；递归/交叉引用用 visited；轮数上界取自文件长度。
隔离栈只拦崩、不拦卡。

## D11 RAW 画幅 / CFA 相位按局部坐标算 → 黑边或红蓝互换

**现象**：RAW 四周黑边；或中性灰发色；或分箱档与全尺寸档色道不一致。

**原因**：传感器原样含光学遮光区；画幅起点常为奇数；CFA 图案要按**绝对坐标**查。

**做法**：画幅优先 `crop_area` → `active_area` → 整块；展开矩形横向铺满整行；
CFA 相位用绝对坐标（或折进 48×48 表）。不要用亮度启发式猜黑边。

## D12 色彩矩阵读恒零字段 → 整张偏色

**现象**：真机 RAW 发青/品红，白平衡像没做。

**原因**：rawler 0.7.2 的 `xyz_to_cam` 恒为零；零矩阵触发行列式兜底，整条链
实际没接色彩矩阵。

**做法**：读 `color_matrix`，优先 D65，缺则按 `Illuminant` 枚举序取最小；
`normalize(xyz2cam · SRGB_TO_XYZ_D65)` 行和归一；白平衡折进列；
用中性灰 + "换矩阵必须换结果"两条用例钉住。

## D13 文档写旧路径/旧数字 → 假绿

**现象**：README 链接 404；`check_docs.py` 仍绿；发布表体积/版本过期。

**原因**：门禁只覆盖一部分路径；手写第二份真源。

**做法**：字段/大小只信 `decoders.json`；版本只信 `Cargo.toml`；文档链接写相对路径；
行锚只写符号名；新增文档登记到 `DOCS_MANIFEST.md`。

## D14 ZIP 条目的 mtime 隐形进入包字节 → 零改动重打包也换 sha

**现象**：一行代码、一个字都没改，只是把 `package_decoders.py` 再跑一遍，八枚 ZIP 的
SHA-256 全变了（实测 `doc` 从 `c5eaf0c2…` 变成 `8cbfe5ab…`），**字节数却一模一样**。
交付记录里的"最终 ZIP sha"于是变成"哪一刻打的包"的指纹，不是内容的指纹。

**原因**：压缩器从磁盘 mtime 取条目的 DOS 时间，这个字段逐字节进入每个条目的
local/central header。crate 许可证文件甚至带着上游的 2006 年 mtime。改用 7-Zip 之后还多一层：
它会额外写 NTFS 高精度时间字段，光钉住 DOS 时间仍然不够。

**做法**：现行打包走 7-Zip（`-tzip -mx=9`，见 [`releasing.md`](releasing.md)）：`pack_zip()`
先把暂存目录（含目录自身）的 mtime 统一压到 `ZIP_FIXED_DATE = 1980-01-01`，再用
`-mtm=off -mta=off -mtc=off` 关掉高精度时间字段。**少任何一步都会让重打包换字节**。
判据 = 同一份内容连打两次，ZIP 的 SHA-256 一致、逐条目 `date_time` 全是
`(1980, 1, 1, 0, 0, 0)`（2026-11-04 两仓打包函数各连打两次实测）。
★ 2026-11-02 起**不再记也不核对哈希**（清单与宿主都删了，见 `catalog.md`「清单里没有哈希」），
所以这条坑现在只值"可复现"一个用处：想确认"改的到底是内容还是时间戳"时重打一次看字节；
平时的发布流程里没有"要填的 sha"这一栏了。

## D15 行尾探针自己造假：`grep -c $"\r"` 数的是翻译引号，不是回车

**现象**：一句话报出"本仓 219 个文本文件虽然 `.gitattributes` 写了 `eol=lf`、实际存 CRLF"，
还顺手给出修复方案（`git add --renormalize` + 提交）。用户批准后才复测：**假警报**。

**原因**：Git-Bash 里 `$"…"` 是**本地化翻译引号**（Windows gettext 语法），不是 ANSI-C 转义——
要写回车得用 `$'\r'`。于是 `grep` 拿到的是字面量、数的是"匹配某串的行数"：同一份 `README.md`
报 32，真实 CR 数是 0。另一半原因是**工作副本与 blob 是两件事**：`text eol=lf` 下磁盘上可以是
CRLF，blob 与 index 仍是 LF、`git status` 干净（归一化发生在 add）。

**做法**：数行尾只按字节读——`git cat-file blob HEAD:<file>` 取 blob 数 `0x0D`，工作副本用
`tr -cd '\r' | wc -c` 或 Python `b.count(b"\r")`；**永不用 `grep -c $"\r"`**。缺陷规模这类数字
在写进结论前要用一把**独立**量具复测同一格（同一条管道里的两个计数不能互证）。
`git add --renormalize` 在两仓都是 no-op，别为它开"修复"任务；真要让磁盘也变 LF 就重检出，
不产生提交。

## D16 `usvg::Options::default()` 的字体库是空的 → `<text>` 静默消失

**现象**：导出的 SVG 字符串里明明白白有 `<text x=… font-size=…>字</text>`，屏上一个字形都没有
（连英文也没有），而且不报错。

**原因**：`Options::default()` 里装的是**空的** `fontdb::Database`；usvg 解析期查不到字体就把
文字节点丢掉。同仓其它成员（cad / wmf / xps / doc-legacy）都装了系统字体库，只有疏忽的那一个漏了。

**做法**：`OnceLock<Arc<Database>>` + `load_system_fonts()`，每次解析只克隆 `Arc`（见 `doc-cad` 的
`font_db()`）。判据必须**落在像素上**：只断 SVG 字符串的用例在老代码上也是绿的。

## D17 `LB` 以 `<ETX>` 收尾：拼回流时补 `;` 会造出空命令，后面的几何全丢

**现象**：带 `;` 的标签能画出来，**标签之后**的线段消失。

**原因**：`nom-hpgl` 的 `Label` 用 `take_until("<ETX>")` 把 `<ETX>` 吃掉了，顶层分隔符是
`one_of!(";<ETX>")`；把命令 `join(";")` 拼回去时又给已带 `<ETX>` 的 `LB` 补了一个 `;`，
下一次解析撞上空命令，`separated_list_complete` 就地停下。

**做法**：逐条拼回流，只有"不以 `<ETX>` 结尾"的命令才补 `;`。与 D11/`PC` 那类"只删命令不删
分隔符"是同一个坑的两面。

## D18 版本号写在两处：`Cargo.toml` 与 `export_metadata!` 字面量

**现象**：升了版本、`decoders.json` 也对，装完以后界面仍显示旧版本；同 id 两份 DLL 时仲裁也按旧值。

**原因**：`[package] version`（`decoders.json`/更新比对用）与 `src/lib.rs` 的
`rupix_abi::export_metadata! { "version" => "x.y.z" }`（宿主实际读的那个）是两份真源，
只改一处不报错也不红。

**做法**：两处同时改；`tools/check_history.py` 已比对两者（红证就是 doc 0.2.2 那次真实漏改）。

## D19 补丁副本留在仓库外 = 发布源码不可重建（附：`autocrlf` 让 `git apply` 写出 CRLF）

**现象**：`doc` 的源码、许可证、清单都在公开仓里，唯独"能重建它"这件事不成立——
依赖用的是仓库外 `../.3rd/emfsdk-0.2.0` 的补丁副本，别人克隆本仓后既拿不到那份副本，
也不知道我们改了什么。

**原因**：`.3rd` 刻意不入库（源码仓只放自家代码），但"副本不入库"被误当成"补丁也不入库"。
两者的区别是：副本是上游的**内容**（1.2 MB，可重下），补丁是我们的**改动**（10 KB，只此一份）。

**做法**：每份补丁在 `patches/<crate>-<version>/` 留三件——`README.md`（为什么改 / 怎么重放 /
上游怎么核对）、`manifest.json`（补丁清单 + 触及路径 + 核对日期）、`*.patch`（对上游原文）。
判据是"上游原文 + 补丁 = 构建用的副本，逐字节一致"，`tools/check_patches.py` 核可离线核的部分；
上游核对日期超 90 天直接判红，因为"上游发了新版我们不知道"不会自己显形。

**附带一坑**：Windows 上 `core.autocrlf=true` 是常态，`git apply` 会把补丁里的 LF 写成 CRLF，
打完的文件比原文多出每行一个字节（本机实测 `lib.rs` 差 583 字节），副本的行尾就和上游那份不一致。
重放补丁一律 `git -c core.autocrlf=false apply`，本仓 `.gitattributes` 也把 `*.patch` 钉成 LF。

## D20 第三方解码库的**工作线程** panic，在宿主那边看不见：表现为"永不返回"而不是崩溃

- **症状**（2026-11-04，modern 0.3.3）：AVIF 偶发打不开，一直转圈，宿主日志里那一笔
  `iv_render` 整段不回包（`phase=query`、`run_ms` 一直涨），后面所有图全堵在队列里。
  **同一个文件换一次运行又能正常打开**——race 决定成败，所以"重试就好了"。
- **真因**：`zenavif` → `rav1d-safe 0.5.7` 的多线程路径有真实的别名竞争。
  `rav1d-disjoint-mut` 的 `DisjointMut` 校验器当场抓到并 **panic**：

  ```
  thread 'rav1d-worker-2' panicked at rav1d-safe-0.5.7/include/dav1d/picture.rs:641:19:
        overlapping DisjointMut:
   current: &mut _[40448..40512] on ThreadId(15) ...
  existing: &mut _[40500..40504]
  ```

  panic 在 **worker 线程**上，而死等它的解码调用者只是在 `recv` 一个永远不会来的结果。
  ⇒ 对宿主来说**没有异常、没有返回值、没有任何信号**，只有一个不返回的 `iv_render`。
- **为什么宿主侧一切防护都失效**：`catch_unwind` 拦的是**本线程**的 unwind，SEH 拦的是
  **本线程**的硬件异常。worker 线程的 panic 既不会传上来，也不会让进程崩——它只是让
  那个等待永远成立。所以"给 FFI 加 try/catch"这类办法在这条路上**一个字节都帮不上**。
  （宿主能做的只有超时后放弃并把这条调用链判死；真正根治必须让外来线程不存在，
  或者把解码搬进可杀死的进程。）
- **判据**：错误信息里出现 `thread '<名字>-worker-N' panicked`，而宿主侧看到的是超时挂死。
  两者的**线程名不是同一个**，这条对照是关键。
- **修法**：把该格式的线程数钉成 1（`zenavif::DecoderConfig::new().threads(1)`）；`modern` 0.3.4 起 AVIF/AVIFS 一律走这条单线程路径。
  没有 worker 线程 ⇒ 既没有竞争，也没有"等死掉的 worker"这条路。
- **类**：D12/D13 —— 都是"看得见的指标指向错的层"。
