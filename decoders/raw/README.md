# raw — 相机 RAW + 裸像素数据插件

一句话：**29 种相机 RAW 后缀 + 11 种裸像素后缀（共 40）的唯一认领者**。相机 RAW 部分
rawler 只做 demux + 传感器解压，**显影（黑电平 → 去马赛克 → 色彩矩阵含白平衡 → gamma）
全部是本仓库自己写的**（矩阵的三步线性代数用 rawler 的 `pub` 件，m85；链的形状与取样是我们的）；
裸像素部分整体转发给 `rawpx` 成员（参数化 `iv_render_raw`）。

## 身份（`src/lib.rs` 实读）

| 字段 | 值 |
|---|---|
| id / crate / 版本 | `raw` / rupix-raw / **0.4.0** |
| name | `Camera RAW & Raw Pixel Data` |
| extensions | `3fr;ari;arw;cr2;cr3;crm;crw;dcr;dcs;dng;erf;fff;iiq;kdc;mef;mos;mrw;nef;nrw;orf;ori;pef;qtk;raf;raw;rw2;rwl;srw;x3f;rgb;rgba;gray;gry;bgra;yuv;i420;yv12;nv12;yuy2;uyvy` |
| is_vector / has_alpha | `0` / `0` |
| 产物 | `raw_0.4.0.dll`（7,542,272 B） |

`crate-type = ["cdylib", "rlib"]`：`rlib` 只为 `examples/` 能当库调 `raw::iv_probe`。

依赖：`rawler = "0.7.2"`（锁 0.7.2，校验和 `04f4cc35…`；带进 27 个 crate，含 jxl-oxide、image、
rayon、memmap2、chrono）+ `rupix-rawpx`（同仓 `bench/singles/rawpx`，`default-features = false`）。
根 `Cargo.toml` 的档 profile 用全局 `opt-level = 3`（不再逐包列名单）。
**无 `build.rs` / 无 C 源 / 无 `.3rd` 依赖**：全部纯 Rust。

后缀表不是抄来的：`examples/dump_exts.rs` 打印 `rawler::decoders::supported_extensions()`
来校准相机 RAW 那 29 个。`.raw` 同时是相机 RAW 与裸像素 dump 的后缀，归属由**请求类型**
决定：常规打开走魔数（探针恒忽略 ext），参数化请求由宿主按 `CAP_RAW_PARAM` 筛到本档，
再由 `iv_render_raw` 转发给 rawpx。

## 认领（`iv_probe`，`lib.rs`）

`guard` 包住；`data.is_null()` → 0；`looks_like_raw` → **100**，否则 0。**`ext` 完全忽略**
（纯魔数，符合 `ext == NULL` 约定），只有一档置信度，没有 50。

魔数表：`FUJIFILM`(8B)、`FOVb`(X3F)、`\0MRM`、`IIII`(IIQ)、`3ASH`(3FR/FFF)、
偏移 4 处 `ftyp` + `crx`/`c<digit>`(CR3)，以及**任何 TIFF 头** `II*\0` / `MM\0*`。`len < 12` → false。
"blanket 认领 TIFF 头"的理由写在模块头。副作用：与 `tiff`、`common` 的认领面重叠，
且不看后缀——真 TIFF 照片文件可能先进这里。

**能力位图**：`export_caps!(CAP_FULL | CAP_SCALED | CAP_RAW_PARAM)`——m83 起这枚插件真的按
`REQ_ALLOW_SCALED` 出档位；rawpx 并入后多导出 `iv_render_raw`，所以必须同时声明
`CAP_RAW_PARAM`（用例 `caps_declare_scaled` 钉住三个位）。

**`iv_render_raw` 是可选导出**（规范 §十二）：宽/高/布局由宿主向用户收集后传入，
本档整体转发给 `rawpx::render_raw_impl`，本文件一行裸数据转换逻辑都不写。

**没有** sidecar 逻辑、**没有** X3F 专用解析（只有魔数与后缀条目）、**没有**内嵌 JPEG 预览层选择：
永远走全传感器解码。

## 解码路径

* 查询（`out == NULL`）：`rawler::decode_dummy` 只读头拿尺寸 + 方向（`dummy_decode`，`lib.rs:254-257`），
  再和渲染阶段一样过 `tier_plan` 决定**这一趟报整图还是报档位**（`:178-181`）。
  ★ 报的尺寸是**画幅**（下面那节），不是整块传感器原样。
* 渲染：`rawler::decode(&src, &RawDecodeParams::default())`（`:333`）——**永远 page 0**。
  厂商/容器分派（TIFF 族的 dcraw 式解码器 vs `cr3`/`raf`/`x3f`/`3fr`/`iiq` 原生解析，
  含无损 JPEG / JXL 的 CFA 解压）全在 rawler 内部，本 crate **没有任何按厂商分支**。
* 两条出画腿（`develop_cached`，`:306-362`）：点亮缩放意图且这一档真能分箱 ⇒ `develop_binned`
  直接在 CFA 上出低清档（`:797-888`）；否则自写显影出画幅（`develop` `:477-479` → `develop_in` `:487-642`）。
* **不用** rawler 的 goap / demosaic，也不用 `RawImage::crop`，自己显影（`:487-642`）：
  黑/白电平线性化 → `cpp == 1 && Cfa` 时走**自写双线性 demosaic**（`:955-1017`）、
  `cpp == 3` 直通（Foveon）、否则灰度复制（`:599-610`）→ **色彩矩阵一步同时带走白平衡**
  （`cam_to_srgb_wb` = `inv3(normalize(xyz2cam · SRGB_TO_XYZ_D65)) · diag(wb_coeffs)`，
  `:1066-1086`；矩阵来源 `xyz2cam` 在 `:1029-1045`，见下面「色彩矩阵」那节）→
  sRGB gamma（精算基准 `:1128-1136`，实跑走 m79 的单值桶表 `:1150-1188`）→ RGBA8，α 写死 255（`:638`）。
* ★ **白平衡折进矩阵，不在样本循环里乘**（m82 修的呈现缺陷）。原来写在样本循环，且 `cpp == 1`
  时所有样本一律取 `wb[0]`——三道乘同一个系数＝**没做白平衡**，而色彩矩阵的前提是输入已白平衡，
  于是红道在线性 sRGB 上被算成负数、钳到 0：真机照片整张发青（实测 NEF 均值 RGB `8/181/160`，
  同一张文件里相机自己写下的 JPEG 预览是 `122/140/151`）。折进矩阵＝一次列缩放，逐样本一次乘法都不做。
  用例 `white_balance_is_per_cfa_channel`（三道增益 `[2,1,4]` 配成"乘完相等"⇒ 必须出中性灰）钉住它。
* ★ **m82 删掉了"元数据撒谎兜底"**（原：观测峰值距黑电平 < 声明白跨度 5% ⇒ 按观测值重归一，
  并把黑电平收紧到 `峰值×0.1`）。按它自己的触发条件，它**只可能在平场/黑帧上成立**，
  于是把传感器噪声刷成满量程亮场——`.temp/raw-samples/X100V_compressed.RAF` 是一张真黑帧
  （原样均值 996.7、黑电平 1019、声明白 16383，相机自己的预览也是黑的），在应用里渲染成**一整块品红**。
  查看器不替照片做自动色阶。用例 `flat_black_frame_stays_black` 钉住"黑帧必须出黑"。
  ★ 遗留一条：compressed RAF 的真实白电平还没有**有内容的**真机样本可验，见 `main/docs/待办.md` ㉔。

## 色彩矩阵：m85 才真的接上（㉕）

**m85 之前这份矩阵从来没生效过。** `cam_to_xyz_matrix` 读的是 `img.xyz_to_cam`，而那个字段在
rawler 0.7.2 里**恒为全零**（`decoders/camera.rs:30` 声明、`:260` 初始化成零，之后再无人写；
字段自己标着 `TODO: deprecated, use color_matrix`）。零矩阵行列式为零 ⇒ 那道
`det < 1e-9 → 单位阵` 的兜底**每次都触发** ⇒ 整条链实际是「把相机三道当 XYZ，再乘
XYZ(D50)→线性 sRGB」。三张真机实测（`.temp/m85_probe_wb.txt`）：`xyz_to_cam` 全零 = `true` ×3，
`color_matrix` 里躺着的是 D65(9) 与 A(9) 两条**没人读**的标定。

* ★ 中性灰判据（`probe_raw_wb`，不等真图就能定性）：乘过白平衡增益的相机 RGB 里场景白就是
  `(1,1,1)`，喂进矩阵看它变成什么。**修前**三张全给线性 `(1.026, 0.971, 1.248)`（编码后
  `[255,248,255]`，蓝道高 28%）；**修后**三张全给 `(1.0, 1.0, 1.0)`。
* 现在的组成照 rawler 自己的标定步（`imgop/develop.rs` + `imgop/raw.rs::map_3ch_to_rgb`），
  三个件都是 pub、照用不抄：`xyz2cam`（`:1029-1045`，**优先 D65**，缺则按 `Illuminant` 枚举序取最小
  ——`color_matrix` 是 `HashMap`，不排序同一张文件两次运行可能取到不同 illuminant 的那份）
  → `normalize(xyz2cam · SRGB_TO_XYZ_D65)`（**行和归一**是全部要害：归一后每行之和 1 ⇒
  该矩阵与它的逆都把 `(1,1,1)` 映成 `(1,1,1)` ⇒ 中性进中性出**按构造成立**，不靠调参）
  → `inv3`（自带奇异兜底；上游 `pseudo_inverse` 在奇异时是除零出 NaN）→ 白平衡折进列。
  ★ 原来那步 `XYZ(D50·Bradford)→线性 sRGB` **整个删掉**：相机矩阵按 D65 标定，被当成 D50 PCS
  解是多余且错的白点适配（把它加回来的变异 M3 一次红 6 条用例）。
* 呈现像素上的效果（`.temp/m85-out/<样本>_arms_cmp.png`，三联：左＝全尺寸显影、中＝分箱档、
  右＝相机自己写进文件的预览；台账 `.temp/m85_probe_1920.txt` / `m85_cmp_after.txt`）：

  | 样本 | 我方档位均值 修前 → 修后 | 相机预览（外部裁判） | MAD(离全尺寸) 分箱腿 修前 → 修后 |
  |---|---|---|---|
  | glarus.nef | `89.6/133.5/147.8` → `117.3/129.6/139.2` | `123.2/141.0/152.5` | 3.917 → **1.830** |
  | Canon_EOS_30D.CR2 | `100.0/122.6/140.3` → `111.6/117.8/127.8` | `155.2/165.3/177.2` | 1.111 → **0.690** |

  修前那两张图上**云是品红的、整张蒙一层青**；修后三道比例与相机预览同形（NEF 的 R 缺口从
  −34 级收到 −6 级），剩下的差距是**等比的亮度**（预览亮 15%~40%）——那是相机自己的色调曲线，
  不是色偏，登记为 ㉗。X100V 那张是黑帧（㉔），在画质轴上无判读力。
* ★ 用例怎么才算"测到了矩阵有没有值进来"：中性灰那条**抓不到退化**（恒等也保中性），所以
  另有 `render_depends_on_the_color_matrix`——同一份有彩度的像素，D80 矩阵 / 30D 矩阵 / 无标定
  数据三次渲染必须两两不同。合成夹具的 `xyz_to_cam` **一律留全零**（那正是真机交出来的样子），
  于是"把来源改回那个字段"这一枚变异当场红。四条变异（M1 回到恒零字段 / M2 去掉行归一 /
  M3 加回 D50 / M4 取错 illuminant）各自先红、按 sha256 还原，红的用例名见
  `.temp/m85_mutations.txt`。
* ★ 速度账：**本轮对首开无主张**。端点双臂（`.build/decoders` 里换 DLL、交替配对）先用 `build.ps1` 发布那枚量过
  （−1.2% / +3.2% / −0.2%，区间全重叠），随后宿主仓 `full.bat` 把同一份源码**又编了一遍并覆盖发布**（出货那枚），
  ⇒ 按规矩用**出货那枚在端点回验**：正序＋反序各 3 轮后三张里只有 RAF 两轮同向 +7%，于是**单拿 RAF 再配第三对**
  ⇒ **反号 −5.3%**，同臂跨 pass 漂移（12%）大于两臂之差 ⇒ 判**无可测差异**、RAF 不立案。
  台账 `.temp/m85_ship_ledger.txt` / `m85_ship_rev_ledger.txt` / `m85_raf_ledger.txt`，判词见 `.plan/m85` §5.3。
  ★ 这枚规矩的正面表述：**"同源码重编只差时间戳"是上一对产物的段级读数，不能拿来免掉这一对的端点重测**
  （宿主仓 `docs/踩坑记录.md` K108 第 4 条）。
* ★ 遗留一条（㉘）：过曝处仍按道钳位，CR2 日心留了一小点品红。rawler 那份
  `clip_euclidean_norm_avg`（先保色相缩到 1、再与欧氏灰取平均）是现成的修法，
  但它是**每像素**的活，本轮没有为它付端点配对——量完再判。

## 画幅：遮光区不当画面交回宿主（m84）

传感器原样四周有一圈**光学遮光区**（金属封住不让进光，相机拿它估黑电平/噪声）。rawler 把它留在
原样里，只在元数据写下画幅矩形（`active_area` = 有效区、`crop_area` = 相机建议画幅）。m84 之前
本插件两条腿都吃整块 `img.width × height` ⇒ **照片上边和左边各一条黑边**。现在：

* `frame(img)`（`lib.rs:449-467`）只读元数据定画幅，优先级与 rawler 自己的
  `CropDefault` 同序：**`crop_area` → `active_area` → 整块原样**。越界钳进原样；裁出 0 宽/0 高当作没有。
  ★ **没有任何亮度启发式**——实测 NEF 那圈遮光区写着**满量程亮**（原样均值 3311.7 ≈ 白电平 3880），
  按"黑边"反推裁切线在那张上会裁出 0 像素。
* 全尺寸腿 `develop_in`（`:487-642`）只出画幅那一块，且线性化/去马赛克吃**上下各扩 1 行的真实邻域**
  （贴到原样边缘就不扩）⇒ **画幅内部与"整块显影再裁出"逐字节相同**（三张真机 + 五种矩形实测 YES，
  量具 `probe_raw_frame.rs`；单测 `cropped_develop_is_byte_identical_to_the_sliced_whole_frame` 钉住）。
  ★ 展开矩形**横向铺满整行**：这样它在原样缓冲里是一段连续区间，线性化仍是**一整遍平铺**，行宽也
  保持传感器那个偶数。
* ★ **CFA 相位按绝对坐标查**：画幅起点常常是奇数（实测 CR2 上 19、NEF 左 14、RAF 上 13），
  按画幅内局部坐标查图案就把 R 与 B 认错道。分箱腿同样按绝对坐标：块起点 = 画幅原点 + 钳到画幅内的
  整块（`:797-888`），除数按钳后起点的**绝对相位**查表。
* ★★ **相位怎么查，是有读数的**：把 `color_at(ex.y + y, ex.x + x)` 直接留在去马赛克的逐样本闭包里，
  rawler 那句 `pattern[(row+48)%48][(col+48)%48]` 的两次取模就提不出循环。同一份二进制只换这一处、
  同进程交替双臂（`bench_raw_crop`，三张真机各 6 趟取中位数，台账 `.temp/m84_bench_phase.txt`）：
  整块腿 NEF 253.3 → **310.9**、RAF 774.0 → **999.4**、CR2 190.2 → **242.5** ms（+22.7% ~ +29.1%），
  而两臂**像素指纹逐张相同** ⇒ 纯写法之差。修法是把偏移**折进一张 48×48 表**（每次显影建一次，
  2304 次查询），内层重新变成 `rot[y % 48][x % 48]`。
  顺带两处同进程配对留下的改写：闭包参数从 `&dyn Fn` 改单形化 `impl Fn`、`cpp == 1` 的电平常数
  提到循环外（交替三轮同向才收）。撤回的有两条：`#[inline(never)]`/`#[inline(always)]` 标注与
  「把展开矩形左右也裁进去」——前者无可复现差异，后者行宽从偶数变 3874 反而更慢。
* 档位判据 `bin_plan`（`:674-736`）的尺寸与步长上限一律改用画幅——申报的档位尺寸与 `develop_binned`
  产出的那块必须同源，否则 m83 立的「申报尺寸＝宿主解释缓冲区的依据」当场破。
  扫参用例 `tier_never_lets_the_host_upscale` 因此多了一维画幅（整块 / 奇数起点 / 非对称各算一次）。

三张真机的画幅与外部裁判（`probe_raw_frame.rs`，正本 `.plan/m84` §二）：

| 样本 | 整块原样 | 画幅（crop_area） | 遮光区 LTRB | 保留 | 宽高比 全→裁 | 相机自己的预览 |
|---|---|---|---|---|---|---|
| Canon_EOS_30D.CR2 | 3596×2360 | 84,19 3504×2336 | 84/19/8/5 | 96.45% | 1.5237→**1.5000** | 1728×1152 = 1.5000 |
| glarus.nef | 3904×2616 | 14,9 3872×2592 | 14/9/18/15 | 98.27% | 1.4924→1.4938 | **3872×2592（逐像素相等）** |
| X100V_compressed.RAF | 6384×4182 | 6,13 6240×4160 | 6/13/138/9 | 97.23% | 1.5265→**1.5000** | 4416×2944 = 1.5000 |

呈现像素上的效果（CR2 左上角 320×256，对照图 `.temp/m84-out/Canon_EOS_30D_topleft_cmp.png`，左半裁前／右半裁后）：
整行口径 **带内首行均值 4.9 → 画幅首行 117.7**（`.temp/m84_probe2.txt`）；补丁口径由 `.temp/m84_diff.py` 重算
——**首行 8.7 → 109.6、首列 3.2 → 91.3、中心区 106.0 → 107.6**（中心区是同一块画面，只差取样起点）。
★ 三条样本不一样，而且**不该一样**：NEF 那圈遮光区写着亮（首行 204.5 → 201.7，裁掉的是"错的那一圈"而不是黑的），
RAF 是黑帧（0.0 → 0.7）。这轮的账是**遮光区不再当画面**，不是"黑边消失"。
★ 这轮**不主张提速**：裁掉的面积只有 1.7%~3.6%，全尺寸腿本来就整张解，档位腿只少扫几条带；
端点双臂只用来证明没有可归因的变慢。

## 性能档位与首开归因（m78，2026-10-21 实测）

一次 RAW 首开（三张真机样本：CR2 8.5 MP / NEF 10.2 MP / X100V RAF 26.7 MP）按段拆开：
**宿主侧清白**（`post_ms` 10–19 ms、mmap 交字节 0.05 ms），**rawler 解 CFA 只占 68/70/155 ms**，
剩下的 **81%/85%/94% 全在上面那条自写显影链 `develop` 里**（改前 568/740/2345 ms）；
`develop` 内部再拆，**gamma 的 3×像素次 `powf` 占它 50–54%**（185/239/840 ms，同次数查表只要 8/9/25 ms）。

* 因此本 crate 有 `[profile.release.package.rupix-raw] opt-level = 3`（workspace 发布档是 `z`，为体积准备）：
  端点首开 **698.7 / 874.4 / 2493.6 → 529.7 / 640.7 / 1781.1 ms**，每 MP 85–95 → 63–68 ms，
  **z 档与 3 档各自解出的像素指纹 fnv1a64 逐张相同 ⇒ 输出逐字节不变**，DLL +4,608 B（6,899,712 B）。
  判据见 `<宿主>/docs/踩坑记录.md` **K102**：提档位值不值，看**热循环在不在本体这层**。
* `develop` / `develop_in` 是 `pub`，只为按段计时探针（下面 `bench_raw_stages`）与画幅量具
  （`probe_raw_frame`，它要拿"整块"当对照臂）；本体的调用点仍然只有 `develop_cached` 一处。
* ★ **m79 落地**：`develop` 的 gamma 那一步（原来每像素 3 次 `powf`，占整条显影链 50–56%）改走**单值桶表**
  （`:1150-1188`）——65536 个桶里"本桶与左右邻桶 8 位取值一致"的查表，跨输出边界的逐桶标记回精算。
  插件侧交替双臂（同一小时、交替跑）：`develop` **CR2 394.5 → 242.2、NEF 525.7 → 308.7、RAF 1732.6 → 790.3 ms**；
  端点交替双臂（`.build/decoders` 里换 DLL 跑）：首开 **568.6 → 404.3、676.9 → 498.4、1900.3 → 1097.9 ms（−26~−42%）**。
  **三张真机像素指纹与 m78 记录逐张相同**（`66323a29…`/`d8e17886…`/`f6c50b9e…`）⇒ 一个像素的取值都没动；
  raw DLL 6,899,712 → **6,905,856 B**。等价性由上面那两条穷举用例钉住，不是"抽样看着像"。
* ★ 两条等价改写**量过之后撤回**（`inv3` 挪出每像素循环、去马赛克"坐标→通道"先摊平成表）：
  像素指纹相同＝真等价，但 3 档下时间无可复现收益——LLVM 已经做了，别再手写第二遍。
* 两条待裁的账（都带价签，见 `main/docs/待办.md` §18）：**gamma 查表**已在 m79 落地；
  **按视口档显影**（本插件以前和 av/jxl 一样不理 `target_w/h`，一屏只要 1.4% 的像素却解整张）
  画质在 m82 判过、接线在 m83 落地——见下一节。

## 按视口档分箱显影（m82 量、m83 接线）

`bin_plan`（`:674-736`）+ `develop_binned`（`:797-888`）：给定宿主那一框，直接在 CFA 上按
`step × step` 块**同色求平均**产出该档 RGBA8，不再先解整张。步长上限由「缩完不许让宿主放大」
这条立约定死（与 TIFF 的 `scaled_step` 同一口径）：`step ≤ ⌊max(w/tw, h/th)⌋`；又因为本插件报的
方向可能是转置的（竖构图 RAW 的原样是横的），取两种朝向里**更严**的那个上限。
步长**不必**是 CFA 周期的整数倍——`bin_phase_counts`（`:745-786`）用一张积分图逐个相位块数 R/G/B
的样本数，**任一块缺任一道色就不分箱**（四色传感器 CYGM/QRBC、f32 原样、行列周期不成方一律回全尺寸）。
X-Trans（6×6 图案）在步长 3 下每块恰是 2R5G2B ⇒ 能走这条路；这是把"必须整数倍"放宽之后才拿到的，
否则 26.7 MP 那张 RAF 在 1920 那一框上永远分不了箱。

离线三臂（`bench_raw_bin`，同一进程同一张图；框 1920×1080）：

| 样本 | 步长→档 | A 现路到档 | B 分箱到档 | 省下 | 呈现像素 A−B | 参照 A−A2 |
|---|---|---|---|---|---|---|
| glarus NEF 10.2 MP | 2 → 1952×1308 | 425.5 ms | 211.8 ms | −50.2% | MAD 3.696 | 4.540 |
| X100V RAF 26.7 MP | 3 → 2128×1394 | 1050.4 ms | 344.6 ms | −67.2% | MAD 0.308 | 0.353 |
| Canon CR2 8.5 MP | 2 → 1798×1180 | 338.0 ms | 187.9 ms | −44.4% | MAD 1.131 | 1.491 |

★ 表里那三档尺寸是 **m82 量的（整块原样）**；m84 起按画幅算，同一框上的档尺寸变成 1936×1296 /
2080×1387 / 1752×1168，步长一档没变（2 / 3 / 2）。

★ **画质判据的尺子不是"我觉得这点差看不见"**：A2 是同一条腿、同一份全尺寸像素，只把整套块起点
平移 `step/2`——它与 A 的差就是"换一格同样正当的取样"的代价。三条样本上 **A−B 都小于 A−A2**
（再按宿主那样缩到呈现尺寸同样：2.930 < 3.644 / 0.30 < 0.33 / 1.007 < 1.345），偏色 Δmean ≤ 1.2 级、
平滑区无成片大差 ⇒ 分箱那一档**不比"换一格取样"更伤眼**，判：可落地。
绝对毫秒受这台机器负载漂移支配（同一份代码两次量到 584 与 804 ms），所以只报同一次 run 内的比值。

**接线（m83）**：`iv_render` 读整个 `IvRenderReq`（`:152-156`），两阶段都过同一枚
`tier_plan`（`:265-271`）——查询按档位报 `bytes_needed`，渲染就正好写那么多字节。
★ **两阶段各算一次必须算出同一个答案**是这轮的承重契约：宿主在渲染**之前**就把查询回来的
`actual_w/h` 摘出来解释缓冲区（`main/src/plugins/mod.rs`），所以档位与画幅的判据只准吃元数据
（尺寸 / cpp / 图案 / 原样 dtype / `crop_area` / `active_area`），不准碰像素——`decode_dummy` 与真解
拿到的是同一份元数据。这一点由 `examples/check_raw_tier.rs` 在真机样本上逐张对账（dummy 与真解的
`bin_plan` 与 `frame` 结果、申报尺寸与产出尺寸、以及"整图之后再来一次档位请求"不许串档）。
代价要明写：同一张文件被问两档（首屏档位 + 原图档升级）就会**解两次传感器**——
`rawler::decode` 那一趟占现路首开的 19–34%（107/131/187 ms，m83 实测），
换回来的是一次只搬 8–12 MB 而不是 101.8 MB。
端点双臂（`.build/decoders` 换 DLL、交替配对，`.plan/m83` §三有逐发台账）：
首开 **NEF 427/455 → 187/200、CR2 365/362 → 180/206、RAF 1061–1120 → 272–295 ms（−54%~−74%）**；
同目录连翻 8 次 **RAF 1007–1150 → 221–246、NEF 341–432 → 151–155 ms**；
连翻期进程峰值工作集 **639 → 132 MB**。
★ 这条读数**更正 m80 那句"想动 603 MB 得去宿主侧"**：决定峰值的不是插件那一次瞬时拷贝，
也不是宿主自己的几份常驻缓冲，而是**"这一趟到底要不要产出整图"**——档位路径让整图根本不产生，
峰值就跟着掉。停在一张不动等原图档升级仍然要多付一次解传感器（NEF 31→340、RAF 76→1077 ms 那一腿），
所以两臂的"到 1:1 清晰"总时间只差 +8%~+13%，而**看得见的第一帧早了 3–4 倍**。

## 多帧 / 多页 / 预览层

`target_w` / `target_h` / `flags` 从 m83 起**真的读**（`REQ_ALLOW_SCALED` → 档位，见上面两节）；
`page_index`、`frame_index` 仍忽略。`fill_info`（`:224-236`）写 `page_count = 1`、`orientation`、
**`already_oriented = 0`（宿主旋转）**，但**从不写 `frame_count` / `frame_delay_ms`**——`:233`
注释说依赖宿主把 `IvRenderOut` 预清零。★ **这条依赖 m84 核对过**：宿主 `main/src/decoders/`（`decode.rs` 的查询/渲染两阶段与 `host.rs` 的加载路径）
三处调用点全部 `IvRenderOut::default()`（repr(C) 全整数字段 ⇒ 逐字段零），不是"应该清零"而是逐处读过。
★ 顺带记一处**口径差**：ABI 条文写「静态格式 `frame_count` 必须 = 1」，而本插件与 `av` 交的是清零后的 0，
宿主按 `frame_delay_ms == 0` 判静图（`main/src/core/pixels.rs:75` 的 `is_animated() = delay_ms > 0`；
`viewer.rs:2280` 的 `anim_summary` 与 `:1441` 的取模都先过这道），
所以 0 与 1 在今天**行为等价**、UI 上也不出「帧 1/1」（那串只在 `is_animated()` 里显示）。
⇒ **不改**：改了没有读数可拿，而 ABI v4 已冻结；这处差异登记在此，将来出现第二个宿主时再按修宪处理。

## 元数据 / 色彩

只取：尺寸、画幅（`crop_area` / `active_area`，m84）、方向（rawler `Orientation` → EXIF 1..8，
`:1190-1202`）、显影输入（blacklevel / whitelevel / wb_coeffs / **color_matrix**）。
**EXIF 正文不导出，ICC 不导出**（`icc_len = 0`，`:235`）。`iv_render_raw`：absent（本插件有真魔数）。

## 错误码口径

1 / 4 入口两条同 jpeg；7（64 MP、预算）；4（`out_cap` 不足）。
rawler 映射（`:286-291`）：`Unsupported`→3、`DecoderFailed`→**2 CORRUPT**，消息 = rawler 的 `Display`
（`:273-275`）。★ 这里有个刻意选择，`:277-285` 写明：把不认识的 TIFF 容器报成 CORRUPT 会掐死
宿主向 `core` 的兜底链，所以措辞要保守。

## 限制与已知空白

* `MAX_PIXELS = 64 MP`（`lib.rs`）**只在查询阶段检查**；渲染分支只查预算和 `out_cap`。
  也就是说：宿主若跳过查询直接渲染，这条像素上限就被绕过。
  ★ 这道闸按**整块原样**判（裁切只减不增），所以 m84 没有放宽它。
* 自写双线性 demosaic 比 Malvar / AMaZE 软（`:46` 自己承认，属明写的取舍）。
* 方向**故意不应用**，交给宿主。画幅矩形在**未旋转**的传感器坐标里，宿主先裁后转的顺序与相机一致。
* 画幅只听元数据：`crop_area`/`active_area` 都没有的文件（少数机型）仍交整块原样——
  与 m84 之前逐字节相同。不做"测出来的黑边"式反推（NEF 那圈亮、CR2 那条边实测只有 73 而元数据在 84）。
* 超过 64 MP 的 RAW 今天**开不了**——即使视口只要 1/20 的像素，这条闸在档位之前就拒绝了。
  没有这样的真机样本可验，也没有用户报痛，登记而不改。

## 缓存

thread-local 单槽解码缓存，键 = 首 64 B + 尾 64 B + 长度的 FNV（`:306-362`）。
**m80 起缓存槽与返回值共享同一份像素**（`Developed = (Arc<Vec<u8>>, …)`，`:290`）：
原来插入与命中各 `clone()` 一次整图，26.7 MP 那张一份就是 101.8 MB。
实测（`.build/decoders` 里换 DLL 交替双臂，`<宿主>/examples/mem_chain_probe.rs` + `stall_probe -DecodeTrace`）：
**消掉的是搬运，不是峰值**——26.7 MP 首开的进程峰值两臂同为 **603 MB**（峰值由宿主自己那几份
`base_rgba`/纹理/鸟瞰决定，插件那一次瞬时拷贝不在关键时刻上）；
时间上按 `1` 的命中档 **render 69–71 → 45 ms**（−36%）、首开 render 851 → 824 ms（−27 ms，8.5 MP 那张在噪声内）。
★ **m83 起缓存键补上请求档位**（目标框 + 缩放意图）：同一份文件的视口档与整图档是两块不同的像素，
只按内容键就会串档——把 101.8 MB 的整图当 240×180 的缩略交上去，宿主按查询申报的尺寸解释缓冲区
就花屏。这条由 `check_raw_tier` 的"整图之后再来一次档位请求"那一趟盯着。
m84 不改键：画幅只由内容元数据决定，同一份文件的画幅只有一个。
这一槽仍会一直持有到下一次解码；宿主取消渲染时同样驻留。

## 测试（29 个 `#[test]`）

`probe_detects_dedicated_magics`、`probe_detects_tiff_family`、`probe_rejects_garbage_and_short`、
`orientation_mapping_is_exif`、`matrix_inverse_roundtrips`、`gamma_encode_clamps_and_midtones`、
`neutral_gray_survives_a_real_color_matrix`（真机 D65 矩阵下中性灰必须还是中性灰，两条腿各判一遍）、
`render_depends_on_the_color_matrix`（换矩阵必须换结果——专抓「矩阵读了个恒零字段」这种退化）、
`xyz2cam_prefers_d65_and_is_deterministic`（D65 优先、缺 D65 取枚举序最小、空/短表 → `None`）、
`gamma_buckets_are_exact_where_they_claim_to_be`（穷举 65536 个桶：凡"可查表"的桶必须桶内恒定且与左右邻桶同值，且快路要过半）、
`gamma_buckets_match_direct_encode_over_the_whole_domain`（桶界 + 桶心 + 20 万确定性伪随机点逐个与精算对撞），
`demosaic_keeps_native_samples`、`develop_synthetic_rggb`（增益全 1 的数值通路基线）、
`white_balance_is_per_cfa_channel`（三道增益 `[2,1,4]` 配成"乘完相等"⇒ 必须出中性灰；
  把白平衡退回"一律用 `wb[0]`"当场红）、`flat_black_frame_stays_black`（黑帧必须出黑；
  把 m82 删掉的"元数据兜底"加回来当场红）。
m83 新五条：`caps_declare_scaled`（位图不许悄悄改）、
`tier_plan_needs_the_flag_and_the_headroom`（没点亮缩放意图 / target=0 / 目标框 ≥ 原图一律整图）、
`bin_plan_declines_the_forms_it_claims`（f32 原样、cpp=3、非 CFA 光度、四色图案、缺道色、
  行列周期不等——每种都单独断言，夹具还自证 `cfa_period` 真的是 `(3,12)`）、
`tier_never_lets_the_host_upscale`（6 形状 × 3 画幅 × 9 框，正向与转置**两套**显示尺寸都不得大于档位，
  并断言过半组合真的算出了档——空转的扫参不算判据；m84 加的画幅维逮住"上限按整块算、输出按画幅算"）、
`binned_tier_divides_by_per_phase_sample_counts`（12×9 的 RGGB 走步长 3：块是 4R4G1B / 3R4G2B 这些
  **不相等**的构成，除数写死 `step²` 就不出中性灰；顺带钉 `develop_binned` 的产出尺寸 == `bin_plan` 申报尺寸）。
m84 新五条：`frame_prefers_crop_then_active_then_whole`（优先级与 rawler `CropDefault` 同序）、
`frame_clamps_lies_and_falls_back_instead_of_cropping_nothing`（越界钳、裁空回整块、`saturating_add` 防回绕）、
`cropped_develop_is_byte_identical_to_the_sliced_whole_frame`（**逐字节**等于整块裁出，五种矩形含右下角 1×1；
  夹具必须逐像素乱序——平场对"邻域钳位"完全钝感）、
`odd_frame_origin_keeps_neutral_flat_gray`（画幅 (2,2) ⇒ 展开矩形起点是奇数，按局部坐标查图案当场不灰；
  ★ 起点 (1,1) 的夹具展开后正好盖住整块，对这条变异**测不到**，MUT-2 第一次跑就是这么溜过去的）、
`binned_crop_keeps_blocks_inside_the_frame_and_uses_absolute_phase`（画幅外一圈写**黑值**，
  块起点忘了加画幅原点就会把黑带累加进来 ⇒ 首行首列不灰）。
4 个共享样本（`testdata/golden/` 下：`CanonRaw.cr2` 8 724 B、`CanonRaw.cr3` 52 502、`DNG.dng` 13 662、
`FujiFilm.raf` 38 452）——都是 exiftool 改过的，**只用于嗅探**（`smoke_raw.rs:6-10`）。
examples：`dump_exts.rs`（校准后缀表）、`smoke_raw.rs`（真解只对 `.temp/raw-samples` 里的文件做，
判过标准 `w > 0 && 非零像素占比 > 1%`）、`bench_raw_stages.rs`（m78 的首开按段计时 + 像素指纹 + 两条诊断，
**一次进程只量一张**——`develop_cached` 的 TLS 缓存会让同进程第二趟必命中，见 K101）、
`bench_raw_bin.rs`（m82-A 的三条腿对照：A 全尺寸显影→面积平均、A2 同一条腿换取样起点、B CFA 分箱直显；
  除时间还落三份同尺寸裸 RGBA8 到 `.temp/m82-out`，供 `.temp/m82_diff.py` 在**呈现像素**上算差值分布）、
`check_raw_tier.rs`（m83 的端到端诚实性：按宿主真实的调用序列走 `iv_render`，逐张真机 RAW 对账
  dummy/真解的档位判断与画幅、申报与产出尺寸、串档、两种朝向的"不许放大"，以及档位/整图/缩略三趟的毫秒与字节）、
`probe_raw_frame.rs`（m84-A 的画幅量具：dummy 与真解的 `active_area`/`crop_area`/`blackareas` 逐字段对撞、
  整块 vs 画幅两条腿的逐字节对撞、实测黑边宽度与呈现像素边缘均值，落左上角补丁给 `.temp/m84_diff.py` 拼对照图）。
`unwrap()` 只在 example（`smoke_raw.rs:72-76`）。
