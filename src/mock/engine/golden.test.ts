/**
 * 引擎黄金样本：把 TS 引擎在一批素材 × 场景 × 变体上的产出写成 JSON，Rust 端
 * （crates/vidforge-core/tests/golden_engine.rs）用同一份文件逐条对照，保证两边实现一致。
 *
 * 规则变更后：确认新输出正确，用 `UPDATE_GOLDEN=1 pnpm vitest run src/mock/engine/golden.test.ts` 重写文件，
 * 再改 Rust 让它的测试通过。
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { Capabilities, EncoderId, MediaInfo, TranscodePlan } from "@/lib/types";
import { MOCK_CAPABILITIES } from "../capabilities";
import { MOCK_MEDIA } from "../media";
import { buildArgSegments, outputPath, type ArgSegment } from "./args";
import { applyFix, evaluate, recommendPlan, SCENARIOS } from "./index";

const GOLDEN = resolve(dirname(fileURLToPath(import.meta.url)), "../../../crates/vidforge-core/tests/fixtures/golden/engine.json");

const MAC: Capabilities = {
  ...MOCK_CAPABILITIES,
  platform: "macos",
  encoders: [
    ...MOCK_CAPABILITIES.encoders.filter((e) => e.vendor === "software"),
    { id: "hevc_videotoolbox", vendor: "apple", codec: "hevc", usable: true, tenBit: true },
    { id: "h264_videotoolbox", vendor: "apple", codec: "h264", usable: true, tenBit: false },
  ],
  tonemap: MOCK_CAPABILITIES.tonemap.map((t) => ({ ...t, available: t.id === "scale_vt" })),
};
const CAPS = { dev: MOCK_CAPABILITIES, mac: MAC } as const;
type CapsKey = keyof typeof CAPS;

interface GoldenCase {
  name: string;
  caps: CapsKey;
  media: MediaInfo;
  plan: TranscodePlan;
  output: string;
  segments: ArgSegment[];
}

const media = (id: string) => MOCK_MEDIA.find((m) => m.id === id)!;

function make(name: string, m: MediaInfo, plan: TranscodePlan, capsKey: CapsKey = "dev"): GoldenCase {
  return { name, caps: capsKey, media: m, plan, output: outputPath(m, plan), segments: buildArgSegments(m, plan, CAPS[capsKey]) };
}

function tweak(base: TranscodePlan, fn: (p: TranscodePlan) => void): TranscodePlan {
  const p = structuredClone(base);
  fn(p);
  return p;
}

const ALL_ENCODERS: EncoderId[] = [
  "libx264", "libx265", "libsvtav1", "h264_qsv", "hevc_qsv", "av1_qsv", "h264_nvenc", "hevc_nvenc", "av1_nvenc",
  "h264_amf", "hevc_amf", "av1_amf", "h264_videotoolbox", "hevc_videotoolbox",
];

function cases(): GoldenCase[] {
  const out: GoldenCase[] = [];
  for (const capsKey of ["dev", "mac"] as const) {
    for (const m of MOCK_MEDIA) {
      for (const s of SCENARIOS) {
        const plan = recommendPlan(m, s.id, CAPS[capsKey]);
        out.push(make(`${capsKey}/${m.id}/${s.id}`, m, plan, capsKey));
        // 每个保真度冲突的每个一键修正
        if (capsKey === "dev") {
          for (const item of evaluate(m, plan, CAPS.dev).fidelity) {
            for (const fix of item.fixes) {
              out.push(make(`dev/${m.id}/${s.id}/fix:${fix.id}`, m, applyFix(plan, fix.id, m, CAPS.dev)));
            }
          }
        }
      }
    }
  }

  // 每个编码器 × 8/10bit 的参数写法
  const drone = media("m-drone");
  const archive = recommendPlan(drone, "archive", CAPS.dev);
  for (const enc of ALL_ENCODERS) {
    for (const bitDepth of [8, 10] as const) {
      out.push(
        make(`encoder/${enc}/${bitDepth}bit`, drone, tweak(archive, (p) => {
          p.video.encoder = enc;
          p.video.encoderAuto = false;
          p.video.bitDepth = bitDepth;
          p.video.gop = 60;
        })),
      );
    }
  }

  // 四条色调映射管线，原尺寸与缩放
  const iphone = media("m-iphone");
  const mobile = recommendPlan(iphone, "mobile", CAPS.dev);
  for (const tonemap of ["libplacebo", "tonemap_opencl", "zscale", "scale_vt"] as const) {
    for (const resolution of ["source", "720"] as const) {
      out.push(make(`tonemap/${tonemap}/${resolution}`, iphone, tweak(mobile, (p) => {
        p.video.hdrAction = "tonemap";
        p.video.tonemap = tonemap;
        p.video.resolution = resolution;
      })));
    }
  }
  out.push(make("tonemap/scale_vt/hw-encoder", iphone, tweak(mobile, (p) => {
    p.video.tonemap = "scale_vt";
    p.video.encoder = "h264_videotoolbox";
    p.video.encoderAuto = false;
  })));

  // 帧率：上限截断、NTSC 分数
  out.push(make("fps/cap-30", drone, tweak(archive, (p) => void (p.video.fps = { kind: "cap", max: 30 }))));
  out.push(make("fps/cfr-ntsc", drone, tweak(archive, (p) => void (p.video.fps = { kind: "cfr", fps: 30000 / 1001 }))));
  // 视频比音频短 2 帧：转 CFR 时补齐尾部
  const camera = media("m-camera");
  const padded: MediaInfo = {
    ...camera,
    video: camera.video.map((v) => ({ ...v, durationSec: camera.durationSec - 0.067 })),
    audio: camera.audio.map((a) => ({ ...a, durationSec: camera.durationSec })),
  };
  out.push(make("fps/cfr-tail-pad", padded, tweak(recommendPlan(padded, "editing", CAPS.dev), (p) => void (p.video.fps = { kind: "cfr", fps: 30 }))));
  out.push(make("fps/keep-no-pad", padded, recommendPlan(padded, "archive", CAPS.dev)));

  // 字幕与容器组合
  const bluray = media("m-bluray");
  const collection = recommendPlan(bluray, "collection", CAPS.dev);
  for (const subtitles of ["all", "text_only", "none"] as const) {
    for (const container of ["mkv", "mp4", "mov"] as const) {
      out.push(make(`subs/${subtitles}/${container}`, { ...bluray, attachments: 2 }, tweak(collection, (p) => {
        p.subtitles = subtitles;
        p.container = container;
      })));
    }
  }

  // 附加参数与 x265 参数
  out.push(make("extra/args-and-params", drone, tweak(archive, (p) => {
    p.video.encoder = "libx265";
    p.video.extraParams = "aq-mode=3";
    p.video.extraArgs = `-metadata title="My Video" -tune grain`;
  })));

  // 竖屏缩放
  const screen = media("m-screen");
  out.push(make("scale/portrait-720", screen, tweak(recommendPlan(screen, "social", CAPS.dev), (p) => void (p.video.resolution = "720"))));
  return out;
}

function serialize(list: GoldenCase[]): string {
  const caps = JSON.stringify(CAPS);
  return `{\n"caps": ${caps},\n"cases": [\n${list.map((c) => JSON.stringify(c)).join(",\n")}\n]}\n`;
}

describe("引擎黄金样本", () => {
  it("TS 引擎的产出与 golden/engine.json 一致（Rust 端对照同一份文件）", () => {
    const text = serialize(cases());
    if (process.env.UPDATE_GOLDEN || !existsSync(GOLDEN)) writeFileSync(GOLDEN, text);
    const golden = JSON.parse(readFileSync(GOLDEN, "utf-8"));
    const current = JSON.parse(text);
    expect(current.cases.length).toBe(golden.cases.length);
    for (let i = 0; i < current.cases.length; i++) {
      expect(current.cases[i], current.cases[i].name).toEqual(golden.cases[i]);
    }
  });
});
