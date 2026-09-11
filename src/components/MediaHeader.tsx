import { Layers, X } from "lucide-react";
import { isEnglish, tr } from "@/i18n";
import type { MediaInfo } from "@/lib/types";
import { cn } from "@/lib/cn";
import { channelLabel, formatBitrate, formatBytes, formatDuration, formatFps } from "@/lib/format";
import { mediaFeatures } from "@/lib/media-features";
import { useUi } from "@/stores/ui";
import { SOURCE_ICON } from "./FileList";
import { Badge, Button } from "./ui";

const hdrName = (kind: "none" | "hdr10" | "hlg" | "pq_no_meta"): string =>
  ({ none: "SDR", hdr10: "HDR10 (PQ)", hlg: "HLG", pq_no_meta: tr("PQ（无元数据）", "PQ (no metadata)") })[kind];

export function MediaHeader({ media }: { media: MediaInfo }) {
  const setDetailsOpen = useUi((s) => s.setDetailsOpen);
  const v = media.video[0];
  const Icon = SOURCE_ICON[media.sourceHint];
  const features = mediaFeatures(media);

  const meta = [
    v && `${v.width}×${v.height}`,
    v && v.codec.toUpperCase(),
    v && `${formatFps(v.fpsAvg)} fps`,
    formatDuration(media.durationSec),
    formatBytes(media.sizeBytes),
    formatBitrate(media.bitrate),
    media.device,
  ].filter(Boolean);

  return (
    <div className="flex items-center gap-3">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-accent/12 text-accent">
        <Icon className="size-[18px]" />
      </div>
      <div className="min-w-0 flex-1">
        <h1 className="selectable truncate text-[15px] leading-tight font-semibold" title={media.path}>
          {media.name}
        </h1>
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted tabular">
          {meta.map((m, i) => (
            <span key={i} className="flex items-center gap-2">
              {i > 0 && <span className="size-0.5 rounded-full bg-line-strong" />}
              {m}
            </span>
          ))}
          {features.length > 0 && <span className="mx-0.5 h-3 w-px bg-line" />}
          {features.map((f) => (
            <Badge key={f.key} tone={f.tone} title={f.title}>
              {f.label}
            </Badge>
          ))}
        </div>
      </div>
      <Button size="sm" icon={<Layers className="size-3.5" />} onClick={() => setDetailsOpen(true)}>
        {tr("流信息", "Streams")}
      </Button>
    </div>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="flex gap-3 py-1 text-xs">
      <span className={cn("shrink-0 text-subtle", isEnglish() ? "w-24" : "w-20")}>{k}</span>
      <span className="selectable min-w-0 flex-1 text-fg tabular">{v}</span>
    </div>
  );
}

export function MediaDetails({ media }: { media: MediaInfo }) {
  const open = useUi((s) => s.detailsOpen);
  const setOpen = useUi((s) => s.setDetailsOpen);

  return (
    <>
      <div
        onClick={() => setOpen(false)}
        className={cn("fixed inset-0 z-30 bg-black/30 transition-opacity", open ? "opacity-100" : "pointer-events-none opacity-0")}
      />
      <aside
        inert={!open}
        aria-hidden={!open}
        aria-label={tr("流信息", "Streams")}
        className={cn(
          "fixed top-0 right-0 bottom-0 z-40 flex w-[440px] max-w-full flex-col border-l border-line bg-panel shadow-card transition-transform duration-200",
          open ? "translate-x-0" : "translate-x-full",
        )}
      >
        <header className="flex h-12 items-center border-b border-line px-4">
          <h2 className="font-semibold">{tr("流信息", "Streams")}</h2>
          <Button size="sm" variant="ghost" className="ml-auto" icon={<X className="size-4" />} onClick={() => setOpen(false)} />
        </header>
        <div className="flex-1 space-y-5 overflow-y-auto p-4">
          <div>
            <h3 className="mb-1 text-xs font-semibold text-muted">{tr("容器", "Container")}</h3>
            <Row k={tr("格式", "Format")} v={media.container} />
            <Row k={tr("时长", "Duration")} v={formatDuration(media.durationSec)} />
            <Row k={tr("体积", "Size")} v={formatBytes(media.sizeBytes)} />
            <Row k={tr("总码率", "Bitrate")} v={formatBitrate(media.bitrate)} />
            <Row k={tr("章节", "Chapters")} v={media.chapters || tr("无", "None")} />
          </div>

          {media.video.map((v) => (
            <div key={v.index}>
              <h3 className="mb-1 text-xs font-semibold text-muted">
                {tr("视频", "Video")} #{v.index}
              </h3>
              <Row k={tr("编码", "Codec")} v={`${v.codec} ${v.profile ?? ""}`} />
              <Row k={tr("分辨率", "Resolution")} v={`${v.width}×${v.height}`} />
              <Row
                k={tr("帧率", "Frame rate")}
                v={
                  <>
                    {tr(
                      `平均 ${formatFps(v.fpsAvg)} / 名义 ${formatFps(v.fpsNominal)}`,
                      `avg ${formatFps(v.fpsAvg)} / nominal ${formatFps(v.fpsNominal)}`,
                    )}
                    {v.isVfr && (
                      <Badge tone="vfr" className="ml-2">
                        {tr("可变帧率", "VFR")}
                      </Badge>
                    )}
                  </>
                }
              />
              <Row k={tr("像素格式", "Pixel format")} v={tr(`${v.pixFmt}（${v.bitDepth}bit）`, `${v.pixFmt} (${v.bitDepth}-bit)`)} />
              <Row k={tr("码率", "Bitrate")} v={formatBitrate(v.bitrate)} />
              <Row k={tr("色彩", "Color")} v={`${v.color.primaries} / ${v.color.transfer} / ${v.color.space}`} />
              <Row k={tr("动态范围", "Dynamic range")} v={hdrName(v.color.hdrKind)} />
              {v.hdr10 && (
                <Row
                  k="HDR10"
                  v={`${tr("母版", "Mastering")} ${v.hdr10.masteringPrimaries.toUpperCase()} · ${v.hdr10.maxLuminance} / ${v.hdr10.minLuminance} nits · MaxCLL ${v.hdr10.maxCll ?? "—"} · MaxFALL ${v.hdr10.maxFall ?? "—"}`}
                />
              )}
              {v.dolbyVision && (
                <Row
                  k={tr("杜比视界", "Dolby Vision")}
                  v={`Profile ${v.dolbyVision.profile}${v.dolbyVision.profile === 8 ? `.${v.dolbyVision.blCompatId}` : ""} · ${
                    v.dolbyVision.hasEnhancementLayer
                      ? tr(`双层（${v.dolbyVision.elType}）`, `dual layer (${v.dolbyVision.elType})`)
                      : tr("单层", "single layer")
                  } · ${tr("兼容 ID", "compat ID")} ${v.dolbyVision.blCompatId}`}
                />
              )}
              {v.frameCount && <Row k={tr("帧数", "Frames")} v={v.frameCount.toLocaleString()} />}
            </div>
          ))}

          {media.audio.length > 0 && (
            <div>
              <h3 className="mb-2 text-xs font-semibold text-muted">
                {tr(`音频（${media.audio.length}）`, `Audio (${media.audio.length})`)}
              </h3>
              <div className="space-y-1.5">
                {media.audio.map((a) => (
                  <div key={a.index} className="rounded-md border border-line bg-raised/50 px-3 py-2 text-xs">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-subtle">#{a.index}</span>
                      <span className="font-medium">{a.title ?? a.codec.toUpperCase()}</span>
                      {a.atmos && <Badge tone="atmos">{tr("全景声", "Atmos")}</Badge>}
                      {a.lossless && <Badge tone="lossless">{tr("无损", "Lossless")}</Badge>}
                      {a.isDefault && <Badge>{tr("默认", "Default")}</Badge>}
                    </div>
                    <div className="mt-1 text-muted tabular">
                      {a.codec} · {channelLabel(a.channels, a.channelLayout)} · {a.sampleRate / 1000} kHz · {formatBitrate(a.bitrate)}
                      {a.language && a.language !== "und" ? ` · ${a.language}` : ""}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {media.subtitle.length > 0 && (
            <div>
              <h3 className="mb-2 text-xs font-semibold text-muted">
                {tr(`字幕（${media.subtitle.length}）`, `Subtitles (${media.subtitle.length})`)}
              </h3>
              <div className="space-y-1">
                {media.subtitle.map((s) => (
                  <div key={s.index} className="flex items-center gap-2 text-xs">
                    <span className="font-mono text-subtle">#{s.index}</span>
                    <span>{s.title ?? s.language}</span>
                    <span className="text-subtle">{s.codec}</span>
                    {s.imageBased && <Badge>{tr("图形字幕", "Image")}</Badge>}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </aside>
    </>
  );
}
