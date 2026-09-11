import { afterEach, describe, expect, it, vi } from "vitest";
import { backend } from "@/backend";
import { pendingCapabilities } from "@/lib/defaults";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { useCapabilities } from "./capability";

afterEach(() => {
  vi.restoreAllMocks();
  useCapabilities.setState({ caps: MOCK_CAPABILITIES, probing: false, progress: undefined, error: undefined });
});

describe("能力 store", () => {
  it("浏览器预览下用 mock 后端，初始即为演示数据", () => {
    expect(backend.kind).toBe("mock");
    expect(useCapabilities.getState().caps.status).toBe("ready");
  });

  it("重新探测期间 probing 为真，并接收进度；完成后清空进度", async () => {
    const seen: string[] = [];
    const off = useCapabilities.subscribe((s) => s.progress && seen.push(s.progress.stage));
    const p = useCapabilities.getState().reprobe();
    expect(useCapabilities.getState().probing).toBe(true);
    await p;
    off();
    expect(useCapabilities.getState().probing).toBe(false);
    expect(useCapabilities.getState().progress).toBeUndefined();
    expect(seen).toEqual(["检测编译能力", "初始化硬件设备", "试编码", "检测色调映射"]);
  });

  it("探测中重复调用不会并发发起第二次", async () => {
    const spy = vi.spyOn(backend, "getCapabilities");
    const a = useCapabilities.getState().reprobe();
    const b = useCapabilities.getState().reprobe();
    await Promise.all([a, b]);
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("后端调用失败时记录错误，保留原有能力数据", async () => {
    vi.spyOn(backend, "getCapabilities").mockRejectedValue(new Error("IPC 断开"));
    await useCapabilities.getState().load();
    const s = useCapabilities.getState();
    expect(s.error).toBe("IPC 断开");
    expect(s.probing).toBe(false);
    expect(s.caps).toBe(MOCK_CAPABILITIES);
  });
});

describe("探测完成前的占位能力", () => {
  it("软件编码器按可用处理，硬件与色调映射一律不可用", () => {
    const c = pendingCapabilities();
    expect(c.status).toBe("probing");
    expect(c.encoders.every((e) => e.vendor === "software" && e.usable)).toBe(true);
    expect(c.tonemap.every((t) => !t.available)).toBe(true);
    expect(c.dolbyVisionEncode).toBe(false);
  });
});
