import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { applyTheme, useUi } from "./stores/ui";
import "./index.css";

// 在首帧渲染前设置主题，避免闪烁
applyTheme(useUi.getState().theme);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
