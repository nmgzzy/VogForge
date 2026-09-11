import { beforeEach, describe, expect, it } from "vitest";
import { useProject } from "./project";

const state = () => useProject.getState();
const current = () => {
  const s = state();
  return s.selectedId ? s.plans[s.selectedId] : undefined;
};

beforeEach(() => {
  useProject.setState({ files: [], plans: {}, selectedId: undefined });
  state().loadSamples();
});

describe("project store", () => {
  it("载入示例后选中第一个文件，并按源特征给出起始场景", () => {
    const s = state();
    expect(s.files).toHaveLength(6);
    expect(s.selectedId).toBe("m-iphone");
    expect(s.plans["m-iphone"]?.scenario).toBe("archive");
    expect(s.plans["m-bluray"]?.scenario).toBe("collection");
    expect(s.plans["m-screen"]?.scenario).toBe("editing");
    expect(s.plans["m-stream"]?.scenario).toBe("remux");
  });

  it("重复添加同一文件不会产生重复项", () => {
    state().loadSamples();
    expect(state().files).toHaveLength(6);
  });

  it("切换场景会重新推荐整份计划", () => {
    state().setScenario("streaming");
    expect(current()?.scenario).toBe("streaming");
    expect(current()?.container).toBe("mp4");
    expect(current()?.video.encoder).toBe("hevc_qsv");
  });

  it("修改编码格式后自动重选编码器（计划保持自洽）", () => {
    state().patchPlan((p) => {
      p.video.codec = "av1";
    });
    expect(current()?.video.encoder).toBe("libsvtav1");
  });

  it("修改容器后音轨随之重建", () => {
    state().select("m-camera");
    state().setScenario("archive");
    expect(current()?.audio[0]?.action).toBe("copy");
    state().patchPlan((p) => {
      p.container = "mp4";
      p.audioMode = "compat_only";
    });
    // PCM 不能进 MP4，重建后转为 AAC
    expect(current()?.audio[0]?.codec).toBe("aac");
  });

  it("一键修正作用于当前文件", () => {
    state().select("m-bluray");
    state().applyFix("remux");
    expect(current()?.video.action).toBe("copy");
    expect(state().plans["m-iphone"]?.video.action).toBe("encode");
  });

  it("移除当前选中的文件后自动选中下一个", () => {
    state().removeFile("m-iphone");
    expect(state().files).toHaveLength(5);
    expect(state().selectedId).toBe("m-bluray");
    expect(state().plans["m-iphone"]).toBeUndefined();
  });

  it("把当前场景应用到全部文件", () => {
    state().setScenario("mobile");
    state().applyScenarioToAll();
    expect(Object.values(state().plans).every((p) => p.scenario === "mobile")).toBe(true);
  });

  it("清空列表", () => {
    state().clear();
    expect(state().files).toHaveLength(0);
    expect(state().selectedId).toBeUndefined();
  });
});

describe("导入文件（经 Backend 接口）", () => {
  beforeEach(() => {
    useProject.setState({ files: [], plans: {}, selectedId: undefined, importReport: undefined, importing: false });
  });

  it("分析成功的文件加入列表并选中第一个新文件；全部成功时不打扰用户", async () => {
    await state().importPaths(["IMG_4521.MOV", "C0087.MP4"]);
    const s = state();
    expect(s.files.map((f) => f.id)).toEqual(["m-iphone", "m-camera"]);
    expect(s.selectedId).toBe("m-iphone");
    expect(s.importReport).toBeUndefined();
    expect(s.importing).toBe(false);
  });

  it("失败与重复都写进导入结果", async () => {
    await state().importPaths(["IMG_4521.MOV"]);
    await state().importPaths(["IMG_4521.MOV", "D:/clips/broken.mp4"]);
    const r = state().importReport!;
    expect(r.added).toBe(0);
    expect(r.duplicate).toBe(1);
    expect(r.failures).toEqual([{ path: "D:/clips/broken.mp4", reason: "文件不完整或已损坏（moov atom not found）" }]);
    state().dismissImportReport();
    expect(state().importReport).toBeUndefined();
  });

  it("空列表不发起导入", async () => {
    await state().importPaths([]);
    expect(state().files).toHaveLength(0);
    expect(state().importReport).toBeUndefined();
  });
});

describe("导入进行中又拖入的文件", () => {
  beforeEach(() => {
    useProject.setState({ files: [], plans: {}, selectedId: undefined, importReport: undefined, importing: false, importQueued: 0 });
  });

  it("排队到当前这批之后处理，结果合并成一份报告，不会被静默丢弃", async () => {
    const first = state().importPaths(["IMG_4521.MOV"]);
    const second = state().importPaths(["C0087.MP4", "D:/clips/broken.mp4"]);
    expect(state().importQueued).toBe(2);
    await Promise.all([first, second]);
    const s = state();
    expect(s.files.map((f) => f.id)).toEqual(["m-iphone", "m-camera"]);
    expect(s.importing).toBe(false);
    expect(s.importQueued).toBe(0);
    expect(s.importReport?.added).toBe(2);
    expect(s.importReport?.failures).toHaveLength(1);
    expect(s.selectedId).toBe("m-iphone");
  });
});
