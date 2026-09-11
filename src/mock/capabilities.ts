import type { Capabilities } from "@/lib/types";
import dev from "../../crates/vidforge-core/tests/fixtures/samples/capabilities.json";

/**
 * 浏览器预览用的环境能力。数据取自开发机的真实探测结果（2026-09-11，vidforge-core 探测输出），
 * 包括 NVENC / AMF 的原始报错字符串，见 docs/ffmpeg-facts.md 第 7.2 节。与 Rust 引擎的行为测试共用。
 */
export const MOCK_CAPABILITIES = dev as Capabilities;
