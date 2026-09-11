import type { Capabilities, EncoderId, Platform, Settings } from "@/lib/types";

/** 与 Rust 端 `Settings::default()` 一致，后端尚未返回时先用它渲染 */
export const DEFAULT_SETTINGS: Settings = {
  namingTemplate: "{name}_{height}p_{codec}",
  keepTree: true,
  conflict: "rename",
  after: "none",
  hwEncode: true,
  hwDecode: true,
  cpuSlots: 1,
  gpuSlots: 1,
  notify: true,
  language: "zh-CN",
  theme: "system",
  onboarded: false,
};

function currentPlatform(): Platform {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return /Mac OS X|Macintosh/.test(ua) ? "macos" : "windows";
}

/**
 * 与 Rust 端 `Capabilities::placeholder(Probing)` 一致：探测完成前，软件编码器按"可能可用"处理，
 * 让界面先给出软编方案（需求文档：能力探测不阻塞界面）。
 */
export function pendingCapabilities(): Capabilities {
  const soft: EncoderId[] = ["libx264", "libx265", "libsvtav1"];
  return {
    status: "probing",
    statusDetail: "",
    ffmpegPath: "",
    ffprobePath: "",
    searched: [],
    notes: [],
    version: "",
    versionNumber: "",
    buildSource: "",
    buildFlags: [],
    encoders: soft.map((id) => ({
      id,
      vendor: "software",
      codec: id === "libx264" ? "h264" : id === "libx265" ? "hevc" : "av1",
      usable: true,
      tenBit: true,
    })),
    hwaccels: [],
    devices: [],
    tonemap: (["libplacebo", "tonemap_opencl", "zscale", "scale_vt"] as const).map((id) => ({ id, available: false, note: "" })),
    dolbyVisionEncode: false,
    doviSplit: false,
    external: [],
    gpus: [],
    platform: currentPlatform(),
    probedAt: "",
    fingerprint: "",
  };
}
