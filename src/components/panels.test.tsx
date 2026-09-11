import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { MediaInfo, Scenario } from "@/lib/types";
import { evaluate, recommendPlan } from "@/mock/engine";
import { MOCK_CAPABILITIES as caps } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
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
