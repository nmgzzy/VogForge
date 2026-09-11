import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/cn";
import type { FeatureTone } from "@/lib/media-features";

// ───────────────────────── Button ─────────────────────────

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "warn";

const BUTTON_VARIANT: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover shadow-sm",
  secondary: "bg-raised text-fg border border-line hover:border-line-strong hover:bg-sunken",
  ghost: "text-muted hover:text-fg hover:bg-raised",
  danger: "text-danger hover:bg-danger/10",
  warn: "bg-warn/15 text-warn border border-warn/30 hover:bg-warn/25",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "xs" | "sm" | "md";
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon, className, children, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      className={cn(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap transition-colors",
        "disabled:pointer-events-none disabled:opacity-45",
        size === "xs" && "h-6 px-2 text-xs",
        size === "sm" && "h-7 px-2.5 text-xs",
        size === "md" && "h-8 px-3 text-[13px]",
        !children && icon && (size === "md" ? "w-8 px-0" : size === "sm" ? "w-7 px-0" : "w-6 px-0"),
        BUTTON_VARIANT[variant],
        className,
      )}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
});

// ───────────────────────── Badge ─────────────────────────

export type BadgeTone = FeatureTone | "accent" | "ok" | "warn" | "danger";

const BADGE_TONE: Record<BadgeTone, string> = {
  neutral: "bg-raised text-muted border-line",
  accent: "bg-accent/12 text-accent border-accent/25",
  ok: "bg-ok/12 text-ok border-ok/25",
  warn: "bg-warn/14 text-warn border-warn/30",
  danger: "bg-danger/12 text-danger border-danger/25",
  hdr: "bg-hdr/14 text-hdr border-hdr/30",
  dv: "bg-dv/14 text-dv border-dv/30",
  atmos: "bg-atmos/14 text-atmos border-atmos/30",
  lossless: "bg-lossless/14 text-lossless border-lossless/30",
  vfr: "bg-vfr/14 text-vfr border-vfr/30",
};

export function Badge({
  tone = "neutral",
  children,
  title,
  className,
  icon,
}: {
  tone?: BadgeTone;
  children: ReactNode;
  title?: string;
  className?: string;
  icon?: ReactNode;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex h-5 shrink-0 items-center gap-1 rounded border px-1.5 text-[11px] leading-none font-medium whitespace-nowrap",
        BADGE_TONE[tone],
        className,
      )}
    >
      {icon}
      {children}
    </span>
  );
}

// ───────────────────────── Segmented ─────────────────────────

export interface SegmentOption<T extends string> {
  value: T;
  label: ReactNode;
  disabled?: boolean;
  title?: string;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  className,
  size = "md",
}: {
  value: T;
  options: SegmentOption<T>[];
  onChange: (v: T) => void;
  className?: string;
  size?: "sm" | "md";
}) {
  return (
    <div
      role="radiogroup"
      className={cn("inline-flex rounded-md border border-line bg-sunken p-0.5", className)}
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            role="radio"
            aria-checked={active}
            disabled={o.disabled}
            title={o.title}
            onClick={() => onChange(o.value)}
            className={cn(
              "flex-1 rounded-[5px] px-2.5 font-medium whitespace-nowrap transition-colors",
              size === "md" ? "h-7 text-[12.5px]" : "h-6 text-xs",
              active ? "bg-panel text-fg shadow-sm" : "text-muted hover:text-fg",
              "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:text-muted",
            )}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

// ───────────────────────── Select ─────────────────────────

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export function Select({
  value,
  options,
  onChange,
  className,
  mono,
}: {
  value: string;
  options: SelectOption[];
  onChange: (v: string) => void;
  className?: string;
  mono?: boolean;
}) {
  return (
    <div className={cn("relative", className)}>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={cn(
          "h-8 w-full appearance-none rounded-md border border-line bg-panel pr-7 pl-2.5 text-[13px] text-fg",
          "transition-colors hover:border-line-strong focus:border-accent focus:outline-none",
          mono && "font-mono text-xs",
        )}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-subtle" />
    </div>
  );
}

// ───────────────────────── Switch ─────────────────────────

export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors",
        checked ? "border-accent bg-accent" : "border-line-strong bg-sunken",
        "disabled:opacity-45",
      )}
    >
      <span
        className={cn(
          "absolute size-3 rounded-full shadow-sm transition-transform",
          checked ? "translate-x-[15px] bg-accent-fg" : "translate-x-[2px] bg-subtle",
        )}
      />
    </button>
  );
}

// ───────────────────────── Checkbox ─────────────────────────

export function Checkbox({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "flex size-4 shrink-0 items-center justify-center rounded border transition-colors",
        checked ? "border-accent bg-accent text-accent-fg" : "border-line-strong bg-panel hover:border-accent",
        "disabled:cursor-not-allowed disabled:opacity-40",
      )}
    >
      {checked && (
        <svg viewBox="0 0 12 12" className="size-3" aria-hidden>
          <path d="M2.5 6.2 5 8.6 9.5 3.6" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
    </button>
  );
}

// ───────────────────────── Layout helpers ─────────────────────────

export function Section({
  step,
  title,
  aside,
  children,
  className,
}: {
  step?: number | string;
  title: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("rounded-lg border border-line bg-panel", className)}>
      <header className="flex min-h-10 items-center gap-2 border-b border-line px-4 py-2">
        {step !== undefined && (
          <span className="flex size-5 items-center justify-center rounded-full bg-accent/12 text-[11px] font-semibold text-accent tabular">
            {step}
          </span>
        )}
        <h2 className="text-[13px] font-semibold">{title}</h2>
        {aside && <div className="ml-auto flex items-center gap-2">{aside}</div>}
      </header>
      <div className="px-4 py-3.5">{children}</div>
    </section>
  );
}

export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-xs font-medium text-muted">{label}</span>
        {hint && <span className="truncate text-[11px] text-subtle">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

export function Mono({ children, className }: { children: ReactNode; className?: string }) {
  return <code className={cn("font-mono text-[11.5px] text-muted", className)}>{children}</code>;
}

export function Empty({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 px-6 py-12 text-center">
      <div className="flex size-12 items-center justify-center rounded-xl bg-raised text-subtle">{icon}</div>
      <div>
        <p className="font-medium">{title}</p>
        {description && <p className="mt-1 max-w-sm text-xs text-muted">{description}</p>}
      </div>
      {action}
    </div>
  );
}

export function ProgressBar({ value, live, tone = "accent" }: { value: number; live?: boolean; tone?: "accent" | "ok" | "danger" | "muted" }) {
  const color = { accent: "bg-accent", ok: "bg-ok", danger: "bg-danger", muted: "bg-line-strong" }[tone];
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-sunken">
      <div
        className={cn("h-full rounded-full transition-[width] duration-500 ease-out", color, live && "progress-live")}
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  );
}
