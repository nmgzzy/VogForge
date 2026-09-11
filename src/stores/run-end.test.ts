import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backend } from "@/backend";
import { notifications, trashed } from "@/backend/mock";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import type { Job, JobStatus } from "@/lib/types";
import { makeJob } from "@/mock/queue";
import { MOCK_MEDIA } from "@/mock/media";
import { recommendPlan } from "@/lib/engine";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { useQueue } from "./queue";
import { runEndActions, watchRunEnd } from "./run-end";
import { useSettings } from "./settings";

const ok = [{ label: "时长", expected: "1:00", actual: "1:00", ok: true }];
const bad = [{ label: "HDR10", expected: "保留", actual: "缺失", ok: false }];

function job(id: string, status: JobStatus, report?: Job["report"]): Job {
  const m = MOCK_MEDIA[0]!;
  const j = makeJob(m, recommendPlan(m, "archive", MOCK_CAPABILITIES), DEFAULT_SETTINGS, id);
  return { ...j, status, report };
}

beforeEach(async () => {
  notifications.length = 0;
  trashed.length = 0;
  await useSettings.getState().update({ ...DEFAULT_SETTINGS });
});
afterEach(() => vi.restoreAllMocks());

describe("一批任务跑完之后", () => {
  it("开了通知就发一条，写明成功与失败的数量", async () => {
    await useSettings.getState().update({ notify: true });
    await runEndActions([job("a", "done", ok), job("b", "failed")]);
    expect(notifications).toEqual([{ title: "VidForge：队列已完成", body: "1 个完成，1 个失败" }]);
  });

  it("全部因目标已存在而跳过时也通知，写明跳过的数量", async () => {
    await useSettings.getState().update({ notify: true });
    await runEndActions([job("a", "skipped"), job("b", "skipped")]);
    expect(notifications).toEqual([{ title: "VidForge：队列已完成", body: "0 个完成，2 个已跳过" }]);
  });

  it("全部是取消的任务时什么都不做", async () => {
    await useSettings.getState().update({ notify: true, after: "trash" });
    const confirm = vi.spyOn(backend, "confirm");
    await runEndActions([job("a", "cancelled")]);
    expect(notifications).toEqual([]);
    expect(confirm).not.toHaveBeenCalled();
  });

  it("移到回收站：每次确认，只处理校验全部通过的任务；不同意就不动", async () => {
    await useSettings.getState().update({ after: "trash" });
    const confirm = vi.spyOn(backend, "confirm").mockResolvedValue(false);
    const jobs = [job("good", "done", ok), job("bad", "done", bad), job("none", "done"), job("f", "failed")];
    await runEndActions(jobs);
    expect(confirm).toHaveBeenCalledOnce();
    expect(trashed).toEqual([]);

    confirm.mockResolvedValue(true);
    await runEndActions(jobs);
    expect(confirm.mock.calls[1]![0]).toContain("校验全部通过");
    expect(trashed).toEqual(["good"]);
  });

  it("打开输出目录：同一个目录只打开一次", async () => {
    await useSettings.getState().update({ after: "open" });
    const reveal = vi.spyOn(backend, "revealPath").mockResolvedValue();
    await runEndActions([job("a", "done", ok), job("b", "done", ok)]);
    expect(reveal).toHaveBeenCalledOnce();
  });

  it("只在进行中的任务全部结束时触发一次；全部暂停不算结束", async () => {
    await useSettings.getState().update({ notify: true });
    useQueue.setState({ jobs: [job("old", "done", ok)], paused: false });
    const off = watchRunEnd();
    useQueue.setState({ jobs: [job("old", "done", ok), job("a", "running")] });
    useQueue.setState({ jobs: [job("old", "done", ok), job("a", "paused")], paused: true });
    useQueue.setState({ jobs: [job("old", "done", ok), job("a", "running")], paused: false });
    expect(notifications).toEqual([]);
    useQueue.setState({ jobs: [job("old", "done", ok), job("a", "done", ok)] });
    await vi.waitFor(() => expect(notifications).toHaveLength(1));
    // 启动时读回的历史任务不算进这一批
    expect(notifications[0]!.body).toBe("1 个任务全部完成");
    useQueue.setState({ jobs: [job("old", "done", ok), job("a", "done", ok)] });
    expect(notifications).toHaveLength(1);
    off();
  });
});
