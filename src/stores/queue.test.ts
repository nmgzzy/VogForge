import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { backend } from "@/backend";
import { mockQueue } from "@/backend/mock";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { recommendPlan } from "@/lib/engine";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { buildReport } from "@/mock/queue";
import { useQueue } from "./queue";
import { useSettings } from "./settings";

const q = () => useQueue.getState();
const job = (id: string) => q().jobs.find((j) => j.id === id)!;
const running = () => q().jobs.filter((j) => j.status === "running");
const software = (id: string) => job(id).encoderUsed.startsWith("lib");
const drone = MOCK_MEDIA.find((x) => x.id === "m-drone")!;

let off: (() => void) | undefined;

beforeEach(async () => {
  await useSettings.getState().update({ ...DEFAULT_SETTINGS });
  mockQueue.reset();
  useQueue.setState({ jobs: [], paused: false, selectedJobId: undefined, error: undefined });
  off = q().init();
  await Promise.resolve();
});

afterEach(() => off?.());

describe("队列 store（界面镜像，状态来自后端推送）", () => {
  it("取到预置任务，默认选中进行中的第一个", () => {
    const statuses = new Set(q().jobs.map((j) => j.status));
    expect([...statuses].sort()).toEqual(["done", "failed", "queued", "running"]);
    expect(job(q().selectedJobId!).status).toBe("running");
  });

  it("运行中的进度逐块推送，不必整体刷新", () => {
    const before = job("job-run-cpu").progress.percent;
    mockQueue.tick(0.5);
    expect(job("job-run-cpu").progress.percent).toBeGreaterThan(before);
  });

  it("CPU 票同时只放行 1 个软编任务；并发调到 2 后排队的软编开始", async () => {
    for (let i = 0; i < 5; i++) mockQueue.tick(0.5);
    expect(running().filter((j) => software(j.id)).length).toBeLessThanOrEqual(1);
    await useSettings.getState().update({ cpuSlots: 2 });
    mockQueue.tick(0.5);
    expect(running().filter((j) => software(j.id)).length).toBe(2);
  });

  it("完成时生成校验报告", () => {
    for (let i = 0; i < 20; i++) mockQueue.tick(0.5);
    const gpu = job("job-run-gpu");
    expect(gpu.status).toBe("done");
    expect(gpu.report?.every((r) => r.ok)).toBe(true);
  });

  it("全部暂停：进行中的挂起、排队的不开始；全部继续后恢复", async () => {
    await q().setGlobalPaused(true);
    expect(q().paused).toBe(true);
    expect(job("job-run-cpu").status).toBe("paused");
    const before = job("job-run-cpu").progress.percent;
    mockQueue.tick(0.5);
    expect(job("job-run-cpu").progress.percent).toBe(before);
    await q().setGlobalPaused(false);
    expect(job("job-run-cpu").status).toBe("running");
  });

  it("暂停的任务仍占用票据，不会放行下一个同类任务", async () => {
    await q().pause("job-run-cpu");
    for (let i = 0; i < 3; i++) mockQueue.tick(0.5);
    const holding = q().jobs.filter((j) => (j.status === "running" || j.status === "paused") && software(j.id));
    expect(holding.map((j) => j.id)).toEqual(["job-run-cpu"]);
    expect(job("job-q-bluray").status).toBe("queued");
    await q().resume("job-run-cpu");
    expect(job("job-run-cpu").status).toBe("running");
  });

  it("后端拒绝的操作显示原因", async () => {
    await q().pause("job-q-bluray");
    expect(q().error).toBe("只有进行中的任务可以暂停");
    q().dismissError();
    expect(q().error).toBeUndefined();
  });

  it("取消、重试、换序、清除已完成", async () => {
    await q().cancel("job-q-bluray");
    expect(job("job-q-bluray").status).toBe("cancelled");
    await q().retry("job-failed");
    expect(job("job-failed").status).toBe("queued");
    expect(job("job-failed").progress.percent).toBe(0);
    const ids = () => q().jobs.map((j) => j.id);
    const i = ids().indexOf("job-q-screen");
    await q().move("job-q-screen", -1);
    expect(ids().indexOf("job-q-screen")).toBe(i - 1);
    await q().clearFinished();
    expect(q().jobs.some((j) => j.status === "done" || j.status === "cancelled")).toBe(false);
  });

  it("加入队列：命令与输出路径按当前设置计算，与转码页预览一致", async () => {
    await useSettings.getState().update({ outputDir: "E:/out", namingTemplate: "{name}-{scenario}" });
    await q().enqueue([{ media: drone, plan: recommendPlan(drone, "archive", MOCK_CAPABILITIES) }]);
    const added = q().jobs.at(-1)!;
    expect(added.status).toBe("queued");
    expect(added.args[0]).toBe("ffmpeg");
    expect(added.outputPath).toBe("E:/out/DJI_20260812_0142-归档.mkv");
  });

  it("两遍编码的任务带着第一遍命令", async () => {
    const plan = recommendPlan(drone, "archive", MOCK_CAPABILITIES);
    plan.video.rateControl = { kind: "two_pass", kbps: 8000 };
    await q().enqueue([{ media: drone, plan }]);
    expect(q().jobs.at(-1)!.firstPass).toContain("-pass");
  });

  it("桌面端与预览走同一个接口", () => {
    expect(backend.kind).toBe("mock");
  });
});

describe("预览里的模拟校验报告如实反映计划", () => {
  it("蓝光 P7 按收藏场景重编码后，报告里杜比视界标为未保留", () => {
    const report = buildReport({ ...job("job-q-bluray"), status: "done" });
    const dv = report.find((r) => r.label === "杜比视界");
    expect(dv?.ok).toBe(false);
    expect(dv?.actual).toContain("未保留");
    expect(report.find((r) => r.label === "全部音轨")?.ok).toBe(true);
  });

  it("iPhone 归档保留杜比视界，报告全部通过；未勾选的项不出现", () => {
    expect(job("job-done").report?.every((r) => r.ok)).toBe(true);
    expect(buildReport(job("job-run-cpu")).some((r) => r.label === "杜比视界")).toBe(false);
  });

  it("完成时若有未保留项，事件以警告记录", async () => {
    await useSettings.getState().update({ cpuSlots: 4 });
    mockQueue.tick(0.5);
    expect(job("job-q-bluray").status).toBe("running");
    mockQueue.tick(1e6);
    const j = job("job-q-bluray");
    expect(j.status).toBe("done");
    expect(j.events.at(-1)?.level).toBe("warn");
    expect(j.events.at(-1)?.message).toContain("杜比视界");
  });
});
