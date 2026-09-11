# VidForge

Windows / macOS 桌面视频转码工具。后端调用系统 ffmpeg，按用途自动推荐参数，并能判断、保留、核对杜比视界、HDR、杜比全景声等高价值信息。

当前处于**阶段 3 完成**：Rust 核心库能定位 ffmpeg、做三层能力探测（编译能力、硬件设备初始化、真实试编码），并用 ffprobe 分析拖入的文件与文件夹（HDR10 / HLG / 杜比视界 / 全景声 / 无损音轨 / 图形字幕 / 可变帧率 / 拍摄设备）。转码页的推荐与命令生成暂时仍由前端 mock 引擎驱动，阶段 4–5 迁到 Rust。

## 文档

| 文档 | 内容 |
|---|---|
| [需求](docs/requirements.md) | 用户场景、功能需求（含优先级）、非功能需求、验收标准 |
| [设计](docs/design.md) | 架构、数据模型、能力探测、保真度求解、策略引擎、帧率策略、前端设计 |
| [实施计划](docs/plan.md) | 七个阶段、里程碑、测试推进原则、风险 |
| [TODO](docs/todo.md) | 按阶段的可勾选任务清单 |
| [ffmpeg 技术事实](docs/ffmpeg-facts.md) | 实现时必须知道的 ffmpeg 行为，每条标注来源（源码 / 文档 / 本机实测） |

## 运行

需要 Node 22+、pnpm、Rust stable；Windows 另需 MSVC 生成工具与 WebView2（Windows 11 自带）。转码本身需要 ffmpeg 7.1 或更高版本，应用会自动查找，也可以在设置里指定。

```bash
pnpm install
pnpm tauri dev    # 桌面应用
pnpm dev          # 只看界面：浏览器打开 http://localhost:1420，使用内置演示数据
```

浏览器预览内置 6 个示例素材（iPhone 杜比视界、蓝光 remux、无人机、手机录屏、相机、流媒体片源）与开发机的真实探测结果，队列进度是模拟推进的。

## 测试

```bash
pnpm test         # 前端 Vitest
pnpm typecheck    # TypeScript 类型检查
pnpm test:rust    # Rust 单元测试与真实 ffmpeg 集成测试
pnpm bindings     # 改了 Rust 模型后重新生成 src/bindings/ 下的 TS 类型
```

- 前端测试的重点是 mock 引擎：遍历全部示例素材 × 全部场景，断言 `docs/ffmpeg-facts.md` 中的每条技术事实在生成的命令里都成立。
- Rust 测试用本机采集的真实 ffmpeg / ffprobe 输出做解析与分类测试（`crates/vidforge-core/tests/fixtures/`）。两个集成测试在真实 ffmpeg 上运行，找不到时自动跳过：`probe_real.rs` 跑完整能力探测，能力受限与旧版本构建通过环境变量 `VIDFORGE_TEST_FFMPEG_ESSENTIALS`、`VIDFORGE_TEST_FFMPEG_OLD` 指定；`media_real.rs` 合成一批测试素材再走完整导入流程。
- 桌面应用端到端：以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev` 启动后，用 `node scripts/tauri-cdp.mjs` 驱动窗口与截图（仅 Windows）。

## 目录

```
docs/            需求、设计、计划、TODO、技术事实
src/
  backend/       前后端适配层：Tauri 实现与浏览器预览用的 mock 实现
  bindings/      ts-rs 从 Rust 生成的类型（勿手改）
  components/    界面组件
  views/         五个页面：转码 / 队列 / 环境 / 预设 / 设置
  stores/        Zustand 状态：ui / capability / settings / project / queue
  mock/          浏览器预览用的引擎、示例素材、环境与队列模拟
  lib/           类型定义与纯函数工具
crates/
  vidforge-core/ Rust 核心库：全部业务逻辑，不依赖 Tauri
src-tauri/       Tauri 外壳：只做命令注册与事件转发
scripts/         开发辅助脚本
```
