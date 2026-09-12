//! 帧率策略（设计文档 4.9，技术事实文档 4.3–4.4）。

use crate::model::{FpsInsight, VideoStream};

/// 标准帧率档。NTSC 帧率用精确分数，传给 ffmpeg 也用分数形式，避免 29.97 这类近似值累积时间轴误差
pub struct StandardFps {
    pub value: f64,
    pub arg: &'static str,
    pub label: &'static str,
}

pub const STANDARD_FPS: [StandardFps; 9] = [
    StandardFps { value: 24000.0 / 1001.0, arg: "24000/1001", label: "23.976" },
    StandardFps { value: 24.0, arg: "24", label: "24" },
    StandardFps { value: 25.0, arg: "25", label: "25" },
    StandardFps { value: 30000.0 / 1001.0, arg: "30000/1001", label: "29.97" },
    StandardFps { value: 30.0, arg: "30", label: "30" },
    StandardFps { value: 50.0, arg: "50", label: "50" },
    StandardFps { value: 60000.0 / 1001.0, arg: "60000/1001", label: "59.94" },
    StandardFps { value: 60.0, arg: "60", label: "60" },
    StandardFps { value: 120.0, arg: "120", label: "120" },
];

const SNAP_TOLERANCE: f64 = 0.02;
/// 误差小于它才认为源本来就是这个档，典型是真正的 NTSC 源
const EXACT_TOLERANCE: f64 = 0.001;

/// 吸附到标准帧率档（容差 2%），吸附不上时四舍五入到整数。
///
/// 29.97 与 30 只差 0.1%，"取最近"会被噪声左右；可变帧率的实际平均值因掉帧总是偏低（29.41、29.8），
/// 按最近档会被误判成 NTSC。所以只有误差小于 0.1% 才采纳 NTSC 档，其余模糊情况优先整数档。
pub fn snap_fps(fps: f64) -> f64 {
    let mut within: Vec<(f64, f64)> = STANDARD_FPS
        .iter()
        .map(|s| (s.value, (fps - s.value).abs() / s.value))
        .filter(|(_, err)| *err <= SNAP_TOLERANCE)
        .collect();
    within.sort_by(|a, b| a.1.total_cmp(&b.1));
    let Some(&(nearest, err)) = within.first() else { return fps.round() };
    if err < EXACT_TOLERANCE {
        return nearest;
    }
    within.iter().find(|(v, _)| v.fract() == 0.0).map(|(v, _)| *v).unwrap_or(nearest)
}

/// 传给 `-r` 的帧率写法：标准档用分数或整数，其余保留三位小数
pub fn fps_arg(fps: f64) -> String {
    if let Some(s) = STANDARD_FPS.iter().find(|s| (s.value - fps).abs() < 1e-6) {
        return s.arg.to_string();
    }
    format_number((fps * 1000.0).round() / 1000.0)
}

/// 与 JS 的 `String(number)` 一致：整数不带小数点
pub fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// 源的帧率（吸附到标准档）：优先名义帧率，名义帧率异常（录屏常见 1000/1）时用平均帧率。
/// 两者都读不出（ffprobe 给 0/0）时为 None。它也是固定帧率目标的上限（不提帧率）
pub fn source_rate(v: &VideoStream) -> Option<f64> {
    let nominal_sane = (10.0..=240.0).contains(&v.fps_nominal);
    let fps = snap_fps(if nominal_sane { v.fps_nominal } else { v.fps_avg });
    (fps.is_finite() && fps > 0.0).then_some(fps)
}

/// 推荐的 CFR 目标：源的帧率（只复制帧、不丢帧）；源帧率读不出时用 30
pub fn recommend_cfr_target(v: &VideoStream) -> f64 {
    source_rate(v).unwrap_or(30.0)
}

/// 帧率波动剧烈：平均帧率不到名义帧率的 60%，典型如录屏
pub fn is_extreme_vfr(v: &VideoStream) -> bool {
    v.is_vfr && v.fps_nominal > 0.0 && v.fps_avg / v.fps_nominal < 0.6
}

pub fn fps_insight(v: &VideoStream, duration_sec: f64, target_fps: f64) -> FpsInsight {
    let source_frames = v.frame_count.unwrap_or_else(|| (v.fps_avg * duration_sec).round() as u64);
    let target_frames = (target_fps * duration_sec).round() as u64;
    FpsInsight {
        source_frames,
        target_frames,
        duplicated: target_frames.saturating_sub(source_frames),
        dropped: source_frames.saturating_sub(target_frames),
        target_fps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_prefers_integer_unless_exact_ntsc() {
        assert_eq!(snap_fps(29.41), 30.0, "掉帧的 30fps 不能被当成 29.97");
        assert_eq!(snap_fps(29.8), 30.0);
        assert!((snap_fps(30000.0 / 1001.0) - 30000.0 / 1001.0).abs() < 1e-9);
        assert!((snap_fps(23.976) - 24000.0 / 1001.0).abs() < 1e-9);
        assert_eq!(snap_fps(17.06), 17.0, "偏离所有标准档时四舍五入");
    }

    #[test]
    fn fps_args() {
        assert_eq!(fps_arg(30000.0 / 1001.0), "30000/1001");
        assert_eq!(fps_arg(30.0), "30");
        assert_eq!(fps_arg(17.0), "17");
        assert_eq!(fps_arg(12.3456), "12.346");
    }
}
