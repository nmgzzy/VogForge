// 通过 WebView2 的远程调试端口驱动正在运行的 VidForge 桌面窗口，用于端到端自测与截图。
//
// 用法（Windows）：
//   1. 以调试端口启动应用：WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev
//   2. node scripts/tauri-cdp.mjs <步骤>...
//      步骤：shot:<文件>        截图
//            click:<文本>       点击包含该文本的按钮或链接
//            wait:<文本>        等待页面出现该文本（最长 30 秒）
//            fill:<占位文字>|<值> 填写输入框并回车
//            eval:<表达式>      在页面里求值并打印结果
//            size:<宽>x<高>     模拟窗口尺寸（检查最小窗口 960×640 的布局）
//            sleep:<毫秒>
import { chromium } from "playwright-core";

const port = process.env.CDP_PORT ?? "9222";
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts().flatMap((c) => c.pages())[0];
if (!page) throw new Error("没有找到 VidForge 窗口");

for (const step of process.argv.slice(2)) {
  const i = step.indexOf(":");
  const [cmd, arg] = i < 0 ? [step, ""] : [step.slice(0, i), step.slice(i + 1)];
  if (cmd === "shot") {
    await page.screenshot({ path: arg });
    console.log(`截图 ${arg}`);
  } else if (cmd === "click") {
    await page.getByRole("button", { name: arg }).or(page.getByText(arg, { exact: false })).first().click();
    console.log(`点击 ${arg}`);
  } else if (cmd === "wait") {
    await page.getByText(arg, { exact: false }).first().waitFor({ timeout: 30_000 });
    console.log(`出现 ${arg}`);
  } else if (cmd === "fill") {
    // fill:<占位文字>|<值>，填写后回车提交
    const [placeholder, value = ""] = arg.split("|");
    const input = page.getByPlaceholder(placeholder).first();
    await input.fill(value);
    await input.press("Enter");
    console.log(`填写 ${placeholder} = ${value}`);
  } else if (cmd === "eval") {
    console.log(JSON.stringify(await page.evaluate(arg), null, 2));
  } else if (cmd === "size") {
    const [width, height] = arg.split("x").map(Number);
    await page.setViewportSize({ width, height });
    console.log(`尺寸 ${width}×${height}`);
  } else if (cmd === "sleep") {
    await page.waitForTimeout(Number(arg));
  } else {
    throw new Error(`未知步骤：${step}`);
  }
}
// 不调用 browser.close()：只断开连接，不能关掉被调试的应用窗口
process.exit(0);
