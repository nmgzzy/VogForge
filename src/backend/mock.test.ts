import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { mockBackend } from "./mock";

describe("mock 后端", () => {
  it("非强制探测当作命中缓存，只报一次进度", async () => {
    const stages: string[] = [];
    const off = mockBackend.onProbeProgress((p) => stages.push(p.stage));
    const caps = await mockBackend.getCapabilities(false, "zh-CN");
    off();
    expect(stages).toEqual(["读取缓存"]);
    expect(caps.status).toBe("ready");
    expect(caps.probedAt).not.toBe("");
  });

  it("取消订阅后不再收到进度", async () => {
    const stages: string[] = [];
    const off = mockBackend.onProbeProgress((p) => stages.push(p.stage));
    off();
    await mockBackend.getCapabilities(false, "zh-CN");
    expect(stages).toEqual([]);
  });

  it("设置读写往返", async () => {
    const saved = await mockBackend.saveSettings({ ...DEFAULT_SETTINGS, gpuSlots: 2 });
    expect(saved.gpuSlots).toBe(2);
    expect((await mockBackend.getSettings()).gpuSlots).toBe(2);
    await mockBackend.saveSettings(DEFAULT_SETTINGS);
  });
});
