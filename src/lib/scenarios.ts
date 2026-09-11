import { tr } from "@/i18n";
import type { Scenario } from "./types";

export interface ScenarioMeta {
  id: Scenario;
  title: string;
  /** 卡片上的短说明，控制在 7 个字（英文约 13 个字母）以内，避免被截断；标题同理，英文不超过 10 个字母 */
  short: string;
  tagline: string;
  outcome: string;
}

/** 场景卡片的文案（按界面语言）；每个场景的具体参数由引擎决定（crates/vidforge-core/src/pipeline/strategy.rs） */
export function scenarios(): ScenarioMeta[] {
  return [
    {
      id: "archive",
      title: tr("素材归档", "Archive"),
      short: tr("手机相机拍摄", "Camera roll"),
      tagline: tr("手机、相机素材压缩后长期保存", "Compress phone and camera footage for long-term storage"),
      outcome: tr("体积大幅下降，保留 HDR 与杜比视界", "Much smaller files that keep HDR and Dolby Vision"),
    },
    {
      id: "collection",
      title: tr("高画质收藏", "Library"),
      short: tr("收藏级保存", "Keep it all"),
      tagline: tr("影片按收藏级保存", "Keep movies at collection quality"),
      outcome: tr("视觉无损，保留全部音轨、字幕与章节", "Visually lossless, with every audio track, subtitle and chapter"),
    },
    {
      id: "streaming",
      title: tr("流媒体直出", "Streaming"),
      short: "Plex / Jellyfin",
      tagline: tr("Plex / Jellyfin 直接播放", "Direct play in Plex / Jellyfin"),
      outcome: tr("主流电视与盒子直出，GPU 加速", "Direct play on common TVs and boxes, GPU accelerated"),
    },
    {
      id: "mobile",
      title: tr("手机平板", "Mobile"),
      short: tr("随时随地看", "On the go"),
      tagline: tr("随时随地观看", "Watch anywhere"),
      outcome: tr("1080p SDR，体积小，处处可播", "1080p SDR, small, plays everywhere"),
    },
    {
      id: "social",
      title: tr("社交分享", "Social"),
      short: tr("微信 B站 抖音", "Upload online"),
      tagline: tr("微信、B站、抖音等平台", "WeChat, Bilibili, TikTok and similar sites"),
      outcome: tr("平台友好，减少二次压缩变糊", "Platform friendly, less blur from re-compression"),
    },
    {
      id: "editing",
      title: tr("剪辑预处理", "Editing"),
      short: tr("导入剪辑前", "Before editing"),
      tagline: tr("导入 Premiere / 达芬奇 / FCP 前", "Before importing into Premiere / Resolve / FCP"),
      outcome: tr("固定帧率，音画不再逐渐失步", "Constant frame rate, so audio never drifts"),
    },
    {
      id: "smallest",
      title: tr("最小体积", "Smallest"),
      short: tr("尽量省空间", "Save space"),
      tagline: tr("尽量省空间，画质可接受", "Save as much space as possible at acceptable quality"),
      outcome: tr("AV1 编码，体积最小", "AV1, smallest files"),
    },
    {
      id: "remux",
      title: tr("原样封装", "Remux"),
      short: tr("不重新编码", "No re-encoding"),
      tagline: tr("不重编码，只换容器或整理音轨", "No re-encoding: change the container or tidy up tracks"),
      outcome: tr("零画质损失，秒级完成", "Zero quality loss, done in seconds"),
    },
  ];
}

export const scenarioTitle = (id: Scenario): string | undefined => scenarios().find((s) => s.id === id)?.title;
