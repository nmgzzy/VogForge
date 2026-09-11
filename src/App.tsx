import { useEffect } from "react";
import { backend } from "@/backend";
import { Onboarding } from "@/components/Onboarding";
import { Sidebar } from "@/components/Sidebar";
import { setLang } from "@/i18n";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { useQueue } from "@/stores/queue";
import { watchRunEnd } from "@/stores/run-end";
import { useSettings } from "@/stores/settings";
import { applyTheme, useUi } from "@/stores/ui";
import { EnvironmentView } from "@/views/EnvironmentView";
import { PresetsView } from "@/views/PresetsView";
import { QueueView } from "@/views/QueueView";
import { SettingsView } from "@/views/SettingsView";
import { TranscodeView } from "@/views/TranscodeView";

export default function App() {
  const view = useUi((s) => s.view);
  const theme = useUi((s) => s.theme);
  const lang = useSettings((s) => s.settings.language);
  // 先于子组件同步语言，子组件渲染时的 tr() 就是新语言；下面的 key 让整个界面按新语言重新挂载
  setLang(lang);

  // 跟随系统主题时监听系统切换
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = () => applyTheme("system");
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [theme]);

  // 启动：读取设置、探测环境（优先命中缓存）。能力变化后按新能力重新整理已有计划
  useEffect(() => {
    void useSettings.getState().load();
    void useCapabilities.getState().load();
    const offCaps = useCapabilities.subscribe((s, prev) => {
      if (s.caps !== prev.caps) useProject.getState().refreshPlans();
    });
    // 硬件编码 / 解码开关会改变可用的编码器，同样要重新整理
    const offSettings = useSettings.subscribe((s, prev) => {
      const a = s.settings;
      const b = prev.settings;
      if (a.hwEncode !== b.hwEncode || a.hwDecode !== b.hwDecode) useProject.getState().refreshPlans();
      // 环境页的说明由后端按语言生成：换语言后重新取一次（命中缓存，不重新探测）
      if (a.language !== b.language) void useCapabilities.getState().load();
    });
    return () => {
      offCaps();
      offSettings();
    };
  }, []);

  // 队列状态来自后端推送；浏览器预览载入示例素材，桌面应用从空列表开始。一批任务跑完后发通知、执行完成后动作
  useEffect(() => {
    if (backend.kind === "mock" && useProject.getState().files.length === 0) useProject.getState().loadSamples();
    const offQueue = useQueue.getState().init();
    const offRunEnd = watchRunEnd();
    return () => {
      offQueue();
      offRunEnd();
    };
  }, []);

  return (
    <div key={lang} className="flex h-full">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        {view === "transcode" && <TranscodeView />}
        {view === "queue" && <QueueView />}
        {view === "environment" && <EnvironmentView />}
        {view === "presets" && <PresetsView />}
        {view === "settings" && <SettingsView />}
      </main>
      <Onboarding />
    </div>
  );
}
