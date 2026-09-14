import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ImportReportCard } from "./FileList";

describe("导入结果卡片", () => {
  it("有失败时醒目提示，并列出文件名与原因", () => {
    render(
      <ImportReportCard
        report={{ failures: [{ path: "D:\\clips\\broken.mp4", reason: "文件不完整或已损坏" }], skipped: 2, added: 3, duplicate: 0 }}
        onClose={() => undefined}
      />,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("1 个文件无法分析")).toBeInTheDocument();
    expect(screen.getByText("broken.mp4")).toBeInTheDocument();
    expect(screen.getByText("已添加 3 个，跳过 2 个非视频文件")).toBeInTheDocument();
  });

  it("只有跳过或重复时保持安静（status 而非 alert），可以关闭", () => {
    const close = vi.fn();
    render(<ImportReportCard report={{ failures: [], skipped: 0, added: 0, duplicate: 2 }} onClose={close} />);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("2 个已在列表中");
    fireEvent.click(screen.getByRole("button", { name: "关闭导入结果" }));
    expect(close).toHaveBeenCalledOnce();
  });

  it("跳过的文件列出扩展名；与文件无关的失败标题写无法导入", () => {
    const { unmount } = render(
      <ImportReportCard
        report={{ failures: [], skipped: 3, added: 0, duplicate: 0, skippedExts: [".rmvb", ".txt", ""] }}
        onClose={() => undefined}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("跳过 3 个非视频文件（.rmvb、.txt、无扩展名）");
    unmount();
    render(
      <ImportReportCard
        report={{ failures: [{ path: "", reason: "还没有可用的 ffprobe" }], skipped: 0, added: 0, duplicate: 0 }}
        onClose={() => undefined}
      />,
    );
    expect(screen.getByText("无法导入")).toBeInTheDocument();
    expect(screen.queryByText("1 个文件无法分析")).toBeNull();
  });

  it("空结果说明扫的是哪个文件夹", () => {
    render(
      <ImportReportCard report={{ failures: [], skipped: 0, added: 0, duplicate: 0, paths: ["D:/clips/旅行"] }} onClose={() => undefined} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("旅行 里没有找到视频文件");
  });

  it("一个都没加进来（例如空文件夹）时也给出结果，不能表现成没反应", () => {
    render(<ImportReportCard report={{ failures: [], skipped: 0, added: 0, duplicate: 0 }} onClose={() => undefined} />);
    expect(screen.getByRole("status")).toHaveTextContent("没有找到视频文件");
  });
});
