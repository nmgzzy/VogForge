import { afterEach, describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import type { Capabilities } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { useCapabilities } from "@/stores/capability";
import { ENV_STATUS, Sidebar } from "@/components/Sidebar";
import { downloadUrl, EnvironmentView } from "./EnvironmentView";

const withCaps = (caps: Capabilities) => useCapabilities.setState({ caps, probing: false, error: undefined });

afterEach(() => withCaps(MOCK_CAPABILITIES));

const missing: Capabilities = {
  ...MOCK_CAPABILITIES,
  status: "missing",
  statusDetail: "没有找到 ffmpeg 与 ffprobe。",
  ffmpegPath: "",
  ffprobePath: "",
  version: "",
  versionNumber: "",
  searched: ["C:\\Windows\\System32", "C:\\ffmpeg\\bin"],
  encoders: [],
};

describe("环境页", () => {
  it("就绪时显示版本、定位来源与编码器矩阵，不显示告警", () => {
    render(<EnvironmentView />);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("9.0.1")).toBeInTheDocument();
    expect(screen.getByText(/注册表 PATH/)).toBeInTheDocument();
    expect(screen.getByText("满足全部 v1 功能（需 ≥ 7.1）")).toBeInTheDocument();
    expect(screen.getAllByText("设备或驱动缺失").length).toBeGreaterThan(0);
  });

  it("找不到 ffmpeg 时给出醒目提示、下载入口与查找过的位置，不渲染能力卡片", () => {
    withCaps(missing);
    render(<EnvironmentView />);
    const alert = screen.getByRole("alert");
    expect(within(alert).getByText("没有找到 ffmpeg")).toBeInTheDocument();
    expect(within(alert).getByRole("button", { name: /下载推荐构建/ })).toBeInTheDocument();
    expect(within(alert).getByText("查找过的位置（2）")).toBeInTheDocument();
    expect(screen.queryByText("硬件编码能力")).toBeNull();
  });

  it("版本过低时显示警告与“低于 7.1”标记，能力卡片仍可查看", () => {
    withCaps({ ...MOCK_CAPABILITIES, status: "too_old", statusDetail: "ffmpeg 6.0 低于最低要求 7.1。", versionNumber: "6.0" });
    render(<EnvironmentView />);
    expect(within(screen.getByRole("alert")).getByText("ffmpeg 版本过低")).toBeInTheDocument();
    expect(screen.getByText("低于 7.1，请升级")).toBeInTheDocument();
    expect(screen.getByText("硬件编码能力")).toBeInTheDocument();
  });

  it("未编译的编码器标为“未编译进 ffmpeg”，并解释含义", () => {
    withCaps({
      ...MOCK_CAPABILITIES,
      encoders: MOCK_CAPABILITIES.encoders.map((e) =>
        e.id === "libsvtav1" ? { ...e, usable: false, failure: "not_built", error: "当前 ffmpeg 没有编译这个编码器" } : e,
      ),
    });
    render(<EnvironmentView />);
    expect(screen.getByText("未编译进 ffmpeg")).toBeInTheDocument();
  });

  it("没有任何色调映射管线时提示“转为 SDR”会置灰", () => {
    withCaps({ ...MOCK_CAPABILITIES, tonemap: MOCK_CAPABILITIES.tonemap.map((t) => ({ ...t, available: false })) });
    render(<EnvironmentView />);
    expect(screen.getByText(/没有可用的色调映射管线/)).toBeInTheDocument();
  });

  it("下载页按平台区分：macOS 推荐 jellyfin-ffmpeg", () => {
    expect(downloadUrl("macos")).toContain("jellyfin-ffmpeg");
    expect(downloadUrl("windows")).toContain("gyan.dev");
  });
});

describe("侧栏环境状态", () => {
  it("后端调用失败时显示“环境探测失败”，而不是一直停在探测中", () => {
    useCapabilities.setState({ caps: { ...MOCK_CAPABILITIES, status: "probing" }, probing: false, error: "IPC 断开" });
    render(<Sidebar />);
    expect(screen.getByText("环境探测失败")).toBeInTheDocument();
    expect(screen.queryByText("正在探测环境…")).toBeNull();
    useCapabilities.setState({ error: undefined });
  });

  it("每种状态都有文案，异常状态不闪烁", () => {
    expect(ENV_STATUS.ready.pulse).toBe(true);
    for (const s of ["missing", "too_old", "broken"] as const) {
      expect(ENV_STATUS[s].pulse).toBe(false);
      expect(ENV_STATUS[s].hint).not.toBe("");
    }
  });
});
