# ffmpeg 技术事实与陷阱

版本 0.1 · 2026-09-11

这份文档记录实现转码逻辑时必须知道的 ffmpeg 行为细节。每条都标注来源：

- **[源码]** 从 FFmpeg 源码或 changelog 核实
- **[文档]** 官方手册
- **[实测]** 在本项目开发机上实际验证过
- **[待验]** 尚未验证，代码中不得依赖

基线版本：FFmpeg **9.0.1**（2026-08 发布）。项目最低要求 **7.1**。

## 1. 版本门槛一览

| 能力 | 最低版本 | 来源 |
|---|---|---|
| HDR10 静态元数据自动透传（libx265/libx264/libsvtav1） | 7.0 | [源码] Changelog 7.0 |
| 杜比视界 RPU 编码（libx265 的 `-dolbyvision`） | **7.1** | [源码] commit 39ca87ed1，committer date 晚于 7.0 分支切出 |
| `dovi_rpu` bitstream filter | 7.1 | [源码] |
| NVENC 写 HDR10 元数据 | 7.1 | [文档] Changelog 7.1 |
| QSV 默认码率控制从 VBR 改为 CQP | 7.0 | [文档] Changelog 7.0 |
| `dovi_split` bitstream filter（拆 DV P7 双层） | **9.0** | [文档] Changelog 9.0 |

网上大量文章称 `-dolbyvision` 从 7.0 起可用，这是错的。7.0 的 `libavcodec/libx265.c` 完全不含相关代码。

## 2. HDR10 静态元数据

### 2.1 软编自动透传，不要手写

FFmpeg ≥ 7.0 会把输入流解析出的 MDCV（母版显示）与 CLL（内容光亮度）side data 自动传给 libx265 / libx264 / libsvtav1。**[源码]**

执行顺序决定谁覆盖谁 **[源码]**：先自动处理 side data，再解析用户的 `-x265-params`，最后配置杜比视界。因此用户手写的 `master-display=` 会覆盖自动值。

**实现要求**：默认什么都不传，靠自动透传。只在用户在专家面板显式修改时才注入。

### 2.2 定点分母按编码格式不同 —— 关键陷阱

ffprobe 返回的是有理数字符串。分母随编码格式变化 **[实测]**：

| 格式 | 色度/白点 | max/min luminance | 1000 nits 的实际呈现 |
|---|---|---|---|
| HEVC | /50000 | /10000 | `"10000000/10000"` |
| AV1 (libsvtav1) | /65536 | max /256，min /16384 | `"256000/256"` |

本机验证方式：同一个 HDR10 源分别用 libx265 与 libsvtav1 编码，ffprobe 读出的 `max_luminance` 分别是上表两个值，都等于 1000 nits。

另外两个坑 **[实测]**（阶段 3 做媒体分析时发现）：

- AV1 的最低亮度是 Q18.14 定点，源里的 0.0001 nits 会被量化成 `"2/16384"` ≈ 0.000122。比较最低亮度时容差要放宽到 5e-5 量级，否则 HEVC 转 AV1 后会误报"元数据变了"。
- 同一个 AV1 文件，帧级 side data 是 `"256000/256"`，而 MKV 容器里的流级 side data 已被约分成 `"1000/1"`、`"17/25"`。所以连"只比分子"都不可行，必须求值。

**实现要求**：

1. `MediaInfo` 里一律存求值后的 `f64`，不存字符串。
2. 校验器比较时用浮点容差（1e-4），**绝不比较字符串或分子**。否则跨编码格式校验会全部误报失败。
3. 不要把 HEVC 的 `master-display` 字符串套用到 AV1。

### 2.3 手动注入的写法

```bash
# 注意：字符串里的顺序是 G、B、R，不是 RGB
-x265-params "hdr10=1:hdr10-opt=1:repeat-headers=1:\
colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:\
master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):\
max-cll=1000,400"
```

从 ffprobe 值换算：色度乘 50000、亮度乘 10000 后取整。**不能直接取分子** —— 某些路径会约分，例如 `17/25`。

两组常见母版值：

```
DCI-P3 母版（绝大多数 UHD 蓝光），1000 nits：
  G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)
BT.2020 母版，1000 nits：
  G(8500,39850)B(6550,2300)R(35400,14600)WP(15635,16450)L(10000000,1)
```

`repeat-headers=1` 让每个 IDR 重复 VPS/SPS/PPS 与 SEI，流式播放和拖动进度条都需要。

### 2.4 HLG

```bash
-vf "setparams=color_primaries=bt2020:color_trc=arib-std-b67:colorspace=bt2020nc:range=tv"
```

要点：

- 9.0 起 `-color_primaries` / `-color_trc` 输出选项不会写进输出，要用 `setparams` 滤镜给帧打标签（见第 12 节）。重编码 HLG 源时标签随帧沿用，不需要这一步。
- ffmpeg 的枚举名是 `arib-std-b67`（即 BT.2100 HLG）。
- HLG **不需要** MaxCLL / MDCV（那是 PQ 的东西，HLG 是 display-referred）。硬写上去部分电视会误判。
- iPhone 等设备拍的 HLG 素材通常同时是杜比视界 Profile 8.4。只要 HLG 不要 DV 时必须显式 `-dolbyvision 0`。

### 2.5 libaom 不写 HDR10

`libaomenc.c` 完全没有 MDCV / CLL 的 OBU 写入代码 **[源码]**。用 `libaom-av1` 编码 HDR 内容时，静态元数据不会进入码流，只能靠容器承载，而很多播放器只读码流。

**实现要求**：AV1 加 HDR 一律用 `libsvtav1`，不提供 libaom 选项。

## 3. 杜比视界

### 3.1 `-dolbyvision` 是三态，默认 auto

```
-dolbyvision <boolean>  E..V....... Enable Dolby Vision RPU coding (default auto)
```
**[实测]** 本机 ffmpeg 9.0.1 确认存在。

三态语义 **[源码]** `dovi_rpu.h`：`auto`(-1) 表示输入含 DV 元数据时才启用；`1` 强制启用（失败则报错退出）；`0` 禁用。

**实现要求**：

- 用户选"保留杜比视界" → 传 `-dolbyvision 1`，让失败显式可见。
- 用户选"不保留" → **必须显式传 `-dolbyvision 0`**。留空会被 auto 偷偷开启，导致输出带上用户不想要的 DV。

### 3.2 机制是重新生成，不是比特级透传

**[源码]** 解码器把 RPU 解析成 `AV_FRAME_DATA_DOVI_METADATA`，`ff_dovi_rpu_generate()` 重新序列化后挂到 `x265_picture.rpu`。

含义：逐帧动态元数据会保留，但 FFmpeg 的 RPU 写出器不支持的扩展块会丢失。不能向用户承诺"完全一致"，只能说"保留动态元数据"。

### 3.3 Profile 支持范围

**[源码]** `dovi_rpuenc.c`：

| Profile | 能否保留 | 说明 |
|---|---|---|
| 5 | 可以 | 单层 IPT-PQ-c2。**无 HDR10 回退层**，非 DV 播放器会显示绿/紫画面，必须警告用户 |
| 7（MEL/FEL 双层） | **不能** | ffmpeg 明确拒绝：`Coding of Dolby Vision enhancement layers is currently unsupported`。必须降级 8.1 |
| 8.1 | 可以 | 最通用，基础层是 HDR10，非 DV 设备可回退 |
| 8.4 | 可以 | 基础层是 HLG，iPhone 拍摄格式 |
| 10.x (AV1) | 可以 | 用 libsvtav1 的 `dolbyvision` |

严格合规模式（默认）下，非 Profile 9 的 DV **必须** `-pix_fmt yuv420p10le`，否则报错。

P7 降级到 8.1 的代价：FEL（完整增强层）的亮度/色度映射数据丢失，只剩基础动态元数据。MEL（最小增强层）本身不含画质信息，转 8.1 几乎无损。

### 3.4 硬件编码器不能输出 DV

**[源码]** 全部 `ff_dovi_*` 调用只出现在 `libx265.c` / `libsvtav1.c` / `libaomenc.c`。`nvenc.c` / `qsvenc_hevc.c` / `amfenc.c` / `videotoolboxenc.c` 都没有。

所以在纯 ffmpeg 范围内：**要保留杜比视界，必须软编**。这是保真度求解器里"勾选 DV 就禁用 GPU 编码"的依据。

（理论上可以硬编后用 dovi_tool 注入 RPU，但要求帧数与显示顺序完全一致、不能缩放裁切，约束太多，v1 不做。）

### 3.5 容器与 tag

| 组合 | 结果 |
|---|---|
| P8.1 + MKV | 最稳。matroska muxer 从 side data 写 BlockAdditionMapping，无需额外参数 |
| P8.1 + MP4 + `-tag:v hvc1 -strict unofficial` | 可用 |
| P8.1 + MP4 无 `-strict unofficial` | **DV 配置 box 不写入**，播放器当普通 HEVC |
| P5 + MP4 | Apple 生态可播，但非 DV 播放器绿/紫屏 |
| P7 双层 + MP4 | 基本无播放器支持 |

**[源码]** `movenc.c` 写 `dvcC`/`dvvC`/`dvwC` box 的条件是 `strict_std_compliance <= FF_COMPLIANCE_UNOFFICIAL`，而默认是 `NORMAL`。不满足时警告：`Not writing 'dvcC'/'dvvC' box. Requires -strict unofficial.`

另外 MP4 里 HEVC 默认写 `hev1` tag，而 Apple 整条 AVFoundation 栈（Finder 缩略图、QuickLook、QuickTime、Photos）只认 `hvc1`。**MP4 输出 HEVC 永远要带 `-tag:v hvc1`**。

### 3.6 验证与剥离

```bash
# 流级配置记录
ffprobe -v error -select_streams v:0 -show_entries stream_side_data_list -of json out.mkv
# 逐帧 RPU（section 名是 frame_side_data_list，本机实测有效）
ffprobe -v error -select_streams v:0 -read_intervals "%+#1" -show_frames \
        -show_entries frame_side_data_list -of json out.mkv
# 彻底剥离 DV
ffmpeg -i in.mkv -c copy -bsf:v dovi_rpu=strip=1 out.mkv
```

纯 remux（`-c copy`）能完整保留 DV：RPU 在码流内部必然保留，容器级配置记录由 demuxer 读出、muxer 写回。MKV→MP4 需加 `-strict unofficial`，MP4→MKV 不需要。

## 4. 帧率：可变帧率与固定帧率

### 4.1 `-vsync` 在 9.0 已被移除

**[实测]** 在 ffmpeg 9.0.1 上 `ffmpeg -h full | grep -- -vsync` 无任何输出。替代选项是 `-fps_mode`，且它是 per-stream 的（`-fps_mode:v`）。

**实现要求**：代码里只用 `-fps_mode`，绝不生成 `-vsync`。网上大量教程仍在用 `-vsync 1` / `-vsync cfr`，在 9.0 上会直接报未知选项。

`-fps_mode` 取值：`passthrough` / `cfr` / `vfr` / `auto`。

### 4.2 VFR 判定：单看帧率字段不可靠

**[实测]** 同一个可变帧率内容分别封装成 MP4 和 MKV，ffprobe 的帧率字段表现完全不同：

| 容器 | `r_frame_rate` | `avg_frame_rate` | 能否判出 VFR |
|---|---|---|---|
| MP4 | `30/1` | `5100/299` ≈ 17.06 | 可以，两者差异明显 |
| MKV | `30/1` | `30/1` | **不能**，两者完全相同 |

原因是 matroska 的帧率字段从 `default_duration` 推导，不反映实际帧间隔。

可靠的辅助判据是采样帧间隔。同一份 MP4 源的前若干帧 `duration_time`：

```
0.033333 0.033333 0.033333 0.033333 0.200000 0.066667 0.066667 0.100000
0.033333 0.066667 0.033333 0.100000 0.033333 0.033333 0.033333 0.133333
```

明显不恒定。

**但 `duration_time` 在 MKV 里不可用 [实测]**（阶段 3 修正）：同一份可变帧率内容封装成 MKV 后，每帧、每包的 `duration_time` 全是 `0.033000`，因为它同样取自 `default_duration`。真正反映帧间隔的是时间戳：同一个 MKV 的包 `pts_time` 在 2.0 秒前每隔 0.033 递增，之后每隔 0.1 递增。所以第二级判据必须看**时间戳间隔**，不能看时长字段。

**实现要求**：VFR 判定用两级判据，任一命中即判为 VFR。

```
1. r_frame_rate 与 avg_frame_rate 相对差异 > 1%
2. 取前 120 个视频包的 pts_time，排序后求相邻差值；
   与中位数相差 20% 以上的间隔至少 2 个、且占比 ≥ 2%，判为 VFR
```

采样命令（读包不解码，比读帧快；有 B 帧时包的时间戳是乱序的，所以要先排序）：

```bash
ffprobe -v error -select_streams v:0 -read_intervals "%+#120" -show_packets \
        -show_entries packet=pts_time -of csv=p=0 input.mkv
```

阈值不能用"标准差 / 均值 > 1%"。MKV 时间戳是毫秒精度，23.976 fps 的固定帧率间隔会在 41 ms 与 42 ms 之间跳，变异系数约 1.2%，按 1% 判会把普通电影误判成 VFR。"偏离中位数 20%"只对真正的帧率跳变敏感；"至少 2 个"避免个别丢帧误报。

### 4.3 VFR 转 CFR：三种方案实测对比

背景：手机拍摄的视频几乎都是 VFR。这类文件进剪辑软件（Premiere / 达芬奇 / Final Cut）时，因帧时间戳不规则会导致音画逐渐累积失步。转成严格 CFR 是标准解法。

测试源：10 秒、名义 30fps、实际平均 17.06fps 的 VFR MP4，带 10 秒音轨。

音频时长在所有方案下都是 10.000000 秒，作为对齐基准：

| 方案 | 输出帧数 | 视频时长 | 结论 |
|---|---|---|---|
| 源（VFR） | 170 | 9.966667 | 比音频短 33ms |
| `-r 30` | 300 | 10.000000 | 与音频完全对齐 |
| `-fps_mode:v cfr -r 30` | 300 | 10.000000 | 与音频完全对齐 |
| `-vf fps=30` | 299 | 9.966667 | **少一帧**，残留 33ms 偏差 |

**[实测]** 结论：

1. `-r` 与 `-fps_mode:v cfr -r` 效果相同，都产出精确的 `帧率 × 时长` 帧数，且音视频时长完全一致。
2. **`fps` 滤镜会丢掉最后一帧**，视频比音频短一个帧时长。虽然单个文件只差 33ms，但这正是要消除的偏差，不应使用。
3. 转 CFR 后视频时长从 9.967 变为 10.000，实际上**修正了源本身的音视频长度不一致**。

**实现要求**：统一使用 `-fps_mode:v cfr -r <target>`。显式写出 `-fps_mode` 比只写 `-r` 更自解释，且意图明确。

**CFR 只能填满到最后一帧结束 [实测]**（阶段 4）。上表的源最后一帧带着自己的时长，所以转换后恰好补到 10.000。若源的视频流本身就比音频短（合成素材实测：视频 5.933 秒、音频 6.000 秒），`-fps_mode:v cfr -r 30` 的输出照样是 5.933 秒，差 2 帧，剪辑时就是音画不齐。解决办法是在滤镜链末尾加 `tpad=stop_mode=clone:stop_duration=<差值>`，把最后一帧延长到音频结束，实测输出音视频时长差小于 1 帧。两条流的时长分别取自 `duration` 字段（MP4）或 `DURATION` 标签（MKV）。

验证输出确实是 CFR：

```bash
# r_frame_rate 应等于 avg_frame_rate
ffprobe -v error -select_streams v:0 -show_entries stream=r_frame_rate,avg_frame_rate,nb_frames -of default=nw=1 out.mp4
# 帧间隔应全部相同
ffprobe -v error -select_streams v:0 -read_intervals "%+2" -show_frames -show_entries frame=duration_time -of csv=p=0 out.mp4
```

实测输出的帧间隔为恒定 `0.033333`。

### 4.4 目标帧率的选择

转 CFR 时目标帧率的取法直接影响是否丢帧：

| 取法 | 后果 |
|---|---|
| 源的 `r_frame_rate`（名义帧率） | 只复制帧、不丢帧，所有原始画面都保留。**推荐默认** |
| 源的 `avg_frame_rate`（实际平均） | 会丢帧，且得到非标准帧率（如 17.06fps），剪辑软件同样不友好 |

推荐算法：

```
1. 取 r_frame_rate 作为候选
2. 吸附到最近的标准帧率档（容差 2%）：
   23.976 / 24 / 25 / 29.97 / 30 / 50 / 59.94 / 60 / 120
3. 若候选值异常（如录屏常见的 1000/1），改用 avg_frame_rate 再吸附
4. 向用户展示：将从 N 帧变为 M 帧（复制 M-N 帧）
```

吸附时有一个陷阱，由单元测试锁定：

- 29.97 与 30、59.94 与 60、23.976 与 24 只差 0.1%，通常同时落在 2% 容差内。
- 可变帧率的实际平均值因为掉帧总是偏低（名义 30 的手机视频平均 29.41 或 29.8），按"取最近档"会误判成 NTSC 的 29.97。
- 规则：误差小于 0.1% 才认定就是该档（真正的 NTSC 源），其余模糊情况优先整数档。

极端 VFR（例如录屏从 1fps 跳到 60fps）转 CFR 会产生大量重复帧，体积显著增加。此时应提示用户，并建议改用较低的目标帧率或保持 VFR。

### 4.5 音频侧的同步保障

视频转 CFR 时音频默认不需要处理，因为音频时间戳本身是连续的。但源音频时间戳有抖动或有起始偏移时，可加：

```bash
-af "aresample=async=1"
```

**[文档]** `async` 语义：`0` 禁用；`1` 填充与裁剪；`>1` 表示每秒最大拉伸/压缩的样本数。

不要使用已废弃的 `-async` 全局选项。

### 4.6 何时该转 CFR，何时不该

这是策略引擎里的分岔点，两种选择都有正确的场合：

| 用途 | 建议 | 理由 |
|---|---|---|
| 后续剪辑 | **转 CFR** | 剪辑软件对 VFR 支持差，会累积音画偏差 |
| 归档、日常播放 | 保持 VFR | 播放器能正确处理，不引入重复帧，体积更小 |
| 流媒体分发 | 转 CFR | 部分播放器与转码服务对 VFR 处理不一致 |
| 做慢动作、逐帧分析 | 转 CFR | 需要均匀时间轴 |

## 5. 色调映射（HDR 转 SDR）

必须真正做色调映射，只改色彩标签会让画面发灰。四条管线按质量降序：

### 5.1 libplacebo（首选）

依赖 `--enable-libplacebo` + Vulkan。**[实测]** 本机可用。

```bash
-vf "libplacebo=tonemapping=bt.2390:peak_detect=1:contrast_recovery=0.3:\
gamut_mode=perceptual:apply_dolbyvision=1:\
colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p"
```

独门优势 **[源码]**：`apply_dolbyvision` 能用 DV RPU 做正确的 Profile 5 映射。其他管线处理 P5 会出现绿紫色，因为 P5 用的是 IPT-PQ-c2 色彩空间。

它会自行创建 Vulkan device，也接受软件帧（内部上传），一般不需要 `-init_hw_device` 或 `hwupload`。

可同时做缩放，质量优于 `scale`：

```bash
-vf "libplacebo=w=1920:h=1080:upscaler=ewa_lanczos:downscaler=mitchell:tonemapping=bt.2390:..."
```

### 5.2 tonemap_opencl

依赖 `--enable-opencl` + OpenCL runtime。**[实测]** 本机可用。

```bash
-init_hw_device opencl=ocl -filter_hw_device ocl
-vf "format=p010,hwupload,tonemap_opencl=tonemap=hable:desat=0:\
t=bt709:m=bt709:p=bt709:r=tv:format=nv12,hwdownload,format=nv12"
```

**[源码]** 限制：输入 transfer 必须是 SMPTE2084 或 ARIB-STD-B67；输出格式只支持 p010 / nv12（即最高 8bit 输出 nv12）。有场景自适应峰值检测（`threshold`，默认 0.2），质量优于 zscale 管线。

### 5.3 zscale + tonemap（CPU 兜底）

依赖 `--enable-libzimg`。**[实测]** 本机可用。

```bash
-vf "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
```

**[源码]** `tonemap` 滤镜要求线性光浮点输入，所以必须前置 `zscale=t=linear` 和 `format=gbrpf32le`。

调参要点：

- `desat` 默认是 2.0，实测经验上设 `0` 更不容易出现高光泛白。
- `npl`（名义峰值亮度）影响线性化标度，HDR 源常用 100，保守可用 250-400。
- 算法质量排序：`hable` ≈ `mobius` > `reinhard` > `clip`。
- 此管线不读 MaxCLL 也不做逐场景峰值检测，高动态片源容易过曝，可用 `tonemap=peak=<nits/100>` 手动覆盖。
- 无法正确处理 DV Profile 5。

### 5.4 scale_vt（macOS，不是色调映射器）

**[源码]** `vf_scale_vt.c` 只有 `w` / `h` / `color_matrix` / `color_primaries` / `color_transfer` 五个选项，内部调用 `VTPixelTransferSession`，**代码里没有任何 tone mapping 逻辑**。设 `color_transfer=bt709` 时的实际效果取决于 Apple 内部实现，不可控。

**实现要求**：不把 `scale_vt` 当色调映射器，仅作最后兜底并在界面说明效果不保证。

macOS 上真正的硬件色调映射只存在于 jellyfin-ffmpeg 的 patch（`tonemap_videotoolbox` Metal 滤镜、`tonemapx` SIMD 滤镜），上游 ffmpeg 没有。

## 6. 音频与杜比全景声

### 6.1 Atmos 无法编码，只能流复制

以下三种格式 ffmpeg 都**不能**编码：

- E-AC-3 JOC（DD+ Atmos）—— `eac3` 编码器最多 5.1，连 7.1 都编不了，更没有 JOC object bed
- TrueHD + Atmos —— 有 `truehd` 编码器，但不支持 Atmos 所在的 extension substream
- DTS-HD MA / DTS:X —— 只有实验性的 `dca`（仅 DTS core，需 `-strict -2`）

原因是 Dolby / DTS 的授权限制，生成这些格式需要商业授权的编码器。

**实现要求**：对 TrueHD / E-AC-3 JOC / DTS-HD MA 轨道，"保留"只有一个含义 —— `-c:a copy`。界面必须明确说明任何重编码都会丢失 Atmos 元数据，不能含糊成"尽量保留"。

### 6.2 双轨策略

保留原始无损轨的同时生成兼容轨：

```bash
-map 0:a:0 -c:a:0 copy -metadata:s:a:0 title="TrueHD 7.1 Atmos (原始)" \
-map 0:a:0 -c:a:1 eac3 -b:a:1 640k -ac:a:1 6 -metadata:s:a:1 title="DD+ 5.1 (兼容)" \
-map 0:a:0 -c:a:2 aac  -b:a:2 256k -ac:a:2 2 -metadata:s:a:2 title="AAC 2.0 (立体声)"
```

### 6.3 编码器取舍

| 编码器 | 可用性 | 取舍 |
|---|---|---|
| `aac`（ffmpeg 原生） | 总是可用 | 无授权问题，≥128k/声道基本透明；无 HE-AAC，低码率较差 |
| `libfdk_aac` | **几乎总是不可用** | 质量最好且有 HE-AAC，但所有官方构建都不含（`--enable-nonfree` 不可再分发）。设计上不可依赖 |
| `libopus` | 几乎总是可用 | 同码率质量最佳，多声道好；Apple 生态与老电视兼容性差 |
| `eac3` | 内置 | 电视和 AVR 兼容性最好，可做 5.1；效率差，需 640k |
| `ac3` | 内置 | 兼容性天花板，只到 5.1 |

默认策略：MP4 用 `aac` 2.0 加可选 `eac3` 5.1；MKV 用 `libopus` 或 `aac` 加 `eac3`。

**Opus 必须写 `libopus` [实测]**（阶段 6 批量转码时发现）。`-c:a opus` 选中的是 ffmpeg 自带的原生 Opus 编码器，它是实验性的，不加 `-strict -2` 直接失败：`The encoder 'opus' is experimental but experimental codecs are not enabled`，随后 `Could not open encoder before EOF`，立体声、5.1、pan 降混后都一样。`-c:a libopus` 正常。构建里没有 libopus 时（编译开关里看得到）退回 AAC。

### 6.4 多声道降混

简单 `-ac 2` 会让对白偏小。推荐显式 `pan` 矩阵（中置提升约 3dB）加限幅：

```bash
# 5.1 转 2.0
-af "pan=stereo|FL=0.707*FC+1.0*FL+0.707*BL+0.707*SL|\
FR=0.707*FC+1.0*FR+0.707*BR+0.707*SR,alimiter=limit=0.97:level=false"
# 7.1 转 2.0
-af "pan=stereo|FL=0.707*FC+1.0*FL+0.6*BL+0.6*SL|FR=0.707*FC+1.0*FR+0.6*BR+0.6*SR"
```

注意：`pan` 会清除输入的声道布局语义，之后**不要再接 `-ac 2`**。LFE 通常丢弃，混进立体声会让低频发浑。

### 6.5 响度标准化必须两遍

`loudnorm` 单次调用是单遍模式，开头几秒会有 ramp-up。正确做法是两遍：

```bash
# 第一遍：只测量
ffmpeg -hide_banner -i in.mkv -map 0:a:0 \
  -af "loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json" -f null -
# 输出 JSON 含 input_i / input_tp / input_lra / input_thresh / target_offset

# 第二遍：带测量值做线性归一化
ffmpeg -i in.mkv -map 0:a:0 \
  -af "loudnorm=I=-16:TP=-1.5:LRA=11:linear=true:\
measured_I=-23.7:measured_TP=-5.2:measured_LRA=14.1:measured_thresh=-34.2:offset=-0.1" \
  -c:a aac -b:a 256k out.m4a
```

目标值建议：

| 场景 | 参数 |
|---|---|
| 电视/影院，保留动态 | `I=-23:TP=-2:LRA=20`（EBU R128） |
| 手机/耳机，对白优先 | `I=-16:TP=-1.5:LRA=11` |

不要用 `dynaudnorm` 默认参数处理电影。它会压缩动态范围，在对白与爆炸交替时产生明显的"呼吸感"。

阶段 6 实测（ffmpeg 9.0.1，-25 dB / -30 dB 正弦波）：

- 第一遍的 JSON 打在 stderr（`-loglevel info`），值全是字符串，如 `"input_i" : "-46.75"`；取最后一个花括号块解析。
- **loudnorm 的输出固定是 192 kHz。** 不处理时 AAC 被自动定到 96 kHz、FLAC 直接写 192 kHz。第二遍必须在 loudnorm 后面接 `aresample=<目标采样率>`。
- 多声道降为立体声时，`pan` 放在 loudnorm 之前，测量与编码两遍都要带，否则测的不是最终信号。
- 两遍之后输出实测 -16 ± 0.5 LUFS。

VidForge 的写法：第二遍 `…,loudnorm=I=-16:TP=-1.5:LRA=11:measured_I=…:measured_TP=…:measured_LRA=…:measured_thresh=…:offset=…:linear=true,aresample=48000`，只作用于重新编码的音轨（原样复制的轨加不了滤镜）。

**[待验]** swresample 的 `center_mixlev` / `surround_mixlev` 文档写的是 dB（区间 [-32,32]），但常见用法传线性系数（如 1.4125）。实现前需实测确认。

## 7. 硬件编解码

### 7.1 为什么必须真实试编码

`ffmpeg -encoders` 只反映**编译时**是否带上对应支持，而这些后端都是运行时动态加载：

| 后端 | 运行时依赖 | 缺失时的表现 |
|---|---|---|
| NVENC | `nvcuda.dll` / `nvEncodeAPI64.dll`（NVIDIA 驱动） | 编码器照常列出，一用就失败 |
| QSV | Intel 驱动 + libvpl runtime | 同上 |
| AMF | `amfrt64.dll`（AMD 驱动） | 同上 |
| VideoToolbox | 硬件支持 | 可能静默回退软编（除非 `-allow_sw 0`） |

### 7.2 本机实测结果

开发机：Intel Core Ultra 7 356H（Arc 核显），无独立显卡，ffmpeg 9.0.1 gyan full。

试编码模板（跨平台通用）：

```bash
ffmpeg -hide_banner -nostdin -loglevel error \
  -f lavfi -i testsrc2=s=1280x720:r=30 \
  -frames:v 3 -pix_fmt <SWFMT> -c:v <ENCODER> <OPTS> -f null -
```

| 编码器 | 像素格式 | 结果 |
|---|---|---|
| `h264_qsv` | nv12 | PASS |
| `hevc_qsv` | nv12 | PASS |
| `hevc_qsv` + `-profile:v main10` | p010le | PASS |
| `av1_qsv` | nv12 | PASS |
| `av1_qsv` | p010le | PASS |
| `vp9_qsv` | nv12 | PASS |
| `h264_nvenc` | yuv420p | FAIL — `Cannot load nvcuda.dll` |
| `hevc_amf` | yuv420p | FAIL — `DLL amfrt64.dll failed to open` |

两个细节：

1. 用 `-f null -` 而不是 Windows 的 `NUL`。`NUL` 不可 seek，MP4/MOV muxer 会因此失败，引入与编码器无关的噪声。若确实要测封装，用可流式的 `-f matroska -y NUL`。
2. 跑 3 帧而不是 1 帧。硬件编码器有 lookahead 与 B 帧队列，1 帧可能走不到真正的编码路径。

另外 `vp9_qsv` 编码器在本机存在，而多数资料称 ffmpeg 没有这个编码器。这直接印证了能力必须靠真实试编码确定。

### 7.3 硬件编码器写 HDR10 元数据的情况

| 编码器 | 写 MDCV/CLL | 来源 |
|---|---|---|
| `hevc_qsv` | **会写** | **[实测]** 本机验证：1000 nits 的 MDCV 与 CLL 在输出码流中完整保留 |
| `hevc_nvenc` / `av1_nvenc` | 会写 | [源码] `outputMasteringDisplay` / `outputMaxCll`，FFmpeg 7.1 起 |
| `hevc_amf` / `av1_amf` | 会写 | [源码] 但触发条件是 `color_trc` 等于 SMPTE2084，**HLG 不触发**。**[待验]** 本机无 AMD 卡 |
| `hevc_videotoolbox` | **不写** | [源码] `videotoolboxenc.c` 零相关代码 |
| `h264_nvenc` | 不写 | H.264 本身无此 SEI 标准路径 |

**含义**：Windows 上"流媒体输出 + 保留 HDR10"可以走硬编，速度大幅受益，不必强制软编。只有杜比视界才必须软编。

### 7.4 Intel QSV 的已知坑

| 坑 | 说明 |
|---|---|
| `-low_power` | Core Ultra（Meteor Lake 及更新）编码统一走 VDEnc，此选项已无意义；旧平台传了可能初始化失败。**默认不传** |
| 10bit 格式 | `hevc_qsv` 只接受 nv12 / p010le / qsv。10bit 必须 `-pix_fmt p010le` 且 `-profile:v main10`，否则报 `Current pixel format is unsupported` |
| 默认 RC 变更 | FFmpeg 7.0 起默认从 VBR 改为 CQP。**永远显式指定 RC 模式**，不能只传 `-b:v` |
| `load_plugin` | Skylake 时代 MediaSDK 的遗留选项，Core Ultra 上不要传 |
| 多 GPU | 核显加独显共存时默认选哪个不确定，需允许用户指定 adapter |

QSV 的 RC 模式选择逻辑 [源码] `qsvenc.c` 的 `select_rc_mode()`：设了 `-q:v` 走 CQP；`look_ahead` 加 `global_quality` 走 LA_ICQ；`bitrate` 加 `global_quality` 加 `maxrate` 走 QVBR；都没设走 CQP 并打印提示。几种组合的实测结果见 8.2 节，其中 `global_quality` 加 `maxrate`、不给 `bitrate` 会静默落到 CQP。

ffmpeg 会打印 `Using the %s ratecontrol method`。解析这行可确认实际生效的 RC 模式，建议在 dry-run 时抓取。

### 7.5 硬解与混合管线

```bash
# 硬解 + 软编：不写 -hwaccel_output_format 时自动下载到系统内存，无需 hwdownload
-hwaccel qsv -i in.mkv -c:v libx265

# 全硬件流水（零拷贝，最快）
-hwaccel qsv -hwaccel_output_format qsv -i in.mkv -vf "vpp_qsv=w=1920:h=1080:format=p010le" -c:v hevc_qsv

# 强制了 hw 输出格式后接软件滤镜或软编，必须 hwdownload
-hwaccel qsv -hwaccel_output_format qsv -i in.mkv -vf "hwdownload,format=p010le" -c:v libx265
```

要点：

- `format=` 的值必须等于 hw frames context 的 `sw_format`：10bit 是 `p010le`，8bit 是 `nv12`。错了报 `Invalid input format`。
- 格式名与 hwaccel 名不同：`d3d11va` 对应 `d3d11`，`videotoolbox` 对应 `videotoolbox_vld`。
- **`hwdownload` 会丢失部分 side data**，尤其 DV RPU 与 HDR10+。所以 DV / HDR10+ 场景必须走全软件路径。

**[实测]** 本机列出的 hwaccel：`cuda vaapi dxva2 qsv d3d11va opencl vulkan d3d12va amf`。列出不等于可用，仍需第 2、3 层探测。

**`-hwaccel qsv` 在 9.0 会把帧留在 GPU 上 [实测]**（阶段 4 真实转码时发现）。只写 `-hwaccel qsv` 不写输出格式时，ffmpeg 打印 `WARNING: defaulting hwaccel_output_format to qsv for compatibility with old commandlines`，帧以 QSV 表面形式留在显存；后面再要求 `-pix_fmt p010le` 就报 `Impossible to convert between the formats supported by the filter` 并失败。上面"硬解 + 软编"那条写法对 qsv 不成立。

**实现要求**：硬解一律写 `-hwaccel auto`。实测 `-hwaccel auto` 解码后帧自动下载到内存，接软件滤镜、libx265、hevc_qsv 都正常，而且 HDR10 的帧级 MDCV / CLL side data 仍在（libx265 自动透传后输出的 1000 nits 元数据完整）。要做零拷贝的全硬件流水，必须同时写 `-hwaccel_output_format` 并改用 `scale_qsv` / `vpp_qsv` 这类硬件滤镜，v1 不做。

### 7.6 探测实现中踩到的坑

以下均为 **[实测]**（阶段 2 实现能力探测时在开发机上验证）。

**不支持的像素格式会被静默替换。** 给 `h264_qsv` 传 `-pix_fmt p010le`，ffmpeg 只在 warning 级别打印 `Incompatible pixel format 'p010le' for codec 'h264_qsv', auto-selecting format 'nv12'`，然后照常编码、退出码 0。用 `-v error` 试编码时这条警告被吞掉，会把 8bit 误判为支持 10bit。所以 10bit 试编码之前，必须先确认目标格式出现在 `-h encoder=<名称>` 的 `Supported pixel formats:` 列表里。本机实测列表：

| 编码器 | Supported pixel formats（节选） |
|---|---|
| `h264_qsv` | `nv12 qsv`（没有 10bit） |
| `hevc_qsv` | `nv12 p010le p012le … qsv` |
| `av1_qsv` | `nv12 p010le qsv` |
| `*_nvenc` / `*_amf` | 含 `p010le`，但 H.264 10bit 仍取决于显卡代际，需试编码确认 |

**编译开关要看组件，不看 configuration 行。** `--enable-xxx` 只列出显式开启的库，自动检测到的（macOS 上的 VideoToolbox、多数 Linux 发行版的硬件后端）不会出现在里面。判断是否具备某能力，一律看对应组件是否存在：编码器看 `-encoders`，libplacebo / libzimg / libvmaf 看 `-filters` 里的 `libplacebo` / `zscale` / `libvmaf`，libbluray 看 `-protocols` 里的 `bluray`。

**第 2 层的写法。** `-init_hw_device` 单独使用会报缺少输出，需要带一个最小的输入输出：

```bash
ffmpeg -hide_banner -nostdin -v error -init_hw_device qsv=hw -f lavfi -i nullsrc=s=64x64:d=0.04 -f null -
```

本机结果：`qsv d3d11va d3d12va dxva2 vulkan opencl` 成功；`cuda` 报 `Cannot load nvcuda.dll`，`amf` 报 `DLL amfrt64.dll failed to open`，`vaapi` 报 `Failed to initialise VAAPI connection`。9.0 已支持 `-init_hw_device amf`。

**耗时。** QSV 设备初始化约 600 ms，QSV 试编码 3 帧约 800 ms；NVENC / AMF 因缺 DLL 在 80 ms 内失败；三条色调映射管线试运行 0.1–1.7 s（`tonemap_opencl` 最慢）。三层全部做完、4 路并发时约 4–5 秒，所以探测必须放后台并缓存。

**其他。** `libsvtav1` 在 `-v error` 下仍会往 stderr 打印 `Svt[info]` 横幅，判断失败时不能看 stderr 是否为空，要看退出码。Windows 注册表里本机核显的名称是 `Intel(R) Graphics`（不含 Arc 字样）。

## 8. 进度与码率控制

### 8.1 `-progress` 是块协议

```bash
-nostdin -hide_banner -loglevel warning -progress pipe:1 -nostats
```

每个周期输出一整组字段，以 `progress=continue`（末次为 `progress=end`）收尾：

```
frame=1234
fps=57.32
stream_0_0_q=28.0
bitrate=4521.7kbits/s
total_size=27891200
out_time_us=49400000
out_time_ms=49400000
out_time=00:00:49.400000
dup_frames=0
drop_frames=0
speed=1.72x
progress=continue
```

三个坑：

1. **`out_time_ms` 的单位实际是微秒**（与 `out_time_us` 是同一个值）。这是 ffmpeg 的历史 bug，为兼容性一直未改。只用 `out_time_us`。
2. 必须按块累积再提交，不能逐行更新 UI。
3. 开头几块里 `bitrate` / `speed` / `total_size` 可能是 `N/A`；纯音频任务没有 `frame` / `fps`；多输出流时每路一行 `stream_N_M_q`。

`dup_frames` 与 `drop_frames` 在 VFR 转 CFR 时很有用，可以直接告诉用户复制或丢弃了多少帧。

若输出也走 stdout，改用 `-progress tcp://127.0.0.1:<port>`，自建 TCP listener 最干净。

### 8.2 码率控制参数对照

VidForge 生成的写法（`pipeline/args.rs` 的 `rate_args`）。码率单位 k；目标码率模式的峰值取 1.5 倍，缓冲一律取 2 倍峰值：

| 模式 | libx264 / libx265 | libsvtav1 | QSV（h264 / hevc） | NVENC | AMF | VideoToolbox |
|---|---|---|---|---|---|---|
| 恒定质量 | `-crf Q` | `-crf Q` | `-global_quality Q`（ICQ） | `-rc vbr -b:v 0 -cq Q` | `-rc cqp -qp_i Q -qp_p Q` | `-q:v Q`（仅 Apple Silicon） |
| 目标码率 | `-b:v T -maxrate 1.5T -bufsize 3T` | `-b:v T`（VBR） | `-b:v T -maxrate 1.5T -bufsize 3T`（VBR） | `-rc vbr -b:v T -maxrate 1.5T -bufsize 3T` | `-rc vbr_peak -b:v T -maxrate 1.5T -bufsize 3T` | `-b:v T` |
| 限峰值 | `-crf Q -maxrate M -bufsize 2M` | `-crf Q -maxrate M` | `-global_quality Q -b:v 2M/3 -maxrate M -bufsize 2M`（QVBR） | `-rc vbr -b:v 0 -cq Q -maxrate M -bufsize 2M` | 不支持 | 不支持 |
| 两遍 | `-b:v T -pass 1/2 -passlogfile P` | 同左 | 不支持 | 不支持 | 不支持 | 不支持 |

不支持的组合由引擎换成最接近的模式：限峰值 M 换成目标码率 2M/3（峰值仍是 M）；两遍在自动选编码器时改用软件编码器，手选了硬件编码器时换成单遍目标码率。`av1_qsv` 的限峰值同样按不支持处理。

阶段 5 在开发机上实测（FFmpeg 9.0.1 gyan full，Arc 核显，4 秒 720p 测试图，`transcode_real.rs` 固化了其中可自动核对的部分）：

| 写法 | 结果 |
|---|---|
| QSV 只给 `-global_quality` | ICQ（verbose 日志 `Using the intelligent constant quality (ICQ) ratecontrol method`） |
| QSV `-global_quality` 加 `-maxrate`、不给 `-b:v` | **静默落到 CQP**，峰值限制不生效 |
| QSV `-global_quality` 加 `-b:v T -maxrate M`（T < M） | QVBR，hevc 8/10bit 与 h264 均可 |
| QSV `-b:v T -maxrate T` | CBR（目标等于峰值时走 CBR，所以 QVBR 的目标必须小于峰值） |
| `av1_qsv` 走 QVBR | `Error while opening encoder`，不可用 |
| libx265 `-crf Q -maxrate M` 不给 `-bufsize` | **峰值限制被静默忽略**（输出 5.4 Mbps，上限 1 Mbps）；加 `-bufsize` 后生效 |
| libsvtav1 `-b:v 1500k` 单遍 | 4 秒片段实际 2.4 Mbps，短片单遍 VBR 偏差大；两遍 1.57 Mbps |
| libsvtav1 `-crf Q -maxrate 1000k` | 峰值只是大致遵守（1.5 Mbps） |
| 两遍：三个软件编码器都用 `-pass N -passlogfile P` | ffmpeg 写 `P-<输出流序号>.log`（x264 另有 `.mbtree`，x265 另有 `.cutree`），1500k 目标实际 1.46–1.57 Mbps |

几个容易错的点：

- 9.0 的命令行对 libx264 / libx265 / libsvtav1 统一处理 `-passlogfile`，libx265 不必另用 `-x265-stats`（两者都能用）。
- 第一遍必须看到与第二遍完全相同的帧：同样的滤镜、`-fps_mode:v cfr -r`、尾部补齐；只编码视频，输出 `-f null -`。
- NVENC 的 `-cq` 必须配 `-rc vbr -b:v 0`，否则会被 bitrate 约束。
- VideoToolbox 的 `-q:v` 仅 Apple Silicon 可用，Intel Mac 报 `qscale not available for encoder`。
- **不存在跨编码器的统一质量刻度**。界面上的质量档位必须按编码器分别映射，并标注不等价。

其余写法备查（VidForge 未使用）：

| 目标 | libx265 | libsvtav1 | NVENC | QSV | VideoToolbox |
|---|---|---|---|---|---|
| 恒定 QP | `-x265-params qp=22` | `-qp 30` | `-rc constqp -qp 22` | `-q:v 22` | 无 |
| CBR | `-b:v 6M` 加 vbv 参数与 `strict-cbr=1` | `-b:v 6M -svtav1-params rc=2` | `-rc cbr -b:v 6M` | `-b:v 6M -maxrate 6M` | `-b:v 6M -constant_bit_rate 1`（macOS 13+） |
| 速度档 | `-preset ultrafast..placebo` | `-preset 0..13`（数字小=慢） | `-preset p1..p7` | `-preset veryfast..veryslow` | `-realtime 0` |

### 8.3 质量评估（v2）

```bash
# 注意输入顺序：第一路是待测，第二路是参考
ffmpeg -i distorted.mkv -i reference.mkv \
  -lavfi "[0:v]settb=AVTB,setpts=PTS-STARTPTS[d];[1:v]settb=AVTB,setpts=PTS-STARTPTS[r];[d][r]libvmaf=log_path=vmaf.json:log_fmt=json:n_threads=8" \
  -f null -
```

要点：两路尺寸必须一致（先把待测上采样到参考尺寸）；抽样可用 `n_subsample=10`；HDR 内容的 VMAF 分数不能与 SDR 直接比较，因为模型是 SDR 训练的。`ssim` / `psnr` 滤镜无外部依赖，总是可用。

## 9. 容器兼容矩阵

| 流类型 | MKV | MP4 |
|---|---|---|
| H.264 / HEVC / AV1 | 可以 | 可以（HEVC 必须 `-tag:v hvc1`） |
| 杜比视界 P8.1 | 可以（自动写 BlockAdditionMapping） | 需 `-strict unofficial` |
| AAC / AC-3 / E-AC-3（含 JOC） | 可以 | 可以 |
| **TrueHD** | 可以 | **不行** — ffmpeg 报 `codec not currently supported in container` |
| DTS / DTS-HD MA | 可以 | **[待验]** 建议按不支持处理 |
| FLAC / Opus | 可以 | 可以 |
| PCM | 可以 | 不行（MOV 可以） |
| **PGS 图形字幕** | 可以 | **不行** — 报 `Subtitle codec 0x17000 is not supported` |
| SRT / ASS | 可以 | 仅 `mov_text` |
| 章节 | 完整 | 有限 |
| 附件（字体等） | 可以 | 不行 |

MP4 输出的固定附加参数：`-tag:v hvc1`（HEVC）、`-movflags +faststart`（便于流式播放与拖动）。

MOV 与 MP4 同属一族，音频白名单不同：AAC / AC-3 / E-AC-3 / ALAC / PCM 可以，TrueHD 与 DTS 同样不行。剪辑预处理场景输出 MOV，蓝光片源的 TrueHD / DTS 要转成 24bit PCM 而不是原样复制，否则命令必然失败（阶段 4 事实断言发现的真实缺陷）。

## 10. ffmpeg 构建能力差异

| 能力 | gyan essentials | gyan full | BtbN gpl | Homebrew | evermeet |
|---|---|---|---|---|---|
| libx265 / libx264 / libvmaf | 有 | 有 | 有 | 有 | 有 |
| libsvtav1 | 无 | 有 | 有 | 有 | 无 |
| libplacebo + vulkan | 无 | 有 | 有 | **无** | **无** |
| opencl | 无 | 有 | 有 | **无** | **无** |
| libzimg (zscale) | 有 | 有 | 有 | **无** | 有 |
| libbluray | 无 | 有 | 有 | 无 | 有 |
| libfdk_aac | 无 | 无 | 无 | 无 | 无 |

**macOS 是最大的坑**：Homebrew 构建三条色调映射管线全部缺失（无 libplacebo、无 opencl、无 libzimg）。evermeet 有 libzimg 但无 libplacebo 与 libsvtav1。

应对：macOS 上引导用户下载 jellyfin-ffmpeg（含 Metal 色调映射滤镜），或接受能力退化并在界面明示。Windows 上引导 gyan full 或 BtbN gpl。

## 11. 开发机环境基线

记录于 2026-09-11，作为测试参照。

| 项 | 值 |
|---|---|
| OS | Windows 11 Pro 26200 |
| CPU | Intel Core Ultra 7 356H |
| GPU | Intel Arc 核显（驱动 32.0.101.8864） |
| ffmpeg | 9.0.1-full_build-www.gyan.dev，位于 `C:\Program1\ffmpeg\bin` |
| Rust | 1.96 stable x86_64-pc-windows-msvc |
| MSVC | 14.51（VS Community 2026）加 Windows SDK 10.0.26100 |
| WebView2 | 152.0.4191.66 |
| Node / pnpm | 24.16 / 11.22 |

本机具备 v1 与 v2 全部功能的验证条件，唯独缺 NVIDIA 与 AMD 显卡，相关代码路径只能靠错误分类逻辑的单元测试覆盖。

**不能用这台机器的配置假设用户环境**。开发期应准备一个能力受限的 ffmpeg（例如 gyan essentials）用于测试降级路径。

## 12. 色彩标签（阶段 3 实测）

以下均为 **[实测]**，ffmpeg 9.0.1。

**`-color_primaries` / `-color_trc` 输出选项不生效。** 用 lavfi 生成的帧（色彩属性未指定）编码时加上 `-color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc`，输出里只有 `color_space` 生效，`color_primaries` 与 `color_transfer` 都是 unknown；换成 `-color_primaries:v` 也一样。原因是编码器的色彩属性取自帧，而 7.1 起只有 colorspace 与 range 参与滤镜图协商。

| 做法 | primaries / transfer 是否写入 |
|---|---|
| `-color_primaries` / `-color_trc` 输出选项 | 否 |
| `-vf setparams=color_primaries=...:color_trc=...:colorspace=...` | 是 |
| 色调映射滤镜（libplacebo、zscale）指定输出色彩 | 是，滤镜会给帧打标签 |
| 重编码 HDR 源、不改色彩 | 是，沿用解码出的帧属性 |

**实现要求**：命令构建不依赖这两个输出选项。需要显式打标签时用 `setparams`；色调映射后由滤镜负责；保留 HDR 时什么都不用传。

**流级色彩字段可能是 unknown，而帧里有值。** 用 x265 的 `-x265-params colorprim=...:transfer=...` 编码的 MKV，色彩只写在 HEVC 码流的 VUI 里，容器没有 Colour 元素，ffprobe `-show_streams` 里 `color_transfer` 与 `color_primaries` 缺失；解码出的首帧则是 `smpte2084` / `bt2020`。媒体分析只看流级字段会把这种 HDR10 片源误判成 SDR，所以分析时要读首帧的 `color_*` 字段兜底（设计文档 5.2 节的首帧采样顺带完成）。

**旋转写在 Display Matrix 里。** 手机竖拍视频的编码尺寸仍是横向（如 1920×1080），`side_data_list` 里的 `Display Matrix` 带 `"rotation": -90`。转码时 ffmpeg 默认自动旋转，流复制时保留这条 side data。

## 13. 进程管理（阶段 6 实测）

以下均为 **[实测]**，Windows 11，ffmpeg 9.0.1。

**应用被强杀时 ffmpeg 不会跟着退出。** Windows 上结束父进程不影响子进程：用任务管理器或 `taskkill /F` 结束应用后，正在转码的 ffmpeg 继续运行、继续写临时文件，下次启动恢复队列时还可能因为文件被占用而删不掉。解决办法是把每个 ffmpeg 放进一个设置了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的作业对象：应用退出、崩溃或被强杀时系统关闭作业句柄，其中的进程随之结束。实测强杀应用后 ffmpeg 立即消失。Linux 用 `prctl(PR_SET_PDEATHSIG, SIGKILL)`；macOS 没有对应机制，要另想办法（阶段 7）。

**暂停可以用挂起线程实现。** ffmpeg 没有暂停命令（`-nostdin` 下也不能发 `q`）。逐个 `SuspendThread` 挂起进程的全部线程后，`-progress` 输出停止、编码不再推进；`ResumeThread` 后接着跑，输出正常。类 Unix 用 `SIGSTOP` / `SIGCONT`。挂起的进程仍占着内存与 GPU 会话，所以暂停的任务照样占并发票。

**取消直接结束进程即可。** 输出写在 `.vidforge-part` 临时文件里，结束进程后删掉它（和两遍编码的 `.2pass-*.log` 统计文件）就不留痕迹；挂起中的进程也能直接结束。
