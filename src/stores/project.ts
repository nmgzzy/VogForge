import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { backend } from "@/backend";
import type { ImportFailure, ImportProgress, MediaInfo, Scenario, TranscodePlan } from "@/lib/types";
import { MOCK_MEDIA } from "@/mock/media";
import { applyFix as engineApplyFix, recommendPlan, suggestScenario, updatePlan } from "@/lib/engine";
import { useCapabilities } from "./capability";

export interface ImportReport {
  failures: ImportFailure[];
  /** 文件夹里扩展名不像视频、被跳过的文件数 */
  skipped: number;
  added: number;
  /** 已在列表里、没有重复添加的文件数 */
  duplicate: number;
}

interface ProjectState {
  files: MediaInfo[];
  selectedId?: string;
  plans: Record<string, TranscodePlan>;
  importing: boolean;
  importProgress?: ImportProgress;
  /** 分析进行中又加入、正在排队的路径数 */
  importQueued: number;
  /** 最近一次导入里需要告诉用户的事（失败、跳过、重复）；没有就是 undefined */
  importReport?: ImportReport;

  addFiles: (files: MediaInfo[]) => void;
  /** 分析文件与文件夹并加入列表 */
  importPaths: (paths: string[]) => Promise<void>;
  dismissImportReport: () => void;
  loadSamples: () => void;
  removeFile: (id: string) => void;
  clear: () => void;
  select: (id: string) => void;

  setScenario: (scenario: Scenario) => void;
  /** 修改当前计划。修改后自动 normalize，保证计划自洽 */
  patchPlan: (fn: (p: TranscodePlan) => void) => void;
  applyFix: (fixId: string) => void;
  /** 把当前文件的场景应用到全部文件（各文件按自身情况重新推荐） */
  applyScenarioToAll: () => void;
  /** 环境能力变化后（例如探测完成），按新能力重新整理全部计划 */
  refreshPlans: () => void;
}

const caps = () => useCapabilities.getState().caps;

/** 导入进行中又加入的路径 */
const pendingImports: string[] = [];

export const useProject = create<ProjectState>((set, get) => ({
  files: [],
  plans: {},

  addFiles: (incoming) =>
    set((s) => {
      const known = new Set(s.files.map((f) => f.id));
      const fresh = incoming.filter((f) => !known.has(f.id));
      const plans = { ...s.plans };
      for (const f of fresh) plans[f.id] = recommendPlan(f, suggestScenario(f), caps());
      return {
        files: [...s.files, ...fresh],
        plans,
        selectedId: s.selectedId ?? fresh[0]?.id,
      };
    }),

  importing: false,
  importQueued: 0,

  importPaths: async (paths) => {
    if (paths.length === 0) return;
    // 分析进行中又拖进来的文件排队，当前这批完成后接着处理，结果合并成一份报告
    if (get().importing) {
      pendingImports.push(...paths);
      set({ importQueued: pendingImports.length });
      return;
    }
    set({ importing: true, importProgress: undefined, importReport: undefined });
    const total: ImportReport = { failures: [], skipped: 0, added: 0, duplicate: 0 };
    let firstNew: string | undefined;
    const off = backend.onImportProgress((importProgress) => set({ importProgress }));
    try {
      for (let batch = paths; batch.length > 0; batch = pendingImports.splice(0)) {
        set({ importQueued: pendingImports.length });
        try {
          const r = await backend.importMedia(batch);
          const known = new Set(get().files.map((f) => f.id));
          const fresh = r.media.filter((m) => !known.has(m.id));
          get().addFiles(r.media);
          firstNew ??= fresh[0]?.id;
          total.failures.push(...r.failures);
          total.skipped += r.skipped;
          total.added += fresh.length;
          total.duplicate += r.media.length - fresh.length;
        } catch (e) {
          total.failures.push({ path: "", reason: e instanceof Error ? e.message : String(e) });
        }
      }
    } finally {
      off();
      const worthTelling = total.failures.length > 0 || total.skipped > 0 || total.duplicate > 0;
      set({
        importing: false,
        importProgress: undefined,
        importQueued: 0,
        importReport: worthTelling ? total : undefined,
        ...(firstNew ? { selectedId: firstNew } : {}),
      });
    }
  },

  dismissImportReport: () => set({ importReport: undefined }),

  loadSamples: () => get().addFiles(MOCK_MEDIA),

  removeFile: (id) =>
    set((s) => {
      const files = s.files.filter((f) => f.id !== id);
      const plans = { ...s.plans };
      delete plans[id];
      const selectedId = s.selectedId === id ? files[0]?.id : s.selectedId;
      return { files, plans, selectedId };
    }),

  clear: () => set({ files: [], plans: {}, selectedId: undefined }),

  select: (selectedId) => set({ selectedId }),

  setScenario: (scenario) => {
    const { selectedId, files } = get();
    const media = files.find((f) => f.id === selectedId);
    if (!media) return;
    set((s) => ({ plans: { ...s.plans, [media.id]: recommendPlan(media, scenario, caps()) } }));
  },

  patchPlan: (fn) => {
    const { selectedId, files, plans } = get();
    const media = files.find((f) => f.id === selectedId);
    const plan = selectedId ? plans[selectedId] : undefined;
    if (!media || !plan) return;
    const draft = structuredClone(plan);
    fn(draft);
    set((s) => ({ plans: { ...s.plans, [media.id]: updatePlan(draft, media, caps()) } }));
  },

  applyFix: (fixId) => {
    const { selectedId, files, plans } = get();
    const media = files.find((f) => f.id === selectedId);
    const plan = selectedId ? plans[selectedId] : undefined;
    if (!media || !plan) return;
    set((s) => ({ plans: { ...s.plans, [media.id]: engineApplyFix(plan, fixId, media, caps()) } }));
  },

  applyScenarioToAll: () => {
    const { selectedId, files, plans } = get();
    const scenario = selectedId ? plans[selectedId]?.scenario : undefined;
    if (!scenario) return;
    const next: Record<string, TranscodePlan> = {};
    for (const f of files) next[f.id] = f.id === selectedId ? plans[f.id]! : recommendPlan(f, scenario, caps());
    set({ plans: next });
  },

  refreshPlans: () => {
    const { files, plans } = get();
    const next: Record<string, TranscodePlan> = {};
    for (const f of files) {
      const p = plans[f.id];
      if (p) next[f.id] = updatePlan(structuredClone(p), f, caps());
    }
    set({ plans: next });
  },
}));

/** 当前选中的文件与计划。用 useShallow 保证引用未变时不触发重渲染 */
export function useSelected(): { media?: MediaInfo; plan?: TranscodePlan } {
  return useProject(
    useShallow((s) => {
      const media = s.files.find((f) => f.id === s.selectedId);
      return { media, plan: media ? s.plans[media.id] : undefined };
    }),
  );
}
