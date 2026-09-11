import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { useSettings } from "@/stores/settings";
import { SettingsView } from "./SettingsView";

describe("设置页", () => {
  beforeEach(async () => {
    await useSettings.getState().update({ ...DEFAULT_SETTINGS });
  });

  it("改为覆盖同名文件前要求确认，不同意就保持原样", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<SettingsView />);
    fireEvent.click(screen.getByRole("radio", { name: "覆盖" }));
    await waitFor(() => expect(confirm).toHaveBeenCalledOnce());
    expect(confirm.mock.calls[0]![0]).toContain("无法撤销");
    expect(useSettings.getState().settings.conflict).toBe("rename");

    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByRole("radio", { name: "覆盖" }));
    await waitFor(() => expect(useSettings.getState().settings.conflict).toBe("overwrite"));
    // 其他策略不打扰
    confirm.mockClear();
    fireEvent.click(screen.getByRole("radio", { name: "跳过" }));
    await waitFor(() => expect(useSettings.getState().settings.conflict).toBe("skip"));
    expect(confirm).not.toHaveBeenCalled();
    confirm.mockRestore();
  });
});
