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
