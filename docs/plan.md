# VidForge 实施计划

版本 0.1 · 2026-09-12

配套文档：[需求](requirements.md) · [设计](design.md) · [TODO](todo.md) · [ffmpeg 技术事实](ffmpeg-facts.md)

当前进度：阶段 0–7 已完成，达到里程碑 M6（结果可信：每个任务都有逐项核对的保真度报告，验收标准除两项需要特定条件外均有证据，见文末验收记录）。剩下 macOS 实机验证与杜比视界真实素材两项，逐项状态见 [TODO](todo.md)。

## 总体策略

分七个阶段，每个阶段结束时项目都处于**可运行、可验证**的状态，而不是等到最后才第一次跑起来。

阶段顺序的依据是依赖关系，不是功能重要性：能力探测要先于一切（因为所有决策都依赖 `Capabilities`），命令构建要先于策略引擎（策略的产物是 Plan，Plan 要能变成命令才可验证），执行与队列要先于校验。

界面开发与后端并行：先用 mock 数据把界面做出来确认交互，再逐步接真实后端。这样界面问题能早发现，不会等到后端完成才暴露。

## 阶段划分

### 阶段 0：脚手架与文档

| 项 | 内容 |
|---|---|
| 交付 | Cargo workspace（`vidforge-core` + `src-tauri`）、Vite + React + TS、Tailwind v4、Zustand、ts-rs 导出链路、`insta` 快照测试框架、Vitest、基础布局与路由 |
| 验证 | `pnpm tauri dev` 能起窗口；`cargo test` 通过；`pnpm test` 通过 |
| 风险 | Tauri 2 首次编译耗时较长（依赖树大），属正常 |

### 阶段 1：前端界面骨架（mock 数据）

提前到这里，是为了尽早确认交互设计。

| 项 | 内容 |
|---|---|
| 交付 | 五个页面（转码 / 队列 / 环境 / 预设 / 设置）的完整界面；文件列表与特征徽章；L1 场景卡片；L2 关键旋钮；保真度勾选清单的三态视觉；L3 专家面板；命令预览条；队列进度条与日志视图。全部用 mock 数据驱动 |
| 验证 | 浏览器里 `pnpm dev` 可完整浏览所有界面与交互状态；深浅色主题正常；窄窗口不破版 |
| 产出 | 界面确认后再进入后端，避免返工 |

### 阶段 2：ffmpeg 定位与能力探测

| 项 | 内容 |
|---|---|
| 交付 | `locate.rs` 四级定位（含读注册表 PATH）、`capability.rs` 三层探测与缓存、环境页接真实数据 |
| 验证 | 本机探测结果须与已实测基线一致：QSV 六项 PASS，NVENC 报 `Cannot load nvcuda.dll`，AMF 报 `DLL amfrt64.dll failed to open`；另需测 ffmpeg 不存在、版本过低、能力受限（gyan essentials）三种情况 |
| 依赖 | 无 |

### 阶段 3：媒体分析

| 项 | 内容 |
|---|---|
| 交付 | `probe.rs`：ffprobe JSON 到 `MediaInfo`；HDR / DV / Atmos / 无损音轨 / PGS 识别；**两级 VFR 判定**；设备来源推断；文件导入（拖拽、批量、文件夹递归） |
| 验证 | 用 fixture JSON 做单测，覆盖手机 HDR、多音轨 MKV、VFR MP4、蓝光 remux 四类；VFR 判定需同时通过 MP4 与 MKV 两个 case（MKV 靠判据 2 命中） |
| 依赖 | 阶段 2 |

### 阶段 4：命令构建与容器矩阵（测试最重的一环）

这是整个项目质量的支点。ffmpeg 参数写错会静默丢元数据、退出码仍为 0，只能靠测试网拦住。

| 项 | 内容 |
|---|---|
| 交付 | `args.rs` 命令构建、`container.rs` 兼容矩阵、`color.rs` 色彩与色调映射管线选择、`audio.rs` 音轨策略与降混、`fps.rs` 帧率策略 |
| 验证 | `insta` 快照覆盖 20+ 组合；每条技术事实对应一个断言（例如 MP4 输出 HEVC 必须含 `-tag:v hvc1`、输出不得含 `-vsync`、CFR 必须是 `-fps_mode:v cfr` 加 `-r`、不保留 DV 时必须显式 `-dolbyvision 0`）；抽样命令在真实 ffmpeg 上跑通 |
| 依赖 | 阶段 3 |

### 阶段 5：策略引擎与保真度求解

| 项 | 内容 |
|---|---|
| 交付 | `strategy.rs` 场景推荐与常识保护规则、`fidelity.rs` 约束求解与一键修正、`estimate.rs` 体积与耗时预估；界面接真实推荐与求解结果 |
| 验证 | 各类输入源的推荐合理且理由正确；冲突提示与修正按钮行为正确；不重复压缩、不放大、不提帧率、HDR 必映射等保护规则均有单测 |
| 依赖 | 阶段 4 |

### 阶段 6：执行与队列

| 项 | 内容 |
|---|---|
| 交付 | `runner.rs` 子进程管理、`progress.rs` 块协议解析、`scheduler.rs` 并发票据与分类回退、`persist.rs` 持久化；队列页接真实事件 |
| 验证 | 批量 10 个文件跑通；取消不留半成品；强杀应用后重启能恢复队列；禁用硬编时自动回退且日志说明原因；长任务（1 小时以上）无内存泄漏 |
| 依赖 | 阶段 4（不强依赖 5） |

### 阶段 7：校验、打磨与 macOS

| 项 | 内容 |
|---|---|
| 交付 | `verify.rs` 输出校验与保真度报告、引导下载 ffmpeg、设置页、i18n、错误文案、首次使用引导；macOS 构建与验证 |
| 验证 | 走完需求文档第 6 节的 10 条验收标准 |
| 依赖 | 全部 |

## 里程碑

| 里程碑 | 含义 | 对应阶段 |
|---|---|---|
| M1 界面可见 | 能看到完整界面并走通交互（mock 数据） | 0-1 |
| M2 环境可知 | 应用能准确说出当前环境能做什么、不能做什么 | 2 |
| M3 首次转码 | 能完成一次真实转码并看到进度 | 3-4 + 6 的最小路径 |
| M4 推荐可用 | 选场景即得合理参数，保真度冲突能解释与修正 | 5 |
| M5 批量可靠 | 批量任务稳定，可恢复，回退正确 | 6 |
| M6 结果可信 | 保真度报告完整，验收标准全过 | 7 |

M3 刻意安排成跨阶段的最小路径：阶段 4 完成后，用手工构造的 Plan 直接调 runner 跑一次转码，不等策略引擎。这样能在中途就验证整条管线真的通。

## 并行与顺序约束

```
阶段 0 ──> 阶段 1（前端 mock，可独立推进）
       └─> 阶段 2 ──> 阶段 3 ──> 阶段 4 ──┬─> 阶段 5 ──┐
                                        └─> 阶段 6 ──┴─> 阶段 7
```

阶段 1 与 2-4 可并行。阶段 5 与 6 可并行（都只依赖 4）。

## 测试推进原则

1. **测试与实现同阶段交付**，不留到最后补。每个阶段的验证项就是该阶段的测试清单。
2. 纯函数模块（`args` / `strategy` / `fidelity` / `container` / `fps`）的测试覆盖率要求最高，因为它们承载全部决策且最容易出静默错误。
3. 集成测试用**合成素材**，不依赖用户手里的片源。生成脚本放 `tests/fixtures/`，生成带 HDR10 元数据、多音轨、VFR 的测试片段。
4. 对环境敏感的集成测试（如 QSV 编码）先查 `Capabilities` 再决定跳过或执行，不假定 CI 有 GPU。
5. 每条写进 [技术事实文档](ffmpeg-facts.md) 的结论，都应有一个对应的断言把它钉住。文档说"不得生成 `-vsync`"，就要有测试断言输出里没有 `-vsync`。

## 已识别风险

| 风险 | 应对 |
|---|---|
| 用户环境 ffmpeg 能力参差 | 开发机是理想环境（gyan full 9.0.1），必须另备能力受限的 ffmpeg 测降级路径 |
| macOS 色调映射管线缺失 | Homebrew 构建三条管线全无。引导下载 jellyfin-ffmpeg；若用户坚持用系统版则灰显并说明 |
| macOS 需要实机 | 打包签名与 VideoToolbox 路径无法在 Windows 上验证。v1 先完成 Windows，mac 作为阶段 7 的一部分 |
| 无 NVIDIA / AMD 显卡 | 这两条编码路径只能靠错误分类的单元测试覆盖，无法端到端验证。错误判据已从真实报错采集 |
| 杜比视界 P7 双层 | ffmpeg 无法保留，必须降级 8.1 且丢失 FEL 映射。界面必须如实说明，不能含糊承诺 |
| Atmos 无法编码 | 只能流复制。界面必须明确，避免用户以为"尽量保留"意味着能重编码 |

## 验收记录

需求文档第 6 节的验收标准，阶段 7 结束时（2026-09-12，Windows 11 开发机，ffmpeg 9.0.1 gyan full）逐条核对：

| # | 结论 | 依据 |
|---|---|---|
| 1 | 通过 | `capability.rs` 的 `missing_ffmpeg`、`present_but_unusable_ffmpeg_is_broken_not_missing`；环境页与入门引导在缺失时给出下载指引与应用的 ffmpeg 目录（`EnvironmentView.test.tsx`、`Onboarding.test.tsx`，桌面端截图核对）。"未预装"是模拟的缺失状态，没有在一台干净机器上跑 |
| 2 | 通过（合成素材） | `transcode_real.rs` 的 HDR10 保留、`queue_real.rs` 的 `hdr10_fidelity_report_passes_for_x265_qsv_and_svtav1`（libx265、hevc_qsv、libsvtav1 的报告都判 HDR10 已保留）。体积与画质的主观判断需要真实手机素材 |
| 3 | 通过（合成素材） | `tonemapped_output_is_tagged_bt709_by_the_filter`、`qsv_keeps_hdr10_and_mp4_gets_hvc1`；`args_facts.rs` 断言全部黄金样本的 MP4 HEVC 带 `hvc1` 与 faststart |
| 4 | 待真实素材 | 杜比视界识别（fixture）、`-dolbyvision 1` 与 MP4 的 `-strict unofficial`（`args_facts.rs`）、报告核对配置记录与 RPU（`verify.rs` 单元测试）都已覆盖；本机没有带 RPU 的素材，没有做真实编码 |
| 5 | 通过（PGS 除外） | `bluray_collection_copies_every_track_sub_and_chapter`、`remux_copies_every_track_chapter_and_attachment`；桌面端批量里 3 个多音轨 + 章节 + 文本字幕的 MKV 报告全部通过。ffmpeg 没有 PGS 编码器，造不出带 PGS 的样本，PGS 路径只有 fixture 测试 |
| 6 | 通过 | `bluray_streaming_lossless_fix_switches_to_mkv_and_copies_truehd`、`every_fix_resolves_its_conflict`；保真度面板在转码前列出冲突与修正按钮（`panels.test.tsx`） |
| 7 | 通过 | 桌面端关掉 GPU 编码后，20 个文件 × 8 个场景全部完成，决策说明写明"没有可用的硬件编码器，已改用软件编码"；`queue_sim.rs` 的 `hardware_encoding_disabled_in_settings_uses_software` |
| 8 | 通过 | 桌面端导入 20 个混合素材（H.264 / HEVC HDR10 / HLG / 可变帧率 / 多音轨 / AV1 / VP9 / MPEG-4，MP4 / MKV / MOV / WebM / AVI），全部完成且报告全部通过；第二批跑到一半强杀应用，ffmpeg 随之结束，重启后 2 个中断的任务带警告重新排队、临时文件已删，20 个全部完成，目标目录没有残留 |
| 9 | 通过 | `vfr_to_cfr_gives_strictly_constant_frame_rate_and_aligned_audio`；桌面端批量里可变帧率素材走剪辑预处理，报告的"固定帧率""音画对齐"都通过 |
| 10 | Windows 通过，macOS 自动化部分通过 | 每个 P0 功能点在 [TODO](todo.md) 各阶段都有对应测试或端到端记录（设计文档第 7 节列出测试分层）。CI 在 macOS（Homebrew ffmpeg）上跑 clippy 与全部 Rust 测试已通过，首次运行暴露的 6 个依赖 Windows 路径语义的单元测试已改为与平台无关；界面与 VideoToolbox 的手动部分待实机 |
