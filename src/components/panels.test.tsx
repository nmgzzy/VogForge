import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { MediaInfo, Scenario } from "@/lib/types";
import { evaluate, recommendPlan } from "@/lib/engine";
import { MOCK_CAPABILITIES as caps } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { useProject } from "@/stores/project";
import { useUi } from "@/stores/ui";
import { CommandBar } from "./CommandBar";
import { ExpertPanel } from "./ExpertPanel";
import { FidelityPanel } from "./FidelityPanel";
import { DecisionList } from "./InsightPanel";
import { FpsControl } from "./ParamsPanel";

const media = (id: string): MediaInfo => MOCK_MEDIA.find((m) => m.id === id)!;
const result = (id: string, s: Scenario) => {
  const m = media(id);
  return evaluate(m, recommendPlan(m, s, caps), caps);
};

describe("FidelityPanel", () => {
  it("冲突项展开原因与修正按钮，可保留项只占一行", () => {
    const r = result("m-bluray", "collection");
    render(<FidelityPanel plan={r.plan} items={r.fidelity} />);

    const dv = screen.getByTestId("fidelity-dolbyVision");
    expect(within(dv).getByText("无法保留")).toBeInTheDocument();
    expect(within(dv).getByText(/Profile 7/)).toBeInTheDocument();
    expect(within(dv).getByRole("button", { name: /改为原样封装/ })).toBeInTheDocument();

    const audio = screen.getByTestId("fidelity-allAudio");
    expect(within(audio).getByText("可保留")).toBeInTheDocument();
    // 可保留项的详细说明只在悬停提示里，不占正文
    expect(within(audio).queryByText(/保留全部 4 条音轨/)).toBeNull();
  });

  it("冲突项排在最前面", () => {
    const r = result("m-bluray", "collection");
    render(<FidelityPanel plan={r.plan} items={r.fidelity} />);
    const rows = screen.getAllByTestId(/^fidelity-/);
    expect(rows[0]).toHaveAttribute("data-testid", "fidelity-dolbyVision");
  });

  it("没勾选的项显示“未要求”，不显示冲突", () => {
    const r = result("m-iphone", "streaming");
    render(<FidelityPanel plan={r.plan} items={r.fidelity} />);
    const dv = screen.getByTestId("fidelity-dolbyVision");
    expect(within(dv).getByText("未要求")).toBeInTheDocument();
    expect(within(dv).queryByRole("button", { name: /开启保留/ })).toBeNull();
  });

  it("汇总标签反映冲突数", () => {
    const r = result("m-bluray", "collection");
    render(<FidelityPanel plan={r.plan} items={r.fidelity} />);
    expect(screen.getByText("1 项冲突")).toBeInTheDocument();
  });
});

describe("DecisionList", () => {
  it("默认只展开警告与提示的理由，展开全部后显示其余", () => {
    const r = result("m-bluray", "collection");
    render(<DecisionList result={r} />);
    // warn：杜比视界仅保留基础层，理由默认可见
    expect(screen.getByText(/ffmpeg 无法编码增强层/)).toBeInTheDocument();
    // info：容器理由默认折叠
    expect(screen.queryByText(/能完整容纳杜比视界/)).toBeNull();
    fireEvent.click(screen.getByText("展开全部说明"));
    expect(screen.getByText(/能完整容纳杜比视界/)).toBeInTheDocument();
  });

  it("点击单条普通说明可单独展开", () => {
    const r = result("m-iphone", "archive");
    render(<DecisionList result={r} />);
    expect(screen.queryByText(/能完整容纳杜比视界/)).toBeNull();
    fireEvent.click(screen.getByText("MKV"));
    expect(screen.getByText(/能完整容纳杜比视界/)).toBeInTheDocument();
  });
});

describe("FpsControl", () => {
  it("可变帧率源未开启转换时提示剪辑风险", () => {
    const m = media("m-iphone");
    const r = result("m-iphone", "archive");
    render(<FpsControl media={m} plan={r.plan} insight={r.fpsInsight} />);
    expect(screen.getByText("源为可变帧率")).toBeInTheDocument();
    expect(screen.getByText(/导入剪辑软件前建议开启/)).toBeInTheDocument();
  });

  it("开启后显示帧数变化与复制帧数", () => {
    const m = media("m-screen");
    const r = result("m-screen", "editing");
    render(<FpsControl media={m} plan={r.plan} insight={r.fpsInsight} />);
    expect(screen.getByText(/7,128 → 24,720 帧，复制 17,592/)).toBeInTheDocument();
    expect(screen.getByText(/源帧率波动大/)).toBeInTheDocument();
  });

  it("固定帧率源不显示提示", () => {
    const m = media("m-drone");
    const r = result("m-drone", "archive");
    render(<FpsControl media={m} plan={r.plan} insight={r.fpsInsight} />);
    expect(screen.getByText(/源为固定 59.94 fps/)).toBeInTheDocument();
    expect(screen.queryByText(/建议开启/)).toBeNull();
  });
});

describe("码率控制（更多参数）", () => {
  const selected = () => {
    const s = useProject.getState();
    const media = s.files.find((f) => f.id === s.selectedId)!;
    const plan = s.plans[media.id]!;
    return { media, plan, result: evaluate(media, plan, caps) };
  };
  const renderExpert = () => {
    const { plan, result } = selected();
    const view = render(<ExpertPanel plan={plan} result={result} />);
    fireEvent.click(screen.getByRole("button", { name: /更多参数/ }));
    return view;
  };

  beforeEach(() => {
    useUi.setState({ commandExpanded: false });
    useProject.setState({ files: [], plans: {}, selectedId: undefined });
    useProject.getState().loadSamples();
    useProject.getState().select("m-drone");
  });

  it("切到目标码率时以当前画质的预计码率为起点，并能改数值", () => {
    const before = selected().result.estimate.videoBps;
    const view = renderExpert();
    fireEvent.click(screen.getByRole("radio", { name: "目标码率" }));
    const rc = selected().plan.video.rateControl;
    expect(rc.kind).toBe("bitrate");
    expect(rc.kind !== "quality" && Math.abs(rc.kbps - before / 1000)).toBeLessThanOrEqual(50);

    const { plan, result } = selected();
    view.rerender(<ExpertPanel plan={plan} result={result} />);
    const input = screen.getByRole("textbox", { name: "目标码率" });
    fireEvent.change(input, { target: { value: "12.5" } });
    fireEvent.blur(input);
    expect(selected().plan.video.rateControl).toEqual({ kind: "bitrate", kbps: 12500 });
  });

  it("手选硬件编码器时两遍不可选，并说明原因", () => {
    useProject.getState().setScenario("streaming");
    useProject.getState().patchPlan((p) => {
      p.video.encoderAuto = false;
      p.video.encoder = "hevc_qsv";
    });
    renderExpert();
    const twoPass = screen.getByRole("radio", { name: "两遍" });
    expect(twoPass).toBeDisabled();
    expect(twoPass).toHaveAttribute("title", expect.stringContaining("只有软件编码器支持"));
    // QSV 能做"质量 + 限峰值"（QVBR）
    expect(screen.getByRole("radio", { name: "限峰值" })).toBeEnabled();
  });

  it("响度标准化：只有重新编码的音轨可用，开启后命令栏列出每条音轨的测量命令", () => {
    // 航拍素材没有音轨，选项不可用
    renderExpert();
    expect(screen.getByRole("radio", { name: "标准化 -16 LUFS" })).toBeDisabled();

    useProject.getState().select("m-bluray");
    useProject.getState().setScenario("streaming");
    const { plan, result } = selected();
    const view = render(<ExpertPanel plan={plan} result={result} />);
    fireEvent.click(within(view.container).getByRole("button", { name: /更多参数/ }));
    fireEvent.click(within(view.container).getByRole("radio", { name: "标准化 -16 LUFS" }));
    const after = selected();
    expect(after.plan.loudnorm).toBe(true);
    render(<CommandBar media={after.media} plan={after.plan} result={after.result} />);
    fireEvent.click(screen.getByTitle("展开查看完整命令"));
    expect(screen.getAllByTestId("loudness-measure")).toHaveLength(2);
  });

  it("两遍编码时命令栏标出两遍，复制内容包含第一遍", () => {
    useProject.getState().patchPlan((p) => {
      p.video.rateControl = { kind: "two_pass", kbps: 8000 };
    });
    const { media, plan, result } = selected();
    expect(result.firstPass).toBeDefined();
    render(<CommandBar media={media} plan={plan} result={result} />);
    expect(screen.getByText("两遍")).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("展开查看完整命令"));
    expect(screen.getByTestId("first-pass")).toHaveTextContent("-pass 1");
  });
});
