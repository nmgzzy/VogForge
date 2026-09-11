import { Bookmark, Clapperboard, Cpu, ListVideo, Monitor, Moon, Settings, Sun, type LucideIcon } from "lucide-react";
import { backend } from "@/backend";
import { cn } from "@/lib/cn";
import type { EnvStatus } from "@/lib/types";
import { useCapabilities } from "@/stores/capability";
import { useQueue } from "@/stores/queue";
import { useUi, type ThemePref, type View } from "@/stores/ui";

const NAV: { id: View; label: string; icon: LucideIcon }[] = [
  { id: "transcode", label: "转码", icon: Clapperboard },
  { id: "queue", label: "任务队列", icon: ListVideo },
  { id: "environment", label: "环境与硬件", icon: Cpu },
  { id: "presets", label: "预设", icon: Bookmark },
  { id: "settings", label: "设置", icon: Settings },
];

export function LogoMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden>
      <defs>
        <linearGradient id="vf-logo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style={{ stopColor: "var(--vf-accent)" }} />
          <stop offset="1" style={{ stopColor: "var(--vf-dv)" }} />
        </linearGradient>
      </defs>
      <rect width="32" height="32" rx="8" fill="url(#vf-logo)" />
      <path d="M12 9.2v13.6L22.6 16z" fill="#fff" />
      <path d="M7.5 12h2.6M6.5 16h3.6M7.5 20h2.6" stroke="#fff" strokeWidth="1.7" strokeLinecap="round" opacity=".7" />
    </svg>
  );
}

/** 侧栏底部的环境状态：正常时安静的绿点，出问题才醒目 */
export const ENV_STATUS: Record<EnvStatus, { label: string; dot: string; pulse: boolean; hint: string }> = {
  ready: { label: "环境就绪", dot: "bg-ok", pulse: true, hint: "" },
  probing: { label: "正在探测环境…", dot: "bg-accent", pulse: true, hint: "探测完成前先按 CPU 软编给方案" },
  missing: { label: "未找到 ffmpeg", dot: "bg-danger", pulse: false, hint: "点此查看如何安装" },
  too_old: { label: "ffmpeg 版本过低", dot: "bg-warn", pulse: false, hint: "需要 7.1 或更高版本" },
  broken: { label: "ffmpeg 无法运行", dot: "bg-danger", pulse: false, hint: "点此查看原因" },
};

/** 调用后端本身失败（不是没找到 ffmpeg） */
export const ENV_ERROR = { label: "环境探测失败", dot: "bg-danger", pulse: false, hint: "点此查看原因并重试" };

const THEME_CYCLE: Record<ThemePref, { next: ThemePref; icon: LucideIcon; label: string }> = {
  system: { next: "light", icon: Monitor, label: "跟随系统" },
  light: { next: "dark", icon: Sun, label: "浅色" },
  dark: { next: "system", icon: Moon, label: "深色" },
};

export function Sidebar() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const caps = useCapabilities((s) => s.caps);
  const probing = useCapabilities((s) => s.probing);
  const error = useCapabilities((s) => s.error);
  const active = useQueue((s) => s.jobs.filter((j) => j.status === "running" || j.status === "queued").length);
  const running = useQueue((s) => s.jobs.filter((j) => j.status === "running").length);

  const hw = caps.encoders.filter((e) => e.vendor !== "software" && e.usable);
  const hwVendors = [...new Set(hw.map((e) => e.vendor))];
  const vendorName = { intel: "Intel QSV", nvidia: "NVENC", amd: "AMF", apple: "VideoToolbox", software: "" };
  const t = THEME_CYCLE[theme];
  const status = error
    ? ENV_ERROR
    : probing && caps.status !== "ready"
      ? ENV_STATUS.probing
      : ENV_STATUS[caps.status];

  return (
    <aside className="flex w-[196px] shrink-0 flex-col border-r border-line bg-sunken">
      <div className="flex h-14 items-center gap-2.5 px-4">
        <LogoMark className="size-7" />
        <div className="leading-tight">
          <div className="text-[14px] font-semibold tracking-tight">VidForge</div>
          <div className="text-[10.5px] text-subtle">{backend.kind === "mock" ? "v0.1 · 浏览器预览" : "v0.1"}</div>
        </div>
      </div>

      <nav className="flex flex-col gap-0.5 px-2 pt-2">
        {NAV.map((n) => {
          const on = view === n.id;
          return (
            <button
              key={n.id}
              onClick={() => setView(n.id)}
              className={cn(
                "group flex h-9 items-center gap-2.5 rounded-md px-2.5 text-[13px] transition-colors",
                on ? "bg-panel font-medium text-fg shadow-sm" : "text-muted hover:bg-raised hover:text-fg",
              )}
            >
              <n.icon className={cn("size-4", on ? "text-accent" : "text-subtle group-hover:text-muted")} />
              <span>{n.label}</span>
              {n.id === "queue" && active > 0 && (
                <span
                  className={cn(
                    "ml-auto flex h-[18px] min-w-[18px] items-center justify-center rounded-full px-1.5 text-[10.5px] font-semibold tabular",
                    running > 0 ? "bg-accent text-accent-fg" : "bg-raised text-muted",
                  )}
                >
                  {active}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      <div className="mt-auto flex flex-col gap-2 p-3">
        <button
          onClick={() => setView("environment")}
          className="rounded-lg border border-line bg-panel p-2.5 text-left transition-colors hover:border-line-strong"
        >
          <div className="flex items-center gap-1.5 text-[11px] text-subtle">
            <span className="relative flex size-1.5">
              {status.pulse && <span className={cn("absolute inline-flex size-full animate-ping rounded-full opacity-60", status.dot)} />}
              <span className={cn("relative inline-flex size-1.5 rounded-full", status.dot)} />
            </span>
            {status.label}
          </div>
          {caps.versionNumber && <div className="mt-1 font-mono text-[11.5px] text-fg">ffmpeg {caps.versionNumber}</div>}
          <div className="mt-0.5 text-[11px] text-muted">
            {caps.status === "ready" && !error
              ? hwVendors.length
                ? `GPU：${hwVendors.map((v) => vendorName[v]).join("、")}`
                : "仅 CPU 软编"
              : status.hint}
          </div>
        </button>

        <button
          onClick={() => setTheme(t.next)}
          title={`主题：${t.label}（点击切换）`}
          className="flex h-8 items-center gap-2 rounded-md px-2.5 text-xs text-muted transition-colors hover:bg-raised hover:text-fg"
        >
          <t.icon className="size-3.5" />
          主题：{t.label}
        </button>
      </div>
    </aside>
  );
}
