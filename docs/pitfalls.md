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

## D02 三个构建图共用一个 target → `raw.dll` / `raw.pdb` 互相覆盖

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
