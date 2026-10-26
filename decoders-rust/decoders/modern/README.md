# `modern` —— 现代格式合并档（HEIC / HEIF / AVIF / JPEG XL）

一枚 DLL 认领 6 个后缀：`heic heif hif avif avifs jxl`。
成员两个：`av`（HEIC/HEIF + AVIF，`heic` 开 `av1` feature）与 `jxl`（`jxl-oxide`）。

> 发布形态：`build.ps1` 产出 `modern.dll`，发布名 `modern_<ver>.dll`。
> 单库 `av` / `jxl` 不再发布——它们在 `decoders-rust/singles/` 里保留，只作 bench 与 A/B 的参考实现。

## 为什么这两个放一起

1. **崩溃域要与日常格式隔开**：`heic` 0.1.x、`jxl` 0.8.x 是整批库里最年轻的两个，
   一张坏 HEIC 三振出局，不该连累 `common` 那 29 个格式；
2. **低频但强绑定**：会打开 HEIC 的人几乎一定也会碰到别人发来的 JXL，反之亦然；
3. 同属"现代容器 + 现代编码"，将来接 AVIF 序列 / JPEG XL 动画时同族共享解析。

## 认领（`iv_probe`）

`iv_probe` 让两个成员各跑一遍探针，对外报**最高**的那个置信度（认=100，不认=0）：

| 成员 | 认什么 | 置信度 |
|---|---|---|
| `av` | ISOBMFF 容器里的 `heic/heif/hif/avif/avifs`（按 `ftyp` 品牌） | 100 |
| `jxl` | JXL 的两种容器：裸 codestream（`0xFF0A`）与 ISOBMFF 封装 | 100 |
| 其它 | — | 0 |

合并解码器对外只有一个身份，"我有多确定"必须取最确定的那个——否则一个报 100 的成员
会被同表里报 0 的成员拉平，宿主就可能把它排到别的解码器后面去。

## 渲染（`iv_render`）

两阶段，与宿主同口径：`out == NULL` 是查询（回填尺寸/格式），非空是渲染。

选人与让位都在 `rupix_merge_router`（**三个合并档共用一份**，本 crate 不复制逻辑）：
查询阶段按置信度逐个问，软失败（`ERR_UNSUPPORTED` / `ERR_UNRECOGNIZED`）换下一家；
渲染阶段也让位，但**换人前重问一次尺寸**——宿主是按上一个赢家申报的 `bytes_needed`
分配缓冲的，换人后可能更大，装不下就如实报 `ERR_BUFFER_TOO_SMALL`。

## 能力位

只声明 `CAP_FULL`：HEIC/AVIF/JXL 都是预测编码，没有"解码期降采样"的口子
（不给按 1/2、1/4 少算的机会），所以缩放一律交给基座 `core::geometry`。

## 构建

```powershell
# 单库（bench 参考实现；不是发布物）——必须从 decoders-rust/singles/ 目录跑
cd ..\..\bench
cargo build -p rupix-av --release
cargo build -p rupix-jxl --release

# 发布档：从仓库根跑（与 common/raw 同一个 workspace）
cd ..\..
cargo build --release -p rupix-modern
cargo test  --release -p rupix-modern -- --test-threads=1
```

测试：`claims_only_its_own_six_suffixes`（探针）、
`both_members_render_through_one_entry_point`（两阶段，两个成员各一张）、
`unknown_bytes_are_not_claimed_and_not_rendered`（不认就不解）。

## 历史

对外可见变化见 [`HISTORY.md`](HISTORY.md)。本页只保留当前状态、成员与限制。
