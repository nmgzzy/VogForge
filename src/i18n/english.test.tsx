import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { mockQueue } from "@/backend/mock";
import { Sidebar } from "@/components/Sidebar";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { localizeCaps } from "@/lib/engine";
import { formatEta } from "@/lib/format";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { useQueue } from "@/stores/queue";
import { useSettings } from "@/stores/settings";
import { EnvironmentView } from "@/views/EnvironmentView";
import { PresetsView } from "@/views/PresetsView";
import { QueueView } from "@/views/QueueView";
import { SettingsView } from "@/views/SettingsView";
import { TranscodeView } from "@/views/TranscodeView";
import { setLang } from ".";

/** 素材自带的文字（文件名、路径、音轨与字幕标题、设备名）不属于界面文案 */
const MEDIA_TEXT = MOCK_MEDIA.flatMap((m) => [
  m.name,
  m.name.replace(/\.[^.]+$/, ""),
  ...m.path.split(/[\\/]/),
  m.device ?? "",
  ...m.audio.map((a) => a.title ?? ""),
  ...m.subtitle.map((s) => s.title ?? ""),
])
  .concat(["简体中文"]) // 语言选择框里的中文名本来就该是中文
  .filter(Boolean)
  .sort((a, b) => b.length - a.length);

const HAN = /[一-鿿，。；：（）「」、]/;

/** 页面上可见的文字与悬停提示里残留的中文 */
function leftovers(root: HTMLElement): string[] {
  const texts = [root.textContent ?? ""];
  root.querySelectorAll("[title],[aria-label],[placeholder]").forEach((el) => {
    for (const a of ["title", "aria-label", "placeholder"]) texts.push(el.getAttribute(a) ?? "");
  });
  root.querySelectorAll("option").forEach((o) => texts.push(o.textContent ?? ""));
  return texts
    .map((t) => MEDIA_TEXT.reduce((acc, m) => acc.split(m).join(" "), t))
    .flatMap((t) => t.split(/\s{2,}|\n/))
    .filter((t) => HAN.test(t));
}

describe("英文界面", () => {
  beforeAll(async () => {
    setLang("en");
    await useSettings.getState().update({ ...DEFAULT_SETTINGS, language: "en" });
    useCapabilities.setState({ caps: localizeCaps(MOCK_CAPABILITIES, "en"), probing: false, error: undefined });
  });
  afterAll(async () => {
    setLang("zh-CN");
    await useSettings.getState().update({ ...DEFAULT_SETTINGS });
    useCapabilities.setState({ caps: MOCK_CAPABILITIES });
  });
  beforeEach(() => {
    useProject.setState({ files: [], plans: {}, selectedId: undefined });
    useProject.getState().loadSamples();
  });

  it("转码页：每个示例素材的全部文字都是英文（素材自带的名字除外）", () => {
    for (const m of MOCK_MEDIA) {
      useProject.getState().select(m.id);
      const { container, unmount } = render(<TranscodeView />);
      expect(leftovers(container), m.id).toEqual([]);
      unmount();
    }
    render(<TranscodeView />);
    expect(screen.getByText("Choose a purpose")).toBeInTheDocument();
    expect(screen.getAllByText("Why these settings").length).toBeGreaterThan(0);
  });

  it("队列、环境、预设、设置与侧栏都是英文", () => {
    mockQueue.reset();
    const off = useQueue.getState().init();
    for (const View of [QueueView, EnvironmentView, PresetsView, SettingsView, Sidebar]) {
      const { container, unmount } = render(<View />);
      expect(leftovers(container), View.name).toEqual([]);
      unmount();
    }
    off();
  });

  it("格式化函数跟随语言", () => {
    expect(formatEta(185)).toBe("about 3 min");
  });
});
