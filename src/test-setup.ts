import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { initEngineSync } from "@/lib/engine";

// 组件与 store 测试用的是真实的 Rust 决策引擎（WebAssembly），与桌面应用同一份代码。
// jsdom 环境里 import.meta.url 不是 file: 地址，按项目根目录定位
initEngineSync(readFileSync(resolve("src/wasm/pkg/vidforge_wasm_bg.wasm")));
