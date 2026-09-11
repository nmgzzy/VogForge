# VidForge 设计文档

版本 0.1 · 2026-09-11

配套文档：[需求](requirements.md) · [实施计划](plan.md) · [TODO](todo.md) · [ffmpeg 技术事实](ffmpeg-facts.md)

## 1. 架构总览

```
┌─────────────────────────────────────────────────────────┐
│  前端 React + TypeScript (src/)                          │
│  导入 · 参数编辑 · 保真度勾选 · 队列 · 环境 · 设置          │
└───────────────────────┬─────────────────────────────────┘
                        │ Tauri invoke / event
┌───────────────────────▼─────────────────────────────────┐
│  src-tauri/  —— 仅做胶水层                               │
│  command 注册 · 事件推送 · 窗口与文件对话框 · ts-rs 导出    │
└───────────────────────┬─────────────────────────────────┘
                        │ 纯 Rust 函数调用
┌───────────────────────▼─────────────────────────────────┐
│  crates/vidforge-core/  —— 全部业务逻辑，无 Tauri 依赖     │
│                                                          │
│  ffmpeg/    locate 定位 · capability 探测 · probe 分析     │
│             runner 执行 · progress 进度 · download 下载    │
│                                                          │
│  pipeline/  model 数据模型 · args 命令构建 ★              │
│             strategy 策略推荐 ★ · fidelity 保真求解 ★      │
│             color 色彩/HDR · audio 音频 · fps 帧率策略       │
│             container 容器 · estimate 预估                │
│                                                          │
│  queue/     scheduler 调度与回退 · persist 持久化          │
│  verify     输出校验与保真度报告                           │
│  external   可选外部工具探测                              │
└──────────────────────────────────────────────────────────┘
                        │ 子进程
                  ffmpeg / ffprobe
```

### 1.1 两条不可动摇的架构原则

**原则一：业务逻辑全部在 `vidforge-core`，且不依赖 Tauri。**
动机是双重的：单元测试不需要起 GUI；将来要出命令行版本时只需新增一层外壳。`src-tauri` 里不允许出现任何决策逻辑，只做参数转换和事件转发。

**原则二：标★的四个模块是纯函数，不做 IO。**
`args` / `strategy` / `fidelity` / `container` 接收 `Capabilities` 结构体作为输入，而不是自己去探测环境。这样同一份输入永远得到同一份输出，可以用快照测试把上百种参数组合锁死。

这条原则是整个项目的质量核心。原因很直接：ffmpeg 参数写错一个字，转码会正常完成、退出码为 0，但元数据已经悄悄丢了。这类错误无法靠"跑一下看看"发现，只能靠可回归的测试网。

## 2. 技术选型

| 层 | 选择 | 理由 |
|---|---|---|
| 桌面框架 | Tauri 2 | 安装包约 15MB（Electron 约 200MB）；内存占用约 1/3；长时间跑批量任务时 Rust 管理子进程更可靠 |
| 前端 | React 19 + TypeScript + Vite | 生态成熟，类型安全 |
| 样式 | Tailwind CSS v4 | 配置极简，设计一致性好 |
| 状态 | Zustand | 轻量，无样板代码 |
| 图标 | Lucide React | 线性风格，适合工具类界面 |
| 后端语言 | Rust (stable) | Tauri 要求；进程管理与流解析稳健 |
| 异步运行时 | Tokio | Tauri 自带 |
| 类型共享 | ts-rs | 从 Rust 结构体生成 TS 类型，杜绝前后端结构漂移 |
| Rust 测试 | 内置 test + insta | insta 做命令构建的快照测试 |
| 前端测试 | Vitest | 与 Vite 同源 |

## 3. 核心数据模型

定义在 `pipeline/model.rs`，通过 ts-rs 导出到 `src/bindings/`。

### 3.1 MediaInfo —— ffprobe 分析结果

```
MediaInfo
├─ path, container, duration_sec, size_bytes, overall_bitrate
├─ video: Vec<VideoStream>
│   ├─ index, codec, profile, level, width, height
│   ├─ fps_avg, fps_r, is_vfr
│   ├─ bit_depth, pix_fmt, bitrate
│   ├─ color: ColorInfo { primaries, transfer, space, range, hdr_kind }
│   ├─ hdr10: Option<Hdr10Metadata>      // MDCV + MaxCLL/MaxFALL
│   ├─ dolby_vision: Option<DoviInfo>    // profile, bl_compat_id, has_rpu, has_el
│   ├─ hdr10plus: bool
│   └─ rotation
├─ audio: Vec<AudioStream>
│   ├─ index, codec, profile, channels, channel_layout
│   ├─ sample_rate, bitrate, language, title, is_default
│   └─ lossless: bool, atmos: bool, dts_x: bool
├─ subtitle: Vec<SubtitleStream>          // codec, language, is_image_based
├─ chapters: Vec<Chapter>
├─ attachments: usize
└─ source_hint: SourceHint                // iPhone | GoPro | DJI | Camera | ScreenRec | BluRay | Streaming | Unknown
```

`hdr_kind` 取值：`None` / `HDR10` / `HLG` / `PQ_NoMeta`（标了 PQ 但无元数据）。

`Hdr10Metadata` 的亮度与色度值一律存为 `f64`（已求值），不保留 `"34000/50000"` 这类有理数字符串。原因见技术事实文档第 2 节：HEVC 与 AV1 的定点分母不同，字符串比对必然误判。

实现补充（`model/media.rs`、`ffmpeg/probe.rs`）：

- `id` 由规范化路径派生（Windows 下大小写不敏感），同一文件重复导入得到同一个 id，列表据此去重。
- `import_root` 记录通过文件夹导入时的根目录，"保留源目录结构"据此计算相对路径。
- 色彩字段优先取流级，流级是 unknown 时取解码出的首帧（技术事实文档 12 节）；HDR10 元数据优先取首帧 side data，其次流级。
- 杜比视界增强层类型（MEL / FEL）只能从 RPU 看出，读首帧 `Dolby Vision Metadata` 里的 `disable_residual_flag`。
- 首帧与包的采样按流序号选第一条真实视频流：MP4 的封面图也是 `codec_type=video`，`v:0` 可能选中它。
- 没有视频流的文件（纯音频、只有封面图）拒绝导入：命令构建以视频为中心，放进来只会得到跑不起来的命令。

导入（`import.rs`）：拖入或选中的文件按用户意图直接分析；文件夹递归扫描，只收视频扩展名，跳过以点开头的条目（含 macOS 在 NAS 上留下的 `._` 资源分叉文件）、回收站、系统卷信息与 Windows 隐藏/系统属性的条目，不跟随目录符号链接。读不了的文件夹记为失败而不是当成空目录。4 路并发分析；分析进行中再拖入的路径排队，当前批次完成后接着处理，结果合并成一份报告。

### 3.2 TranscodePlan —— 一次转码的完整描述

```
TranscodePlan
├─ video: VideoPlan
│   ├─ action: Copy | Encode
│   ├─ encoder: EncoderId                // libx265 | hevc_qsv | hevc_nvenc | ...
│   ├─ rate_control: Crf | Cq | Bitrate{target,max,bufsize} | TwoPass
│   ├─ preset, profile, level, pix_fmt, bit_depth
│   ├─ scale: Option<Scale>, fps: FpsPolicy
│   ├─ hdr_action: Keep | ToneMapToSdr(pipeline) | StripMetadata
│   ├─ dolby_vision: Preserve | Disable | Remux
│   ├─ extra_x265_params, extra_args
│   └─ decision_notes: Vec<Decision>
├─ audio: Vec<AudioTrackPlan>             // 每条输出轨：源轨索引 + Copy/Encode + 标题
├─ subtitle: SubtitlePlan
├─ container: Mkv | Mp4
├─ metadata: MetadataPlan                 // 章节/附件/全局 metadata
├─ output: OutputPlan                     // 命名模板、冲突策略、目录结构
└─ fidelity: FidelityRequest              // 用户勾选的保留项
```

`Decision` 是界面"为什么这么选"的数据来源：

```
Decision { field: String, value: String, reason_zh: String, severity: Info | Warn }
```

### 3.3 Capabilities —— 环境能力快照

定义在 `model/caps.rs`。

```
Capabilities
├─ status: Ready | Probing | Missing | TooOld | Broken, status_detail   // 能否开始转码
├─ ffmpeg_path, ffprobe_path, locate_source, searched, notes
├─ version（原始版本串）, version_number（9.0.1 或"7.1+（开发版）"）, build_source
├─ build_flags: Vec<BuildFlag { name, present, affects }>   // 按组件是否存在判断
├─ encoders: Vec<EncoderProbe>          // 第 3 层：当前平台关心的全部编码器
│   └─ EncoderProbe { id, vendor, codec, usable, ten_bit, error, failure: Option<FailureKind> }
├─ hwaccels, devices: Vec<DeviceProbe { id, available, error }>   // 第 2 层
├─ tonemap: Vec<TonemapProbe { id, available, note }>             // 按 5.3 的顺序
├─ dolby_vision_encode, dovi_split
├─ external: Vec<ExternalTool>          // dovi_tool / hdr10plus_tool / mkvmerge
├─ gpus: Vec<GpuInfo { name, driver }>, platform, probed_at
└─ fingerprint: String                  // 缓存 key
```

`FailureKind` 除 4.2 节的五类外，还有 `NotBuilt`：当前 ffmpeg 根本没编译这个编码器。探测完成前，前端用 `status = Probing` 的占位快照：软件编码器按可用处理，硬件与色调映射一律不可用，这样界面不必等探测结束就能给出软编方案。

## 4. 关键设计

### 4.1 能力探测（`ffmpeg/capability.rs`）

三层递进，缺一不可：

| 层 | 做什么 | 为什么必需 |
|---|---|---|
| 1 编译能力 | `-buildconf` `-version` `-encoders` `-filters` `-bsfs` `-protocols` `-hwaccels` | 判断 libplacebo / libzimg / libvmaf / libsvtav1 是否编译进去 |
| 2 设备初始化 | `-init_hw_device qsv=d` 等，退出码 0 才算有设备 | 区分"没有这个硬件"与"参数不对" |
| 3 真实试编码 | `-f lavfi -i testsrc2 -frames:v 3 ... -f null -` | 编码器存在不等于可用。驱动缺失时 `h264_nvenc` 照样出现在列表里，一用就失败 |

第 3 层的两个细节：

- 用 `-f null -` 而不是 Windows 的 `NUL`。`NUL` 不可 seek，MP4 muxer 会因此失败，引入与编码器无关的噪声。
- 跑 3 帧而不是 1 帧。硬件编码器有 lookahead 队列，1 帧可能走不到真正的编码路径。

- 10bit 试编码之前，先确认 10bit 像素格式出现在 `-h encoder=` 的支持列表里。ffmpeg 遇到不支持的格式会静默换成 8bit 继续编码，只看退出码会误判（技术事实文档 7.6 节）。
- 编译开关按组件是否存在判断（编码器、滤镜、协议），不看 configuration 行，因为自动检测到的库不会出现在 `--enable-` 列表里。
- 色调映射管线除了检查滤镜存在，还要检查依赖设备（libplacebo 依赖 Vulkan、tonemap_opencl 依赖 OpenCL），最后用带 BT.2020/PQ 标签的测试图真跑 2 帧。

三层探测用 4 路并发，开发机上完整跑一遍约 4–5 秒。结果缓存在 `~/.vidforge/capabilities.json`，key = ffmpeg 路径 + 文件修改时间 + 文件大小 + 版本串 + GPU 名称 + 驱动版本，外加缓存格式版本号，驱动更新或探测逻辑变化后自动失效重测。命中缓存时仍会重新查找外部工具，开销可以忽略。

**任务级 dry-run**：每个任务开始前，用这个任务真实的编码参数（preset/profile/pix_fmt/RC 全套）换成 lavfi 输入跑 3 帧。耗时不到 1 秒，能抓住"设备可用但这组参数不支持"的情况，比跑了 20 分钟才失败要好得多。

### 4.2 硬件编码回退（`queue/scheduler.rs`）

失败后先按 stderr 分类，再决定动作。盲目重试或直接放弃都不对。

| 分类 | 判据（stderr 子串） | 动作 |
|---|---|---|
| DeviceMissing | `Cannot load nvcuda.dll` / `Cannot load nvEncodeAPI64.dll` / `DLL amfrt64.dll failed to open` / `No capable devices found` / `Error initializing an internal MFX session` | 本次会话永久禁用该厂商，直接回退软编，不再重试同厂商 |
| Capability | `10 bit encode not supported` / `Codec not supported` | 降级参数重试一次：10bit→8bit、main10→main |
| Param | `Selected ratecontrol mode is unsupported` / `Current pixel format is unsupported` / `Invalid argument` | 去掉可选参数（`-low_power` / `-multipass` / `-tune`）重试一次 |
| Resource | `out of memory` / session 数超限 | 降低并发、串行化，指数退避重试 3 次 |
| Unknown | 其他 | 回退软编 |

回退链按平台区分（HEVC 为例，H.264 与 AV1 同构）：

- Windows：`hevc_nvenc` → `hevc_qsv` → `hevc_amf` → `libx265`
- macOS：`hevc_videotoolbox` → `libx265`

两条硬规则：

1. 任务运行超过 10 秒才失败时不静默回退。此时输出文件可能已写入大量数据，先删除再用软编从头重跑，并明确告知用户。
2. 以下情形跳过探测直接禁用硬编：需要杜比视界 RPU、需要码流内 HDR10+、或用户要求 CRF 级别的质量一致性（硬编的 CQ 与 CRF 不等价）。

所有回退都必须在任务日志里写明原因并在界面可见。静默回退到软编会让用户以为"这软件怎么这么慢"。

### 4.3 命令构建（`pipeline/args.rs`）

签名：`fn build(plan: &TranscodePlan, media: &MediaInfo, caps: &Capabilities) -> Result<Vec<String>>`

返回 argv 数组而非字符串，避免引号转义问题。参数顺序固定分段，便于快照测试阅读：

```
[全局]   -hide_banner -nostdin -loglevel ... -progress ...
[硬解]   -hwaccel ... -hwaccel_output_format ...
[输入]   -i <path>
[映射]   -map ...
[视频]   -c:v ... -pix_fmt ... [RC] [preset/profile] [色彩标签] [-x265-params]
[滤镜]   -vf / -filter_complex
[音频]   -c:a:N ... -b:a:N ... -ac:a:N ... -metadata:s:a:N ...
[字幕]   -c:s ...
[元数据] -map_chapters -map_metadata
[容器]   -tag:v hvc1 -movflags +faststart / -strict unofficial
[输出]   <tmp_path>
```

构建过程中的每个非平凡决定都往 `decision_notes` 追加一条 `Decision`，界面直接展示。

### 4.4 保真度约束求解（`pipeline/fidelity.rs`）

产品的核心差异点。签名：

```
fn resolve(req, media, plan, caps) -> Resolution

Resolution { items: Vec<FidelityItem> }
FidelityItem {
  kind: FidelityKind,
  state: Achievable | NeedsChange(Vec<Condition>) | Impossible(Reason),
  fixes: Vec<Fix>          // 每个 Fix 是对 TranscodePlan 的一个补丁 + 一句中文说明
}
```

判定规则表：

| 勾选项 | 可达成条件 | 冲突与修正 |
|---|---|---|
| 杜比视界（单层 P5/8.1/8.4） | ffmpeg ≥ 7.1 且 libx265 有 `dolbyvision` 选项；编码器为 libx265 或 libsvtav1；`pix_fmt = yuv420p10le` | 选了硬编 → 修正为 libx265（提示 GPU 编码将禁用）；ffmpeg 过低 → 修正为原样封装 |
| 杜比视界（P7 双层） | v1 不可达成 | 修正选项：降级为 8.1（说明会丢 FEL 映射）或原样封装保持双层 |
| HDR10 静态元数据 | 保持 10bit + BT.2020/PQ；编码器属于 libx265 / libx264 / libsvtav1 / hevc_qsv / hevc_nvenc / hevc_amf | 目标为 SDR → 说明将色调映射，元数据不再适用；编码器为 videotoolbox → 修正为 libx265 |
| HDR10+ 动态元数据 | v1 不可达成（ffmpeg 无法透传给 libx265） | 灰显，说明需 x265 CLI 或事后注入 |
| 杜比全景声 / 无损音轨 | 该音轨 action = Copy 且容器 = MKV | 容器为 MP4 → 修正为 MKV；音轨设为 Encode → 修正为 Copy 并追加兼容轨 |
| 全部音轨 | 容器支持所有源音轨编码 | 列出不兼容轨道，修正为切换容器 |
| 全部字幕 | 图形字幕（PGS/VobSub）要求容器 = MKV | MP4 → 修正为 MKV，或烧录（v2） |
| 章节 / 附件 | 容器 = MKV（MP4 章节支持有限，附件不支持） | 修正为 MKV |
| 10bit 位深 | 编码器支持 10bit（已通过第 3 层探测确认） | 修正为降 8bit 或改软编 |

`Fix` 点一下就应用到 Plan。这比只告诉用户"不行"要有用得多。

### 4.5 场景策略引擎（`pipeline/strategy.rs`）

`fn recommend(media, intent, caps) -> TranscodePlan`

表驱动规则，每条规则产出一个 `Decision`。预设与目标：

| 场景 | 编码 | 质量 | 音频 | 容器 | HDR |
|---|---|---|---|---|---|
| 相机素材归档 | HEVC 10bit 软编 | CRF 20-22 | 主轨 copy 或 AAC 256k | MKV | 保留 |
| NAS 高画质收藏 | HEVC/AV1 10bit 软编 slow | CRF 18-20 | 全部 copy | MKV | 全保留 |
| 流媒体直出 | H.264/HEVC 8bit 硬编优先 | CQ 对应 CRF 21-23 | AAC 2.0 + EAC3 5.1 | MP4 | 视播放端决定 |
| 手机平板观看 | H.264 High 8bit | CRF 23，限 1080p | AAC 2.0 128k | MP4 | 色调映射 SDR |
| 社交分享 | H.264 High 8bit | CRF 23，限 1080p | AAC 2.0 | MP4 + faststart | 色调映射 SDR |
| 最小体积 | AV1 或 HEVC | CRF 26-28 | Opus 96k | MKV | 色调映射 SDR |
| 剪辑预处理 | H.264/HEVC 8或10bit，短 GOP | CRF 16-18（高码率） | 全部 copy 或 PCM | MOV/MKV | 保留 |
| 原样封装 | copy | — | copy | MKV | 全保留 |

内建的常识保护（多数工具缺失，这些是"用户友好"的实质）：

| 规则 | 说明 |
|---|---|
| 不重复压缩 | 源码率已低于目标预估码率 → 提示不建议转码，推荐原样封装 |
| 不放大 | 目标分辨率高于源时自动回退到源分辨率 |
| 不提帧率 | 目标帧率高于源时自动回退 |
| VFR 按用途分岔 | 归档与播放用途保持可变帧率；剪辑与分发用途转固定帧率。详见 4.9 |
| HDR 必须映射 | HDR 源输出 SDR 时强制插入色调映射滤镜，绝不只改色彩标签 |
| 高价值内容提醒 | 检测到杜比视界 / Atmos / 无损音轨时主动建议启用保真度保留 |
| P5 警告 | 杜比视界 Profile 5 无 HDR10 回退层，非 DV 播放器会显示绿/紫画面 |
| 只用编得了的编码器 | 软件编码器没编译进当前 ffmpeg 时改用同格式的硬件编码器；整个格式都编不了时换一种能编的格式并给出警告；界面把编不了的格式置灰。绝不生成一条跑不起来的命令 |

### 4.6 进度解析（`ffmpeg/progress.rs`）

`-progress pipe:1 -nostats` 的输出是块协议，不是逐行协议。每个周期输出一组字段，以 `progress=continue`（或末次 `progress=end`）收尾。解析器必须按块累积后再提交一次状态更新。

三个已知坑：

1. `out_time_ms` 字段的单位实际是微秒（ffmpeg 历史遗留，为兼容性未修）。只使用 `out_time_us`。
2. 开头几块里 `bitrate` / `speed` / `total_size` 可能是 `N/A`；纯音频任务没有 `frame` / `fps` 字段。
3. `speed` 是近期平均值，前几块不可信。

ETA 算法：

```
eta = (duration_sec - out_time_us / 1e6) / speed_smoothed
speed_smoothed = 0.8 * prev + 0.2 * current
```

前 5 秒不显示 ETA。另外同时维护一个全程平均速度 `out_time / wall_clock`，与瞬时速度加权（后期偏向瞬时），避免场景切换导致 ETA 跳动。

### 4.7 队列调度（`queue/scheduler.rs`）

**并发票据模型**。软编任务占 1 张 CPU 票（x265 自己会吃满多核，并行跑两个只会互相拖慢）；硬编任务占 1 张 GPU 票（默认 1，可配到 2）。两类票独立，所以一个软编加一个硬编可以同时跑。

输出安全：

- 先写 `<目标名>.vidforge-part`，成功后才改名为最终名。取消或崩溃都不会在目标目录留下半成品。
- 同名冲突策略：跳过 / 自动加序号 / 覆盖（覆盖需二次确认）。
- 源文件绝不自动删除。"完成后动作"默认"无操作"。

持久化：队列状态写 `~/.vidforge/queue.json`，应用重启后恢复未完成任务。恢复是重新开始而非续传，因为 ffmpeg 本身不支持断点续传；分段编码加 concat 的方案留到 v2。

### 4.8 输出校验（`verify.rs`）

转码后对输出跑 ffprobe，逐项比对：

| 项 | 判据 |
|---|---|
| 时长 | 与源差值 < 0.5 秒 |
| 帧数 | 未改帧率时应与源一致（容差 1 帧） |
| 流数量 | 音轨 / 字幕数符合 plan 预期 |
| 色彩标签 | primaries / transfer / space 符合 plan |
| HDR10 元数据 | 若勾选保留，MDCV 与 CLL 必须存在，且按有理数求值比较数值（容差 1e-4） |
| 杜比视界 | 若勾选保留，流级配置记录与首帧 RPU side data 必须存在 |
| 音频编码 | 标记 copy 的轨道，编码必须与源一致 |

输出为保真度报告：用户勾选的每一项对应实际结果，未达预期标红并给出原因。存在未修正冲突就提交的任务，报告必须如实显示"未保留"，完成事件也以警告记录，不得一律写"校验通过"。这让"尽量保留"成为可验证的事实，而不是口头承诺。

### 4.9 可变帧率转固定帧率（`pipeline/fps.rs`）

手机拍摄的视频几乎都是可变帧率。这类文件进剪辑软件会因帧时间戳不规则导致音画逐渐累积失步，是手机素材进入剪辑流程时最常见的问题。但转固定帧率并非总是更好，所以设计成按用途分岔而非一刀切。

按用途分岔的决策：

| 用途（Intent） | 帧率策略 | 理由 |
|---|---|---|
| 剪辑预处理 | 转 CFR | 剪辑软件对 VFR 支持差，会累积音画偏差 |
| 流媒体分发 | 转 CFR | 部分播放器与转码服务对 VFR 处理不一致 |
| 相机素材归档 | 保持 VFR | 播放器能正确处理，不引入重复帧，体积更小 |
| 日常播放、手机观看 | 保持 VFR | 同上 |
| 原样封装 | 保持 VFR | 不重编码 |

用户始终可以手动覆盖，界面显示当前选择的理由。

帧率策略的数据模型：

```
FpsPolicy
├─ KeepSource                      // 不传帧率参数，保持原时间戳
├─ ConstantRate { fps: Rational }  // 转 CFR
└─ CapAt { max_fps: Rational }     // 仅当源超过上限时下调
```

生成的参数：

实测对比了 `-r`、`-fps_mode:v cfr -r`、`fps` 滤镜三种写法，数据见 [ffmpeg-facts.md 4.3 节](ffmpeg-facts.md#43-vfr-转-cfr三种方案实测对比)。据此确定：

- 生成 `-fps_mode:v cfr -r <target>`。比只写 `-r` 更自解释，意图明确。
- 不使用 `fps` 滤镜，它会丢掉最后一帧，正好留下要消除的那种偏差。
- 不生成 `-vsync`。该选项在 ffmpeg 9.0 已被移除，生成了会直接报未知选项。
- 源音频时间戳有抖动时，对重新编码的音轨追加 `-af aresample=async=1`；原样复制的音轨实测无需处理。

目标帧率的推荐算法：

```
1. 候选 = 源的 r_frame_rate（名义帧率）
2. 吸附到最近的标准帧率档（容差 2%）：
   23.976 / 24 / 25 / 29.97 / 30 / 50 / 59.94 / 60 / 120
3. 若候选异常（如录屏常见的 1000/1），改用 avg_frame_rate 再吸附
4. 产出 Decision：将从 N 帧变为 M 帧，复制 M-N 帧
```

吸附时 29.97 与 30 这类相邻档的取舍规则见 [ffmpeg-facts.md 4.4 节](ffmpeg-facts.md#44-目标帧率的选择)：误差小于 0.1% 才采纳 NTSC 档，否则优先整数档。

取名义帧率而非实际平均帧率，是因为前者只会复制帧、不丢帧，所有原始画面都保留；后者会丢帧且得到非标准帧率（如 17.06fps），剪辑软件同样不友好。

体积与极端情况：

转 CFR 会复制帧，体积通常上升。帧率波动极端时（例如录屏从 1fps 跳到 60fps）会产生大量重复帧，体积显著增加。此时产出 `Warn` 级别的 `Decision`，建议用户改用较低目标帧率或保持 VFR。

重复帧对编码器很友好（几乎零残差），实际体积增幅远小于帧数增幅，界面预估时应按此修正而非按帧数线性外推。

校验：

ffmpeg 的 `-progress` 输出里有 `dup_frames` 与 `drop_frames`，可以直接告诉用户实际复制或丢弃了多少帧。

输出校验额外检查两项：

```
r_frame_rate == avg_frame_rate                     // 确实是 CFR
|视频时长 - 音频时长| < 1 个帧时长                   // 音画对齐
```


## 5. 若干实现细节

### 5.1 ffmpeg 定位顺序

1. 用户在设置里手动指定的路径（目录或 ffmpeg 可执行文件本身都可以）
2. 应用内置目录 `~/.vidforge/ffmpeg/bin`（引导下载后存放处）
3. 当前进程的 `PATH`
4. 注册表中的 Machine/User PATH（仅 Windows）
5. 平台常见安装位置
   - Windows：winget 的 Links 与 Packages 目录、`~\scoop\apps\ffmpeg\current\bin`、`~\scoop\shims`、chocolatey bin、`C:\ffmpeg\bin`、`C:\Program Files\ffmpeg\bin`
   - macOS：`/opt/homebrew/bin`、`/usr/local/bin`、`/opt/local/bin`（从 Finder 启动的应用不继承 shell 的 PATH）

Windows 上刚安装完 ffmpeg 时，注册表 PATH 已更新但正在运行的进程环境未刷新，所以第 4 步要读注册表，而不只看进程环境变量。这个场景在开发期已实际遇到，从 Git Bash 启动的应用也是靠这一步找到 ffmpeg 的。

取舍规则：

- 候选目录按大小写不敏感去重（Windows）。同一目录必须同时有 ffmpeg 与 ffprobe，且两者都能输出版本信息才算找到。
- 目录里有 ffmpeg 却用不了（缺 ffprobe、无法运行、输出不可识别）时记为 `Broken`，与根本没装（`Missing`）区分开，界面给出的建议不同。
- 用户指定的路径总是被采用，即使版本过低（这是用户的明确选择，界面会提示升级）。
- 其余来源优先采用第一个满足最低版本的；都不满足时才退回第一个找到的旧版本。避免 PATH 里一个旧版本挡住常见位置里的新版本。

### 5.2 VFR 判定必须用两级判据

只比较帧率字段是不够的。实测同一份可变帧率内容封装成不同容器后：

| 容器 | `r_frame_rate` | `avg_frame_rate` | 单看字段能否判出 |
|---|---|---|---|
| MP4 | `30/1` | `5100/299` ≈ 17.06 | 可以 |
| MKV | `30/1` | `30/1` | **不能** |

matroska 的帧率字段从 `default_duration` 推导，不反映实际帧间隔。所以判定用两级判据，任一命中即判为 VFR：

```
判据 1：|r_frame_rate - avg_frame_rate| / r_frame_rate > 1%
判据 2：前 120 个视频包的 pts_time 排序后求间隔，
        偏离中位数 20% 以上的间隔 ≥ 2 个且占比 ≥ 2%
```

判据 2 不能用 `duration_time`：MKV 里它也取自 `default_duration`，对可变帧率内容同样是常数；阈值也不能用变异系数，毫秒取整会让 23.976 fps 的电影误判。细节与实测数据见技术事实文档 4.2 节。采样命令：

```bash
ffprobe -v error -select_streams v:0 -read_intervals "%+#120" -show_packets -show_entries packet=pts_time -of csv=p=0 input.mkv
```

### 5.3 色调映射管线选择

按可用性降级，顺序固定：

| 优先级 | 管线 | 依赖 | 备注 |
|---|---|---|---|
| 1 | libplacebo | `--enable-libplacebo` + Vulkan | 质量最好，且是唯一能正确处理杜比视界 P5 的 |
| 2 | tonemap_opencl | `--enable-opencl` + OpenCL runtime | 有场景自适应峰值检测；输出限 8bit |
| 3 | zscale + tonemap | `--enable-libzimg` | 纯 CPU，慢但总能跑 |
| 4 | scale_vt（macOS） | — | 实际不做感知色调映射，仅兜底 |

选择规则：

- 按上表顺序取第一个在 `Capabilities` 中可用的管线；已选管线在当前环境不可用时，自动换成可用的。
- 全部不可用时不生成任何色调映射滤镜，保留 HDR，在推荐说明里给出警告，"转为 SDR"选项置灰。
- `scale_vt` 只接受 VideoToolbox 硬件帧，需要 `-hwaccel videotoolbox -hwaccel_output_format videotoolbox_vld`；后接软件编码器时再 `hwdownload,format=nv12`。

macOS 的 Homebrew 构建三条管线全缺，这是跨平台最大的坑，必须引导用户换构建。

### 5.4 音频双轨策略

检测到无损或 Atmos 音轨且用户勾选保留时，默认生成两条输出轨：

1. 原轨 `copy`，标题标注"TrueHD 7.1 Atmos（原始）"
2. 兼容轨，按容器选择：MKV 用 Opus 或 AAC；MP4 用 AAC 2.0 加可选 E-AC-3 5.1

多声道降混到 2.0 时使用显式 `pan` 矩阵（中置 +3dB 增强对白）加 `alimiter` 防削波，而非简单 `-ac 2`。需要响度标准化时使用两遍 `loudnorm`，单遍会有起始段 ramp-up 问题。

## 6. 前端设计

### 6.1 界面结构

```
┌──────────────────────────────────────────────────────┐
│ 标题栏：VidForge         [环境状态徽章] [主题] [设置]  │
├────────┬─────────────────────────────────────────────┤
│        │  主工作区                                    │
│ 侧栏   │  ┌─ 文件列表 ─────────────────────────┐     │
│        │  │ 缩略信息 + 高价值特征徽章            │     │
│ 转码   │  └────────────────────────────────────┘     │
│ 队列   │  ┌─ 参数面板 ─────────────────────────┐     │
│ 环境   │  │ L1 场景选择                         │     │
│ 设置   │  │ L2 关键旋钮                         │     │
│        │  │ 保真度勾选清单（三态徽章 + 修正）     │     │
│        │  │ L3 专家面板（折叠）                  │     │
│        │  └────────────────────────────────────┘     │
├────────┴─────────────────────────────────────────────┤
│ 命令预览条：ffmpeg -i ... [复制]      [加入队列]      │
└──────────────────────────────────────────────────────┘
```

### 6.2 参数分层

| 层 | 内容 | 默认状态 |
|---|---|---|
| L1 场景 | 8 个场景卡片，每张只显示标题与不超过 7 字的短说明，完整描述在悬停提示里 | 展开 |
| L2 关键参数 | 画质档位、编码格式、分辨率、编码器、容器、HDR（仅 HDR 源）、帧率、音频 | 展开 |
| L3 更多参数 | 色深、字幕、preset、质量数值、GOP、色调映射管线、`-x265-params`、附加参数 | 折叠，标题栏显示当前取值摘要 |

色深与字幕通常由场景决定，放在 L3。质量选择在 L2 用语义档位（视觉无损 / 高 / 标准 / 小体积），在 L3 才露出具体数值。质量档位到数值的映射按编码器分别定义，并在界面标注"跨编码器不等价"——x265 CRF 18、NVENC CQ 26、QSV global_quality 24 之间没有统一刻度。

### 6.3 命令预览条

常驻底部，实时显示将要执行的完整 ffmpeg 命令，可一键复制。对专业用户这是信任的基础；对开发者这是最好的调试工具。

复制时按目标终端加引号：Windows 用 PowerShell 规则，macOS 用 POSIX 规则，两者都用单引号。含逗号或以 `@` 开头的参数必须加引号——PowerShell 会把逗号当数组运算符拆开滤镜链，把 `@` 开头当成 splatting。"附加 ffmpeg 参数"输入框按引号拆分，`-metadata title="My Video"` 是两个参数而不是三个。

### 6.4 保真度勾选清单的视觉语言

| 状态 | 视觉 | 交互 |
|---|---|---|
| 可保留 | 绿色标签，单行 | 详细说明在悬停提示里 |
| 需调整 | 琥珀色整行卡片，展开原因 | 显示"一键修正"按钮 |
| 无法保留 | 红色整行卡片，展开原因 | 给出替代方案（如原样封装） |
| 未勾选 | 灰色"未要求" | 勾选后才判定是否冲突 |

冲突项排在最前；其余项在宽度足够时两列排布。源文件里不存在的内容（如没有章节）合并成一行"不适用"。

### 6.5 状态管理

Zustand store，职责不重叠：

| store | 持有 | 来源 |
|---|---|---|
| `useCapabilities` | `Capabilities`、探测进度、调用错误 | 启动时后端探测（优先缓存），只读 |
| `useSettings` | 用户设置 | 后端读写 `~/.vidforge/config.json` |
| `useProject` | 导入的文件列表、当前选中、各文件的 `TranscodePlan` | 用户操作 + 后端推荐 |
| `useQueue` | 任务列表与实时进度 | 后端事件推送 |

能力快照变化（例如探测完成）后，`useProject.refreshPlans` 按新能力重新整理全部计划。

前端不做任何编码决策。场景推荐、参数校验、保真度求解全部走后端，前端只负责展示和收集输入。这样决策逻辑只有一份实现，且可被 Rust 测试覆盖。

**后端适配层。** 前端通过 `src/backend/` 的 `Backend` 接口访问后端：运行在 Tauri 窗口里时是 `invoke` 与事件监听，浏览器预览和组件测试里是 mock 实现。store 只依赖接口，不感知运行环境。命令与事件名：

| 命令 / 事件 | 作用 |
|---|---|
| `get_capabilities(force)` | 探测环境；`force = false` 时优先用缓存。并发调用会串行化 |
| `get_settings` / `save_settings(settings)` | 读写设置，后端把越界值拉回合理范围后返回 |
| 事件 `probe://progress` | 探测进度 `ProbeProgress { stage, done, total }` |

### 6.6 信息密度原则

同一屏里，正常状态保持安静，问题状态才醒目；默认只给结论，解释按需展开。

- 推荐说明默认只显示"字段 · 取值"；警告与提示类条目展开理由，普通条目点击单行或"展开全部说明"再看。
- 同一条提示只出现一处。例如可变帧率的剪辑风险由帧率控件提示，推荐说明里不再重复。
- 预估卡片只保留体积区间、占源比例、一条对比条、耗时与编码方式。

## 7. 测试策略

| 层 | 范围 | 工具 |
|---|---|---|
| 单元 | probe JSON 解析、progress 块协议解析、容器矩阵、保真度求解、策略规则、预估边界 | `cargo test` |
| 快照 | 命令构建：覆盖 20+ 场景 × 编码器 × 容器 × 保真度组合 | `insta` |
| 集成 | 用合成素材跑完整管线并用 ffprobe 校验输出 | `cargo test --test integration` |
| 前端 | store 逻辑、保真度状态渲染、参数映射 | Vitest |
| 手动 | 见需求文档第 6 节验收标准 | 清单核对 |

**合成测试素材**是集成测试能跑起来的关键。用 ffmpeg 自己生成带 BT.2020/PQ 加 MDCV/MaxCLL 的 HDR10 片段、多音轨片段、VFR 片段，这样测试不依赖用户手里的蓝光原盘，CI 里也能跑。生成脚本放 `tests/fixtures/`。

集成测试对环境能力敏感的部分（例如 QSV 编码）必须先查 `Capabilities` 再决定跳过还是执行，不能假定 CI 机器有 GPU。
