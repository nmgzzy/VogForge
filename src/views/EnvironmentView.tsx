import { CircleCheck, CircleX, FolderOpen, Loader2, MonitorCog, RefreshCw, Sparkles, TerminalSquare, Wrench } from "lucide-react";
import type { Codec, EncoderProbe, Vendor } from "@/lib/types";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, VENDOR_LABEL } from "@/mock/engine/encoders";
import { useCapabilities } from "@/stores/capability";
import { Badge, Button } from "@/components/ui";

const FAILURE_LABEL = {
  device_missing: "设备或驱动缺失",
  capability: "硬件不支持该格式",
  param: "参数不被接受",
  resource: "资源不足",
  unknown: "未知错误",
} as const;

function Card({ title, icon, aside, children, className }: { title: string; icon: React.ReactNode; aside?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return (
    <section className={cn("rounded-lg border border-line bg-panel", className)}>
      <header className="flex h-11 items-center gap-2 border-b border-line px-4">
        <span className="text-subtle">{icon}</span>
        <h2 className="text-[13px] font-semibold">{title}</h2>
        {aside && <div className="ml-auto">{aside}</div>}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

function Cell({ probe }: { probe?: EncoderProbe }) {
  if (!probe) return <span className="text-[11px] text-subtle">—</span>;
  if (probe.usable) {
    return (
      <div className="flex flex-col items-center gap-1">
        <CircleCheck className="size-4 text-ok" />
        <span className="font-mono text-[10.5px] text-muted">{probe.id}</span>
        {probe.tenBit ? <Badge tone="ok">10bit</Badge> : <Badge>仅 8bit</Badge>}
      </div>
    );
  }
  return (
    <div className="group relative flex flex-col items-center gap-1" title={probe.error}>
      <CircleX className="size-4 text-danger/70" />
      <span className="font-mono text-[10.5px] text-subtle line-through">{probe.id}</span>
      <span className="text-[10.5px] text-danger/80">{probe.failure ? FAILURE_LABEL[probe.failure] : "不可用"}</span>
    </div>
  );
}

export function EnvironmentView() {
  const caps = useCapabilities((s) => s.caps);
  const probing = useCapabilities((s) => s.probing);
  const reprobe = useCapabilities((s) => s.reprobe);

  const vendors: Vendor[] = caps.platform === "windows" ? ["software", "intel", "nvidia", "amd"] : ["software", "apple"];
  const codecs: Codec[] = ["h264", "hevc", "av1"];
  const find = (v: Vendor, c: Codec) => caps.encoders.find((e) => e.vendor === v && e.codec === c);
  const missing = caps.buildFlags.filter((f) => !f.present);
  const unusable = caps.encoders.filter((e) => !e.usable);

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">环境与硬件</h1>
        <span className="text-xs text-subtle">上次探测：{new Date(caps.probedAt).toLocaleString()}</span>
        <Button
          size="sm"
          className="ml-auto"
          icon={probing ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
          onClick={reprobe}
          disabled={probing}
        >
          {probing ? "探测中…" : "重新探测"}
        </Button>
      </header>

      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto grid max-w-[1180px] gap-4 p-5 lg:grid-cols-2">
          <Card
            title="ffmpeg"
            icon={<TerminalSquare className="size-4" />}
            aside={
              <Button size="xs" variant="ghost" icon={<FolderOpen className="size-3" />}>
                更换
              </Button>
            }
            className="lg:col-span-2"
          >
            <div className="flex flex-wrap items-start gap-x-10 gap-y-3">
              <div>
                <div className="text-[11px] text-subtle">版本</div>
                <div className="mt-0.5 flex items-center gap-2">
                  <span className="font-mono text-[18px] font-semibold">{caps.version}</span>
                  <Badge tone="ok">满足全部 v1 功能（需 ≥ 7.1）</Badge>
                </div>
                <div className="mt-0.5 text-xs text-muted">{caps.buildSource}</div>
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[11px] text-subtle">路径</div>
                <div className="selectable mt-0.5 font-mono text-xs">{caps.ffmpegPath}</div>
                <div className="selectable font-mono text-xs text-muted">{caps.ffprobePath}</div>
              </div>
              <div>
                <div className="text-[11px] text-subtle">显卡</div>
                {caps.gpus.map((g) => (
                  <div key={g.name} className="mt-0.5 text-xs">
                    {g.name}
                    <span className="ml-1.5 font-mono text-[11px] text-subtle">{g.driver}</span>
                  </div>
                ))}
              </div>
            </div>
          </Card>

          <Card
            title="硬件编码能力"
            icon={<MonitorCog className="size-4" />}
            className="lg:col-span-2"
            aside={<span className="text-[11px] text-subtle">每个编码器都做过 3 帧真实试编码，而不只是查列表</span>}
          >
            <div className="overflow-x-auto">
              <table className="w-full min-w-[560px] text-center text-xs">
                <thead>
                  <tr className="text-[11px] text-subtle">
                    <th className="w-36 pb-3 text-left font-medium" />
                    {codecs.map((c) => (
                      <th key={c} className="pb-3 font-medium">
                        {CODEC_LABEL[c]}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-line">
                  {vendors.map((v) => (
                    <tr key={v}>
                      <td className="py-3 text-left">
                        <div className="font-medium">{VENDOR_LABEL[v]}</div>
                        <div className="text-[11px] text-subtle">{v === "software" ? "总是可用，画质最好" : "速度快 5–10 倍"}</div>
                      </td>
                      {codecs.map((c) => (
                        <td key={c} className="py-3">
                          <Cell probe={find(v, c)} />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {unusable.length > 0 && (
              <details className="mt-3 rounded-md bg-sunken/70 px-3 py-2 text-xs">
                <summary className="cursor-default text-muted">不可用的原因（{unusable.length}）</summary>
                <ul className="mt-2 space-y-1 font-mono text-[11px] text-subtle">
                  {[...new Set(unusable.map((e) => e.error))].map((err) => (
                    <li key={err} className="selectable break-all">
                      {err}
                    </li>
                  ))}
                </ul>
                <p className="mt-2 text-muted">这些报错属于“设备缺失”：本机没有对应显卡或驱动。自动选择时会直接跳过，转码过程中也不会尝试。</p>
              </details>
            )}
          </Card>

          <Card title="编译能力" icon={<Wrench className="size-4" />}>
            <p className="text-xs">
              <span className="font-medium text-ok">
                {caps.buildFlags.length - missing.length}/{caps.buildFlags.length} 项具备
              </span>
              {missing.length > 0 && (
                <span className="text-muted">
                  ，缺 {missing.map((f) => f.name).join("、")}
                  {missing.length === 1 && missing[0]?.name === "libfdk_aac" && "（官方构建均不含，不影响功能）"}
                </span>
              )}
            </p>
            <details className="mt-2.5 text-xs">
              <summary className="cursor-default text-muted hover:text-fg">查看全部与影响的功能</summary>
              <ul className="mt-2 space-y-1.5">
                {caps.buildFlags.map((f) => (
                  <li key={f.name} className="flex items-start gap-2">
                    {f.present ? (
                      <CircleCheck className="mt-px size-3.5 shrink-0 text-ok" />
                    ) : (
                      <CircleX className="mt-px size-3.5 shrink-0 text-subtle" />
                    )}
                    <span className={cn("w-24 shrink-0 font-mono", !f.present && "text-subtle")}>{f.name}</span>
                    <span className="text-muted">{f.affects}</span>
                  </li>
                ))}
              </ul>
            </details>
          </Card>

          <div className="flex flex-col gap-4">
            <Card title="色调映射（HDR 转 SDR）" icon={<Sparkles className="size-4" />}>
              <ol className="space-y-2">
                {caps.tonemap.map((t, i) => (
                  <li key={t.id} className="flex items-start gap-2.5 text-xs">
                    <span
                      className={cn(
                        "flex size-5 shrink-0 items-center justify-center rounded-full text-[10.5px] font-semibold",
                        t.available ? "bg-accent/12 text-accent" : "bg-raised text-subtle",
                      )}
                    >
                      {i + 1}
                    </span>
                    <div>
                      <span className={cn("font-mono", !t.available && "text-subtle line-through")}>{t.id}</span>
                      {i === 0 && t.available && <Badge tone="accent" className="ml-2">默认</Badge>}
                      <p className="text-muted">{t.note}</p>
                    </div>
                  </li>
                ))}
              </ol>
            </Card>

            <Card title="杜比视界与外部工具" icon={<Sparkles className="size-4" />}>
              <ul className="space-y-1.5 text-xs">
                <li className="flex items-center gap-2">
                  {caps.dolbyVisionEncode ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-danger" />}
                  <span>杜比视界单层保留</span>
                  <span className="text-muted">libx265 -dolbyvision（ffmpeg ≥ 7.1）</span>
                </li>
                <li className="flex items-center gap-2">
                  {caps.doviSplit ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-subtle" />}
                  <span>Profile 7 拆层</span>
                  <span className="text-muted">dovi_split（ffmpeg ≥ 9.0，v2 使用）</span>
                </li>
                {caps.external.map((x) => (
                  <li key={x.name} className="flex items-center gap-2">
                    {x.found ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-subtle" />}
                    <span className="font-mono">{x.name}</span>
                    <span className="text-muted">{x.purpose}</span>
                    {!x.found && <span className="ml-auto text-[11px] text-subtle">未安装 · 可选</span>}
                  </li>
                ))}
              </ul>
              <p className="mt-3 border-t border-line pt-3 text-[11px] text-subtle">
                可用硬件解码：{caps.hwaccels.join("、")}（列出不等于可用，编码前会逐项试编码确认）
              </p>
            </Card>
          </div>
        </div>
      </div>
    </div>
  );
}
