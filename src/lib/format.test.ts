import { describe, expect, it } from "vitest";
import {
  argsToCommand,
  channelLabel,
  formatBitrate,
  formatBytes,
  formatDuration,
  formatEta,
  formatFps,
  formatTimeRange,
  quoteArg,
  resolutionLabel,
  splitArgs,
} from "./format";

describe("formatBytes", () => {
  it("小于 10 的值多给一位小数", () => {
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1.50 GB");
    expect(formatBytes(123 * 1024 ** 2)).toBe("123.0 MB");
  });
  it("字节不带小数", () => {
    expect(formatBytes(512)).toBe("512 B");
  });
  it("非法输入返回 0 B", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });
});

describe("formatDuration", () => {
  it("不足一小时显示 m:ss", () => {
    expect(formatDuration(134)).toBe("2:14");
    expect(formatDuration(5)).toBe("0:05");
  });
  it("超过一小时显示 h:mm:ss", () => {
    expect(formatDuration(3600 + 125)).toBe("1:02:05");
  });
  it("非法输入", () => {
    expect(formatDuration(-1)).toBe("--:--");
  });
});

describe("formatEta", () => {
  it("未知时显示计算中", () => {
    expect(formatEta(undefined)).toBe("计算中");
  });
  it("按量级选择单位", () => {
    expect(formatEta(42)).toBe("约 42 秒");
    expect(formatEta(185)).toBe("约 3 分钟");
    expect(formatEta(3600 * 2 + 60 * 15)).toBe("约 2 小时 15 分");
    expect(formatEta(3600)).toBe("约 1 小时");
  });
});

describe("formatBitrate", () => {
  it("Mbps 与 kbps", () => {
    expect(formatBitrate(45_200_000)).toBe("45 Mbps");
    expect(formatBitrate(4_520_000)).toBe("4.5 Mbps");
    expect(formatBitrate(256_000)).toBe("256 kbps");
    expect(formatBitrate(undefined)).toBe("—");
  });
});

describe("formatFps", () => {
  it("整数帧率不带小数，NTSC 帧率保留两位", () => {
    expect(formatFps(30)).toBe("30");
    expect(formatFps(30000 / 1001)).toBe("29.97");
    expect(formatFps(24000 / 1001)).toBe("23.98");
  });
});

describe("resolutionLabel", () => {
  it("以短边判定，兼容竖屏", () => {
    expect(resolutionLabel(3840, 2160)).toBe("4K");
    expect(resolutionLabel(2160, 3840)).toBe("4K");
    expect(resolutionLabel(1920, 1080)).toBe("1080p");
    expect(resolutionLabel(1080, 1920)).toBe("1080p");
    expect(resolutionLabel(1280, 720)).toBe("720p");
  });
});

describe("channelLabel", () => {
  it("优先使用声道布局", () => {
    expect(channelLabel(8, "7.1")).toBe("7.1");
    expect(channelLabel(6, "5.1(side)")).toBe("5.1");
    expect(channelLabel(2)).toBe("立体声");
    expect(channelLabel(1)).toBe("单声道");
  });
});

describe("quoteArg / argsToCommand", () => {
  it("普通参数原样输出", () => {
    expect(argsToCommand(["-c:v", "libx265", "-crf", "20", "-map", "0:v:0"])).toBe(
      "-c:v libx265 -crf 20 -map 0:v:0",
    );
  });
  it("含空格的路径用单引号包裹", () => {
    expect(argsToCommand(["ffmpeg", "-i", "My Video.mov"])).toBe("ffmpeg -i 'My Video.mov'");
  });
  it("含管道符的滤镜用单引号包裹", () => {
    expect(quoteArg("pan=stereo|FL=FC")).toBe("'pan=stereo|FL=FC'");
  });
  it("Windows 路径的反斜杠保持原样，不被二次转义", () => {
    // PowerShell 单引号字符串完全字面，反斜杠不需要也不应该转义
    expect(quoteArg("D:\\转码输出\\a.mkv")).toBe("'D:\\转码输出\\a.mkv'");
  });
  it("PowerShell 内嵌单引号写成两个", () => {
    expect(quoteArg("it's", "powershell")).toBe("'it''s'");
  });
  it("POSIX 内嵌单引号用 '\\'' 拼接", () => {
    expect(quoteArg("it's", "posix")).toBe("'it'\\''s'");
  });
  it("双引号与美元符在单引号内无需转义", () => {
    expect(quoteArg('title="A $x"')).toBe("'title=\"A $x\"'");
  });
  it("空字符串输出一对引号", () => {
    expect(quoteArg("")).toBe("''");
  });
  it("含逗号的滤镜链必须加引号（PowerShell 会把逗号当数组运算符拆开）", () => {
    expect(quoteArg("zscale=t=linear,format=gbrpf32le")).toBe("'zscale=t=linear,format=gbrpf32le'");
  });
  it("@ 开头的参数必须加引号（PowerShell splatting）", () => {
    expect(quoteArg("@list.txt")).toBe("'@list.txt'");
  });
});

describe("splitArgs", () => {
  it("带空格的引号值保持为一个参数，且去掉引号", () => {
    expect(splitArgs('-metadata title="My Video"')).toEqual(["-metadata", "title=My Video"]);
    expect(splitArgs("-metadata 'title=My Video'")).toEqual(["-metadata", "title=My Video"]);
  });
  it("多个空白视为一个分隔", () => {
    expect(splitArgs("  -tune   grain  ")).toEqual(["-tune", "grain"]);
  });
  it("引号外的反斜杠保持字面，Windows 路径不受影响", () => {
    expect(splitArgs("-i C:\\sub\\a.srt")).toEqual(["-i", "C:\\sub\\a.srt"]);
  });
  it("双引号内支持转义引号与反斜杠", () => {
    expect(splitArgs('"say \\"hi\\"" "a\\\\b"')).toEqual(['say "hi"', "a\\b"]);
  });
  it("单引号内完全字面", () => {
    expect(splitArgs("'a\\\"b'")).toEqual(['a\\"b']);
  });
  it("空引号产生空参数", () => {
    expect(splitArgs('-x ""')).toEqual(["-x", ""]);
  });
  it("与 quoteArg 往返一致（值中不含单引号时）", () => {
    const args = ["-vf", "scale=-2:720,format=yuv420p", "D:\\out dir\\a.mkv"];
    expect(splitArgs(argsToCommand(args, "powershell"))).toEqual(args);
  });
  it("空输入", () => {
    expect(splitArgs("")).toEqual([]);
    expect(splitArgs("   ")).toEqual([]);
  });
});

describe("formatTimeRange", () => {
  it("同单位合并", () => {
    expect(formatTimeRange(20, 45)).toBe("20–45 秒");
    expect(formatTimeRange(240, 480)).toBe("4–8 分钟");
    expect(formatTimeRange(36_400, 68_000)).toBe("10.1–18.9 小时");
  });
  it("跨单位分别标注", () => {
    expect(formatTimeRange(43, 80)).toBe("43 秒 – 1 分钟");
    expect(formatTimeRange(2400, 5400)).toBe("40 分钟 – 1.5 小时");
  });
});
