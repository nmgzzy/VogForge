import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { useCapabilities } from "@/stores/capability";
import { useSettings } from "@/stores/settings";
import { Onboarding } from "./Onboarding";

beforeEach(async () => {
  await useSettings.getState().update({ ...DEFAULT_SETTINGS, onboarded: false });
  useSettings.setState({ loaded: true });
  useCapabilities.setState({ caps: MOCK_CAPABILITIES, probing: false, error: undefined });
});
afterEach(() => useCapabilities.setState({ caps: MOCK_CAPABILITIES }));

describe("首次启动引导", () => {
  it("检查环境 → 能力说明 → 建议，走完后不再出现", async () => {
    render(<Onboarding />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("找到 ffmpeg 9.0.1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByText(/Intel QSV/)).toBeInTheDocument();
    expect(screen.getByText("杜比视界：CPU 编码时可以保留")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByText(/流媒体、手机、社交场景会自动用 Intel QSV 加速/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "开始使用" }));
    await waitFor(() => expect(useSettings.getState().settings.onboarded).toBe(true));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("找不到 ffmpeg 时第一步直接给出下载指引，建议里只提安装", () => {
    useCapabilities.setState({
      caps: { ...MOCK_CAPABILITIES, status: "missing", statusDetail: "没有找到 ffmpeg 与 ffprobe。", encoders: [] },
    });
    render(<Onboarding />);
    expect(screen.getByTestId("ffmpeg-guide")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByText(/先按第一步安装 ffmpeg/)).toBeInTheDocument();
  });

  it("可以跳过；设置还没读回来时不弹出", async () => {
    useSettings.setState({ loaded: false });
    const { rerender } = render(<Onboarding />);
    expect(screen.queryByRole("dialog")).toBeNull();
    useSettings.setState({ loaded: true });
    rerender(<Onboarding />);
    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    await waitFor(() => expect(useSettings.getState().settings.onboarded).toBe(true));
  });
});
