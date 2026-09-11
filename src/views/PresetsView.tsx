import { BookmarkPlus } from "lucide-react";
import { tr } from "@/i18n";
import { scenarios } from "@/lib/scenarios";
import { Badge, Button, Empty } from "@/components/ui";

type Summary = { video: string; audio: string; container: string; hdr: string; fps: string; gpu: string };

/** 内置场景的参数摘要。与 docs/design.md 4.5 的预设表保持一致 */
function summary(id: string): Summary {
  const keep = tr("保持", "Keep");
  const allKept = tr("全部保留", "Keep all");
  const gpuFirst = tr("GPU 优先", "GPU first");
  const toSdr = tr("转为 SDR", "Convert to SDR");
  const aacStereo = tr("AAC 立体声", "AAC stereo");
  const copyAll = tr("全部复制", "Copy all");
  const copy = tr("原样复制", "Copy");
  const table: Record<string, Summary> = {
    archive: {
      video: tr("HEVC 10bit · 高画质", "HEVC 10-bit · High"),
      audio: copy,
      container: "MKV",
      hdr: tr("保留（含杜比视界）", "Keep (incl. Dolby Vision)"),
      fps: keep,
      gpu: "CPU",
    },
    collection: {
      video: tr("HEVC 10bit · 视觉无损 · slow", "HEVC 10-bit · Lossless · slow"),
      audio: copyAll,
      container: "MKV",
      hdr: allKept,
      fps: keep,
      gpu: "CPU",
    },
    streaming: {
      video: tr("HEVC · 标准", "HEVC · Standard"),
      audio: tr("兼容格式 + 立体声", "Compatible + stereo"),
      container: "MP4",
      hdr: tr("保留 HDR10", "Keep HDR10"),
      fps: tr("可变帧率时转固定", "Constant if variable"),
      gpu: gpuFirst,
    },
    mobile: { video: "H.264 · 1080p", audio: aacStereo, container: "MP4", hdr: toSdr, fps: keep, gpu: gpuFirst },
    social: {
      video: tr("H.264 · 1080p · 高画质", "H.264 · 1080p · High"),
      audio: aacStereo,
      container: "MP4",
      hdr: toSdr,
      fps: tr("固定", "Constant"),
      gpu: gpuFirst,
    },
    editing: {
      video: tr("H.264 / HEVC · 短 GOP · 高码率", "H.264 / HEVC · short GOP · high bitrate"),
      audio: copy,
      container: "MOV",
      hdr: tr("保留", "Keep"),
      fps: tr("固定（名义帧率）", "Constant (nominal rate)"),
      gpu: "CPU",
    },
    smallest: {
      video: tr("AV1 · 1080p · 小体积", "AV1 · 1080p · Small"),
      audio: "Opus 96k",
      container: "MKV",
      hdr: tr("保留", "Keep"),
      fps: keep,
      gpu: "CPU",
    },
    remux: { video: tr("不重编码", "No re-encoding"), audio: copyAll, container: "MKV", hdr: allKept, fps: keep, gpu: "—" },
  };
  return table[id]!;
}

export function PresetsView() {
  const headers = [
    tr("场景", "Scenario"),
    tr("视频", "Video"),
    tr("音频", "Audio"),
    tr("容器", "Container"),
    "HDR",
    tr("帧率", "Frame rate"),
    tr("编码", "Encoding"),
  ];
  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">{tr("预设", "Presets")}</h1>
        <Button size="sm" className="ml-auto" icon={<BookmarkPlus className="size-3.5" />}>
          {tr("把当前参数存为预设", "Save current settings as a preset")}
        </Button>
      </header>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1180px] space-y-6 p-5">
          <section>
            <h2 className="mb-1 text-[13px] font-semibold">{tr("内置场景", "Built-in scenarios")}</h2>
            <p className="mb-3 text-xs text-muted">
              {tr(
                "场景是起点而不是固定配置：同一个场景会按源文件的特征（HDR、杜比视界、可变帧率、码率）给出不同参数。",
                "A scenario is a starting point, not a fixed configuration: the same scenario picks different settings depending on the source (HDR, Dolby Vision, variable frame rate, bitrate).",
              )}
            </p>
            <div className="overflow-x-auto rounded-lg border border-line bg-panel">
              <table className="w-full min-w-[860px] text-xs">
                <thead className="bg-raised text-left text-[11px] text-subtle">
                  <tr>
                    {headers.map((h) => (
                      <th key={h} className="px-3 py-2 font-medium">
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-line">
                  {scenarios().map((s) => {
                    const x = summary(s.id);
                    return (
                      <tr key={s.id}>
                        <td className="px-3 py-2.5">
                          <div className="font-medium">{s.title}</div>
                          <div className="text-[11px] text-subtle">{s.tagline}</div>
                        </td>
                        <td className="px-3 py-2.5 text-muted">{x.video}</td>
                        <td className="px-3 py-2.5 text-muted">{x.audio}</td>
                        <td className="px-3 py-2.5">
                          <Badge>{x.container}</Badge>
                        </td>
                        <td className="px-3 py-2.5 text-muted">{x.hdr}</td>
                        <td className="px-3 py-2.5 text-muted">{x.fps}</td>
                        <td className="px-3 py-2.5 text-muted">{x.gpu}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>
          <section>
            <h2 className="mb-3 text-[13px] font-semibold">{tr("我的预设", "My presets")}</h2>
            <div className="rounded-lg border border-dashed border-line-strong bg-panel">
              <Empty
                icon={<BookmarkPlus className="size-5" />}
                title={tr("还没有自定义预设", "No custom presets yet")}
                description={tr(
                  "在转码页调好参数后，可以存为预设，下次一键套用。预设只记录你改动过的参数，其余仍按源文件特征自动推荐。",
                  "Save settings you tuned on the Transcode page as a preset and apply them in one click next time. A preset only stores what you changed; everything else is still recommended per file.",
                )}
              />
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
