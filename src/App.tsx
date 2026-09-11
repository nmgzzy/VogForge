import { useEffect } from "react";
import { backend } from "@/backend";
import { Sidebar } from "@/components/Sidebar";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { useQueue } from "@/stores/queue";
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
    return useCapabilities.subscribe((s, prev) => {
      if (s.caps !== prev.caps) useProject.getState().refreshPlans();
    });
  }, []);

  // 浏览器预览载入示例素材；桌面应用从空列表开始。队列的模拟进度在阶段 6 接入真实执行前两边都要推进
  useEffect(() => {
    if (backend.kind === "mock" && useProject.getState().files.length === 0) useProject.getState().loadSamples();
    const id = window.setInterval(() => useQueue.getState().tick(0.5), 500);
    return () => window.clearInterval(id);
  }, []);

  return (
    <div className="flex h-full">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        {view === "transcode" && <TranscodeView />}
        {view === "queue" && <QueueView />}
        {view === "environment" && <EnvironmentView />}
        {view === "presets" && <PresetsView />}
        {view === "settings" && <SettingsView />}
      </main>
    </div>
  );
}
