import { describe, expect, it } from "vitest";
import type { MediaInfo, Scenario } from "@/lib/types";
import { MOCK_CAPABILITIES as caps } from "../capabilities";
import { MOCK_MEDIA } from "../media";
import { applyFix, evaluate, recommendPlan, SCENARIOS, updatePlan } from "./index";
import { downmixFilter, targetDimensions } from "./args";
import { recommendCfrTarget, snapFps, fpsArg } from "./fps";
import { pickEncoder } from "./encoders";
import { quoteArg } from "@/lib/format";

const byId = (id: string): MediaInfo => {
  const m = MOCK_MEDIA.find((x) => x.id === id);
  if (!m) throw new Error(`no media ${id}`);
  return m;
};
const iphone = byId("m-iphone");
const bluray = byId("m-bluray");
const drone = byId("m-drone");
const screen = byId("m-screen");
const camera = byId("m-camera");
const streamingSrc = byId("m-stream");

const run = (m: MediaInfo, s: Scenario) => evaluate(m, recommendPlan(m, s, caps), caps);
const has = (args: string[], ...seq: string[]) =>
  args.some((_, i) => seq.every((x, j) => args[i + j] === x));

const ALL: [MediaInfo, Scenario][] = MOCK_MEDIA.flatMap((m) => SCENARIOS.map((s) => [m, s.id] as [MediaInfo, Scenario]));

// ───────────────── 技术事实断言：遍历全部样本 × 全部场景 ─────────────────

describe("技术事实（docs/ffmpeg-facts.md）在所有组合下成立", () => {
  it.each(ALL)("%s / %s：不生成 -vsync（9.0 已移除）", (m, s) => {
    expect(run(m, s).args).not.toContain("-vsync");
  });

  it.each(ALL)("%s / %s：CFR 不使用 fps 滤镜（会丢最后一帧）", (m, s) => {
    const vf = run(m, s).args.find((a) => a.includes("fps="));
    expect(vf).toBeUndefined();
  });

  it.each(ALL)("%s / %s：MP4/MOV 输出 HEVC 必带 -tag:v hvc1", (m, s) => {
    const r = run(m, s);
    const hevcOut = r.plan.video.action === "copy" ? m.video[0]?.codec === "hevc" : r.plan.video.codec === "hevc";
    if (r.plan.container !== "mkv" && hevcOut) expect(has(r.args, "-tag:v", "hvc1")).toBe(true);
  });

  it.each(ALL)("%s / %s：源含杜比视界且用 libx265/libsvtav1 时显式传 -dolbyvision", (m, s) => {
    const r = run(m, s);
    const sw = r.plan.video.encoder === "libx265" || r.plan.video.encoder === "libsvtav1";
    if (m.video[0]?.dolbyVision && r.plan.video.action === "encode" && sw) {
      expect(r.args).toContain("-dolbyvision");
    }
  });

  it.each(ALL)("%s / %s：默认不传 -low_power", (m, s) => {
    expect(run(m, s).args).not.toContain("-low_power");
  });

  it.each(ALL)("%s / %s：临时文件写入需要显式 -f", (m, s) => {
    expect(run(m, s).args).toContain("-f");
  });

  it.each(ALL)("%s / %s：只用 -fps_mode:v cfr 搭配 -r", (m, s) => {
    const r = run(m, s);
    if (r.plan.video.fps.kind === "cfr" && r.plan.video.action === "encode") {
      expect(has(r.args, "-fps_mode:v", "cfr", "-r")).toBe(true);
    }
  });

  it.each(ALL)("%s / %s：用了 pan 降混的音轨不再出现 -ac", (m, s) => {
    const args = run(m, s).args;
    args.forEach((a, i) => {
      if (a.startsWith("-filter:a:") && args[i + 1]?.includes("pan=stereo")) {
        const idx = a.split(":")[2];
        expect(args).not.toContain(`-ac:a:${idx}`);
      }
    });
  });

  it.each(ALL)("%s / %s：复制到 PowerShell 时含逗号的参数都被引号包裹", (m, s) => {
    for (const a of run(m, s).args) if (a.includes(",")) expect(quoteArg(a).startsWith("'")).toBe(true);
  });

  it.each(ALL)("%s / %s：保留杜比视界时不加硬件解码（hwdownload 会丢 RPU）", (m, s) => {
    const r = run(m, s);
    if (r.plan.video.dovi === "preserve") expect(r.args).not.toContain("-hwaccel");
  });
});

// ───────────────── 场景推荐 ─────────────────

describe("iPhone 杜比视界 8.4 素材", () => {
  it("归档：CPU 编码、10bit、保留杜比视界与可变帧率", () => {
    const r = run(iphone, "archive");
    expect(r.plan.video.encoder).toBe("libx265");
    expect(r.plan.video.bitDepth).toBe(10);
    expect(r.plan.video.dovi).toBe("preserve");
    expect(r.plan.video.fps.kind).toBe("keep");
    expect(r.plan.container).toBe("mkv");
    expect(has(r.args, "-dolbyvision", "1")).toBe(true);
    expect(has(r.args, "-pix_fmt", "yuv420p10le")).toBe(true);
    expect(has(r.args, "-color_trc", "arib-std-b67")).toBe(true);
  });

  it("归档：杜比视界与 HDR 均判定为可保留", () => {
    const r = run(iphone, "archive");
    const f = Object.fromEntries(r.fidelity.map((x) => [x.kind, x.state]));
    expect(f.dolbyVision).toBe("achievable");
    expect(f.hdr10).toBe("achievable");
    expect(f.tenBit).toBe("achievable");
  });

  it("流媒体：QSV 硬编、转 CFR、不保留杜比视界", () => {
    const r = run(iphone, "streaming");
    expect(r.plan.video.encoder).toBe("hevc_qsv");
    expect(r.plan.video.dovi).toBe("disable");
    expect(r.plan.video.fps).toEqual({ kind: "cfr", fps: 30 });
    expect(has(r.args, "-fps_mode:v", "cfr", "-r", "30")).toBe(true);
    expect(has(r.args, "-pix_fmt", "p010le")).toBe(true);
    expect(has(r.args, "-profile:v", "main10")).toBe(true);
    expect(r.args).not.toContain("-strict");
  });

  it("流媒体下勾选杜比视界：给出一键修正，应用后改用 CPU 编码", () => {
    const r = run(iphone, "streaming");
    const dv = r.fidelity.find((x) => x.kind === "dolbyVision");
    expect(dv?.state).toBe("needs_change");
    expect(dv?.fixes[0]?.id).toBe("dovi_preserve");
    const fixed = evaluate(iphone, applyFix(r.plan, "dovi_preserve", iphone, caps), caps);
    expect(fixed.plan.video.encoder).toBe("libx265");
    expect(fixed.fidelity.find((x) => x.kind === "dolbyVision")?.state).toBe("achievable");
    // MP4 输出杜比视界必须 -strict unofficial，否则配置 box 不写入
    expect(has(fixed.args, "-strict", "unofficial")).toBe(true);
  });

  it("手机观看：色调映射为 SDR，并利用杜比视界元数据", () => {
    const r = run(iphone, "mobile");
    expect(r.plan.video.hdrAction).toBe("tonemap");
    const vf = r.args[r.args.indexOf("-vf") + 1] ?? "";
    expect(vf).toContain("libplacebo");
    expect(vf).toContain("apply_dolbyvision=1");
    expect(has(r.args, "-color_trc", "bt709")).toBe(true);
  });

  it("手机观看下勾选 HDR：修正后改为 HEVC 10bit 并保留 HDR", () => {
    const r = run(iphone, "mobile");
    const next = evaluate(iphone, applyFix(r.plan, "keep_hdr", iphone, caps), caps);
    expect(next.plan.video.codec).toBe("hevc");
    expect(next.plan.video.hdrAction).toBe("keep");
    expect(next.plan.video.bitDepth).toBe(10);
    expect(next.fidelity.find((x) => x.kind === "hdr10")?.state).toBe("achievable");
  });
});

describe("蓝光 remux：杜比视界 P7 + TrueHD Atmos + PGS", () => {
  it("收藏：P7 双层判定为不可保留，并提供原样封装", () => {
    const r = run(bluray, "collection");
    const dv = r.fidelity.find((x) => x.kind === "dolbyVision");
    expect(dv?.state).toBe("impossible");
    expect(dv?.fixes.map((f) => f.id)).toContain("remux");
    expect(r.plan.video.dovi).toBe("disable");
    expect(has(r.args, "-dolbyvision", "0")).toBe(true);
  });

  it("改为原样封装后杜比视界可保留，且不重新编码", () => {
    const plan = applyFix(recommendPlan(bluray, "collection", caps), "remux", bluray, caps);
    const r = evaluate(bluray, plan, caps);
    expect(r.plan.video.action).toBe("copy");
    expect(has(r.args, "-c:v", "copy")).toBe(true);
    expect(r.fidelity.find((x) => x.kind === "dolbyVision")?.state).toBe("achievable");
  });

  it("收藏：全部音轨复制、PGS 字幕保留、章节保留", () => {
    const r = run(bluray, "collection");
    expect(r.plan.audio.every((t) => t.action === "copy")).toBe(true);
    expect(r.plan.audio).toHaveLength(4);
    expect(has(r.args, "-map", "0:s?")).toBe(true);
    expect(has(r.args, "-map_chapters", "0")).toBe(true);
    const f = Object.fromEntries(r.fidelity.map((x) => [x.kind, x.state]));
    expect(f.lossless).toBe("achievable");
    expect(f.allAudio).toBe("achievable");
    expect(f.allSubtitles).toBe("achievable");
  });

  it("流媒体：Atmos 转为 DD+ 5.1 并追加立体声，全景声判定需修正", () => {
    const r = run(bluray, "streaming");
    const truehd = r.plan.audio.filter((t) => t.sourceIndex === 1);
    expect(truehd.map((t) => t.codec)).toEqual(["eac3", "aac"]);
    expect(r.fidelity.find((x) => x.kind === "lossless")?.state).toBe("needs_change");
  });

  it("流媒体：MP4 只映射文本字幕并转为 mov_text", () => {
    const r = run(bluray, "streaming");
    expect(has(r.args, "-map", "0:8")).toBe(true);
    expect(has(r.args, "-map", "0:5")).toBe(false);
    expect(has(r.args, "-c:s", "mov_text")).toBe(true);
  });

  it("流媒体下修正全景声：切换到 MKV 并原样复制 TrueHD", () => {
    const r = run(bluray, "streaming");
    const next = evaluate(bluray, applyFix(r.plan, "keep_lossless", bluray, caps), caps);
    expect(next.plan.container).toBe("mkv");
    expect(next.plan.audio.some((t) => t.sourceIndex === 1 && t.action === "copy")).toBe(true);
    expect(next.fidelity.find((x) => x.kind === "lossless")?.state).toBe("achievable");
  });

  it("7.1 降混立体声使用 7.1 声道名", () => {
    const f = downmixFilter(bluray.audio[0]!);
    expect(f).toContain("SL");
    expect(f).toContain("BL");
    expect(f).toContain("alimiter");
  });
});

describe("录屏：极端可变帧率", () => {
  it("剪辑预处理：转为名义帧率 60 并给出体积警告", () => {
    const r = run(screen, "editing");
    expect(r.plan.video.fps).toEqual({ kind: "cfr", fps: 60 });
    expect(r.fpsInsight?.duplicated).toBeGreaterThan(10_000);
    const fps = r.decisions.find((d) => d.field === "帧率");
    expect(fps?.severity).toBe("warn");
    expect(has(r.args, "-g", "30")).toBe(true);
  });

  it("剪辑预处理：复制的音轨不加滤镜，重新编码的音轨做重采样同步补偿", () => {
    const r = run(screen, "editing");
    // 默认复制音频：实测 -c:a copy 配合 -fps_mode:v cfr 音视频时长完全一致，无需处理
    expect(r.args.some((a) => a.includes("aresample"))).toBe(false);
    // 改为重新编码时，编码轨追加 aresample=async=1
    const plan = updatePlan({ ...r.plan, audioMode: "compat_only", scenario: "smallest" }, screen, caps);
    const r2 = evaluate(screen, { ...plan, scenario: "editing", video: r.plan.video }, caps);
    expect(r2.plan.audio[0]?.action).toBe("encode");
    expect(r2.args.some((a) => a.includes("aresample=async=1"))).toBe(true);
  });

  it("竖屏缩放按短边计算，且不放大", () => {
    const v = screen.video[0]!;
    expect(targetDimensions(v, "720")).toEqual({ w: 720, h: 1600, portrait: true });
    expect(targetDimensions(v, "1080")).toBeNull();
    expect(targetDimensions(v, "2160")).toBeNull();
  });

  it("归档：保持可变帧率，并说明剪辑时如何转换", () => {
    const r = run(screen, "archive");
    expect(r.plan.video.fps.kind).toBe("keep");
    const d = r.decisions.find((x) => x.field === "帧率");
    // 帧率控件已有醒目提示，推荐说明里降为普通条目，避免重复
    expect(d?.severity).toBe("info");
    expect(d?.reason).toContain("转为固定帧率");
  });
});

describe("常识保护", () => {
  it("已高度压缩的片源：提示不建议转码", () => {
    expect(run(streamingSrc, "archive").notWorthIt).toBeDefined();
  });

  it("高码率无人机素材：值得转码，体积降到 40% 以下", () => {
    const r = run(drone, "archive");
    expect(r.notWorthIt).toBeUndefined();
    expect(r.estimate.ratio).toBeLessThan(0.4);
  });

  it("剪辑预处理不提示不建议转码（体积预期上升）", () => {
    expect(run(streamingSrc, "editing").notWorthIt).toBeUndefined();
  });

  it("原样封装不提示不建议转码", () => {
    expect(run(streamingSrc, "remux").notWorthIt).toBeUndefined();
  });

  it("SDR 源的 HDR 与杜比视界项为不适用", () => {
    const r = run(drone, "archive");
    const f = Object.fromEntries(r.fidelity.map((x) => [x.kind, x.state]));
    expect(f.hdr10).toBe("not_applicable");
    expect(f.dolbyVision).toBe("not_applicable");
  });
});

describe("相机 PCM 音频", () => {
  it("流媒体 MP4 不支持 PCM，转为 AAC", () => {
    const r = run(camera, "streaming");
    expect(r.plan.audio[0]?.action).toBe("encode");
    expect(r.plan.audio[0]?.codec).toBe("aac");
  });

  it("归档 MKV 直接复制 PCM", () => {
    const r = run(camera, "archive");
    expect(r.plan.audio[0]?.action).toBe("copy");
  });
});

// ───────────────── 单元 ─────────────────

describe("帧率吸附", () => {
  it("吸附到标准档（容差 2%）", () => {
    expect(snapFps(29.41)).toBe(30);
    expect(snapFps(23.976)).toBeCloseTo(24000 / 1001);
    expect(snapFps(59.94)).toBeCloseTo(60000 / 1001);
    expect(snapFps(25.2)).toBe(25);
  });
  it("吸附不上时四舍五入", () => {
    expect(snapFps(17.3)).toBe(17);
  });
  it("29.97 与 30 都在容差内时：只有精确匹配才选 NTSC，否则优先整数档", () => {
    // 可变帧率的平均值因掉帧偏低，不能据此判定为 NTSC
    expect(snapFps(29.8)).toBe(30);
    expect(snapFps(59.6)).toBe(60);
    expect(snapFps(23.9)).toBe(24);
    // 真正的 NTSC 源误差极小
    expect(snapFps(30000 / 1001)).toBeCloseTo(30000 / 1001);
    expect(snapFps(29.97)).toBeCloseTo(30000 / 1001);
  });
  it("NTSC 帧率以分数形式传给 ffmpeg", () => {
    expect(fpsArg(30000 / 1001)).toBe("30000/1001");
    expect(fpsArg(24000 / 1001)).toBe("24000/1001");
    expect(fpsArg(30)).toBe("30");
  });
  it("名义帧率异常时回退到平均帧率", () => {
    const v = { ...screen.video[0]!, fpsNominal: 1000, fpsAvg: 29.8 };
    expect(recommendCfrTarget(v)).toBe(30);
  });
});

describe("编码器选择", () => {
  it("需要杜比视界时强制软件编码", () => {
    expect(pickEncoder("hevc", { preferHw: true, need10bit: true, needDv: true }, caps).encoder).toBe("libx265");
  });
  it("偏好硬编时跳过不可用的 NVENC，选中 QSV", () => {
    expect(pickEncoder("hevc", { preferHw: true, need10bit: true, needDv: false }, caps).encoder).toBe("hevc_qsv");
  });
  it("硬编不支持 10bit 时回退软编", () => {
    expect(pickEncoder("h264", { preferHw: true, need10bit: true, needDv: false }, caps).encoder).toBe("libx264");
  });
  it("没有任何可用硬编时回退软编", () => {
    const none = { ...caps, encoders: caps.encoders.map((e) => ({ ...e, usable: e.vendor === "software" })) };
    const p = pickEncoder("av1", { preferHw: true, need10bit: false, needDv: false }, none);
    expect(p.encoder).toBe("libsvtav1");
    expect(p.reason).toContain("没有可用");
  });
});

describe("决策理由与实际计划一致", () => {
  const reason = (m: MediaInfo, s: Scenario, field: string) => run(m, s).decisions.find((d) => d.field === field)?.reason ?? "";

  it("全部复制时不声称生成了兼容轨", () => {
    expect(reason(bluray, "collection", "音频")).not.toContain("兼容轨");
  });

  it("原样复制加兼容轨时说明兼容轨", () => {
    expect(reason(bluray, "archive", "音频")).toContain("兼容轨");
  });

  it("剪辑预处理的编码器理由针对剪辑，而不是长期保存", () => {
    const r = reason(screen, "editing", "编码器");
    expect(r).toContain("剪辑");
    expect(r).not.toContain("长期保存");
  });

  it("CPU 慢速编码 4K 长片时给出提速建议", () => {
    const d = run(bluray, "collection").decisions.find((x) => x.field === "耗时");
    expect(d?.severity).toBe("tip");
    expect(d?.reason).toContain("medium");
  });

  it("短片不给耗时提示", () => {
    expect(run(iphone, "archive").decisions.find((x) => x.field === "耗时")).toBeUndefined();
  });
});

describe("macOS：VideoToolbox 不写 HDR10 元数据", () => {
  const mac = {
    ...caps,
    platform: "macos" as const,
    encoders: [
      ...caps.encoders.filter((e) => e.vendor === "software"),
      { id: "hevc_videotoolbox" as const, vendor: "apple" as const, codec: "hevc" as const, usable: true, tenBit: true },
      { id: "h264_videotoolbox" as const, vendor: "apple" as const, codec: "h264" as const, usable: true, tenBit: false },
    ],
  };
  const runMac = (m: MediaInfo, s: Scenario) => evaluate(m, recommendPlan(m, s, mac), mac);

  it("流媒体保留 HDR10 时跳过 VideoToolbox，改用 CPU 并说明原因", () => {
    const r = runMac(bluray, "streaming");
    expect(r.plan.video.encoder).toBe("libx265");
    expect(r.fidelity.find((x) => x.kind === "hdr10")?.state).toBe("achievable");
    expect(pickEncoder("hevc", { preferHw: true, need10bit: true, needDv: false, needHdr10: true }, mac).reason).toContain("HDR10");
  });

  it("HLG 不依赖元数据，仍可使用 VideoToolbox", () => {
    expect(runMac(iphone, "streaming").plan.video.encoder).toBe("hevc_videotoolbox");
  });

  it("保留 HDR 的一键修正在 macOS 上真正消除冲突", () => {
    const r = runMac(bluray, "mobile");
    const fixed = evaluate(bluray, applyFix(r.plan, "keep_hdr", bluray, mac), mac);
    expect(fixed.plan.video.encoder).not.toBe("hevc_videotoolbox");
    expect(fixed.fidelity.find((x) => x.kind === "hdr10")?.state).toBe("achievable");
  });
});

describe("Codex 审查修复：色调映射按能力选择", () => {
  const withTonemap = (available: string[]) => ({
    ...caps,
    tonemap: caps.tonemap.map((t) => ({ ...t, available: available.includes(t.id) })),
  });

  it("缺少 libplacebo 时降级到 OpenCL", () => {
    const c = withTonemap(["tonemap_opencl", "zscale"]);
    const r = evaluate(iphone, recommendPlan(iphone, "mobile", c), c);
    expect(r.plan.video.tonemap).toBe("tonemap_opencl");
    expect(r.args.join(" ")).toContain("tonemap_opencl");
    expect(r.args.join(" ")).not.toContain("libplacebo");
  });

  it("只剩 zscale 时使用 CPU 管线", () => {
    const c = withTonemap(["zscale"]);
    const r = evaluate(iphone, recommendPlan(iphone, "mobile", c), c);
    expect(r.plan.video.tonemap).toBe("zscale");
    expect(r.args.join(" ")).toContain("zscale=t=linear");
  });

  it("全部不可用时不生成色调映射滤镜，保留 HDR 并给出警告", () => {
    const c = withTonemap([]);
    const r = evaluate(iphone, recommendPlan(iphone, "mobile", c), c);
    expect(r.plan.video.hdrAction).toBe("keep");
    expect(r.args.join(" ")).not.toMatch(/libplacebo|tonemap|zscale/);
    expect(r.decisions.find((d) => d.field === "HDR")?.severity).toBe("warn");
  });

  it("已选的管线在当前环境不可用时，规范化会换成可用的", () => {
    const plan = recommendPlan(iphone, "mobile", caps); // 开发机上是 libplacebo
    const c = withTonemap(["zscale"]);
    expect(updatePlan(plan, iphone, c).video.tonemap).toBe("zscale");
  });

  it("scale_vt 生成真正的 VideoToolbox 链，而不是 zscale", () => {
    const c = { ...withTonemap(["scale_vt"]), platform: "macos" as const };
    const r = evaluate(iphone, recommendPlan(iphone, "mobile", c), c);
    const cmd = r.args.join(" ");
    expect(r.plan.video.tonemap).toBe("scale_vt");
    expect(cmd).toContain("scale_vt=");
    expect(cmd).not.toContain("zscale");
    expect(has(r.args, "-hwaccel", "videotoolbox", "-hwaccel_output_format", "videotoolbox_vld")).toBe(true);
    // 软件编码器需要先把硬件帧下载回内存
    expect(cmd).toContain("hwdownload,format=nv12");
  });
});

describe("Codex 审查修复：附加参数支持引号", () => {
  it("title=\"My Video\" 保持为一个 argv", () => {
    const plan = recommendPlan(drone, "archive", caps);
    plan.video.extraArgs = '-metadata title="My Video"';
    const r = evaluate(drone, plan, caps);
    expect(has(r.args, "-metadata", "title=My Video")).toBe(true);
  });
});
