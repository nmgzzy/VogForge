import { useState, type DragEvent } from "react";
import {
  Camera,
  CloudDownload,
  Disc3,
  FilePlus2,
  FileVideo,
  FolderPlus,
  MonitorSmartphone,
  Plane,
  Smartphone,
  Trash2,
  Upload,
  X,
  type LucideIcon,
} from "lucide-react";
import type { MediaInfo, SourceHint } from "@/lib/types";
import { cn } from "@/lib/cn";
import { formatBytes, formatDuration, resolutionLabel } from "@/lib/format";
import { mediaFeatures } from "@/lib/media-features";
import { SCENARIOS } from "@/mock/engine";
import { CODEC_LABEL } from "@/mock/engine/encoders";
import { useProject } from "@/stores/project";
import { Badge, Button, Empty } from "./ui";

export const SOURCE_ICON: Record<SourceHint, LucideIcon> = {
  iphone: Smartphone,
  android: Smartphone,
  gopro: Camera,
  dji: Plane,
  camera: Camera,
  screen: MonitorSmartphone,
  bluray: Disc3,
  streaming: CloudDownload,
  unknown: FileVideo,
};

function codecName(codec: string): string {
  return codec in CODEC_LABEL ? CODEC_LABEL[codec as keyof typeof CODEC_LABEL] : codec.toUpperCase();
}

function FileCard({ m, selected, onSelect }: { m: MediaInfo; selected: boolean; onSelect: () => void }) {
  const remove = useProject((s) => s.removeFile);
  const scenario = useProject((s) => s.plans[m.id]?.scenario);
  const v = m.video[0];
  const Icon = SOURCE_ICON[m.sourceHint];
  const features = mediaFeatures(m);
  const shown = features.slice(0, 3);
  const more = features.length - shown.length;
  const scenarioTitle = SCENARIOS.find((s) => s.id === scenario)?.title;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onSelect()}
      className={cn(
        "group relative flex gap-3 rounded-lg border p-2.5 transition-colors",
        selected ? "border-accent/50 bg-accent/[0.06]" : "border-transparent hover:border-line hover:bg-raised/60",
      )}
    >
      {selected && <span className="absolute top-2.5 bottom-2.5 left-0 w-[3px] rounded-full bg-accent" />}
      <div
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-md",
          selected ? "bg-accent/15 text-accent" : "bg-raised text-subtle",
        )}
      >
        <Icon className="size-[18px]" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 pr-1">
          <p className="min-w-0 flex-1 truncate text-[13px] font-medium" title={m.name}>
            {m.name}
          </p>
          {scenarioTitle && (
            <span className="shrink-0 text-[10.5px] text-subtle group-focus-within:invisible group-hover:invisible">{scenarioTitle}</span>
          )}
        </div>
        <p className="mt-0.5 truncate text-[11.5px] text-muted tabular">
          {v ? `${resolutionLabel(v.width, v.height)} · ${codecName(v.codec)} · ` : ""}
          {formatDuration(m.durationSec)} · {formatBytes(m.sizeBytes)}
        </p>
        {shown.length > 0 && (
          <div className="mt-1.5 flex flex-wrap items-center gap-1">
            {shown.map((f) => (
              <Badge key={f.key} tone={f.tone} title={f.title}>
                {f.label}
              </Badge>
            ))}
            {more > 0 && <Badge title={features.slice(3).map((f) => f.label).join("、")}>+{more}</Badge>}
          </div>
        )}
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          remove(m.id);
        }}
        title="从列表移除（不会删除文件）"
        aria-label={`从列表移除 ${m.name}`}
        className="absolute top-2 right-2 flex size-6 items-center justify-center rounded text-subtle opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-sunken hover:text-fg focus-visible:opacity-100"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

export function FileList() {
  const files = useProject((s) => s.files);
  const selectedId = useProject((s) => s.selectedId);
  const select = useProject((s) => s.select);
  const loadSamples = useProject((s) => s.loadSamples);
  const clear = useProject((s) => s.clear);
  const [dragging, setDragging] = useState(false);
  const [notice, setNotice] = useState<string>();

  const total = files.reduce((n, f) => n + f.sizeBytes, 0);

  // 浏览器预览无法读取本地文件的真实路径，拖入时载入示例素材；Tauri 环境下由后端 probe
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    loadSamples();
    setNotice("浏览器预览模式无法读取本地文件，已载入示例素材。桌面版会直接分析拖入的文件。");
    window.setTimeout(() => setNotice(undefined), 5000);
  };

  return (
    <div
      className="relative flex w-[288px] shrink-0 flex-col border-r border-line bg-bg"
      onDragOver={(e) => {
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDragging(false);
      }}
      onDrop={onDrop}
    >
      <header className="flex h-12 items-center gap-2 border-b border-line px-3">
        <h2 className="text-[13px] font-semibold">源文件</h2>
        {files.length > 0 && <span className="text-xs text-subtle tabular">{files.length}</span>}
        <div className="ml-auto flex gap-1">
          <Button size="sm" variant="ghost" icon={<FilePlus2 className="size-3.5" />} onClick={loadSamples} title="添加文件">
            文件
          </Button>
          <Button size="sm" variant="ghost" icon={<FolderPlus className="size-3.5" />} onClick={loadSamples} title="添加文件夹（可递归）">
            文件夹
          </Button>
        </div>
      </header>

      {notice && (
        <div className="mx-3 mt-3 rounded-md border border-accent/30 bg-accent/10 px-3 py-2 text-xs text-accent">{notice}</div>
      )}

      <div className="flex-1 overflow-y-auto p-2">
        {files.length === 0 ? (
          <Empty
            icon={<Upload className="size-5" />}
            title="拖入视频或文件夹"
            description="支持 MP4 / MOV / MKV / M2TS 等常见格式。文件夹会递归扫描，可按扩展名过滤。"
            action={
              <Button size="sm" variant="primary" onClick={loadSamples}>
                载入示例素材
              </Button>
            }
          />
        ) : (
          <div className="flex flex-col gap-1">
            {files.map((m) => (
              <FileCard key={m.id} m={m} selected={m.id === selectedId} onSelect={() => select(m.id)} />
            ))}
          </div>
        )}
      </div>

      {files.length > 0 && (
        <footer className="flex h-10 items-center gap-2 border-t border-line px-3 text-[11.5px] text-muted">
          <span className="tabular">共 {formatBytes(total)}</span>
          <Button size="xs" variant="ghost" className="ml-auto" icon={<Trash2 className="size-3" />} onClick={clear}>
            清空列表
          </Button>
        </footer>
      )}

      {dragging && (
        <div className="pointer-events-none absolute inset-2 z-10 flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-accent bg-accent/10 backdrop-blur-[2px]">
          <Upload className="size-7 text-accent" />
          <p className="font-medium text-accent">松开以添加</p>
        </div>
      )}
    </div>
  );
}
