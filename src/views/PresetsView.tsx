import { BookmarkPlus } from "lucide-react";
import { SCENARIOS } from "@/mock/engine";
import { Badge, Button, Empty } from "@/components/ui";

/** 内置场景的参数摘要。与 docs/design.md 4.5 的预设表保持一致 */
const SUMMARY: Record<string, { video: string; audio: string; container: string; hdr: string; fps: string; gpu: string }> = {
  archive: { video: "HEVC 10bit · 高画质", audio: "原样复制", container: "MKV", hdr: "保留（含杜比视界）", fps: "保持", gpu: "CPU" },
  collection: { video: "HEVC 10bit · 视觉无损 · slow", audio: "全部复制", container: "MKV", hdr: "全部保留", fps: "保持", gpu: "CPU" },
  streaming: { video: "HEVC · 标准", audio: "兼容格式 + 立体声", container: "MP4", hdr: "保留 HDR10", fps: "可变帧率时转固定", gpu: "GPU 优先" },
  mobile: { video: "H.264 · 1080p", audio: "AAC 立体声", container: "MP4", hdr: "转为 SDR", fps: "保持", gpu: "GPU 优先" },
  social: { video: "H.264 · 1080p · 高画质", audio: "AAC 立体声", container: "MP4", hdr: "转为 SDR", fps: "固定", gpu: "GPU 优先" },
  editing: { video: "H.264 / HEVC · 短 GOP · 高码率", audio: "原样复制", container: "MOV", hdr: "保留", fps: "固定（名义帧率）", gpu: "CPU" },
  smallest: { video: "AV1 · 1080p · 小体积", audio: "Opus 96k", container: "MKV", hdr: "保留", fps: "保持", gpu: "CPU" },
  remux: { video: "不重编码", audio: "全部复制", container: "MKV", hdr: "全部保留", fps: "保持", gpu: "—" },
};

export function PresetsView() {
  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">预设</h1>
        <Button size="sm" className="ml-auto" icon={<BookmarkPlus className="size-3.5" />}>
          把当前参数存为预设
        </Button>
      </header>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1180px] space-y-6 p-5">
          <section>
            <h2 className="mb-1 text-[13px] font-semibold">内置场景</h2>
            <p className="mb-3 text-xs text-muted">场景是起点而不是固定配置：同一个场景会按源文件的特征（HDR、杜比视界、可变帧率、码率）给出不同参数。</p>
            <div className="overflow-x-auto rounded-lg border border-line bg-panel">
              <table className="w-full min-w-[860px] text-xs">
                <thead className="bg-raised text-left text-[11px] text-subtle">
                  <tr>
                    {["场景", "视频", "音频", "容器", "HDR", "帧率", "编码"].map((h) => (
                      <th key={h} className="px-3 py-2 font-medium">
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-line">
                  {SCENARIOS.map((s) => {
                    const x = SUMMARY[s.id]!;
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
            <h2 className="mb-3 text-[13px] font-semibold">我的预设</h2>
            <div className="rounded-lg border border-dashed border-line-strong bg-panel">
              <Empty
                icon={<BookmarkPlus className="size-5" />}
                title="还没有自定义预设"
                description="在转码页调好参数后，可以存为预设，下次一键套用。预设只记录你改动过的参数，其余仍按源文件特征自动推荐。"
              />
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
