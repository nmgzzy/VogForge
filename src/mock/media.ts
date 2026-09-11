import type { MediaInfo } from "@/lib/types";
import samples from "../../crates/vidforge-core/tests/fixtures/samples/media.json";

/**
 * 演示用媒体样本：iPhone 杜比视界 8.4、蓝光 remux（DV P7 + TrueHD Atmos + PGS）、高码率航拍、
 * 极端可变帧率录屏、相机 HLG + PCM、已高度压缩的流媒体片源。数值参照真实设备的 ffprobe 输出量级。
 *
 * 数据与 Rust 引擎的行为测试共用（crates/vidforge-core/tests/engine_behavior.rs），改动会同时影响两边。
 */
export const MOCK_MEDIA = samples as MediaInfo[];
