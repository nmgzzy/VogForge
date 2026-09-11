import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { applyTheme, useUi } from "./stores/ui";
import "./index.css";

// 在首帧渲染前设置主题，避免闪烁
applyTheme(useUi.getState().theme);

// 开发模式下把 store 挂到 window，供 scripts/tauri-cdp.mjs 做端到端自测（原生文件对话框无法自动化）
if (import.meta.env.DEV) {
  void Promise.all([
    import("./stores/project"),
    import("./stores/capability"),
    import("./stores/queue"),
    import("./stores/settings"),
  ]).then(([p, c, q, s]) => {
    Object.assign(window, {
      __vidforge: { useProject: p.useProject, useCapabilities: c.useCapabilities, useQueue: q.useQueue, useSettings: s.useSettings, useUi },
    });
  });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
