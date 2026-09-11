import type { Capabilities, ToneMapPipeline } from "@/lib/types";

/** 色调映射管线的质量排序，见 docs/design.md 5.3 */
export const TONEMAP_ORDER: readonly ToneMapPipeline[] = ["libplacebo", "tonemap_opencl", "zscale", "scale_vt"];

export function tonemapAvailable(id: ToneMapPipeline, caps: Capabilities): boolean {
  return caps.tonemap.some((t) => t.id === id && t.available);
}

/** 按质量顺序选第一个可用的管线；全部不可用时返回 undefined，由调用方阻止转为 SDR */
export function pickTonemap(caps: Capabilities): ToneMapPipeline | undefined {
  return TONEMAP_ORDER.find((id) => tonemapAvailable(id, caps));
}
