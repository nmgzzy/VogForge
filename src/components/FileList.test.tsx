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
});
