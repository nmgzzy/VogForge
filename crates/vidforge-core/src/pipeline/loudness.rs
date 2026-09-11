//! 响度标准化（设计文档 5.4，技术事实文档 6.5）：两遍 `loudnorm`，目标 -16 LUFS / -1.5 dBTP / LRA 11。
//!
//! 第一遍对每条要重新编码的音轨单独测量（输出 JSON 到 stderr），第二遍把测得的值带进滤镜做线性调整；
//! 单遍 loudnorm 开头几秒会有音量爬升。loudnorm 的输出固定是 192 kHz，后面必须接 `aresample`
//! 把采样率降回来，否则 AAC 会被自动定到 96 kHz、FLAC 直接写 192 kHz（阶段 6 实测）。
//! 原样复制的音轨无法加滤镜，不做标准化。

use serde::Deserialize;

use crate::model::{AudioCodec, MediaInfo, StreamAction, TranscodePlan};

use super::args::downmix_filter;

pub const TARGET_I: f64 = -16.0;
pub const TARGET_TP: f64 = -1.5;
pub const TARGET_LRA: f64 = 11.0;

/// 第一遍测得的值
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessMeasure {
    pub input_i: f64,
    pub input_tp: f64,
    pub input_lra: f64,
    pub input_thresh: f64,
    pub target_offset: f64,
}

fn target() -> String {
    format!("loudnorm=I={TARGET_I}:TP={TARGET_TP}:LRA={TARGET_LRA}")
}

/// 输出音轨在 loudnorm 之前要做的处理（多声道降为立体声时的 pan 矩阵），测量与编码两遍保持一致
pub(crate) fn pre_filter(media: &MediaInfo, plan: &TranscodePlan, track: usize) -> Option<String> {
    let t = plan.audio.get(track)?;
    let src = media.audio.iter().find(|a| a.index == t.source_index)?;
    (t.channels == Some(2) && src.channels > 2).then(|| downmix_filter(src))
}

/// 这条输出音轨是否做响度标准化
pub fn applies(plan: &TranscodePlan, track: usize) -> bool {
    plan.loudnorm && plan.audio.get(track).is_some_and(|t| t.action == StreamAction::Encode)
}

/// loudnorm 之后的采样率：Opus 固定 48 kHz，其余沿用源的采样率
pub(crate) fn output_rate(media: &MediaInfo, plan: &TranscodePlan, track: usize) -> u32 {
    let t = &plan.audio[track];
    if t.codec == Some(AudioCodec::Opus) {
        return 48_000;
    }
    media.audio.iter().find(|a| a.index == t.source_index).map_or(48_000, |a| a.sample_rate).clamp(8_000, 192_000)
}

/// 第二遍的 loudnorm 滤镜（后接 aresample）。没有测量值时是单遍写法，界面预览用
pub fn filter(measure: Option<&LoudnessMeasure>, rate: u32) -> String {
    let norm = match measure {
        Some(m) => format!(
            "{}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
            target(),
            m.input_i,
            m.input_tp,
            m.input_lra,
            m.input_thresh,
            m.target_offset
        ),
        None => target(),
    };
    format!("{norm},aresample={rate}")
}

/// 第一遍：测量第 `track` 条输出音轨的响度。只解码这一条音轨，输出丢弃
pub fn measure_args(media: &MediaInfo, plan: &TranscodePlan, track: usize) -> Option<Vec<String>> {
    if !applies(plan, track) {
        return None;
    }
    let t = &plan.audio[track];
    let mut chain: Vec<String> = pre_filter(media, plan, track).into_iter().collect();
    chain.push(format!("{}:print_format=json", target()));
    let a = [
        "ffmpeg",
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "info",
        "-i",
        &media.path,
        "-map",
        &format!("0:{}", t.source_index),
        "-filter:a",
        &chain.join(","),
        "-f",
        "null",
        "-",
    ];
    Some(a.iter().map(|s| s.to_string()).collect())
}

/// 每条要标准化的输出音轨的测量命令，按音轨顺序
pub fn measure_all(media: &MediaInfo, plan: &TranscodePlan) -> Vec<(usize, Vec<String>)> {
    (0..plan.audio.len()).filter_map(|i| measure_args(media, plan, i).map(|a| (i, a))).collect()
}

#[derive(Deserialize)]
struct Raw {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

/// 从第一遍的 stderr 里取出 loudnorm 打印的 JSON（最后一个花括号块）
pub fn parse_measure(stderr: &str) -> Option<LoudnessMeasure> {
    let end = stderr.rfind('}')?;
    let start = stderr[..end].rfind('{')?;
    let raw: Raw = serde_json::from_str(&stderr[start..=end]).ok()?;
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    Some(LoudnessMeasure {
        input_i: num(&raw.input_i)?,
        input_tp: num(&raw.input_tp)?,
        input_lra: num(&raw.input_lra)?,
        input_thresh: num(&raw.input_thresh)?,
        target_offset: num(&raw.target_offset)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 开发机 ffmpeg 9.0.1 对 -25 dB 正弦波的真实输出（节选）
    const STDERR: &str = "[Parsed_loudnorm_0 @ 000001a2b3c4d5e0] \n{\n\t\"input_i\" : \"-46.75\",\n\t\"input_tp\" : \"-43.06\",\n\t\"input_lra\" : \"0.00\",\n\t\"input_thresh\" : \"-56.75\",\n\t\"output_i\" : \"-16.04\",\n\t\"output_tp\" : \"-12.27\",\n\t\"output_lra\" : \"0.00\",\n\t\"output_thresh\" : \"-26.04\",\n\t\"normalization_type\" : \"dynamic\",\n\t\"target_offset\" : \"0.04\"\n}\n[out#0/null @ 0000] video:0KiB audio:3750KiB";

    #[test]
    fn parses_the_json_block_from_stderr() {
        let m = parse_measure(STDERR).unwrap();
        assert_eq!(
            (m.input_i, m.input_tp, m.input_lra, m.input_thresh, m.target_offset),
            (-46.75, -43.06, 0.0, -56.75, 0.04)
        );
        assert_eq!(parse_measure("no json here"), None);
        assert_eq!(
            parse_measure(
                r#"{"input_i":"-inf","input_tp":"1","input_lra":"1","input_thresh":"1","target_offset":"1"}"#
            ),
            None
        );
    }

    #[test]
    fn second_pass_uses_the_measurements_linearly_and_resamples() {
        let m = parse_measure(STDERR).unwrap();
        assert_eq!(
            filter(Some(&m), 48_000),
            "loudnorm=I=-16:TP=-1.5:LRA=11:measured_I=-46.75:measured_TP=-43.06:measured_LRA=0:measured_thresh=-56.75:offset=0.04:linear=true,aresample=48000"
        );
        assert_eq!(filter(None, 44_100), "loudnorm=I=-16:TP=-1.5:LRA=11,aresample=44100");
    }
}
