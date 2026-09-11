import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { useUi, type ThemePref } from "@/stores/ui";
import { Button, Field, Segmented, Select, Switch } from "@/components/ui";

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

export function SettingsView() {
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const [template, setTemplate] = useState("{name}_{height}p_{codec}");
  const [keepTree, setKeepTree] = useState(true);
  const [hw, setHw] = useState(true);
  const [hwDecode, setHwDecode] = useState(true);
  const [conflict, setConflict] = useState("rename");
  const [after, setAfter] = useState("none");
  const [notify, setNotify] = useState(true);

  const preview = template
    .replace("{name}", "IMG_4521")
    .replace("{height}", "2160")
    .replace("{codec}", "hevc")
    .replace("{scenario}", "归档")
    .replace("{date}", "2026-09-11");

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <h1 className="text-[14px] font-semibold">设置</h1>
      </header>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[860px] space-y-4 p-5">
          <Group title="输出">
            <Row label="输出目录" hint="留空表示与源文件同目录下的 VidForge 文件夹">
              <div className="flex gap-1.5">
                <input className={inputCls} defaultValue="D:\转码输出" />
                <Button icon={<FolderOpen className="size-3.5" />} />
              </div>
            </Row>
            <Row label="文件命名" hint="可用变量：{name} {height} {codec} {scenario} {date}">
              <input className={`${inputCls} font-mono text-xs`} value={template} onChange={(e) => setTemplate(e.target.value)} />
              <p className="mt-1 truncate font-mono text-[11px] text-subtle">预览：{preview}.mkv</p>
            </Row>
            <Row label="保留源目录结构" hint="批量导入文件夹时，在输出目录中重建相同的子目录">
              <Switch checked={keepTree} onChange={setKeepTree} />
            </Row>
            <Row label="同名文件" hint="覆盖前会再次确认">
              <Segmented
                className="w-full"
                value={conflict}
                onChange={setConflict}
                options={[
                  { value: "skip", label: "跳过" },
                  { value: "rename", label: "自动加序号" },
                  { value: "overwrite", label: "覆盖" },
                ]}
              />
            </Row>
            <Row label="完成后" hint="源文件永远不会被自动删除">
              <Select
                value={after}
                onChange={setAfter}
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
              <Switch checked={hw} onChange={setHw} />
            </Row>
            <Row label="使用 GPU 解码" hint="失败时自动回退到软件解码">
              <Switch checked={hwDecode} onChange={setHwDecode} />
            </Row>
          </Group>

          <Group title="ffmpeg">
            <Row label="ffmpeg 路径" hint="留空则自动查找：系统 PATH、常见安装位置、应用目录">
              <div className="flex gap-1.5">
                <input className={`${inputCls} font-mono text-xs`} placeholder="自动" />
                <Button icon={<FolderOpen className="size-3.5" />} />
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
              <Switch checked={notify} onChange={setNotify} />
            </Row>
          </Group>

          <Field label="" className="px-1">
            <p className="text-[11px] text-subtle">预览版中设置不会持久保存，接入后端后写入 ~/.vidforge/config.json。</p>
          </Field>
        </div>
      </div>
    </div>
  );
}
