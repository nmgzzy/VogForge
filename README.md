# VidForge

Windows / macOS 桌面视频转码工具。后端调用系统 ffmpeg，按用途自动推荐参数，并能判断、保留、核对杜比视界、HDR、杜比全景声等高价值信息。

当前处于**阶段 1：前端界面骨架**。界面由前端 mock 引擎驱动，可在浏览器中完整预览；Rust 后端从阶段 2 开始接入。

## 文档

| 文档 | 内容 |
|---|---|
| [需求](docs/requirements.md) | 用户场景、功能需求（含优先级）、非功能需求、验收标准 |
| [设计](docs/design.md) | 架构、数据模型、能力探测、保真度求解、策略引擎、帧率策略、前端设计 |
| [实施计划](docs/plan.md) | 七个阶段、里程碑、测试推进原则、风险 |
| [TODO](docs/todo.md) | 按阶段的可勾选任务清单 |
| [ffmpeg 技术事实](docs/ffmpeg-facts.md) | 实现时必须知道的 ffmpeg 行为，每条标注来源（源码 / 文档 / 本机实测） |

## 运行预览

需要 Node 22+ 与 pnpm。

```bash
pnpm install
pnpm dev          # 浏览器打开 http://localhost:1420
```

预览版内置 6 个示例素材（iPhone 杜比视界、蓝光 remux、无人机、手机录屏、相机、流媒体片源），覆盖各类引擎规则。队列页的进度是模拟推进的。

## 测试

```bash
pnpm test         # Vitest，当前 600 个用例
pnpm typecheck    # TypeScript 类型检查
```

测试重点是 mock 引擎：遍历全部示例素材 × 全部场景，断言 `docs/ffmpeg-facts.md` 中的每条技术事实在生成的命令里都成立（例如不生成 `-vsync`、MP4 输出 HEVC 必带 `-tag:v hvc1`、源含杜比视界时显式传 `-dolbyvision`）。这些用例到阶段 4 会移植成 Rust 测试。

## 目录

```
docs/            需求、设计、计划、TODO、技术事实
src/
  components/    界面组件
  views/         五个页面：转码 / 队列 / 环境 / 预设 / 设置
  stores/        Zustand 状态：ui / capability / project / queue
  mock/          浏览器预览用的引擎、示例素材、环境与队列模拟
  lib/           类型定义与纯函数工具
crates/          Rust 核心库（阶段 2 起）
src-tauri/       Tauri 外壳（阶段 2 起）
```
