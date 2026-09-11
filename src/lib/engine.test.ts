import { describe, expect, it } from "vitest";
import { MOCK_CAPABILITIES as caps } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { DEFAULT_SETTINGS } from "./defaults";
import { encoderMeta, engineMeta, evaluate, recommendPlan, suggestScenario, today, updatePlan, videoHints } from "./engine";
import { quoteArg } from "./format";
import { scenarios } from "./scenarios";
import type { MediaInfo } from "./types";

const media = (id: string): MediaInfo => MOCK_MEDIA.find((m) => m.id === id)!;

describe("WebAssembly 决策引擎", () => {
  it("推荐与评估走 Rust 实现", () => {
    const m = media("m-iphone");
    expect(suggestScenario(m)).toBe("archive");
    const r = evaluate(m, recommendPlan(m, "archive", caps), caps);
    expect(r.plan.video.encoder).toBe("libx265");
    expect(r.args).toContain("-dolbyvision");
    expect(r.segments.flatMap((s) => s.args)).toEqual(r.args);
    expect(r.estimate.videoBps).toBeGreaterThan(0);
  });

  it("输出路径按设置里的目录与命名模板计算，{date} 由调用方传入", () => {
    const m = media("m-drone");
    const plan = recommendPlan(m, "archive", caps);
    const settings = { ...DEFAULT_SETTINGS, outputDir: "E:/out", namingTemplate: "{date}_{name}_{scenario}" };
    // 输出目录写的是正斜杠，拼接时沿用
    expect(evaluate(m, plan, caps, settings, "2026-09-11").args.at(-1)).toBe("E:/out/2026-09-11_DJI_20260812_0142_归档.mkv");
  });

  it("wasm 里也能正确处理 Windows 路径（std::path 在 wasm 上只认 /）", () => {
    const m = media("m-iphone");
    const plan = recommendPlan(m, "archive", caps);
    expect(m.path).toBe("D:\\素材\\2026-08 京都\\IMG_4521.MOV");
    // 默认放在源文件旁的 VidForge 文件夹
    expect(evaluate(m, plan, caps).args.at(-1)).toBe("D:\\素材\\2026-08 京都\\VidForge\\IMG_4521_2160p_hevc.mkv");
    const tree = { ...DEFAULT_SETTINGS, outputDir: "E:\\out", keepTree: true };
    expect(evaluate({ ...m, importRoot: "D:\\素材" }, plan, caps, tree).args.at(-1)).toBe(
      "E:\\out\\2026-08 京都\\IMG_4521_2160p_hevc.mkv",
    );
  });

  it("输入格式不对时抛出可读的错误，而不是返回半截结果", () => {
    expect(() => evaluate({ id: "x" } as unknown as MediaInfo, recommendPlan(media("m-drone"), "archive", caps), caps)).toThrow(
      /媒体信息格式不对/,
    );
  });

  it("规则表覆盖全部编码器，界面据此换算质量数值与可选的码率控制", () => {
    const meta = engineMeta();
    expect(meta.encoders.length).toBeGreaterThanOrEqual(14);
    expect(encoderMeta("libx265").quality.high).toBe(20);
    expect(encoderMeta("hevc_nvenc").presets).toContain("p7");
    expect(encoderMeta("hevc_videotoolbox").rateControls).toEqual(["quality", "bitrate"]);
    expect(meta.standardFps.map((s) => s.label)).toContain("29.97");
  });

  it("帧率建议：录屏名义 60、平均很低，推荐 60 并标记为极端可变帧率", () => {
    expect(videoHints(media("m-screen"))).toEqual({ recommendedFps: 60, extremeVfr: true });
    expect(videoHints(media("m-drone"))?.extremeVfr).toBe(false);
  });

  it("修改后的计划经引擎整理：两遍编码在流媒体场景也会换成软件编码", () => {
    const m = media("m-drone");
    const plan = recommendPlan(m, "streaming", caps);
    expect(plan.video.encoder).toBe("hevc_qsv");
    const next = updatePlan({ ...plan, video: { ...plan.video, rateControl: { kind: "two_pass", kbps: 8000 } } }, m, caps);
    expect(next.video.encoder).toBe("libx265");
    expect(evaluate(m, next, caps).firstPass).toContain("-pass");
  });

  it("本地日期格式", () => {
    expect(today(new Date(2026, 8, 1))).toBe("2026-09-01");
  });

  // 复制到 PowerShell 时，含逗号的参数（滤镜链、pan 矩阵）必须整体加引号，否则会被当成数组拆开
  it.each(MOCK_MEDIA.flatMap((m) => scenarios().map((s) => [m.id, s.id] as const)))("%s / %s：含逗号的参数都被引号包裹", (id, s) => {
    const m = media(id);
    for (const a of evaluate(m, recommendPlan(m, s, caps), caps).args) {
      if (a.includes(",")) expect(quoteArg(a).startsWith("'")).toBe(true);
    }
  });
});
