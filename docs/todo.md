# VidForge TODO

更新于 2026-09-12（阶段 7 完成，剩 macOS 实机与杜比视界真实素材两项）

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

状态：已完成，达到里程碑 M3（Rust 生成的命令在真实 ffmpeg 上转码并通过 ffprobe 核对）。

- [x] `model/plan.rs`：`TranscodePlan` / `VideoPlan` / `AudioTrackPlan` / `FpsPolicy` / `Decision` / `PlanResult` 等，全部 ts-rs 导出，前端类型改为 re-export
- [x] `pipeline/args.rs`：按固定分段顺序生成 argv
  - [x] 全局参数：`-hide_banner -nostdin -y -loglevel warning -progress pipe:1 -nostats`
  - [x] 硬件解码参数：一律 `-hwaccel auto`（实测 `-hwaccel qsv` 会把帧留在 GPU 上导致失败）
  - [x] 流映射（含 MKV 附件 `-map 0:t?`）
  - [x] 视频编码参数，按编码器分派 RC 参数名（AMF / VideoToolbox 10bit 显式给 p010le）
  - [x] 色彩标签：不依赖 `-color_primaries` / `-color_trc`（9.0 不生效），保留靠帧、转 SDR 靠滤镜
  - [x] 滤镜链组装（缩放、四条色调映射管线、CFR 尾部 tpad 补齐）
  - [x] 音频逐轨参数、`pan` 降混、`aresample=async=1`
  - [x] 字幕、章节、元数据
  - [x] 容器附加参数
  - [x] 输出路径由调用方给（写临时文件与改名在阶段 6 的输出规划里做）
- [x] `pipeline/fps.rs`：标准档吸附、`fps_arg`、CFR 目标推荐、极端 VFR 判定、复制/丢弃帧数
- [x] `pipeline/encoders.rs`：质量档位映射、preset、10bit 与 HDR10 支持、自动选编码器
- [x] `pipeline/container.rs`：编码 × 容器兼容矩阵（MKV 全收；MP4 / MOV 白名单）
- [x] 容器装不下的音轨改为重编码（MOV → 24bit PCM，MP4 → E-AC-3 / AAC），不再生成跑不起来的命令
- [x] 视频按真实流序号映射（封面图可能排在前面）；带旋转的竖拍素材按显示方向缩放
- [x] loudnorm 两遍（阶段 6 随执行器一起完成）
- [x] 黄金样本：约 200 个样本由 TS 原型生成、与 Rust 逐条对照（变异检验确认有效）；阶段 5 删除原型后改由 Rust 维护
- [x] 快照测试（`insta`，15 个关键组合）：DV 8.4 归档 / 流媒体 QSV CFR / 手机 SDR / 修正后保留 DV、蓝光收藏 / 流媒体兼容轨 / remux、录屏剪辑 CFR、无人机 AV1、相机社交降混、macOS VideoToolbox、OpenCL 720p、scale_vt 硬编、NVENC 与 AMF 10bit
- [x] 断言型测试（`args_facts.rs`，每条技术事实在全部样本上成立）
  - [x] 任何输出都不含 `-vsync`
  - [x] CFR 用 `-fps_mode:v cfr`，不用 `fps` 滤镜
  - [x] MP4 / MOV 输出 HEVC 必含 `-tag:v hvc1`
  - [x] MP4 输出 DV 必含 `-strict unofficial`
  - [x] 源含 DV 且用 libx265 / libsvtav1 时显式 `-dolbyvision 0|1`
  - [x] QSV 10bit 必为 `p010le`（HEVC 加 `main10`），并显式 `-global_quality`
  - [x] NVENC 的 `-cq` 配 `-rc vbr -b:v 0`
  - [x] 默认不含 `-low_power`
  - [x] 不用 libaom、不用色彩输出选项、带 `-y` 与 `-f`
  - [x] `pan` 降混后不再出现 `-ac`
  - [x] 推荐出的计划不会把 TrueHD / DTS / 图形字幕原样放进 MP4 / MOV
- [x] 集成测试 `transcode_real.rs`（真实 ffmpeg，9 项）：x265 / SVT-AV1 自动透传 HDR10、三条色调映射输出 BT.709、QSV 保留 HDR10 + hvc1、VFR 转 CFR 帧间隔恒定且音画差小于 1 帧、remux 全轨道章节附件保留、pan 降混、硬解后 HDR10 side data 仍在、竖拍素材缩放为 720×1280、带封面图的文件编码正片

## 阶段 5：策略引擎与保真度求解

状态：已完成。决策引擎只剩 Rust 一份：编译成 WebAssembly 在界面里同步调用，TS 原型引擎已删除。

- [x] `strategy.rs`：8 个场景预设、按素材特征推荐起始场景、`normalize_plan` 保持计划自洽
- [x] 常识保护规则
  - [x] 不重复压缩（重编码省不到 25% 时提示，建议原样封装或调低画质 / 目标码率）
  - [x] 不放大分辨率
  - [x] 不提帧率
  - [x] VFR 按用途分岔
  - [x] HDR 转 SDR 必须插入色调映射（没有可用管线时保留 HDR 并警告）
  - [x] 高价值内容提醒（默认勾选对应保真度项）
  - [x] DV P5 无回退层警告
  - [x] 极端 VFR 转 CFR 的体积警告
- [x] 每条决策产出中文理由 `Decision`（`explain.rs`，按当前计划推导，改参数后同步更新）
- [x] 质量档位按编码器分别映射
- [x] 码率控制模式（需求 F-3.3）：恒定质量 / 目标码率 / 限峰值 / 两遍，按编码器分派写法，做不到的组合换成最接近的模式；各写法在真实 ffmpeg 上实测（技术事实文档 8.2）
- [x] 两遍编码的第一遍命令（`build_first_pass`，与第二遍逐段一致），界面预览与复制两行；执行在阶段 6
- [x] 手选编码器后 preset 不属于新编码器时换成默认值
- [x] `fidelity.rs`：八类勾选项的三态判定与一键修正（HDR10+ 按 v2 灰显）
- [x] `estimate.rs`：体积与耗时范围预估（CFR 重复帧按 3% 计入；按码率编码时体积由码率决定、范围收窄；两遍耗时约 1.7 倍）
- [x] `output.rs`：输出目录、命名模板（`{name}` `{height}` `{codec}` `{scenario}` `{date}`）、保留目录结构
- [x] `crates/vidforge-wasm`：引擎的纯函数部分编译成 wasm（`pnpm wasm`），产物 `src/wasm/pkg/` 随仓库提交；CSP 加 `'wasm-unsafe-eval'`（已在嵌入资源的构建里核对响应头）
- [x] 界面接真实推荐与求解结果：`src/lib/engine.ts` 包装，桌面、浏览器预览、组件测试同一份代码；规则表 `engineMeta()` 替代前端的质量刻度 / preset / 帧率档表
- [x] 删除 `src/mock/engine/`；场景文案、编码器名称、保真度说明移到 `src/lib/`；示例素材与环境改为 Rust 测试共用的 JSON（`tests/fixtures/samples/`）
- [x] 测试
  - [x] 每条保护规则的触发与不触发（`engine_behavior.rs`）
  - [x] 每类勾选项的三态判定、每个 Fix 应用后冲突消失（两套环境 × 全部素材 × 全部场景）
  - [x] 场景推荐的快照（开发机与 Mac 两套环境）
  - [x] 码率控制：写法事实断言（全部样本 × 三种模式）、8 个编码器 × 4 种模式快照、真实 ffmpeg 上两遍码率与 QSV 实际 RC 方式
  - [x] 回归样本改由 Rust 维护（`UPDATE_GOLDEN=1` 重写）；前端测试核对提交的 wasm 包与回归样本一致
  - [x] 桌面端到端：真实窗口里导入素材、切换两遍编码、查看两行命令

## 阶段 6：执行与队列

状态：已完成，达到里程碑 M5（批量可靠）。桌面端队列跑的是真实 ffmpeg，浏览器预览是同一接口的模拟队列。

- [x] `queue/process.rs`：子进程启动、逐行读进度、stderr 环形缓冲（500 行）、取消、暂停（Windows `SuspendThread`，类 Unix `SIGSTOP`）
- [x] ffmpeg 放进"句柄关闭即结束"的作业对象：应用被强杀时不留孤儿进程（Linux 用 `PR_SET_PDEATHSIG`）
- [x] `ffmpeg/progress.rs`：块协议解析，只用 `out_time_us`，处理 `N/A` 与负数，速度指数平滑与全程平均加权，前 5 秒不给 ETA，两遍合并进度
- [x] 调度（`queue/mod.rs`）
  - [x] CPU 票、GPU 票与原样封装的 IO 票，暂停的任务仍占票；并发数来自设置，改了立即生效
  - [x] 硬件编码器的任务前预检（同一组视频参数编 3 帧）
  - [x] 失败分类与回退链（按平台，`queue/fallback.rs`）；设备缺失时本次运行停用该厂商
  - [x] 运行超过 10 秒才失败时删除部分输出，改软编从头重跑并说明
  - [x] 资源不足按 1、2、4 秒退避重试，之后 GPU 任务逐个运行
  - [x] 全部暂停 / 全部继续、调序、重试、移除、清除已完成
- [x] 两遍编码的执行：先跑第一遍再跑主命令，进度按两遍合并，结束或取消后清理统计文件
- [x] 响度标准化：每条重新编码的音轨先测量再线性调整，loudnorm 后降采样（技术事实文档 6.5）；Opus 改用 `libopus`
- [x] 输出安全：写 `.vidforge-part` 临时文件，成功后改名；冲突策略跳过 / 自动改名 / 覆盖，改名时再检查一次；源文件永远不作为目标；并发任务的目标在队列状态里登记；设置里选覆盖时要求确认
- [x] `queue/persist.rs`：`~/.vidforge/queue.json` 持久化；重启后没跑完的任务删掉残留、重新排队
- [x] `verify.rs` 基础校验：时长、视频编码、固定帧率、音轨数、字幕数、章节
- [x] 设置里关掉硬件编码 / 解码时，引擎用的能力随之收紧（前后端一致）
- [x] 队列页接真实事件：`queue://snapshot` 与 `queue://progress`；操作被拒绝时显示原因；两遍编码显示第几遍；完成后可在文件夹中显示
- [x] 测试
  - [x] 进度解析：完整块、残缺块、`N/A`、纯音频、多输出流、Windows 换行
  - [x] ETA：前 5 秒不显示、平滑效果、两遍合并
  - [x] 回退决策：每类失败对应的动作（`fallback.rs` 单元测试 + `queue_sim.rs`）
  - [x] 取消后无残留文件（模拟与真实进程）
  - [x] 持久化往返、损坏文件备份、崩溃恢复
  - [x] 批量 10 个合成素材端到端（`queue_real.rs`，覆盖 8 个场景、两遍、目标码率，全部通过基础校验）
  - [x] 真实挂起与继续、NVENC 预检失败回退到 QSV、响度达到 -16 LUFS
  - [x] 桌面端到端：真实窗口里软编与硬编并行、完成后校验 3/3；强杀应用后 ffmpeg 随之结束，重启后任务重新排队
- [x] 长任务观察：1 小时素材（320×180）经真实队列 55 秒跑完，调度所在进程的工作集全程 7.4 MB 不增长，进度推送 107 次，任务事件与日志不随时长增长（`queue_real.rs` 的 `long_job_keeps_bounded_state`，默认忽略，`--ignored` 运行）

## 阶段 7：校验、打磨与 macOS

- [x] `verify.rs`
  - [x] 时长、帧数（MKV 输出没有 `nb_frames` 时数包）、流数量
  - [x] 色彩标签
  - [x] HDR10 元数据（按有理数求值比较，HEVC 与 AV1 分母不同也能判对）
  - [x] 杜比视界配置记录与首帧 RPU
  - [x] HDR10+（勾选保留时核对，重编码如实标红）
  - [x] copy 轨道编码一致性
  - [x] CFR：`r_frame_rate == avg_frame_rate` 且音视频时长差小于 1 帧
  - [x] 保真度报告生成（用户勾选且源里有的每一项）
- [x] 引导下载 ffmpeg：按平台列出推荐构建（Windows gyan full / BtbN gpl，macOS jellyfin-ffmpeg），应用的 ffmpeg 目录可一键打开；缺关键库时环境页也给出指引
- [x] 设置页接真实配置（界面语言、重新打开入门引导）
- [x] i18n：中文与英文（引擎、队列、校验、环境探测、界面全部按语言生成；英文下不残留中文由测试守住）
- [x] 错误文案：ffmpeg / ffprobe 报错转为说明加可行动作，原文放在可展开的"查看原文"（`ffmpeg/errors.rs`）
- [x] 首次使用引导：检查环境 → 能力说明 → 建议
- [x] 完成通知（设置里的 `notify`）与完成后动作（打开输出目录 / 源文件移到回收站，每批确认一次，只处理校验通过的任务）
- [x] macOS 上父进程被强杀时结束 ffmpeg：每个 ffmpeg 配一个 sh 看门狗（单元测试在 macOS CI 上跑）
- [x] 持续集成：前端检查（Linux）；Rust 格式、clippy、全部测试与 wasm 构建在 Windows 与 macOS 上各跑一遍，装真实 ffmpeg。macOS 首次运行暴露的 Windows 路径依赖已修，三个 job 全部通过
- [x] 960×640 最小窗口下各页面可用（队列页的统计徽章在窄窗口里隐藏）
- [x] Windows 硬解改写 `-hwaccel d3d11va`：自测时发现远程桌面断开后 `-hwaccel auto` 让 ffmpeg 崩溃（技术事实文档 7.5 节）
- [~] 走完需求文档第 6 节验收标准：8 条已有证据（见[计划](plan.md)的验收记录），第 4 条待真实杜比视界素材，第 10 条的 macOS 手动部分待实机
- [ ] macOS 实机：安装包签名与公证、VideoToolbox 编码路径、Homebrew 与 jellyfin-ffmpeg 下的界面降级（需要一台 Mac）
- [ ] 用真实杜比视界素材（iPhone P8.4）走一遍验收第 4 条，并把 ffprobe 输出补进技术事实文档

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
