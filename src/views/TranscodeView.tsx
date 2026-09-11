import { useMemo } from "react";
import { FileVideo } from "lucide-react";
import { backend } from "@/backend";
import { evaluate } from "@/mock/engine";
import { useCapabilities } from "@/stores/capability";
import { useProject, useSelected } from "@/stores/project";
import { CommandBar } from "@/components/CommandBar";
import { ExpertPanel } from "@/components/ExpertPanel";
import { FidelityPanel } from "@/components/FidelityPanel";
import { FileList } from "@/components/FileList";
import { DecisionList, EstimateCard } from "@/components/InsightPanel";
import { MediaDetails, MediaHeader } from "@/components/MediaHeader";
import { ParamsPanel } from "@/components/ParamsPanel";
import { NotWorthItBanner, ScenarioPicker } from "@/components/ScenarioPicker";
import { Button, Empty } from "@/components/ui";

export function TranscodeView() {
  const { media, plan } = useSelected();
  const caps = useCapabilities((s) => s.caps);
  const loadSamples = useProject((s) => s.loadSamples);
  const result = useMemo(() => (media && plan ? evaluate(media, plan, caps) : undefined), [media, plan, caps]);

  return (
    <div className="flex h-full min-w-0 flex-1">
      <FileList />

      <div className="flex min-w-0 flex-1 flex-col">
        {media && plan && result ? (
          <>
            <div className="@container flex-1 overflow-y-auto">
              <div className="mx-auto max-w-[1280px] px-5 py-4">
                <MediaHeader media={media} />

                <div className="mt-4 grid gap-4 @min-[900px]:grid-cols-[minmax(0,1fr)_300px]">
                  <div className="flex min-w-0 flex-col gap-4">
                    <ScenarioPicker media={media} value={plan.scenario} />
                    {result.notWorthIt && <NotWorthItBanner message={result.notWorthIt} />}
                    {/* 单栏布局时，预估紧跟场景选择，决策理由放在保真度之后 */}
                    <div className="@min-[900px]:hidden">
                      <EstimateCard media={media} result={result} />
                    </div>
                    <ParamsPanel media={media} plan={plan} result={result} />
                    <FidelityPanel plan={plan} items={result.fidelity} />
                    <div className="@min-[900px]:hidden">
                      <DecisionList result={result} />
                    </div>
                    <ExpertPanel plan={plan} />
                  </div>
                  <div className="hidden flex-col gap-4 @min-[900px]:sticky @min-[900px]:top-4 @min-[900px]:flex @min-[900px]:self-start">
                    <EstimateCard media={media} result={result} />
                    <DecisionList result={result} />
                  </div>
                </div>
              </div>
            </div>
            <CommandBar media={media} plan={plan} result={result} />
            <MediaDetails media={media} />
          </>
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <Empty
              icon={<FileVideo className="size-5" />}
              title="还没有选择文件"
              description="在左侧添加视频后，这里会分析它的特征，按用途推荐参数，并告诉你哪些信息能保留。"
              action={
                backend.kind === "mock" && (
                  <Button variant="primary" size="sm" onClick={loadSamples}>
                    载入示例素材
                  </Button>
                )
              }
            />
          </div>
        )}
      </div>
    </div>
  );
}
