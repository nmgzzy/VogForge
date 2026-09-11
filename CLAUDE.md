# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

VidForge：Windows / macOS 桌面视频转码工具，后端调用系统 ffmpeg。文档、界面文案、代码注释、测试名一律使用简体中文。

## 当前阶段

阶段 0–5 已完成：Cargo workspace、Tauri 外壳、ffmpeg 定位与三层能力探测、ffprobe 媒体分析与文件导入、Rust 决策引擎（`crates/vidforge-core/src/pipeline/`：场景推荐、保真度求解、命令构建、预估、码率控制）都已接通。引擎编译成 WebAssembly 驱动界面，没有第二份 TS 实现。下一步是阶段 6 执行与队列（队列页目前仍是 `src/mock/queue.ts` 的模拟）。阶段划分与验收项见 `docs/plan.md`，逐项进度见 `docs/todo.md`。

**改引擎规则后的流程。** 先让 Rust 测试反映新规则：有意的产出变化用 `UPDATE_GOLDEN=1 cargo test -p vidforge-core --test golden_engine` 重写 `tests/fixtures/golden/engine.json`，`INSTA_UPDATE=always cargo test -p vidforge-core` 重写快照，逐个审阅 diff。然后 `pnpm wasm` 重新生成 `src/wasm/pkg/` 并一起提交；忘了这一步 `src/lib/engine.golden.test.ts` 会失败（它用 wasm 重算回归样本）。

## 常用命令

```bash
pnpm tauri dev                                  # 桌面应用（会先启动 vite）
pnpm dev                                        # 只起前端：http://localhost:1420，strictPort，用 mock 后端
pnpm test                                       # 前端 vitest
pnpm vitest run src/lib/engine.test.ts          # 单个文件
pnpm vitest run -t "quoteArg"                   # 按用例名过滤
pnpm typecheck                                  # tsc -b --noEmit（TypeScript 7）
pnpm test:rust                                  # cargo test --workspace
cargo test -p vidforge-core parse::             # 按模块路径过滤 Rust 用例
pnpm bindings                                   # 改了 Rust 模型后重新生成 src/bindings/
pnpm wasm                                       # 改了引擎后重新生成 src/wasm/pkg/（需 wasm32 target 与 wasm-bindgen-cli 0.2.128）
cargo clippy --workspace --all-targets          # Rust lint，要求零告警
cargo fmt --all                                 # rustfmt.toml：max_width 120
```

前端没有配置 ESLint / Prettier。tsconfig 开了 `strict`、`noUncheckedIndexedAccess`、`noUnusedLocals/Parameters`，类型检查就是 lint。路径别名 `@/` 指向 `src/`。

`crates/vidforge-core/tests/media_real.rs` 用真实 ffmpeg 合成素材再走完整导入流程，也是 `tests/fixtures/probe/` 里真实 fixture 的生成方法（fixture 来源见该目录的 README）。`tests/probe_real.rs` 在真实 ffmpeg 上跑完整探测：默认找开发机的 `C:\Program1\ffmpeg\bin`，找不到就跳过；`VIDFORGE_TEST_FFMPEG_ESSENTIALS`、`VIDFORGE_TEST_FFMPEG_OLD` 指向能力受限与低于 7.1 的构建时才跑对应用例。只有 Intel 显卡的机器会额外核对开发机基线，其他机器设 `VIDFORGE_SKIP_BASELINE` 跳过。

桌面应用端到端自测（仅 Windows）：用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev` 启动，再用 `node scripts/tauri-cdp.mjs wait:文本 click:文本 fill:占位|值 shot:文件.png eval:表达式` 驱动窗口并截图。原生文件对话框与拖放无法通过它操作：开发模式下 store 挂在 `window.__vidforge` 上，可以用 `eval:` 直接调 `window.__vidforge.useProject.getState().importPaths([...])`（走的仍是真实 Tauri 命令）。不要用 `import('/src/stores/…')`，热更新后模块地址带时间戳，会拿到另一个 store 实例。

## 架构

全部业务逻辑放在纯 Rust 库 `crates/vidforge-core`，不依赖 Tauri；`src-tauri` 只做命令注册与事件转发（`src-tauri/src/commands.rs`）。决策模块（命令构建、策略、保真度求解）是纯函数，环境信息一律通过 `model::Capabilities` 传入。与外部进程打交道的代码经过 `ffmpeg::exec::Runner` trait，定位还经过 `ffmpeg::locate::Env` trait，单元测试用假实现，配合 `tests/fixtures/ffmpeg/` 下本机采集的真实 ffmpeg 输出。

前后端共享的类型写在 `vidforge-core/src/model/`，用 ts-rs 导出到 `src/bindings/`（生成物要提交，勿手改）。约定：结构体 `#[serde(rename_all = "camelCase")]`，可选字段加 `#[ts(optional_fields)]` 与 `skip_serializing_if`，对应 TS 的 `field?: T`；u64 导出为 `number`（见 `.cargo/config.toml`）。`src/lib/types.ts` 对已迁移的类型只做 re-export，其余类型随阶段迁移。

前端通过 `src/backend/` 的 `Backend` 接口访问后端：Tauri 窗口里是 `invoke` + 事件（`tauri.ts`），浏览器预览与 vitest 里是 `mock.ts`，由 `isTauri()` 自动选择。store 只依赖这个接口。

决策引擎的入口在 `vidforge-core/src/pipeline/mod.rs`，`crates/vidforge-wasm` 把它们导出成 JSON 字符串进出的 wasm 函数，前端经 `src/lib/engine.ts` 同步调用（设计文档 6.5）：

- `recommendPlan(media, scenario, caps)` —— 按场景生成 `TranscodePlan`
- `updatePlan(plan, media, caps)` —— 用户改参数后调用 `normalize_plan`，把计划修回自洽状态（重选编码器、换掉不属于新编码器的 preset、拉回越界码率、换掉编码器做不到的码率控制……）
- `applyFix(plan, fixId, media, caps)` —— 执行保真度冲突的一键修正，之后同样 normalize
- `evaluate(media, plan, caps, settings?, date?)` —— 派生界面所需的全部内容：推荐理由、保真度、分段命令、两遍编码的第一遍、预估；输出路径按设置里的目录与命名模板计算
- `engineMeta()` / `encoderMeta(id)` / `videoHints(media)` —— 界面用的规则表（质量刻度、preset、支持的码率控制、标准帧率档）与帧率建议

引擎必须先初始化：`main.tsx` 里 `await initEngine()` 之后才动态导入 `App`（部分 store 在模块加载时就调用引擎），vitest 在 `src/test-setup.ts` 里同步初始化。`src/lib/` 里的 `scenarios.ts`、`encoders.ts`、`fidelity.ts` 只放显示用的文案与对 `Capabilities` 的简单查询，不放决策规则。示例素材与环境（`src/mock/media.ts`、`capabilities.ts`）读的是 `crates/vidforge-core/tests/fixtures/samples/` 的 JSON，与 Rust 行为测试共用。

`PlanResult` 是派生数据，从不存储。`stores/project.ts` 只存 `files` 与按媒体 id 索引的 `plans`，所有修改走 `patchPlan`（内部 `structuredClone` 后调 `updatePlan`）；能力快照变化时 `App.tsx` 调 `refreshPlans` 重新整理全部计划。Zustand v5 里返回对象的 selector 必须包 `useShallow`，否则会无限重渲染。

## ffmpeg 行为的单一事实源

`docs/ffmpeg-facts.md` 记录全部已核实的 ffmpeg 行为，每条标注来源（源码 / 文档 / 本机实测）。其中很多规则与直觉相反，例如 9.0 已移除 `-vsync`、`-dolbyvision` 默认 auto 必须显式传 0 或 1、转固定帧率不能用 fps 滤镜、不支持的 `-pix_fmt` 会被静默替换成 8bit。改参数生成或探测逻辑前先查这份文档，不要凭记忆；新验证的行为写进去并标 [实测]。

`crates/vidforge-core/tests/args_facts.rs` 把每条事实在约 200 个回归样本上各断言一遍，`engine_behavior.rs` 断言推荐与求解的行为，`transcode_real.rs` 在真实 ffmpeg 上核对输出。约定：新增一条事实就补一条断言；改参数生成后这些断言必须全绿。

## 文档约定

- `docs/` 下五份文档分工：requirements 需求与验收标准、design 架构与设计、plan 阶段与里程碑、todo 可勾选任务、ffmpeg-facts 技术事实。完成任务后同步勾选 todo（图例 `[x]` `[ ]` `[~]` `[!]`）。
- `docs/design.md` 与 `docs/ffmpeg-facts.md` 的章节号是外部契约，代码注释会引用（如 `docs/design.md 6.5`）。新增内容放到末尾或做成新的末位小节，不要重排已有编号。
- 界面改动遵循设计文档 6.6「信息密度原则」：正常状态保持安静，出问题才醒目；默认只给结论，解释按需展开；低频参数放「更多参数」。改完实际看一遍再交付。

## 容易踩的坑

- 在这台机器上，Bash 工具会把 heredoc 里的 `\\` 折叠成 `\`（经 heredoc 传给 python 也一样）。含反斜杠的文件内容（Windows 路径、正则、转义）一律用 Write / Edit 写。
- 同样的折叠会把经 heredoc 传给 python 的 `\\n` 变成 `\n`，写进源码就是字符串里断了一行。要写 `\n` 转义时用 `chr(92)` 拼，或用 Edit。
- 停掉后台的 `pnpm tauri dev` 只会结束外层 shell，vite（占 1420）、`cargo run` 与 `vidforge.exe` 会留下来，要按进程号结束。WebView2 按应用共用数据目录，已有一个实例在跑时，第二个实例（例如另一个调试端口）拿不到调试端口。
- 决策引擎的 wasm 需要 CSP `script-src 'wasm-unsafe-eval'`（`tauri.conf.json`）。开发模式不下发 CSP，只有嵌入资源的构建（`pnpm tauri build`）才会暴露这类问题。
- 引擎代码（会编译成 wasm）里不要用 `std::path` 解析媒体路径：wasm 上它只认 `/`，`D:\素材\a.mov` 会变成没有父目录的文件名。用 `output.rs` 里按字符串处理、两种分隔符都认的函数。
- `wasm-bindgen` 依赖锁定为 `=0.2.128`，必须与本机 `wasm-bindgen-cli` 版本一致，否则 `pnpm wasm` 生成的胶水代码与 wasm 不匹配。
- 复制命令用的 `quoteArg`（`src/lib/format.ts`）默认面向 PowerShell：逗号是数组运算符、行首 `@` 是 splatting，所以 `SAFE_ARG` 刻意不含这两个字符，不要放宽。
- 用户填写的附加参数用 `splitArgs` 解析，支持引号，不要改回按空格拆分。
- 布局按容器宽度响应（Tailwind v4 的 `@container` 与 `@min-[900px]:`），不是按视口宽度。
- Windows 上启动子进程一律经过 `ffmpeg::exec::command`，它设置了 `CREATE_NO_WINDOW`，否则发布版每次调用 ffmpeg 都会闪一个控制台窗口。
- 开发机的 ffmpeg 是 9.0.1 gyan full，位于 `C:\Program1\ffmpeg\bin`，只在注册表 PATH 里，从 Git Bash 启动的进程看不到，定位时靠注册表那一步找到。
