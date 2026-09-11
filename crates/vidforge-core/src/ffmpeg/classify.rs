//! 硬件编码失败的分类（设计文档 4.2）。判据都是从真实报错采集的 stderr 子串。
//!
//! 匹配顺序有讲究：设备缺失的报错后面几乎总跟着 "Error while opening encoder … Invalid argument"
//! 之类的通用尾巴，所以先判设备缺失与资源不足，再判能力不足，最后才看参数错误。

use crate::model::FailureKind;

const DEVICE_MISSING: &[&str] = &[
    "cannot load nvcuda.dll",
    "cannot load nvencodeapi64.dll",
    "cannot load libcuda",
    "cannot load libnvidia-encode",
    "could not dynamically load cuda",
    "amfrt64.dll failed to open",
    "amfrt32.dll failed to open",
    "libamfrt64.so",
    "no capable devices found",
    "error initializing an internal mfx session",
    "error creating a mfx session",
    "no device available for encoder",
    "no device available for decoder",
    "failed to initialise vaapi connection",
    "device creation failed",
    "failed to create  hardware device context",
    "failed to create hardware device context",
    "driver does not support the required nvenc api version",
    "the minimum required nvidia driver",
];

const RESOURCE: &[&str] = &[
    "out of memory",
    "cannot allocate memory",
    "incompatible client key",
    "too many concurrent sessions",
    "maximum number of sessions",
];

const CAPABILITY: &[&str] = &[
    "10 bit encode not supported",
    "codec not supported",
    "not supported by the hardware",
    "hardware does not support",
    "unsupported profile",
    "profile is not supported",
    "cannot create compression session",
    "no capable encoder",
];

const PARAM: &[&str] = &[
    "selected ratecontrol mode is unsupported",
    "current pixel format is unsupported",
    "unable to parse",
    "error setting option",
    "unrecognized option",
    "option not found",
    "qscale not available",
    "invalid argument",
];

pub fn classify(stderr: &str) -> FailureKind {
    let s = stderr.to_ascii_lowercase();
    let hit = |list: &[&str]| list.iter().any(|p| s.contains(p));
    if hit(DEVICE_MISSING) {
        FailureKind::DeviceMissing
    } else if hit(RESOURCE) {
        FailureKind::Resource
    } else if hit(CAPABILITY) {
        FailureKind::Capability
    } else if hit(PARAM) {
        FailureKind::Param
    } else {
        FailureKind::Unknown
    }
}

/// 从一大段 stderr 里挑出最能说明问题的一行，给界面展示
pub fn key_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty() && !l.starts_with("Svt[") && !l.starts_with("Exiting with exit code"))
        .collect();
    let lower: Vec<String> = lines.iter().map(|l| l.to_ascii_lowercase()).collect();
    for list in [DEVICE_MISSING, RESOURCE, CAPABILITY, PARAM] {
        if let Some(i) = lower.iter().position(|l| list.iter().any(|p| l.contains(p))) {
            return lines[i].to_string();
        }
    }
    lines.first().map(|l| l.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use FailureKind::*;

    /// 本机实测采集的原始报错（ffmpeg 9.0.1 gyan full，Intel Arc 核显，无 NVIDIA / AMD 卡）
    const NVENC_NO_CARD: &str = "[h264_nvenc @ 000001f2e4327240] Cannot load nvcuda.dll\r\n\
[vost#0:0/h264_nvenc @ 000001f2e4733700] [enc:h264_nvenc @ 000001f2e2756640] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height.\r\n\
[vf#0:0 @ 000001f2e4327600] Error sending frames to consumers: Operation not permitted\r\n";

    const AMF_NO_CARD: &str = "[AMF @ 0000016a3681ac80] DLL amfrt64.dll failed to open\r\n\
[h264_amf @ 0000016a36807580] Failed to create  hardware device context (AMF) : Unknown error occurred\r\n\
[vost#0:0/h264_amf @ 0000016a38333580] [enc:h264_amf @ 0000016a362b6740] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height.\r\n";

    const QSV_BAD_PROFILE: &str = "[h264_qsv @ 000001ed4b5564c0] [Eval @ 000000cf847fd900] Undefined constant or missing '(' in 'high10'\r\n\
[h264_qsv @ 000001ed4b5564c0] Unable to parse \"profile\" option value \"high10\"\r\n\
[h264_qsv @ 000001ed4b5564c0] Error setting option profile to value high10.\r\n\
[vost#0:0/h264_qsv @ 000001ed4b556240] Error applying encoder options: Invalid argument\r\n";

    const CUDA_DEVICE_INIT: &str = "[CUDA @ 0000022ce1234780] Cannot load nvcuda.dll\r\n\
[CUDA @ 0000022ce1234780] Could not dynamically load CUDA\r\nDevice creation failed: -1.\r\n";

    const VAAPI_DEVICE_INIT: &str = "[VAAPI @ 000002568e212b40] Failed to initialise VAAPI connection: -1 (unknown libva error).\r\n\
Device creation failed: -5.\r\nFailed to set value 'vaapi=hw' for option 'init_hw_device': I/O error\r\n";

    #[test]
    fn real_errors_from_dev_machine() {
        assert_eq!(classify(NVENC_NO_CARD), DeviceMissing);
        assert_eq!(classify(AMF_NO_CARD), DeviceMissing);
        assert_eq!(classify(QSV_BAD_PROFILE), Param);
        assert_eq!(classify(CUDA_DEVICE_INIT), DeviceMissing);
        assert_eq!(classify(VAAPI_DEVICE_INIT), DeviceMissing);
    }

    #[test]
    fn design_table_cases() {
        let cases: &[(&str, FailureKind)] = &[
            ("[hevc_nvenc @ 0x1] Cannot load nvEncodeAPI64.dll", DeviceMissing),
            ("[hevc_qsv @ 0x1] Error initializing an internal MFX session: unsupported (-3)", DeviceMissing),
            ("No capable devices found", DeviceMissing),
            ("Driver does not support the required nvenc API version. Required: 13.0 Found: 12.1", DeviceMissing),
            (
                "[hevc_nvenc @ 0x1] 10 bit encode not supported\nError while opening encoder: Invalid argument",
                Capability,
            ),
            ("[av1_nvenc @ 0x1] Codec not supported", Capability),
            ("[hevc_qsv @ 0x1] Selected ratecontrol mode is unsupported", Param),
            ("[h264_amf @ 0x1] Current pixel format is unsupported", Param),
            ("[hevc_videotoolbox @ 0x1] qscale not available for encoder. Use -b:v bitrate instead.", Param),
            ("[hevc_nvenc @ 0x1] OpenEncodeSessionEx failed: out of memory (10): (no details)", Resource),
            ("[h264_nvenc @ 0x1] OpenEncodeSessionEx failed: incompatible client key (21): (no details)", Resource),
            ("Conversion failed!", Unknown),
            ("", Unknown),
        ];
        for (text, want) in cases {
            assert_eq!(classify(text), *want, "{text}");
        }
    }

    #[test]
    fn key_line_picks_the_cause_not_the_tail() {
        assert_eq!(key_line(NVENC_NO_CARD), "[h264_nvenc @ 000001f2e4327240] Cannot load nvcuda.dll");
        assert_eq!(key_line(AMF_NO_CARD), "[AMF @ 0000016a3681ac80] DLL amfrt64.dll failed to open");
        assert_eq!(key_line("Svt[info]: banner\nsomething else\n"), "something else");
    }
}
