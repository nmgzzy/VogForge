import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import type { MediaInfo, Scenario, TranscodePlan } from "@/lib/types";
import { MOCK_MEDIA } from "@/mock/media";
import { applyFix as engineApplyFix, recommendPlan, suggestScenario, updatePlan } from "@/mock/engine";
import { useCapabilities } from "./capability";

interface ProjectState {
  files: MediaInfo[];
  selectedId?: string;
  plans: Record<string, TranscodePlan>;

  addFiles: (files: MediaInfo[]) => void;
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
}

const caps = () => useCapabilities.getState().caps;

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
