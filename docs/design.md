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

```
Capabilities
├─ ffmpeg_path, ffprobe_path, version, build_flags: HashSet<String>
├─ encoders, decoders, filters, bsfs, protocols, hwaccels: HashSet<String>
├─ probed: HashMap<EncoderId, ProbeResult>
│   └─ ProbeResult { usable, stderr, classified: Option<FailureKind> }
├─ tonemap_pipelines: Vec<ToneMapPipeline>   // 按质量排序的可用管线
├─ external: ExternalTools                   // dovi_tool / hdr10plus_tool / mkvmerge
└─ fingerprint: String                       // 缓存 key
```

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

探测结果缓存，key = ffmpeg 路径 + 文件修改时间 + 版本串 + GPU 名称 + 驱动版本。驱动更新后自动失效重测。

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

1. 用户在设置里手动指定的路径
2. 应用内置目录（引导下载后存放处）
3. `PATH`
4. 平台常见安装位置
   - Windows：winget Packages 目录、`~\scoop\apps\ffmpeg\current\bin`、chocolatey bin、`C:\ffmpeg\bin`、`C:\Program Files\ffmpeg\bin`
   - macOS：`/opt/homebrew/bin`、`/usr/local/bin`、`/opt/local/bin`

注意 Windows 上刚安装完 ffmpeg 时，注册表 PATH 已更新但正在运行的进程环境未刷新。因此第 4 步还要读取注册表中的 Machine/User PATH，而不只看当前进程的环境变量。这个场景在开发期已实际遇到。

### 5.2 VFR 判定必须用两级判据

只比较帧率字段是不够的。实测同一份可变帧率内容封装成不同容器后：

| 容器 | `r_frame_rate` | `avg_frame_rate` | 单看字段能否判出 |
|---|---|---|---|
| MP4 | `30/1` | `5100/299` ≈ 17.06 | 可以 |
| MKV | `30/1` | `30/1` | **不能** |

matroska 的帧率字段从 `default_duration` 推导，不反映实际帧间隔。所以判定用两级判据，任一命中即判为 VFR：

```
判据 1：|r_frame_rate - avg_frame_rate| / r_frame_rate > 1%
判据 2：采样前 120 帧的 duration_time，
        不同取值数 > 1 且 标准差/均值 > 1%
```

判据 2 的采样命令：

```bash
ffprobe -v error -select_streams v:0 -read_intervals "%+4" -show_frames         -show_entries frame=duration_time -of csv=p=0 input.mp4
```

输出里会混入 SEI side data 行，解析时需过滤非数值行。

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

三个 Zustand store，职责不重叠：

| store | 持有 | 来源 |
|---|---|---|
| `useCapabilityStore` | `Capabilities`、探测进度 | 启动时后端探测，只读 |
| `useProjectStore` | 导入的文件列表、当前选中、当前 `TranscodePlan`、`Resolution` | 用户操作 + 后端推荐 |
| `useQueueStore` | 任务列表与实时进度 | 后端事件推送 |

前端不做任何编码决策。场景推荐、参数校验、保真度求解全部走后端，前端只负责展示和收集输入。这样决策逻辑只有一份实现，且可被 Rust 测试覆盖。

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
