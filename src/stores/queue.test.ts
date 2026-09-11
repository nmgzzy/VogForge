import { beforeEach, describe, expect, it } from "vitest";
import { recommendPlan } from "@/mock/engine";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { buildReport, seedJobs } from "@/mock/queue";
import { useQueue } from "./queue";

const q = () => useQueue.getState();
const job = (id: string) => q().jobs.find((j) => j.id === id)!;
const running = () => q().jobs.filter((j) => j.status === "running");

beforeEach(() => {
  useQueue.setState({ jobs: seedJobs(), selectedJobId: undefined, concurrency: { cpu: 1, gpu: 1 }, globalPaused: false });
});

describe("queue store", () => {
  it("预置任务覆盖运行中、排队、完成、失败四种状态", () => {
    const statuses = new Set(q().jobs.map((j) => j.status));
    expect([...statuses].sort()).toEqual(["done", "failed", "queued", "running"]);
  });

  it("CPU 票同时只放行 1 个软编任务", () => {
    for (let i = 0; i < 5; i++) q().tick(0.5);
    const cpu = running().filter((j) => j.encoderUsed.startsWith("lib"));
    expect(cpu.length).toBeLessThanOrEqual(1);
  });

  it("CPU 并发调到 2 后，排队的软编任务开始运行", () => {
    q().setConcurrency("cpu", 2);
    q().tick(0.5);
    expect(running().filter((j) => j.encoderUsed.startsWith("lib")).length).toBe(2);
  });

  it("并发数限制在 1 到 4 之间", () => {
    q().setConcurrency("gpu", 9);
    expect(q().concurrency.gpu).toBe(4);
    q().setConcurrency("gpu", 0);
    expect(q().concurrency.gpu).toBe(1);
  });

  it("推进进度并在完成时生成保真度报告", () => {
    // GPU 任务速度快，推进足够多轮后必然完成
    for (let i = 0; i < 20; i++) q().tick(0.5);
    const gpu = job("job-run-gpu");
    expect(gpu.status).toBe("done");
    expect(gpu.report?.length).toBeGreaterThan(0);
    expect(gpu.report?.every((r) => r.ok)).toBe(true);
  });

  it("全局暂停时不推进任何进度", () => {
    const before = job("job-run-cpu").progress.percent;
    q().setGlobalPaused(true);
    q().tick(0.5);
    expect(job("job-run-cpu").progress.percent).toBe(before);
  });

  it("暂停的任务仍占用票据，不会放行下一个同类任务", () => {
    q().pause("job-run-cpu");
    for (let i = 0; i < 3; i++) q().tick(0.5);
    const cpuHolding = q().jobs.filter(
      (j) => (j.status === "running" || j.status === "paused") && j.encoderUsed.startsWith("lib"),
    );
    expect(cpuHolding.map((j) => j.id)).toEqual(["job-run-cpu"]);
    expect(job("job-q-bluray").status).toBe("queued");
  });

  it("暂停与继续", () => {
    q().pause("job-run-cpu");
    expect(job("job-run-cpu").status).toBe("paused");
    q().resume("job-run-cpu");
    expect(job("job-run-cpu").status).toBe("running");
  });

  it("取消后记录无残留的事件", () => {
    q().cancel("job-q-bluray");
    const j = job("job-q-bluray");
    expect(j.status).toBe("cancelled");
    expect(j.events.at(-1)?.message).toContain("无残留");
  });

  it("重试会清空进度与报告，重新排队", () => {
    q().retry("job-failed");
    const j = job("job-failed");
    expect(j.status).toBe("queued");
    expect(j.progress.percent).toBe(0);
    expect(j.report).toBeUndefined();
  });

  it("调整排队顺序", () => {
    const ids = () => q().jobs.map((j) => j.id);
    const i = ids().indexOf("job-q-screen");
    q().move("job-q-screen", -1);
    expect(ids().indexOf("job-q-screen")).toBe(i - 1);
  });

  it("加入队列时生成完整命令与输出路径", () => {
    const m = MOCK_MEDIA.find((x) => x.id === "m-drone")!;
    q().enqueue([{ media: m, plan: recommendPlan(m, "smallest", MOCK_CAPABILITIES) }]);
    const added = q().jobs.at(-1)!;
    expect(added.status).toBe("queued");
    expect(added.args[0]).toBe("ffmpeg");
    expect(added.outputPath).toMatch(/_av1\.mkv$/);
  });

  it("清除已完成只移除完成与取消的任务", () => {
    q().cancel("job-q-bluray");
    q().clearFinished();
    expect(q().jobs.some((j) => j.status === "done" || j.status === "cancelled")).toBe(false);
    expect(q().jobs.some((j) => j.status === "failed")).toBe(true);
  });
});

describe("Codex 审查修复：保真度报告如实反映计划", () => {
  it("蓝光 P7 按收藏场景重编码后，报告里杜比视界标为未保留", () => {
    const j = { ...job("job-q-bluray"), status: "done" as const };
    const report = buildReport(j);
    const dv = report.find((r) => r.label === "杜比视界");
    expect(dv?.ok).toBe(false);
    expect(dv?.actual).toContain("未保留");
    // 其余可保留的项仍然通过
    expect(report.find((r) => r.label === "全部音轨")?.ok).toBe(true);
  });

  it("iPhone 归档保留杜比视界，报告全部通过", () => {
    expect(job("job-done").report?.every((r) => r.ok)).toBe(true);
  });

  it("未勾选的项不出现在报告里", () => {
    const report = buildReport(job("job-run-cpu"));
    expect(report.some((r) => r.label === "杜比视界")).toBe(false);
  });

  it("完成时若有未保留项，事件以警告记录", () => {
    q().setConcurrency("cpu", 4);
    useQueue.setState({
      jobs: q().jobs.map((j) =>
        j.id === "job-q-bluray" ? { ...j, status: "running", progress: { ...j.progress, outTimeSec: j.media.durationSec - 0.01 } } : j,
      ),
    });
    q().tick(0.5);
    const j = job("job-q-bluray");
    expect(j.status).toBe("done");
    expect(j.events.at(-1)?.level).toBe("warn");
    expect(j.events.at(-1)?.message).toContain("杜比视界");
  });
});
