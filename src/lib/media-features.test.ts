import { describe, expect, it } from "vitest";
import { MOCK_MEDIA } from "@/mock/media";
import { mediaFeatures } from "./media-features";

const keys = (id: string) => mediaFeatures(MOCK_MEDIA.find((m) => m.id === id)!).map((f) => f.key);
const labels = (id: string) => mediaFeatures(MOCK_MEDIA.find((m) => m.id === id)!).map((f) => f.label);

describe("mediaFeatures", () => {
  it("iPhone：杜比视界 8.4、HLG、可变帧率", () => {
    expect(labels("m-iphone")).toEqual(["杜比视界 8.4", "HLG", "可变帧率"]);
  });

  it("蓝光：P7 FEL、HDR10、全景声、TrueHD、PGS、多音轨", () => {
    expect(labels("m-bluray")).toEqual(["杜比视界 7 FEL", "HDR10", "全景声", "TrueHD", "PGS ×3", "4 音轨"]);
  });

  it("相机：HLG 与 PCM 无损", () => {
    expect(keys("m-camera")).toEqual(["hlg", "lossless"]);
    expect(labels("m-camera")).toContain("PCM");
  });

  it("普通 SDR 8bit 素材没有特征徽章", () => {
    expect(keys("m-drone")).toEqual([]);
  });

  it("录屏：可变帧率", () => {
    expect(keys("m-screen")).toEqual(["vfr"]);
  });

  it("可变帧率的提示说明剪辑用途", () => {
    const f = mediaFeatures(MOCK_MEDIA.find((m) => m.id === "m-screen")!).find((x) => x.key === "vfr");
    expect(f?.title).toContain("剪辑");
  });
});
