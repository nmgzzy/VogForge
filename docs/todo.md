# VidForge TODO

更新于 2026-09-11（阶段 3 完成）

配套文档：[需求](requirements.md) · [设计](design.md) · [计划](plan.md) · [ffmpeg 技术事实](ffmpeg-facts.md)

图例：`[x]` 已完成 · `[ ]` 待办 · `[~]` 进行中 · `[!]` 阻塞

## 阶段 0：脚手架与文档

- [x] 需求文档 `docs/requirements.md`
- [x] 设计文档 `docs/design.md`
- [x] 实施计划 `docs/plan.md`
- [x] ffmpeg 技术事实文档 `docs/ffmpeg-facts.md`（含本机实测）
- [x] TODO 文档（本文件）
- [x] 验证开发环境：Rust / MSVC / Windows SDK / WebView2 / ffmpeg 9.0.1
- [x] 实测硬件编码能力基线（QSV 六项 PASS，NVENC / AMF 报错已采集）
- [x] 实测 QSV 与 SVT-AV1 的 HDR10 元数据保留行为
- [x] 实测 VFR 判定在 MP4 与 MKV 下的差异
- [x] 实测三种 VFR 转 CFR 方案并选定 `-fps_mode:v cfr -r`
- [x] Cargo workspace：根 `Cargo.toml`，成员 `crates/vidforge-core` 与 `src-tauri`
- [x] `vidforge-core` crate 骨架与模块声明
- [x] Tauri 2 应用骨架与 `tauri.conf.json`（含应用图标、CSP、窗口最小尺寸）
- [x] Vite + React 19 + TypeScript 配置
- [x] Tailwind CSS v4、Lucide 图标、Zustand
- [x] ts-rs 导出链路（Rust 类型生成到 `src/bindings/`，`pnpm bindings` 重新生成）
- [x] `insta` 快照测试依赖（阶段 4 开始使用）
- [x] Vitest 配置
- [x] README
- [x] `.gitignore`、git 初始化

## 阶段 1：前端界面骨架（mock 数据）

状态：已完成。浏览器预览 `pnpm dev`，端口 1420。

- [x] 应用外壳：标题栏、侧栏导航、主工作区、底部命令预览条
- [x] 主题：深色 / 浅色 / 跟随系统
- [x] mock 数据：覆盖手机 HDR、iPhone DV 8.4、蓝光 remux（DV P7 + TrueHD Atmos + PGS）、VFR 录屏、普通 H.264 五类源
- [x] 转码页
  - [x] 文件列表：拖拽区、批量导入、文件夹导入入口
  - [x] 文件卡片：分辨率、编码、时长、体积、码率
  - [x] 高价值特征徽章：HDR10 / HLG / 杜比视界（带 Profile 号）/ 全景声 / 无损 / PGS / VFR
  - [x] 媒体信息详情抽屉（全部流）
  - [x] L1 场景卡片（8 个，含"剪辑预处理"）
  - [x] L2 关键旋钮：画质档位、分辨率、帧率（含转 CFR 开关与推荐值）、编码格式、编码器、音频、容器
  - [x] 决策理由展示（"为什么这么选"）
  - [x] 保真度勾选清单：三态徽章 + 条件说明 + 一键修正按钮
  - [x] L3 专家面板（折叠）
  - [x] 体积与耗时预估
  - [x] 不建议转码提示（源码率已很低时）
- [x] 命令预览条：语法高亮、一键复制、加入队列
- [x] 队列页
  - [x] 任务列表：状态、进度条、速度、ETA、输出体积
  - [x] 暂停 / 恢复 / 取消 / 重试 / 调序
  - [x] 任务详情：完整命令、日志、回退记录
  - [x] 保真度报告视图
  - [x] 并发设置
- [x] 环境页
  - [x] ffmpeg 路径、版本、构建来源
  - [x] 编译能力清单（含缺失项及影响的功能）
  - [x] 硬件编码器矩阵（可用 / 不可用 + 原因）
  - [x] 色调映射管线可用性
  - [x] 重新探测按钮
- [x] 设置页：ffmpeg 路径、输出目录与命名模板、冲突策略、并发、语言、主题、硬件加速总开关
- [x] 按反馈降低信息密度：场景卡片单行化、低频参数移入"更多参数"、保真度可保留项单行两列、推荐说明默认折叠（规则见设计文档 6.6）
- [~] 窄窗口适配：1440 与 1280 已验证（编辑区用容器查询自动切换单栏）；960 尚未验证
- [x] Vitest：mock 引擎、格式化、特征徽章、project / queue 两个 store、保真度与推荐说明等面板的交互

## 阶段 2：ffmpeg 定位与能力探测

状态：已完成。桌面应用 `pnpm tauri dev` 的环境页显示真实探测结果。

- [x] `locate.rs`
  - [x] 用户指定路径（目录或可执行文件本身）
  - [x] 应用内置目录 `~/.vidforge/ffmpeg/bin`
  - [x] 当前进程 PATH
  - [x] Windows 注册表 Machine/User PATH（刚安装时进程环境未刷新，开发期已实际遇到）
  - [x] Windows 常见安装位置：winget Links 与 Packages / scoop / chocolatey / `C:\ffmpeg`
  - [x] macOS 常见安装位置：`/opt/homebrew/bin` / `/usr/local/bin` / `/opt/local/bin`
  - [x] 同时定位 ffprobe，版本不一致时给出提示
  - [x] 优先选满足最低版本的构建，用户指定的路径总是采用
- [x] `capability.rs`
  - [x] 第 1 层：解析 `-version` 版本号（发行版与 git 构建）、构建来源
  - [x] 第 1 层：解析 `-encoders` / `-filters` / `-bsfs` / `-protocols` / `-hwaccels`，编译开关按组件是否存在判断
  - [x] 第 1 层：检测 `libx265` 是否含 `dolbyvision` 选项
  - [x] 第 2 层：`-init_hw_device` 设备初始化探测
  - [x] 第 3 层：真实 3 帧试编码（`-f null -`），4 路并发
  - [x] 10bit 试编码前核对编码器自报的像素格式列表（防止被静默换成 8bit）
  - [x] 失败分类：DeviceMissing / Capability / Param / Resource / NotBuilt / Unknown
  - [x] 色调映射管线：滤镜、依赖设备与 2 帧试运行三重确认
  - [x] 结果缓存与失效（路径 + mtime + 大小 + 版本 + GPU + 驱动 + 缓存格式版本）
  - [x] 探测异步进行，推送进度事件
- [x] `external.rs`：dovi_tool / hdr10plus_tool / mkvmerge 探测
- [x] `sysinfo.rs`：显卡与驱动（Windows 读注册表，macOS 用 system_profiler）
- [x] `config.rs`：`~/.vidforge/config.json` 读写，损坏时备份并回到默认值
- [x] 前端后端适配层 `src/backend/`（Tauri 与 mock 两种实现）
- [x] 环境页接真实数据：找不到 / 版本过低的醒目提示、选择目录、下载入口、查找过的位置、探测进度
- [x] 设置页接真实配置（主题与语言除外）
- [x] 侧栏环境状态随探测结果变化（含后端调用失败）
- [x] 有 ffmpeg 却用不了时报"无法运行"（Broken），不误报成"未找到"
- [x] 真实能力流入前端引擎后，编不了的格式与编码器不再出现在计划里（软编缺失改硬编，整格式缺失换格式并警告，界面置灰）
- [x] 测试
  - [x] `-version` / `-buildconf` / `-encoders` / `-filters` 输出解析（本机采集的 3 个真实构建 fixture）
  - [x] 失败分类：用真实采集的报错字符串做表驱动测试
  - [x] 本机探测结果与基线一致（`tests/probe_real.rs`，环境不满足时跳过）
  - [x] ffmpeg 不存在 / 版本过低（gyan 6.0）/ 能力受限（gyan essentials 9.0.1）三种情况，单元测试与真实构建各一遍
  - [x] 前端：能力 store、mock 后端、环境页三种状态
  - [x] 桌面应用端到端：`scripts/tauri-cdp.mjs` 通过 WebView2 调试端口驱动窗口，验证真实探测、切换到旧版 ffmpeg 与恢复自动查找

## 阶段 3：媒体分析

状态：已完成。桌面应用可以拖入或选择文件与文件夹，由 ffprobe 分析后进入转码页。

- [x] `probe.rs`：每个文件三次 ffprobe（流与章节、首帧 side data 与色彩、前 120 个包的时间戳）
- [x] 视频流：编码、分辨率、位深、像素格式、色彩特性、旋转（Display Matrix）
- [x] 色彩字段流级缺失时从首帧读取（x265 只写 VUI 的片源，否则会把 HDR10 误判成 SDR）
- [x] HDR 识别：HDR10 / HLG / PQ 无元数据
- [x] HDR10 元数据：MDCV 与 CLL，**有理数求值为 f64**（HEVC 与 AV1 分母不同，AV1 流级还会约分）
- [x] 杜比视界：从 stream side data 读 profile、bl_compat_id、是否有 EL；EL 类型看 RPU 的 disable_residual_flag
- [x] HDR10+ 识别
- [x] 音频：编码、声道布局、无损判定、Atmos 判定（TrueHD / E-AC-3 JOC）、DTS:X 判定
- [x] 字幕：图形字幕判定（PGS / VobSub / DVB）
- [x] 章节与附件（MP4 封面图算附件，不算视频流）
- [x] VFR 两级判定：判据 1 帧率字段、判据 2 包时间戳间隔（原计划的 `duration_time` 在 MKV 里是常数，已改）
- [x] 设备来源推断：make / model / encoder / handler 标签与文件名
- [x] 文件导入：拖拽（Tauri 窗口级拖放）、多选、文件夹递归、扩展名过滤、跳过隐藏与系统目录（含 Windows 隐藏属性、NAS 上的 `._` 资源分叉文件）
- [x] 导入结果：失败原因中文化（保留原文）、跳过数、重复数；全部成功时不打扰；读不了的文件夹记为失败
- [x] 导入进行中再拖入的文件排队处理，结果合并
- [x] 纯音频文件拒绝导入并说明原因；封面图排在前面时按流序号采样真实视频流
- [x] 桌面应用从空列表开始；浏览器预览仍载入示例素材
- [x] 测试
  - [x] fixture：手机 HDR10（HEVC / AV1）、HLG 手机（带旋转与设备标签）、iPhone DV 8.4、多音轨 MKV、VFR MP4、VFR MKV、23.976 CFR MKV、相机、蓝光 remux（DV P7 FEL + Atmos + PGS）
  - [x] 有理数求值：`"10000000/10000"`、`"256000/256"`、`"1000/1"` 都等于 1000.0
  - [x] VFR：MP4 靠判据 1 命中，MKV 靠判据 2 命中，23.976 毫秒取整不误判
  - [x] 字段缺失或 `N/A` 时不崩溃
  - [x] `tests/media_real.rs`：用真实 ffmpeg 合成 7 个素材 + 损坏文件 + 非视频文件，走完整导入流程核对
  - [x] 前端：导入 store（成功、失败、重复）、导入结果卡片
  - [x] 桌面端到端：在真实窗口里导入合成素材目录，特征徽章与推荐正确

## 阶段 4：命令构建与容器矩阵

- [ ] `model.rs`：`TranscodePlan` / `VideoPlan` / `AudioTrackPlan` / `FpsPolicy` / `Decision` 等，全部 ts-rs 导出
- [ ] `args.rs`：按固定分段顺序生成 argv
  - [ ] 全局参数：`-hide_banner -nostdin -loglevel warning -progress pipe:1 -nostats`
  - [ ] 硬件解码参数
  - [ ] 流映射
  - [ ] 视频编码参数，按编码器分派 RC 参数名
  - [ ] 色彩标签
  - [ ] 滤镜链组装（缩放、色调映射）
  - [ ] 音频逐轨参数
  - [ ] 字幕、章节、元数据
  - [ ] 容器附加参数
  - [ ] 输出到临时文件名 `.vidforge-part`
- [ ] `fps.rs`
  - [ ] `KeepSource` / `ConstantRate` / `CapAt` 三种策略
  - [ ] 目标帧率推荐：名义帧率吸附标准档，异常值回退到平均帧率
  - [ ] 生成 `-fps_mode:v cfr -r <target>`
  - [ ] 按需追加 `-af aresample=async=1`
  - [ ] 预估复制/丢弃帧数
- [ ] `color.rs`：色调映射管线选择与滤镜字符串生成（libplacebo / tonemap_opencl / zscale / scale_vt）
- [ ] 色彩标签不依赖 `-color_primaries` / `-color_trc` 输出选项（9.0 不生效，见技术事实 12 节），需要时用 `setparams`
- [ ] `audio.rs`：copy / 编码 / 双轨策略 / `pan` 降混 / loudnorm 两遍
- [ ] `container.rs`：编码 × 容器兼容矩阵
- [ ] 快照测试（`insta`），至少覆盖：
  - [ ] 手机 HDR → 归档（libx265 10bit，HDR10 自动透传，不手传 master-display）
  - [ ] 手机 HDR → 流媒体（色调映射 SDR，MP4 + hvc1 + faststart）
  - [ ] 手机 HDR → 流媒体（QSV 硬编保留 HDR10）
  - [ ] iPhone DV 8.4 → 归档保留 DV（`-dolbyvision 1` + yuv420p10le）
  - [ ] iPhone DV 8.4 → 不保留 DV（显式 `-dolbyvision 0`）
  - [ ] 蓝光 remux → 高画质收藏（全音轨 copy，PGS copy，MKV）
  - [ ] 蓝光 remux → 双轨策略（TrueHD copy + E-AC-3 + AAC）
  - [ ] VFR 手机素材 → 剪辑预处理（CFR）
  - [ ] VFR 手机素材 → 归档（保持 VFR，无帧率参数）
  - [ ] 普通 H.264 → 最小体积（AV1 via libsvtav1）
  - [ ] 原样封装
  - [ ] 各编码器的恒定质量 RC 参数名（x265 / svtav1 / nvenc / qsv / videotoolbox）
  - [ ] 5.1 降混 2.0 的 pan 矩阵
- [ ] 断言型测试（每条技术事实一个）
  - [ ] 任何输出都不含 `-vsync`
  - [ ] CFR 用 `-fps_mode:v cfr`，不用 `fps` 滤镜
  - [ ] MP4 输出 HEVC 必含 `-tag:v hvc1`
  - [ ] MP4 输出 DV 必含 `-strict unofficial`
  - [ ] 不保留 DV 且源含 DV 时必含 `-dolbyvision 0`
  - [ ] QSV 10bit 必为 `p010le` + `main10`
  - [ ] QSV 必显式指定 RC 模式
  - [ ] 默认不含 `-low_power`
  - [ ] libx265 两遍用 `-x265-stats` 而非 `-passlogfile`
  - [ ] AV1 + HDR 不使用 libaom
  - [ ] `pan` 降混后不再出现 `-ac 2`
  - [ ] TrueHD / PGS 在 MP4 下判为不兼容
- [ ] 集成测试：抽样命令在真实 ffmpeg 上用合成素材跑通

## 阶段 5：策略引擎与保真度求解

- [ ] `strategy.rs`：8 个场景预设
- [ ] 常识保护规则
  - [ ] 不重复压缩（源码率已低于目标预估）
  - [ ] 不放大分辨率
  - [ ] 不提帧率
  - [ ] VFR 按用途分岔
  - [ ] HDR 转 SDR 必须插入色调映射
  - [ ] 高价值内容提醒
  - [ ] DV P5 无回退层警告
  - [ ] 极端 VFR 转 CFR 的体积警告
- [ ] 每条决策产出中文理由 `Decision`
- [ ] 质量档位按编码器分别映射
- [ ] `fidelity.rs`：九类勾选项的判定与一键修正
- [ ] `estimate.rs`：体积与耗时范围预估（CFR 重复帧的体积按非线性修正）
- [ ] 界面接真实推荐与求解结果
- [ ] 测试
  - [ ] 每条保护规则的触发与不触发
  - [ ] 每类勾选项的三态判定
  - [ ] 每个 Fix 应用后冲突消失
  - [ ] 场景推荐的快照

## 阶段 6：执行与队列

- [ ] `runner.rs`：子进程启动、取消（Windows 需结束进程树）、stderr 环形缓冲
- [ ] `progress.rs`：块协议解析，只用 `out_time_us`，处理 `N/A`，ETA 指数平滑
- [ ] `scheduler.rs`
  - [ ] CPU 票与 GPU 票并发模型
  - [ ] 任务前 dry-run
  - [ ] 失败分类与回退链（按平台）
  - [ ] 超过 10 秒才失败时删除部分输出后从头重跑
  - [ ] 重试与指数退避
- [ ] 输出安全：临时名写入、成功后改名、冲突策略
- [ ] `persist.rs`：队列持久化与重启恢复
- [ ] 队列页接真实事件
- [ ] 测试
  - [ ] 进度解析：完整块、残缺块、`N/A`、纯音频、多输出流
  - [ ] ETA：前 5 秒不显示、平滑效果
  - [ ] 回退决策：每类失败对应的动作
  - [ ] 取消后无残留文件
  - [ ] 持久化往返
  - [ ] 批量 10 个合成素材端到端

## 阶段 7：校验、打磨与 macOS

- [ ] `verify.rs`
  - [ ] 时长、帧数、流数量
  - [ ] 色彩标签
  - [ ] HDR10 元数据（浮点容差比较）
  - [ ] 杜比视界配置记录与首帧 RPU
  - [ ] copy 轨道编码一致性
  - [ ] CFR：`r_frame_rate == avg_frame_rate` 且音视频时长差小于 1 帧
  - [ ] 保真度报告生成
- [ ] 引导下载 ffmpeg（Windows：gyan full / BtbN gpl；macOS：jellyfin-ffmpeg）
- [ ] 设置页接真实配置
- [ ] i18n：中文与英文
- [ ] 错误文案：所有 ffmpeg 报错转为中文说明加可行动作，原文可展开
- [ ] 首次使用引导
- [ ] 完成通知
- [ ] macOS 构建、签名、VideoToolbox 路径验证
- [ ] 走完需求文档第 6 节全部验收标准

## v2 待办

- [ ] 杜比视界 P7 双层处理（`dovi_split` + dovi_tool）
- [ ] HDR10+ 动态元数据（hdr10plus_tool + x265 CLI 或事后注入）
- [ ] BDMV 蓝光原盘解析与正片 playlist 选择
- [ ] VMAF / SSIM 抽样质量打分
- [ ] 采样试编码精确体积预估
- [ ] 字幕烧录
- [ ] 分段并行编码与断点续传
- [ ] 命令行版本与文件夹监视

## 待验证的技术事实

以下结论尚未在本机验证，代码中不得依赖，实现前需先确认：

- [ ] `hevc_amf` 是否写 HDR10 SEI（本机无 AMD 卡）
- [ ] `hevc_videotoolbox` 的实际行为（需 mac）
- [ ] swresample 的 `center_mixlev` 接受 dB 还是线性系数
- [ ] DTS 系列在 MP4 容器中的支持程度
