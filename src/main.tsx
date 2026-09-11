import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { initEngine } from "./lib/engine";
import { applyTheme, useUi } from "./stores/ui";
import "./index.css";

// 在首帧渲染前设置主题，避免闪烁
applyTheme(useUi.getState().theme);

// 决策引擎（WebAssembly）必须先就绪：store 在模块加载时就会用它（例如预览模式的演示队列），所以 App 延后导入
await initEngine();
const { default: App } = await import("./App");

// 开发模式下把 store 挂到 window，供 scripts/tauri-cdp.mjs 做端到端自测（原生文件对话框无法自动化）
if (import.meta.env.DEV) {
  void Promise.all([
    import("./stores/project"),
    import("./stores/capability"),
    import("./stores/queue"),
    import("./stores/settings"),
    import("./lib/engine"),
  ]).then(([p, c, q, s, engine]) => {
    Object.assign(window, {
      __vidforge: {
        useProject: p.useProject,
        useCapabilities: c.useCapabilities,
        useQueue: q.useQueue,
        useSettings: s.useSettings,
        useUi,
        engine,
      },
    });
  });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
