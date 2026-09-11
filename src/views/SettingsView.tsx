import { useEffect, useState } from "react";
import { FolderOpen, RotateCcw } from "lucide-react";
import { backend } from "@/backend";
import type { AfterAction, ConflictPolicy } from "@/lib/types";
import { useCapabilities } from "@/stores/capability";
import { useSettings } from "@/stores/settings";
import { useUi, type ThemePref } from "@/stores/ui";
import { Button, Segmented, Select, Switch } from "@/components/ui";

const inputCls =
  "h-8 w-full rounded-md border border-line bg-panel px-2.5 text-[13px] text-fg transition-colors hover:border-line-strong focus:border-accent focus:outline-none";

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-line bg-panel">
      <h2 className="border-b border-line px-4 py-2.5 text-[13px] font-semibold">{title}</h2>
      <div className="divide-y divide-line">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-6 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="text-[13px]">{label}</div>
        {hint && <div className="mt-0.5 text-xs text-muted">{hint}</div>}
      </div>
      <div className="w-[320px] shrink-0">{children}</div>
    </div>
  );
}

/** 文本设置：输入时只改本地草稿，失焦或回车才保存，避免每个按键都写一次配置文件 */
function TextSetting({ value, onCommit, placeholder, mono }: { value: string; onCommit: (v: string) => void; placeholder?: string; mono?: boolean }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <input
      className={`${inputCls} ${mono ? "font-mono text-xs" : ""}`}
      value={draft}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}

export function renderTemplate(template: string): string {
  return template
    .replaceAll("{name}", "IMG_4521")
    .replaceAll("{height}", "2160")
    .replaceAll("{codec}", "hevc")
    .replaceAll("{scenario}", "归档")
    .replaceAll("{date}", "2026-09-11");
}

export function SettingsView() {
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const s = useSettings((st) => st.settings);
  const update = useSettings((st) => st.update);
  const saveError = useSettings((st) => st.error);
  const reprobe = useCapabilities((st) => st.reprobe);
  const canPick = backend.kind === "tauri";

  const pickOutput = async () => {
    const dir = await backend.pickDirectory("选择输出目录");
    if (dir) await update({ outputDir: dir });
  };
  const pickFfmpeg = async () => {
    const dir = await backend.pickDirectory("选择 ffmpeg.exe 所在的目录");
    if (!dir) return;
    await update({ ffmpegPath: dir });
    await reprobe();
  };
  const setFfmpeg = async (v: string) => {
    await update({ ffmpegPath: v.trim() || undefined });
    await reprobe();
  };

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">设置</h1>
        {saveError && <span className="ml-3 text-xs text-danger">{saveError}</span>}
      </header>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[860px] space-y-4 p-5">
          <Group title="输出">
            <Row label="输出目录" hint="留空表示与源文件同目录下的 VidForge 文件夹">
              <div className="flex gap-1.5">
                <TextSetting value={s.outputDir ?? ""} placeholder="源文件旁的 VidForge 文件夹" onCommit={(v) => void update({ outputDir: v.trim() || undefined })} />
                {canPick && <Button icon={<FolderOpen className="size-3.5" />} onClick={() => void pickOutput()} aria-label="选择输出目录" />}
              </div>
            </Row>
            <Row label="文件命名" hint="可用变量：{name} {height} {codec} {scenario} {date}">
              <TextSetting mono value={s.namingTemplate} onCommit={(v) => void update({ namingTemplate: v })} />
              <p className="mt-1 truncate font-mono text-[11px] text-subtle">预览：{renderTemplate(s.namingTemplate)}.mkv</p>
            </Row>
            <Row label="保留源目录结构" hint="批量导入文件夹时，在输出目录中重建相同的子目录">
              <Switch checked={s.keepTree} onChange={(keepTree) => void update({ keepTree })} />
            </Row>
            <Row label="同名文件" hint="覆盖前会再次确认">
              <Segmented<ConflictPolicy>
                className="w-full"
                value={s.conflict}
                onChange={(conflict) => void update({ conflict })}
                options={[
                  { value: "skip", label: "跳过" },
                  { value: "rename", label: "自动加序号" },
                  { value: "overwrite", label: "覆盖" },
                ]}
              />
            </Row>
            <Row label="完成后" hint="源文件永远不会被自动删除">
              <Select
                value={s.after}
                onChange={(after) => void update({ after: after as AfterAction })}
                options={[
                  { value: "none", label: "什么都不做（推荐）" },
                  { value: "open", label: "打开输出目录" },
                  { value: "trash", label: "把源文件移到回收站（每次确认）" },
                ]}
              />
            </Row>
          </Group>

          <Group title="硬件加速">
            <Row label="使用 GPU 编码" hint="关闭后所有任务都用 CPU 软编。杜比视界保留总是使用 CPU">
              <Switch checked={s.hwEncode} onChange={(hwEncode) => void update({ hwEncode })} />
            </Row>
            <Row label="使用 GPU 解码" hint="失败时自动回退到软件解码">
              <Switch checked={s.hwDecode} onChange={(hwDecode) => void update({ hwDecode })} />
            </Row>
          </Group>

          <Group title="ffmpeg">
            <Row label="ffmpeg 路径" hint="留空则自动查找：应用目录、系统 PATH、注册表 PATH、常见安装位置">
              <div className="flex gap-1.5">
                <TextSetting mono value={s.ffmpegPath ?? ""} placeholder="自动" onCommit={(v) => void setFfmpeg(v)} />
                {canPick && <Button icon={<FolderOpen className="size-3.5" />} onClick={() => void pickFfmpeg()} aria-label="选择 ffmpeg 目录" />}
                {s.ffmpegPath && <Button icon={<RotateCcw className="size-3.5" />} onClick={() => void setFfmpeg("")} aria-label="恢复自动查找" />}
              </div>
            </Row>
          </Group>

          <Group title="界面">
            <Row label="主题">
              <Segmented<ThemePref>
                className="w-full"
                value={theme}
                onChange={setTheme}
                options={[
                  { value: "system", label: "跟随系统" },
                  { value: "light", label: "浅色" },
                  { value: "dark", label: "深色" },
                ]}
              />
            </Row>
            <Row label="语言">
              <Select
                value="zh-CN"
                onChange={() => undefined}
                options={[
                  { value: "zh-CN", label: "简体中文" },
                  { value: "en", label: "English（开发中）", disabled: true },
                ]}
              />
            </Row>
            <Row label="全部完成时通知">
              <Switch checked={s.notify} onChange={(notify) => void update({ notify })} />
            </Row>
          </Group>

          <p className="px-1 text-[11px] text-subtle">
            {backend.kind === "mock" ? "浏览器预览中设置只保存在内存里，刷新即恢复默认。" : "设置保存在 ~/.vidforge/config.json。"}
          </p>
        </div>
      </div>
    </div>
  );
}
