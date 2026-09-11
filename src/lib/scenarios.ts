import type { Scenario } from "./types";

export interface ScenarioMeta {
  id: Scenario;
  title: string;
  /** 卡片上的短说明，控制在 7 个字以内，避免被截断 */
  short: string;
  tagline: string;
  outcome: string;
}

/** 场景卡片的文案；每个场景的具体参数由引擎决定（crates/vidforge-core/src/pipeline/strategy.rs） */
export const SCENARIOS: ScenarioMeta[] = [
  { id: "archive", title: "素材归档", short: "手机相机拍摄", tagline: "手机、相机素材压缩后长期保存", outcome: "体积大幅下降，保留 HDR 与杜比视界" },
  { id: "collection", title: "高画质收藏", short: "收藏级保存", tagline: "影片按收藏级保存", outcome: "视觉无损，保留全部音轨、字幕与章节" },
  { id: "streaming", title: "流媒体直出", short: "Plex / Jellyfin", tagline: "Plex / Jellyfin 直接播放", outcome: "主流电视与盒子直出，GPU 加速" },
  { id: "mobile", title: "手机平板", short: "随时随地看", tagline: "随时随地观看", outcome: "1080p SDR，体积小，处处可播" },
  { id: "social", title: "社交分享", short: "微信 B站 抖音", tagline: "微信、B站、抖音等平台", outcome: "平台友好，减少二次压缩变糊" },
  { id: "editing", title: "剪辑预处理", short: "导入剪辑前", tagline: "导入 Premiere / 达芬奇 / FCP 前", outcome: "固定帧率，音画不再逐渐失步" },
  { id: "smallest", title: "最小体积", short: "尽量省空间", tagline: "尽量省空间，画质可接受", outcome: "AV1 编码，体积最小" },
  { id: "remux", title: "原样封装", short: "不重新编码", tagline: "不重编码，只换容器或整理音轨", outcome: "零画质损失，秒级完成" },
];
