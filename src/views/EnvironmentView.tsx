import { useEffect, useState } from "react";
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
import { tr } from "@/i18n";
import type { BuildFlag, Capabilities, Codec, EncoderProbe, FailureKind, LocateSource, Vendor } from "@/lib/types";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, vendorLabel } from "@/lib/encoders";
import { useCapabilities } from "@/stores/capability";
import { useSettings } from "@/stores/settings";
import { Badge, Button, ProgressBar } from "@/components/ui";

export function failureLabel(kind: FailureKind): string {
  switch (kind) {
    case "device_missing":
      return tr("设备或驱动缺失", "No device or driver");
    case "capability":
      return tr("硬件不支持该格式", "Not supported by the hardware");
    case "param":
      return tr("参数不被接受", "Settings rejected");
    case "resource":
      return tr("资源不足", "Out of resources");
    case "not_built":
      return tr("未编译进 ffmpeg", "Not built into ffmpeg");
    case "unknown":
      return tr("未知错误", "Unknown error");
  }
}

function failureExplain(kind: FailureKind): string | undefined {
  switch (kind) {
    case "device_missing":
      return tr(
        "本机没有对应显卡或驱动，自动选择时直接跳过，转码过程中也不会尝试。",
        "This computer has no matching GPU or driver, so automatic selection skips it and jobs never try it.",
      );
    case "not_built":
      return tr(
        "当前 ffmpeg 构建没有包含这个编码器，换用功能更全的构建即可获得。",
        "This ffmpeg build does not include the encoder; a more complete build adds it.",
      );
    case "capability":
      return tr("显卡在，但不支持这种格式或位深，会改用其他编码器。", "The GPU is there but does not support this format or bit depth, so another encoder is used.");
    default:
      return undefined;
  }
}

function locateLabel(source: LocateSource): string {
  switch (source) {
    case "user":
      return tr("设置中指定", "set in Settings");
    case "bundled":
      return tr("应用目录", "app folder");
    case "path":
      return tr("系统 PATH", "system PATH");
    case "registry":
      return tr("注册表 PATH（应用启动后才加入）", "registry PATH (added after the app started)");
    case "common":
      return tr("常见安装位置", "common install location");
  }
}

interface Build {
  name: string;
  url: string;
  note: string;
}

/** 推荐的 ffmpeg 构建（需求 F-8.3，依据 docs/ffmpeg-facts.md 第 10 节） */
export function recommendedBuilds(platform: Capabilities["platform"]): Build[] {
  const jellyfin: Build = {
    name: "jellyfin-ffmpeg",
    url: "https://github.com/jellyfin/jellyfin-ffmpeg/releases",
    note:
      platform === "macos"
        ? tr(
            "下载 macOS 的 portable 包。Homebrew 版缺少全部三条色调映射管线，HDR 转 SDR 会不可用",
            "Get the macOS portable package. The Homebrew build lacks all three tone mapping pipelines, so HDR to SDR would be unavailable",
          )
        : tr("带全部色调映射管线的便携构建", "A portable build with every tone mapping pipeline"),
  };
  const btbn = (file: string): Build => ({
    name: "BtbN gpl",
    url: "https://github.com/BtbN/FFmpeg-Builds/releases",
    note: tr(`下载 ${file}，能力齐全`, `Get ${file}; it has every feature`),
  });
  if (platform === "macos") return [jellyfin];
  if (platform === "linux") return [btbn("ffmpeg-master-latest-linux64-gpl.tar.xz"), jellyfin];
  return [
    {
      name: "gyan.dev full",
      url: "https://www.gyan.dev/ffmpeg/builds/",
      note: tr(
        "下载 ffmpeg-release-full.7z。能力最全；essentials 版缺少 AV1 软编与 GPU 色调映射",
        "Get ffmpeg-release-full.7z. It has everything; the essentials build lacks AV1 software encoding and GPU tone mapping",
      ),
    },
    btbn("ffmpeg-master-latest-win64-gpl.zip"),
  ];
}

export const downloadUrl = (platform: Capabilities["platform"]): string => recommendedBuilds(platform)[0]!.url;

/** 缺了会明显影响功能的编译开关（其余缺失只影响 v2 功能或已有替代） */
const KEY_FLAGS = ["libx265", "libsvtav1", "libplacebo", "libzimg"];

function vendorsFor(platform: Capabilities["platform"]): Vendor[] {
  if (platform === "macos") return ["software", "apple"];
  if (platform === "linux") return ["software", "intel", "nvidia"];
  return ["software", "intel", "nvidia", "amd"];
}

function Card({
  title,
  icon,
  aside,
  children,
  className,
}: {
  title: string;
  icon: React.ReactNode;
  aside?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
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
        {probe.tenBit ? <Badge tone="ok">10bit</Badge> : <Badge>{tr("仅 8bit", "8-bit only")}</Badge>}
      </div>
    );
  }
  return (
    <div className="group relative flex flex-col items-center gap-1" title={probe.error}>
      <CircleX className="size-4 text-danger/70" />
      <span className="font-mono text-[10.5px] text-subtle line-through">{probe.id}</span>
      <span className="text-[10.5px] text-danger/80">{probe.failure ? failureLabel(probe.failure) : tr("不可用", "Unavailable")}</span>
    </div>
  );
}

/** 选择 ffmpeg 所在目录并重新探测 */
function usePickFfmpeg() {
  const update = useSettings((s) => s.update);
  const reprobe = useCapabilities((s) => s.reprobe);
  const pick = async () => {
    const dir = await backend.pickDirectory(tr("选择 ffmpeg.exe 所在的目录", "Choose the folder that contains ffmpeg"));
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

/** 下载与安装推荐构建的步骤（需求 F-8.3）。应用目录里的 ffmpeg 会被优先找到，放进去即可 */
export function FfmpegGuide({ platform }: { platform: Capabilities["platform"] }) {
  const [dir, setDir] = useState<string | null>(null);
  const reprobe = useCapabilities((s) => s.reprobe);
  const { pick, canPick } = usePickFfmpeg();
  useEffect(() => {
    void backend.ffmpegInstallDir().then(setDir);
  }, []);
  const exe = platform === "windows" ? "ffmpeg.exe / ffprobe.exe" : "ffmpeg / ffprobe";
  return (
    <div className="space-y-2.5 text-xs" data-testid="ffmpeg-guide">
      <ol className="list-decimal space-y-2 pl-4 text-muted marker:text-subtle">
        <li>
          <span className="text-fg">{tr("下载推荐构建：", "Download a recommended build:")}</span>
          <div className="mt-1.5 flex flex-col gap-1.5">
            {recommendedBuilds(platform).map((b) => (
              <div key={b.name} className="flex flex-wrap items-center gap-2">
                <Button size="xs" icon={<Download className="size-3" />} onClick={() => void backend.openUrl(b.url)}>
                  {tr(`下载 ${b.name}`, `Download ${b.name}`)}
                </Button>
                <span className="text-[11px] text-subtle">{b.note}</span>
              </div>
            ))}
          </div>
        </li>
        <li>
          {dir ? (
            <>
              {tr(`解压后，把 bin 文件夹里的 ${exe} 放进应用的 ffmpeg 目录（会被优先找到）：`, `Extract it and put ${exe} from the bin folder into the app's ffmpeg folder (it is checked first):`)}
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <code className="selectable rounded bg-sunken px-1.5 py-0.5 font-mono text-[11px] break-all">{dir}</code>
                <Button size="xs" variant="ghost" icon={<FolderOpen className="size-3" />} onClick={() => void backend.openFfmpegDir()}>
                  {tr("打开这个文件夹", "Open this folder")}
                </Button>
              </div>
              {canPick && (
                <p className="mt-1">
                  {tr("或者解压到任意位置，再", "Or extract it anywhere and ")}
                  <button className="text-accent hover:underline" onClick={() => void pick()}>
                    {tr("选择 ffmpeg 所在目录", "choose the ffmpeg folder")}
                  </button>
                  {tr("。", ".")}
                </p>
              )}
            </>
          ) : (
            tr(`解压后把 bin 文件夹加入系统 PATH，或在设置里指定它。`, `Extract it and add the bin folder to PATH, or set it in Settings.`)
          )}
        </li>
        <li>
          <button className="text-accent hover:underline" onClick={() => void reprobe()}>
            {tr("重新探测", "Check again")}
          </button>
          {tr("，应用会重新做一遍能力检测。", " and the app re-runs the capability checks.")}
        </li>
      </ol>
    </div>
  );
}

/** 找不到 / 版本过低 / 调用失败时的醒目提示与可行动作 */
function StatusPanel({ caps, error }: { caps: Capabilities; error?: string }) {
  const { pick, canPick } = usePickFfmpeg();
  const reprobe = useCapabilities((s) => s.reprobe);
  const title = error
    ? tr("环境探测失败", "Environment check failed")
    : caps.status === "missing"
      ? tr("没有找到 ffmpeg", "ffmpeg not found")
      : caps.status === "too_old"
        ? tr("ffmpeg 版本过低", "ffmpeg is too old")
        : tr("ffmpeg 无法运行", "ffmpeg cannot run");
  const danger = error || caps.status !== "too_old";
  return (
    <section
      role="alert"
      className={cn("rounded-lg border p-4 lg:col-span-2", danger ? "border-danger/30 bg-danger/6" : "border-warn/35 bg-warn/8")}
    >
      <div className="flex items-start gap-3">
        <AlertTriangle className={cn("mt-0.5 size-5 shrink-0", danger ? "text-danger" : "text-warn")} />
        <div className="min-w-0 flex-1">
          <h2 className="text-[14px] font-semibold">{title}</h2>
          <p className="mt-1 text-[13px] text-muted">{error ?? caps.statusDetail}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            {canPick && (
              <Button size="sm" variant="primary" icon={<FolderOpen className="size-3.5" />} onClick={() => void pick()}>
                {tr("选择 ffmpeg 所在目录", "Choose the ffmpeg folder")}
              </Button>
            )}
            <Button size="sm" variant="ghost" icon={<RefreshCw className="size-3.5" />} onClick={() => void reprobe()}>
              {tr("安装好了，重新探测", "Installed it, check again")}
            </Button>
          </div>
          {!error && (
            <div className="mt-3 rounded-md border border-line bg-panel/70 p-3">
              <FfmpegGuide platform={caps.platform} />
            </div>
          )}
          {caps.searched.length > 0 && (
            <details className="mt-3 text-xs">
              <summary className="cursor-default text-muted hover:text-fg">
                {tr(`查找过的位置（${caps.searched.length}）`, `Places searched (${caps.searched.length})`)}
              </summary>
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

/** 编译能力卡片：缺关键库时直接给出换构建的指引 */
function BuildCard({ caps, missing }: { caps: Capabilities; missing: BuildFlag[] }) {
  const [guide, setGuide] = useState(false);
  const key = missing.filter((f) => KEY_FLAGS.includes(f.name));
  return (
    <Card title={tr("编译能力", "Build features")} icon={<Wrench className="size-4" />}>
      <p className="text-xs">
        <span className={cn("font-medium", missing.some((f) => f.name !== "libfdk_aac") ? "text-warn" : "text-ok")}>
          {tr(
            `${caps.buildFlags.length - missing.length}/${caps.buildFlags.length} 项具备`,
            `${caps.buildFlags.length - missing.length} of ${caps.buildFlags.length} present`,
          )}
        </span>
        {missing.length > 0 && (
          <span className="text-muted">
            {tr("，缺 ", "; missing ")}
            {missing.map((f) => f.name).join(tr("、", ", "))}
            {missing.length === 1 &&
              missing[0]?.name === "libfdk_aac" &&
              tr("（官方构建均不含，不影响功能）", " (absent from every official build; nothing is lost)")}
          </span>
        )}
      </p>
      {key.length > 0 && (
        <div className="mt-2.5 rounded-md border border-warn/35 bg-warn/[0.07] px-3 py-2 text-xs">
          <p>
            {tr(
              `缺少 ${key.map((f) => f.name).join("、")}，影响：${key.map((f) => f.affects).join("；")}。`,
              `Missing ${key.map((f) => f.name).join(", ")}, which affects: ${key.map((f) => f.affects).join("; ")}.`,
            )}
          </p>
          <button className="mt-1 text-accent hover:underline" onClick={() => setGuide(!guide)}>
            {guide ? tr("收起", "Hide") : tr("换一套功能完整的构建", "Switch to a complete build")}
          </button>
          {guide && (
            <div className="mt-2">
              <FfmpegGuide platform={caps.platform} />
            </div>
          )}
        </div>
      )}
      <details className="mt-2.5 text-xs">
        <summary className="cursor-default text-muted hover:text-fg">{tr("查看全部与影响的功能", "Show all and what they affect")}</summary>
        <ul className="mt-2 space-y-1.5">
          {caps.buildFlags.map((f) => (
            <li key={f.name} className="flex items-start gap-2">
              {f.present ? <CircleCheck className="mt-px size-3.5 shrink-0 text-ok" /> : <CircleX className="mt-px size-3.5 shrink-0 text-subtle" />}
              <span className={cn("w-24 shrink-0 font-mono", !f.present && "text-subtle")}>{f.name}</span>
              <span className="text-muted">{f.affects}</span>
            </li>
          ))}
        </ul>
      </details>
    </Card>
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
  const list = (xs: string[]) => xs.join(tr("、", ", "));

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">{tr("环境与硬件", "Environment")}</h1>
        {caps.probedAt && (
          <span className="truncate text-xs text-subtle">
            {tr("上次探测：", "Last checked: ")}
            {new Date(caps.probedAt).toLocaleString()}
          </span>
        )}
        <Button
          size="sm"
          className="ml-auto shrink-0"
          icon={probing ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
          onClick={() => void reprobe()}
          disabled={probing}
        >
          {probing ? tr("探测中…", "Checking…") : tr("重新探测", "Check again")}
        </Button>
      </header>
      {probing && (
        <div className="flex items-center gap-3 border-b border-line bg-sunken/60 px-5 py-2 text-xs text-muted" aria-live="polite">
          <span className="shrink-0">
            {progress
              ? tr(`${progress.stage}（${progress.done}/${progress.total}）`, `${progress.stage} (${progress.done}/${progress.total})`)
              : tr("正在探测环境…", "Checking environment…")}
          </span>
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
                        {tr("恢复自动查找", "Back to automatic")}
                      </Button>
                    )}
                    <Button size="xs" variant="ghost" icon={<FolderOpen className="size-3" />} onClick={() => void pick()}>
                      {tr("更换", "Change")}
                    </Button>
                  </div>
                )
              }
              className="lg:col-span-2"
            >
              <div className="flex flex-wrap items-start gap-x-10 gap-y-3">
                <div>
                  <div className="text-[11px] text-subtle">{tr("版本", "Version")}</div>
                  <div className="mt-0.5 flex items-center gap-2">
                    <span className="font-mono text-[18px] font-semibold">{caps.versionNumber}</span>
                    {caps.status === "ready" ? (
                      <Badge tone="ok">{tr("满足全部 v1 功能（需 ≥ 7.1）", "Meets every v1 feature (needs ≥ 7.1)")}</Badge>
                    ) : (
                      <Badge tone="danger">{tr("低于 7.1，请升级", "Older than 7.1, please upgrade")}</Badge>
                    )}
                  </div>
                  <div className="mt-0.5 text-xs text-muted">{caps.buildSource}</div>
                </div>
                <div className="min-w-0 flex-1">
                  <div className="text-[11px] text-subtle">
                    {tr("路径", "Path")}
                    {caps.locateSource && <span className="ml-1.5">· {locateLabel(caps.locateSource)}</span>}
                  </div>
                  <div className="selectable mt-0.5 font-mono text-xs break-all">{caps.ffmpegPath}</div>
                  <div className="selectable font-mono text-xs break-all text-muted">{caps.ffprobePath}</div>
                </div>
                <div>
                  <div className="text-[11px] text-subtle">{tr("显卡", "GPU")}</div>
                  {caps.gpus.length === 0 && <div className="mt-0.5 text-xs text-muted">{tr("未识别", "Not detected")}</div>}
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
              title={tr("硬件编码能力", "Hardware encoding")}
              icon={<MonitorCog className="size-4" />}
              className="lg:col-span-2"
              aside={
                <span className="hidden text-[11px] text-subtle sm:inline">
                  {tr("每个编码器都做过 3 帧真实试编码，而不只是查列表", "Every encoder was verified with a real 3-frame test encode, not just a list lookup")}
                </span>
              }
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
                          <div className="font-medium">{vendorLabel(v)}</div>
                          <div className="text-[11px] text-subtle">
                            {v === "software" ? tr("总是可用，画质最好", "Always available, best quality") : tr("速度快 5–10 倍", "5–10× faster")}
                          </div>
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
                  <summary className="cursor-default text-muted">
                    {tr(`不可用的原因（${unusable.length}）`, `Why they are unavailable (${unusable.length})`)}
                  </summary>
                  <ul className="mt-2 space-y-1 font-mono text-[11px] text-subtle">
                    {[...new Set(unusable.map((e) => e.error))].map((err) => (
                      <li key={err} className="selectable break-all">
                        {err}
                      </li>
                    ))}
                  </ul>
                  <ul className="mt-2 space-y-0.5 text-muted">
                    {failureKinds.map((k) => {
                      const why = failureExplain(k);
                      return why ? (
                        <li key={k}>
                          <span className="font-medium">{failureLabel(k)}</span>
                          {tr("：", ": ")}
                          {why}
                        </li>
                      ) : null;
                    })}
                  </ul>
                </details>
              )}
            </Card>
          )}

          {found && <BuildCard caps={caps} missing={missing} />}

          {found && (
            <div className="flex flex-col gap-4">
              <Card title={tr("色调映射（HDR 转 SDR）", "Tone mapping (HDR to SDR)")} icon={<Sparkles className="size-4" />}>
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
                            {tr("默认", "Default")}
                          </Badge>
                        )}
                        <p className="text-muted [overflow-wrap:anywhere]">{t.note}</p>
                      </div>
                    </li>
                  ))}
                </ol>
                {!caps.tonemap.some((t) => t.available) && (
                  <p className="mt-3 text-[11px] text-warn">
                    {tr(
                      "没有可用的色调映射管线，HDR 素材只能保持 HDR 输出，“转为 SDR”会置灰。",
                      "No tone mapping pipeline is available, so HDR footage can only stay HDR and \"Convert to SDR\" is disabled.",
                    )}
                  </p>
                )}
              </Card>

              <Card title={tr("杜比视界与外部工具", "Dolby Vision & external tools")} icon={<Sparkles className="size-4" />}>
                <ul className="space-y-1.5 text-xs">
                  <li className="flex items-center gap-2">
                    {caps.dolbyVisionEncode ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-danger" />}
                    <span>{tr("杜比视界单层保留", "Keep single-layer Dolby Vision")}</span>
                    <span className="text-muted">{tr("libx265 -dolbyvision（ffmpeg ≥ 7.1）", "libx265 -dolbyvision (ffmpeg ≥ 7.1)")}</span>
                  </li>
                  <li className="flex items-center gap-2">
                    {caps.doviSplit ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-subtle" />}
                    <span>{tr("Profile 7 拆层", "Split Profile 7 layers")}</span>
                    <span className="text-muted">{tr("dovi_split（ffmpeg ≥ 9.0，v2 使用）", "dovi_split (ffmpeg ≥ 9.0, used in v2)")}</span>
                  </li>
                  {caps.external.map((x) => (
                    <li key={x.name} className="flex items-center gap-2" title={x.path}>
                      {x.found ? <CircleCheck className="size-3.5 text-ok" /> : <CircleX className="size-3.5 text-subtle" />}
                      <span className="font-mono">{x.name}</span>
                      <span className="text-muted">{x.purpose}</span>
                      <span className="ml-auto text-[11px] text-subtle">
                        {x.found ? (x.version ?? tr("已安装", "Installed")) : tr("未安装 · 可选", "Not installed · optional")}
                      </span>
                    </li>
                  ))}
                </ul>
                <div className="mt-3 space-y-1 border-t border-line pt-3 text-[11px] text-subtle">
                  <p>
                    {tr("可初始化的硬件设备：", "Hardware devices that initialize: ")}
                    {devicesOk.length ? list(devicesOk.map((d) => d.id)) : tr("无", "none")}
                  </p>
                  {devicesBad.length > 0 && (
                    <p title={devicesBad.map((d) => `${d.id}: ${d.error ?? ""}`).join("\n")}>
                      {tr("不可用：", "Unavailable: ")}
                      {list(devicesBad.map((d) => d.id))}
                      {tr("（悬停查看原因）", " (hover for the reason)")}
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
