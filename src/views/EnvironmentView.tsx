import {
  AlertTriangle,
  CircleCheck,
  CircleX,
  Download,
  FolderOpen,
  Loader2,
  MonitorCog,
  RefreshCw,
  RotateCcw,
  Sparkles,
  TerminalSquare,
  Wrench,
} from "lucide-react";
import { backend } from "@/backend";
import type { Capabilities, Codec, EncoderProbe, FailureKind, LocateSource, Vendor } from "@/lib/types";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, VENDOR_LABEL } from "@/mock/engine/encoders";
import { useCapabilities } from "@/stores/capability";
import { useSettings } from "@/stores/settings";
import { Badge, Button, ProgressBar } from "@/components/ui";

export const FAILURE_LABEL: Record<FailureKind, string> = {
  device_missing: "设备或驱动缺失",
  capability: "硬件不支持该格式",
  param: "参数不被接受",
  resource: "资源不足",
  not_built: "未编译进 ffmpeg",
  unknown: "未知错误",
};

const FAILURE_EXPLAIN: Partial<Record<FailureKind, string>> = {
  device_missing: "本机没有对应显卡或驱动，自动选择时直接跳过，转码过程中也不会尝试。",
  not_built: "当前 ffmpeg 构建没有包含这个编码器，换用功能更全的构建即可获得。",
  capability: "显卡在，但不支持这种格式或位深，会改用其他编码器。",
};

const LOCATE_LABEL: Record<LocateSource, string> = {
  user: "设置中指定",
  bundled: "应用目录",
  path: "系统 PATH",
  registry: "注册表 PATH（应用启动后才加入）",
  common: "常见安装位置",
};

/** 推荐的 ffmpeg 构建下载页（docs/ffmpeg-facts.md 第 10 节） */
export function downloadUrl(platform: Capabilities["platform"]): string {
  return platform === "macos" ? "https://github.com/jellyfin/jellyfin-ffmpeg/releases" : "https://www.gyan.dev/ffmpeg/builds/";
}

function vendorsFor(platform: Capabilities["platform"]): Vendor[] {
  if (platform === "macos") return ["software", "apple"];
  if (platform === "linux") return ["software", "intel", "nvidia"];
  return ["software", "intel", "nvidia", "amd"];
}

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

/** 选择 ffmpeg 所在目录并重新探测 */
function usePickFfmpeg() {
  const update = useSettings((s) => s.update);
  const reprobe = useCapabilities((s) => s.reprobe);
  const pick = async () => {
    const dir = await backend.pickDirectory("选择 ffmpeg.exe 所在的目录");
    if (!dir) return;
    await update({ ffmpegPath: dir });
    await reprobe();
  };
  const reset = async () => {
    await update({ ffmpegPath: undefined });
    await reprobe();
  };
  return { pick, reset, canPick: backend.kind === "tauri" };
}

/** 找不到 / 版本过低 / 调用失败时的醒目提示与可行动作 */
function StatusPanel({ caps, error }: { caps: Capabilities; error?: string }) {
  const { pick, canPick } = usePickFfmpeg();
  const reprobe = useCapabilities((s) => s.reprobe);
  const title = error
    ? "环境探测失败"
    : caps.status === "missing"
      ? "没有找到 ffmpeg"
      : caps.status === "too_old"
        ? "ffmpeg 版本过低"
        : "ffmpeg 无法运行";
  const danger = error || caps.status !== "too_old";
  return (
    <section
      role="alert"
      className={cn(
        "rounded-lg border p-4 lg:col-span-2",
        danger ? "border-danger/30 bg-danger/6" : "border-warn/35 bg-warn/8",
      )}
    >
      <div className="flex items-start gap-3">
        <AlertTriangle className={cn("mt-0.5 size-5 shrink-0", danger ? "text-danger" : "text-warn")} />
        <div className="min-w-0 flex-1">
          <h2 className="text-[14px] font-semibold">{title}</h2>
          <p className="mt-1 text-[13px] text-muted">{error ?? caps.statusDetail}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            {canPick && (
              <Button size="sm" variant="primary" icon={<FolderOpen className="size-3.5" />} onClick={() => void pick()}>
                选择 ffmpeg 所在目录
              </Button>
            )}
            <Button size="sm" icon={<Download className="size-3.5" />} onClick={() => void backend.openUrl(downloadUrl(caps.platform))}>
              下载推荐构建
            </Button>
            <Button size="sm" variant="ghost" icon={<RefreshCw className="size-3.5" />} onClick={() => void reprobe()}>
              安装好了，重新探测
            </Button>
          </div>
          <p className="mt-2 text-[11px] text-subtle">
            {caps.platform === "macos"
              ? "macOS 推荐 jellyfin-ffmpeg：Homebrew 版缺少全部三条色调映射管线。"
              : "Windows 推荐 gyan.dev 的 full 构建（essentials 缺少 AV1 软编与 GPU 色调映射）。"}
          </p>
          {caps.searched.length > 0 && (
            <details className="mt-3 text-xs">
              <summary className="cursor-default text-muted hover:text-fg">查找过的位置（{caps.searched.length}）</summary>
              <ul className="mt-1.5 max-h-40 space-y-0.5 overflow-y-auto font-mono text-[11px] text-subtle">
                {caps.searched.map((s) => (
                  <li key={s} className="selectable break-all">
                    {s}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {caps.notes.length > 0 && (
            <ul className="mt-2 space-y-0.5 text-[11px] text-muted">
              {caps.notes.map((n) => (
                <li key={n}>· {n}</li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}

export function EnvironmentView() {
  const caps = useCapabilities((s) => s.caps);
  const probing = useCapabilities((s) => s.probing);
  const progress = useCapabilities((s) => s.progress);
  const error = useCapabilities((s) => s.error);
  const reprobe = useCapabilities((s) => s.reprobe);
  const userPath = useSettings((s) => s.settings.ffmpegPath);
  const { pick, reset, canPick } = usePickFfmpeg();

  const vendors = vendorsFor(caps.platform);
  const codecs: Codec[] = ["h264", "hevc", "av1"];
  const find = (v: Vendor, c: Codec) => caps.encoders.find((e) => e.vendor === v && e.codec === c);
  const missing = caps.buildFlags.filter((f) => !f.present);
  const unusable = caps.encoders.filter((e) => !e.usable && e.vendor !== "software");
  const failureKinds = [...new Set(unusable.map((e) => e.failure).filter((f): f is FailureKind => !!f))];
  const found = caps.status === "ready" || caps.status === "too_old";
  const devicesOk = caps.devices.filter((d) => d.available);
  const devicesBad = caps.devices.filter((d) => !d.available);

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">环境与硬件</h1>
        {caps.probedAt && <span className="text-xs text-subtle">上次探测：{new Date(caps.probedAt).toLocaleString()}</span>}
        <Button
          size="sm"
          className="ml-auto"
          icon={probing ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
          onClick={() => void reprobe()}
          disabled={probing}
        >
          {probing ? "探测中…" : "重新探测"}
        </Button>
      </header>
      {probing && (
        <div className="flex items-center gap-3 border-b border-line bg-sunken/60 px-5 py-2 text-xs text-muted" aria-live="polite">
          <span className="shrink-0">{progress ? `${progress.stage}（${progress.done}/${progress.total}）` : "正在探测环境…"}</span>
          <div className="max-w-xs flex-1">
            <ProgressBar live value={progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0} />
          </div>
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto grid max-w-[1180px] gap-4 p-5 lg:grid-cols-2">
          {(error || (caps.status !== "ready" && caps.status !== "probing")) && <StatusPanel caps={caps} error={error} />}

          {found && (
            <Card
              title="ffmpeg"
              icon={<TerminalSquare className="size-4" />}
              aside={
                canPick && (
                  <div className="flex gap-1">
                    {userPath && (
                      <Button size="xs" variant="ghost" icon={<RotateCcw className="size-3" />} onClick={() => void reset()}>
                        恢复自动查找
                      </Button>
                    )}
                    <Button size="xs" variant="ghost" icon={<FolderOpen className="size-3" />} onClick={() => void pick()}>
                      更换
                    </Button>
                  </div>
                )
              }
              className="lg:col-span-2"
            >
              <div className="flex flex-wrap items-start gap-x-10 gap-y-3">
                <div>
                  <div className="text-[11px] text-subtle">版本</div>
                  <div className="mt-0.5 flex items-center gap-2">
                    <span className="font-mono text-[18px] font-semibold">{caps.versionNumber}</span>
                    {caps.status === "ready" ? (
                      <Badge tone="ok">满足全部 v1 功能（需 ≥ 7.1）</Badge>
                    ) : (
                      <Badge tone="danger">低于 7.1，请升级</Badge>
                    )}
                  </div>
                  <div className="mt-0.5 text-xs text-muted">{caps.buildSource}</div>
                </div>
                <div className="min-w-0 flex-1">
                  <div className="text-[11px] text-subtle">
                    路径{caps.locateSource && <span className="ml-1.5">· {LOCATE_LABEL[caps.locateSource]}</span>}
                  </div>
                  <div className="selectable mt-0.5 font-mono text-xs break-all">{caps.ffmpegPath}</div>
                  <div className="selectable font-mono text-xs break-all text-muted">{caps.ffprobePath}</div>
                </div>
                <div>
                  <div className="text-[11px] text-subtle">显卡</div>
                  {caps.gpus.length === 0 && <div className="mt-0.5 text-xs text-muted">未识别</div>}
                  {caps.gpus.map((g) => (
                    <div key={g.name} className="mt-0.5 text-xs">
                      {g.name}
                      <span className="ml-1.5 font-mono text-[11px] text-subtle">{g.driver}</span>
                    </div>
                  ))}
                </div>
              </div>
              {caps.status === "ready" && caps.notes.length > 0 && (
                <ul className="mt-3 space-y-0.5 border-t border-line pt-2.5 text-[11px] text-warn">
                  {caps.notes.map((n) => (
                    <li key={n}>· {n}</li>
                  ))}
                </ul>
              )}
            </Card>
          )}

          {found && (
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
                  <ul className="mt-2 space-y-0.5 text-muted">
                    {failureKinds.map((k) =>
                      FAILURE_EXPLAIN[k] ? (
                        <li key={k}>
                          <span className="font-medium">{FAILURE_LABEL[k]}</span>：{FAILURE_EXPLAIN[k]}
                        </li>
                      ) : null,
                    )}
                  </ul>
                </details>
              )}
            </Card>
          )}

          {found && (
            <Card title="编译能力" icon={<Wrench className="size-4" />}>
              <p className="text-xs">
                <span className={cn("font-medium", missing.some((f) => f.name !== "libfdk_aac") ? "text-warn" : "text-ok")}>
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
          )}

          {found && (
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
                      <div className="min-w-0">
                        <span className={cn("font-mono", !t.available && "text-subtle line-through")}>{t.id}</span>
                        {t.available && caps.tonemap.findIndex((x) => x.available) === i && (
                          <Badge tone="accent" className="ml-2">
                            默认
                          </Badge>
                        )}
                        <p className="break-all text-muted">{t.note}</p>
                      </div>
                    </li>
                  ))}
                </ol>
                {!caps.tonemap.some((t) => t.available) && (
                  <p className="mt-3 text-[11px] text-warn">没有可用的色调映射管线，HDR 素材只能保持 HDR 输出，“转为 SDR”会置灰。</p>
                )}
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
                    <li key={x.name} className="flex items-center gap-2" title={x.path}>
                      {x.found ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-subtle" />}
                      <span className="font-mono">{x.name}</span>
                      <span className="text-muted">{x.purpose}</span>
                      <span className="ml-auto text-[11px] text-subtle">{x.found ? (x.version ?? "已安装") : "未安装 · 可选"}</span>
                    </li>
                  ))}
                </ul>
                <div className="mt-3 space-y-1 border-t border-line pt-3 text-[11px] text-subtle">
                  <p>
                    可初始化的硬件设备：
                    {devicesOk.length ? devicesOk.map((d) => d.id).join("、") : "无"}
                  </p>
                  {devicesBad.length > 0 && (
                    <p title={devicesBad.map((d) => `${d.id}: ${d.error ?? ""}`).join("\n")}>
                      不可用：{devicesBad.map((d) => d.id).join("、")}（悬停查看原因）
                    </p>
                  )}
                </div>
              </Card>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
