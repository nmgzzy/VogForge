import { useState, type ReactNode } from "react";
import { CircleCheck, CircleX, Loader2, TriangleAlert } from "lucide-react";
import { tr } from "@/i18n";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, vendorLabel } from "@/lib/encoders";
import type { Capabilities, Codec, Lang, Vendor } from "@/lib/types";
import { useCapabilities } from "@/stores/capability";
import { useSettings } from "@/stores/settings";
import { FfmpegGuide } from "@/views/EnvironmentView";
import { LogoMark } from "./Sidebar";
import { Button, ProgressBar, Select } from "./ui";

const CODECS: Codec[] = ["h264", "hevc", "av1"];

/** 缺了会明显影响功能的编译开关，与环境页一致 */
const KEY_FLAGS = ["libx265", "libsvtav1", "libplacebo", "libzimg"];

function Line({ ok, warn, children }: { ok: boolean; warn?: boolean; children: ReactNode }) {
  const Icon = ok ? CircleCheck : warn ? TriangleAlert : CircleX;
  return (
    <li className="flex items-start gap-2">
      <Icon className={cn("mt-px size-4 shrink-0", ok ? "text-ok" : warn ? "text-warn" : "text-subtle")} />
      <span className="min-w-0">{children}</span>
    </li>
  );
}

/** 各厂商能用的硬件编码格式，例如 "Intel QSV：H.264、HEVC 10bit" */
function gpuSummary(caps: Capabilities): { vendor: Vendor; codecs: string }[] {
  const vendors = [...new Set(caps.encoders.filter((e) => e.usable && e.vendor !== "software").map((e) => e.vendor))];
  return vendors.map((vendor) => ({
    vendor,
    codecs: caps.encoders
      .filter((e) => e.vendor === vendor && e.usable)
      .map((e) => `${CODEC_LABEL[e.codec]}${e.tenBit ? " 10bit" : ""}`)
      .join(tr("、", ", ")),
  }));
}

function CheckStep({ caps, probing }: { caps: Capabilities; probing: boolean }) {
  const progress = useCapabilities((s) => s.progress);
  const language = useSettings((s) => s.settings.language);
  const update = useSettings((s) => s.update);
  const ready = caps.status === "ready";
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4 rounded-md border border-line bg-sunken/50 px-3 py-2">
        <span className="text-[13px]">{tr("界面语言", "Interface language")}</span>
        <Select
          className="w-40"
          value={language}
          onChange={(l) => void update({ language: l as Lang })}
          options={[
            { value: "zh-CN", label: "简体中文" },
            { value: "en", label: "English" },
          ]}
        />
      </div>
      {probing || caps.status === "probing" ? (
        <div className="space-y-2 text-[13px] text-muted" aria-live="polite">
          <p className="flex items-center gap-2">
            <Loader2 className="size-4 animate-spin" />
            {progress
              ? tr(`${progress.stage}（${progress.done}/${progress.total}）`, `${progress.stage} (${progress.done}/${progress.total})`)
              : tr("正在查找 ffmpeg…", "Looking for ffmpeg…")}
          </p>
          <ProgressBar live value={progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 5} />
          <p className="text-xs text-subtle">
            {tr(
              "第一次会逐个试编码验证 GPU 能力，大约需要十几秒；之后启动直接读缓存。",
              "The first check test-encodes with every GPU encoder, which takes a few seconds; later launches read the cache.",
            )}
          </p>
        </div>
      ) : ready ? (
        <ul className="space-y-2 text-[13px]">
          <Line ok>
            {tr(`找到 ffmpeg ${caps.versionNumber}`, `Found ffmpeg ${caps.versionNumber}`)}
            <span className="text-muted">{tr(`（${caps.buildSource}）`, ` (${caps.buildSource})`)}</span>
            <div className="selectable font-mono text-[11px] break-all text-subtle">{caps.ffmpegPath}</div>
          </Line>
          <Line ok={caps.gpus.length > 0} warn>
            {caps.gpus.length > 0
              ? tr(`显卡：${caps.gpus.map((g) => g.name).join("、")}`, `GPU: ${caps.gpus.map((g) => g.name).join(", ")}`)
              : tr("没有识别到显卡，转码会使用 CPU", "No GPU detected; encoding uses the CPU")}
          </Line>
        </ul>
      ) : (
        <div className="space-y-3">
          <p className="flex items-start gap-2 text-[13px]">
            <TriangleAlert className="mt-px size-4 shrink-0 text-warn" />
            {caps.statusDetail}
          </p>
          <FfmpegGuide platform={caps.platform} />
        </div>
      )}
    </div>
  );
}

function AbilityStep({ caps }: { caps: Capabilities }) {
  const gpus = gpuSummary(caps);
  const tonemap = caps.tonemap.find((t) => t.available);
  const soft = (c: Codec) => caps.encoders.some((e) => e.codec === c && e.vendor === "software" && e.usable);
  const softList = CODECS.filter(soft).map((c) => CODEC_LABEL[c]);
  return (
    <ul className="space-y-2.5 text-[13px]">
      <Line ok={softList.length === CODECS.length} warn>
        {tr("CPU 编码：", "CPU encoding: ")}
        {softList.length ? softList.join(tr("、", ", ")) : tr("不可用", "unavailable")}
        <div className="text-xs text-muted">{tr("画质最好，适合归档与收藏", "Best quality, suited to archiving and collections")}</div>
      </Line>
      <Line ok={gpus.length > 0} warn>
        {gpus.length > 0 ? (
          gpus.map((g) => (
            <div key={g.vendor}>
              {vendorLabel(g.vendor)}
              {tr("：", ": ")}
              {g.codecs}
            </div>
          ))
        ) : (
          tr("没有可用的 GPU 编码器", "No usable GPU encoder")
        )}
        <div className="text-xs text-muted">
          {tr("启动时逐个真实试编码验证过，不是只查列表", "Each was verified with a real test encode at startup, not just a list lookup")}
        </div>
      </Line>
      <Line ok={!!tonemap} warn>
        {tonemap
          ? tr(`HDR 转 SDR：${tonemap.id}`, `HDR to SDR: ${tonemap.id}`)
          : tr("HDR 转 SDR：不可用，HDR 素材只能保持 HDR", "HDR to SDR: unavailable; HDR footage stays HDR")}
      </Line>
      <Line ok={caps.dolbyVisionEncode} warn>
        {caps.dolbyVisionEncode
          ? tr("杜比视界：CPU 编码时可以保留", "Dolby Vision: kept when encoding on the CPU")
          : tr("杜比视界：当前 ffmpeg 无法写入", "Dolby Vision: this ffmpeg cannot write it")}
      </Line>
      <Line ok>
        {tr("每个任务完成后都用 ffprobe 逐项核对结果", "Every job is checked item by item with ffprobe when it finishes")}
      </Line>
    </ul>
  );
}

function adviceFor(caps: Capabilities): string[] {
  if (caps.status !== "ready") {
    return [tr("先按第一步安装 ffmpeg 7.1 或更高版本，再回到这里。", "Install ffmpeg 7.1 or newer as shown in the first step, then come back.")];
  }
  const out: string[] = [];
  const missing = caps.buildFlags.filter((f) => !f.present && KEY_FLAGS.includes(f.name));
  if (missing.length > 0) {
    out.push(
      tr(
        `当前 ffmpeg 缺少 ${missing.map((f) => f.name).join("、")}，建议换一套功能完整的构建（环境页有下载指引）。`,
        `This ffmpeg lacks ${missing.map((f) => f.name).join(", ")}; switch to a complete build (the Environment page shows how).`,
      ),
    );
  }
  const gpus = gpuSummary(caps);
  if (gpus.length > 0) {
    const names = gpus.map((g) => vendorLabel(g.vendor)).join(tr("、", ", "));
    out.push(
      tr(
        `流媒体、手机、社交场景会自动用 ${names} 加速；归档与收藏默认用 CPU，同体积下画质更好。`,
        `Streaming, phone and social scenarios use ${names} automatically; archive and collection use the CPU for better quality per size.`,
      ),
    );
  } else {
    out.push(
      tr(
        "没有可用的 GPU 编码器，转码全部使用 CPU。高分辨率长片可以放进队列里夜间运行。",
        "With no GPU encoder, everything encodes on the CPU. Queue long high-resolution videos to run overnight.",
      ),
    );
  }
  out.push(
    tr(
      "把视频或整个文件夹拖进窗口开始。每个文件会按特征推荐用途与参数，并告诉你哪些信息能保留。",
      "Drop videos or whole folders into the window to start. Each file gets a recommended purpose and settings, plus what can be kept.",
    ),
  );
  return out;
}

/** 首次启动引导（需求 F-9.5）：检查环境 → 说明能力 → 给出建议。走完或跳过后不再出现，设置页可以重新打开 */
export function Onboarding() {
  const loaded = useSettings((s) => s.loaded);
  const onboarded = useSettings((s) => s.settings.onboarded);
  const update = useSettings((s) => s.update);
  const caps = useCapabilities((s) => s.caps);
  const probing = useCapabilities((s) => s.probing);
  const [step, setStep] = useState(0);
  if (!loaded || onboarded) return null;

  const steps = [tr("检查环境", "Check environment"), tr("能力说明", "What it can do"), tr("建议", "Suggestions")];
  const finish = () => void update({ onboarded: true });
  const last = step === steps.length - 1;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" role="dialog" aria-modal aria-label={tr("入门引导", "Getting started")}>
      <div className="flex max-h-full w-[560px] max-w-full flex-col overflow-hidden rounded-xl border border-line bg-panel shadow-card">
        <header className="flex items-center gap-3 border-b border-line px-5 py-4">
          <LogoMark className="size-8" />
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold">{tr("欢迎使用 VidForge", "Welcome to VidForge")}</h2>
            <p className="text-xs text-muted">
              {tr("先花半分钟看看这台电脑能做什么。", "Take half a minute to see what this computer can do.")}
            </p>
          </div>
          <button className="text-xs text-subtle hover:text-fg" onClick={finish}>
            {tr("跳过", "Skip")}
          </button>
        </header>
        <ol className="flex gap-1 border-b border-line px-5 py-2.5 text-xs">
          {steps.map((s, i) => (
            <li key={s} className={cn("flex items-center gap-1.5", i === step ? "font-medium text-fg" : "text-subtle")}>
              <span
                className={cn(
                  "flex size-5 items-center justify-center rounded-full text-[10.5px]",
                  i === step ? "bg-accent text-accent-fg" : i < step ? "bg-ok/15 text-ok" : "bg-raised",
                )}
              >
                {i + 1}
              </span>
              {s}
              {i < steps.length - 1 && <span className="mx-1.5 text-subtle">›</span>}
            </li>
          ))}
        </ol>
        <div className="min-h-[220px] flex-1 overflow-y-auto px-5 py-4">
          {step === 0 && <CheckStep caps={caps} probing={probing} />}
          {step === 1 && <AbilityStep caps={caps} />}
          {step === 2 && (
            <ul className="list-disc space-y-2 pl-5 text-[13px] marker:text-subtle">
              {adviceFor(caps).map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
        </div>
        <footer className="flex items-center gap-2 border-t border-line px-5 py-3">
          {step > 0 && (
            <Button size="sm" variant="ghost" onClick={() => setStep(step - 1)}>
              {tr("上一步", "Back")}
            </Button>
          )}
          <Button size="sm" variant="primary" className="ml-auto" onClick={last ? finish : () => setStep(step + 1)}>
            {last ? tr("开始使用", "Get started") : tr("下一步", "Next")}
          </Button>
        </footer>
      </div>
    </div>
  );
}
