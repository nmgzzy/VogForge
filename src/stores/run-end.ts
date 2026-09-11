/**
 * 一批任务跑完之后的动作（需求 F-6.10 / F-6.11）：系统通知、打开输出目录、把源文件移到回收站。
 *
 * "跑完"指进行中、排队、暂停的任务从有变成没有，且不是被"全部暂停"停下的。只看这一批里出现过的任务，
 * 启动时读回的历史记录不算。源文件永远不会被静默删除：移到回收站每次都要确认，而且只处理校验全部通过的任务。
 */
import { backend } from "@/backend";
import { tr } from "@/i18n";
import type { Job } from "@/lib/types";
import { useQueue } from "./queue";
import { useSettings } from "./settings";

const active = (j: Job) => j.status === "running" || j.status === "queued" || j.status === "paused";

/** 目录部分；分隔符两种都认 */
const dirOf = (p: string) => p.slice(0, Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")));

/** 同时打开的文件夹上限，批量任务分散在很多目录时不至于弹一堆窗口 */
const MAX_REVEAL = 3;

export async function runEndActions(finished: Job[]): Promise<void> {
  const settings = useSettings.getState().settings;
  const done = finished.filter((j) => j.status === "done");
  const failed = finished.filter((j) => j.status === "failed").length;
  // 目标已存在按设置跳过也是正常结束，全部跳过时同样要告诉用户
  const skipped = finished.filter((j) => j.status === "skipped").length;
  if (done.length === 0 && failed === 0 && skipped === 0) return;

  if (settings.notify) {
    const body =
      failed === 0 && skipped === 0
        ? tr(`${done.length} 个任务全部完成`, `All ${done.length} job(s) finished`)
        : [
            tr(`${done.length} 个完成`, `${done.length} done`),
            failed > 0 && tr(`${failed} 个失败`, `${failed} failed`),
            skipped > 0 && tr(`${skipped} 个已跳过`, `${skipped} skipped`),
          ]
            .filter(Boolean)
            .join(tr("，", ", "));
    await backend.notify(tr("VidForge：队列已完成", "VidForge: queue finished"), body).catch(() => undefined);
  }

  if (settings.after === "open") {
    // 每个输出目录定位一次最后完成的那个文件
    const last = new Map<string, string>();
    for (const j of done) last.set(dirOf(j.outputPath), j.outputPath);
    for (const path of [...last.values()].slice(-MAX_REVEAL)) await backend.revealPath(path).catch(() => undefined);
  }

  if (settings.after === "trash") {
    const verified = done.filter((j) => j.report && j.report.length > 0 && j.report.every((r) => r.ok));
    if (verified.length === 0) return;
    const names = verified.slice(0, 5).map((j) => `· ${j.media.name}`);
    if (verified.length > 5) names.push(tr(`……共 ${verified.length} 个`, `…${verified.length} in total`));
    const ok = await backend.confirm(
      tr(
        `这些源文件已转码完成且校验全部通过，要移到回收站吗？\n\n${names.join("\n")}\n\n可以从回收站恢复。校验没通过的任务不会动。`,
        `These sources finished encoding and passed every check. Move them to the trash?\n\n${names.join("\n")}\n\nYou can restore them from the trash. Jobs that failed a check are left alone.`,
      ),
      tr("把源文件移到回收站", "Move sources to the trash"),
    );
    if (!ok) return;
    try {
      await backend.trashSources(verified.map((j) => j.id));
    } catch (e) {
      useQueue.setState({ error: e instanceof Error ? e.message : String(e) });
    }
  }
}

/** 订阅队列，一批任务跑完时执行上面的动作；返回取消订阅 */
export function watchRunEnd(): () => void {
  let batch = new Set<string>();
  return useQueue.subscribe((s, prev) => {
    for (const j of s.jobs) if (active(j)) batch.add(j.id);
    const wasActive = prev.jobs.some(active);
    const nowActive = s.jobs.some(active);
    if (!wasActive || nowActive || s.paused || batch.size === 0) return;
    const finished = s.jobs.filter((j) => batch.has(j.id));
    batch = new Set();
    void runEndActions(finished);
  });
}
