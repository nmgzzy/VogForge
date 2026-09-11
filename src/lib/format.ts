/** 界面展示用的格式化函数。全部为纯函数，便于测试。 */

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
  const v = bytes / 1024 ** i;
  // 小于 10 的数多给一位小数，避免 "1 GB" 这种信息量不足的显示
  const d = i === 0 ? 0 : v < 10 ? digits + 1 : digits;
  return `${v.toFixed(d)} ${UNITS[i]}`;
}

export function formatDuration(totalSec: number): string {
  if (!Number.isFinite(totalSec) || totalSec < 0) return "--:--";
  const s = Math.floor(totalSec % 60);
  const m = Math.floor((totalSec / 60) % 60);
  const h = Math.floor(totalSec / 3600);
  const mm = String(m).padStart(h > 0 ? 2 : 1, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** 剩余时间用更口语的形式：「约 3 分钟」 */
export function formatEta(sec: number | undefined): string {
  if (sec === undefined || !Number.isFinite(sec)) return "计算中";
  if (sec < 60) return `约 ${Math.max(1, Math.round(sec))} 秒`;
  if (sec < 3600) return `约 ${Math.round(sec / 60)} 分钟`;
  const h = Math.floor(sec / 3600);
  const m = Math.round((sec % 3600) / 60);
  return m > 0 ? `约 ${h} 小时 ${m} 分` : `约 ${h} 小时`;
}

/** 耗时区间用口语化单位，两端同单位时合并：「4–8 分钟」「10.1–18.9 小时」 */
export function formatTimeRange(a: number, b: number): string {
  const unit = (s: number) => (s < 60 ? "秒" : s < 3600 ? "分钟" : "小时");
  const val = (s: number) =>
    s < 60 ? Math.max(1, Math.round(s)) : s < 3600 ? Math.max(1, Math.round(s / 60)) : Math.round(s / 360) / 10;
  const ua = unit(a);
  const ub = unit(b);
  return ua === ub ? `${val(a)}–${val(b)} ${ub}` : `${val(a)} ${ua} – ${val(b)} ${ub}`;
}

export function formatBitrate(bps: number | undefined): string {
  if (!bps || bps <= 0) return "—";
  if (bps >= 1_000_000) return `${(bps / 1_000_000).toFixed(bps >= 10_000_000 ? 0 : 1)} Mbps`;
  return `${Math.round(bps / 1000)} kbps`;
}

/** 29.97 保留两位，30 显示整数 */
export function formatFps(fps: number): string {
  if (!Number.isFinite(fps) || fps <= 0) return "—";
  const r = Math.round(fps);
  return Math.abs(fps - r) < 0.005 ? String(r) : fps.toFixed(2);
}

export function resolutionLabel(width: number, height: number): string {
  // 以短边判定，兼容竖屏视频
  const short = Math.min(width, height);
  if (short >= 2000) return "4K";
  if (short >= 1400) return "1440p";
  if (short >= 1000) return "1080p";
  if (short >= 700) return "720p";
  if (short >= 470) return "480p";
  return `${short}p`;
}

export function channelLabel(channels: number, layout?: string): string {
  if (layout && /7\.1/.test(layout)) return "7.1";
  if (layout && /5\.1/.test(layout)) return "5.1";
  if (channels === 8) return "7.1";
  if (channels === 6) return "5.1";
  if (channels === 2) return "立体声";
  if (channels === 1) return "单声道";
  return `${channels} 声道`;
}

export function formatPercent(ratio: number): string {
  if (!Number.isFinite(ratio)) return "—";
  return `${Math.round(ratio * 100)}%`;
}

/**
 * 不需要加引号的安全字符集。反斜杠、空格、中文、管道符等都会触发加引号。
 * 刻意不含逗号与 @：PowerShell 里逗号是数组运算符，`a,b` 会被拆成两个参数，
 * 滤镜链 `zscale=...,format=...` 不加引号就会被破坏；@ 开头会被当成 splatting。
 */
const SAFE_ARG = /^[A-Za-z0-9_\-.:=+/%]+$/;

export type Shell = "powershell" | "posix";

/**
 * 为复制到终端而加引号。PowerShell 与 bash/zsh 的单引号都是完全字面的，
 * 只有内嵌单引号的转义方式不同：PowerShell 写两个，POSIX 用 '\'' 拼接。
 */
export function quoteArg(a: string, shell: Shell = "powershell"): string {
  if (a !== "" && SAFE_ARG.test(a)) return a;
  const inner = shell === "powershell" ? a.replace(/'/g, "''") : a.replace(/'/g, "'\\''");
  return `'${inner}'`;
}

/**
 * 把用户输入的附加参数拆成 argv。支持单引号（完全字面）与双引号（内部只认 \" 和 \\ 两种转义）。
 * 引号外的反斜杠保持字面，这样 Windows 路径 C:\sub\a.srt 不会被吃掉。未闭合的引号宽松处理为到结尾。
 */
export function splitArgs(input: string): string[] {
  const out: string[] = [];
  let cur = "";
  let inToken = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < input.length; i++) {
    const c = input[i]!;
    if (quote) {
      if (c === quote) {
        quote = null;
      } else if (quote === '"' && c === "\\" && (input[i + 1] === '"' || input[i + 1] === "\\")) {
        cur += input[++i];
      } else {
        cur += c;
      }
      continue;
    }
    if (c === '"' || c === "'") {
      quote = c;
      inToken = true;
    } else if (/\s/.test(c)) {
      if (inToken) out.push(cur);
      cur = "";
      inToken = false;
    } else {
      cur += c;
      inToken = true;
    }
  }
  if (inToken) out.push(cur);
  return out;
}

export function argsToCommand(args: readonly string[], shell: Shell = "powershell"): string {
  return args.map((a) => quoteArg(a, shell)).join(" ");
}
