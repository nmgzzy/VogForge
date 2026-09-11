//! 编码 × 容器兼容矩阵（技术事实文档第 9 节）。

use crate::model::{Codec, Container};

/// 音频能否原样放进容器。MKV 什么都装得下；MP4 / MOV 按白名单
pub fn audio_fits(codec: &str, container: Container) -> bool {
    match container {
        Container::Mkv => true,
        Container::Mp4 => matches!(codec, "aac" | "ac3" | "eac3" | "opus" | "mp3" | "flac"),
        Container::Mov => matches!(codec, "aac" | "ac3" | "eac3" | "alac" | "pcm_s16le" | "pcm_s24le"),
    }
}

/// 字幕能否原样放进容器：图形字幕（PGS / VobSub）只有 MKV 能装
pub fn subtitle_fits(image_based: bool, container: Container) -> bool {
    container == Container::Mkv || !image_based
}

/// 视频编码能否放进容器（v1 用到的三种格式在三种容器里都可以，MP4 / MOV 的 HEVC 需要 hvc1 tag）
pub fn video_fits(_codec: Codec, _container: Container) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truehd_and_pgs_do_not_fit_mp4() {
        assert!(!audio_fits("truehd", Container::Mp4));
        assert!(!audio_fits("dts", Container::Mp4));
        assert!(audio_fits("truehd", Container::Mkv));
        assert!(audio_fits("vorbis", Container::Mkv), "MKV 什么都装得下");
        assert!(!subtitle_fits(true, Container::Mp4));
        assert!(subtitle_fits(false, Container::Mp4));
        assert!(subtitle_fits(true, Container::Mkv));
    }
}
