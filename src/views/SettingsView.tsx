import { useEffect, useState } from "react";
import { FolderOpen, RotateCcw } from "lucide-react";
import { backend } from "@/backend";
import { tr } from "@/i18n";
import type { AfterAction, ConflictPolicy, Lang } from "@/lib/types";
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
    .replaceAll("{scenario}", tr("归档", "archive"))
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

  // 覆盖会直接替换已有文件，无法撤销：选的时候先让用户明确同意（需求 F-6.7）
  const chooseConflict = async (conflict: ConflictPolicy) => {
    if (conflict === "overwrite") {
      const ok = await backend.confirm(
        tr(
          "选择“覆盖”后，输出位置已有的同名文件会被直接替换，无法撤销。源文件永远不会被覆盖。确定使用覆盖吗？",
          "With Overwrite, existing files with the same name in the output location are replaced and cannot be recovered. Source files are never overwritten. Use Overwrite?",
        ),
        tr("覆盖同名文件", "Overwrite existing files"),
      );
      if (!ok) return;
    }
    await update({ conflict });
  };

  const pickOutput = async () => {
    const dir = await backend.pickDirectory(tr("选择输出目录", "Choose the output folder"));
    if (dir) await update({ outputDir: dir });
  };
  const pickFfmpeg = async () => {
    const dir = await backend.pickDirectory(tr("选择 ffmpeg.exe 所在的目录", "Choose the folder that contains ffmpeg"));
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
        <h1 className="text-[14px] font-semibold">{tr("设置", "Settings")}</h1>
        {saveError && <span className="ml-3 text-xs text-danger">{saveError}</span>}
      </header>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[860px] space-y-4 p-5">
          <Group title={tr("输出", "Output")}>
            <Row
              label={tr("输出目录", "Output folder")}
              hint={tr("留空表示与源文件同目录下的 VidForge 文件夹", "Empty means a VidForge folder next to each source file")}
            >
              <div className="flex gap-1.5">
                <TextSetting
                  value={s.outputDir ?? ""}
                  placeholder={tr("源文件旁的 VidForge 文件夹", "VidForge folder next to the source")}
                  onCommit={(v) => void update({ outputDir: v.trim() || undefined })}
                />
                {canPick && (
                  <Button
                    icon={<FolderOpen className="size-3.5" />}
                    onClick={() => void pickOutput()}
                    aria-label={tr("选择输出目录", "Choose the output folder")}
                  />
                )}
              </div>
            </Row>
            <Row label={tr("文件命名", "File naming")} hint={tr("可用变量：{name} {height} {codec} {scenario} {date}", "Variables: {name} {height} {codec} {scenario} {date}")}>
              <TextSetting mono value={s.namingTemplate} onCommit={(v) => void update({ namingTemplate: v })} />
              <p className="mt-1 truncate font-mono text-[11px] text-subtle">
                {tr("预览：", "Preview: ")}
                {renderTemplate(s.namingTemplate)}.mkv
              </p>
            </Row>
            <Row
              label={tr("保留源目录结构", "Keep folder structure")}
              hint={tr("批量导入文件夹时，在输出目录中重建相同的子目录", "When importing folders, recreate the same subfolders in the output folder")}
            >
              <Switch checked={s.keepTree} onChange={(keepTree) => void update({ keepTree })} />
            </Row>
            <Row
              label={tr("同名文件", "Existing files")}
              hint={tr("选择覆盖时会先确认；源文件永远不会被覆盖", "Overwrite asks for confirmation first; source files are never overwritten")}
            >
              <Segmented<ConflictPolicy>
                className="w-full"
                value={s.conflict}
                onChange={(conflict) => void chooseConflict(conflict)}
                options={[
                  { value: "skip", label: tr("跳过", "Skip") },
                  { value: "rename", label: tr("自动加序号", "Rename") },
                  { value: "overwrite", label: tr("覆盖", "Overwrite") },
                ]}
              />
            </Row>
            <Row label={tr("完成后", "When a job finishes")} hint={tr("源文件永远不会被自动删除", "Source files are never deleted automatically")}>
              <Select
                value={s.after}
                onChange={(after) => void update({ after: after as AfterAction })}
                options={[
                  { value: "none", label: tr("什么都不做（推荐）", "Do nothing (recommended)") },
                  { value: "open", label: tr("打开输出目录", "Open the output folder") },
                  { value: "trash", label: tr("把源文件移到回收站（每次确认）", "Move the source to the trash (asks every time)") },
                ]}
              />
            </Row>
          </Group>

          <Group title={tr("硬件加速", "Hardware acceleration")}>
            <Row
              label={tr("使用 GPU 编码", "Use GPU encoding")}
              hint={tr(
                "关闭后所有任务都用 CPU 软编。杜比视界保留总是使用 CPU",
                "When off, every job encodes on the CPU. Keeping Dolby Vision always uses the CPU",
              )}
            >
              <Switch checked={s.hwEncode} onChange={(hwEncode) => void update({ hwEncode })} />
            </Row>
            <Row label={tr("使用 GPU 解码", "Use GPU decoding")} hint={tr("失败时自动回退到软件解码", "Falls back to software decoding on failure")}>
              <Switch checked={s.hwDecode} onChange={(hwDecode) => void update({ hwDecode })} />
            </Row>
          </Group>

          <Group title="ffmpeg">
            <Row
              label={tr("ffmpeg 路径", "ffmpeg location")}
              hint={tr(
                "留空则自动查找：应用目录、系统 PATH、注册表 PATH、常见安装位置",
                "Empty means automatic: the app folder, PATH, the registry PATH and common install locations",
              )}
            >
              <div className="flex gap-1.5">
                <TextSetting mono value={s.ffmpegPath ?? ""} placeholder={tr("自动", "Automatic")} onCommit={(v) => void setFfmpeg(v)} />
                {canPick && (
                  <Button
                    icon={<FolderOpen className="size-3.5" />}
                    onClick={() => void pickFfmpeg()}
                    aria-label={tr("选择 ffmpeg 目录", "Choose the ffmpeg folder")}
                  />
                )}
                {s.ffmpegPath && (
                  <Button
                    icon={<RotateCcw className="size-3.5" />}
                    onClick={() => void setFfmpeg("")}
                    aria-label={tr("恢复自动查找", "Back to automatic")}
                  />
                )}
              </div>
            </Row>
          </Group>

          <Group title={tr("界面", "Interface")}>
            <Row label={tr("主题", "Theme")}>
              <Segmented<ThemePref>
                className="w-full"
                value={theme}
                onChange={setTheme}
                options={[
                  { value: "system", label: tr("跟随系统", "System") },
                  { value: "light", label: tr("浅色", "Light") },
                  { value: "dark", label: tr("深色", "Dark") },
                ]}
              />
            </Row>
            <Row label={tr("语言", "Language")} hint={tr("已有任务的记录保持原来的语言", "Existing job history keeps its original language")}>
              <Select
                value={s.language}
                onChange={(language) => void update({ language: language as Lang })}
                options={[
                  { value: "zh-CN", label: "简体中文" },
                  { value: "en", label: "English" },
                ]}
              />
            </Row>
            <Row label={tr("全部完成时通知", "Notify when all jobs finish")}>
              <Switch checked={s.notify} onChange={(notify) => void update({ notify })} />
            </Row>
            <Row
              label={tr("入门引导", "Getting started")}
              hint={tr("重新检查环境、查看能力说明与建议", "Check the environment again and review what it can do")}
            >
              <Button size="sm" onClick={() => void update({ onboarded: false })}>
                {tr("重新打开", "Open again")}
              </Button>
            </Row>
          </Group>

          <p className="px-1 text-[11px] text-subtle">
            {backend.kind === "mock"
              ? tr("浏览器预览中设置只保存在内存里，刷新即恢复默认。", "In the browser preview, settings live in memory and reset on reload.")
              : tr("设置保存在 ~/.vidforge/config.json。", "Settings are stored in ~/.vidforge/config.json.")}
          </p>
        </div>
      </div>
    </div>
  );
}
