//! `-progress pipe:1` 的解析（设计文档 4.6，技术事实文档 8.1）。
//!
//! 输出是块协议：每个周期一组 `key=value`，以 `progress=continue`（最后一块是 `progress=end`）收尾。
//! 必须按块累积再提交。`out_time_ms` 的单位其实是微秒，只用 `out_time_us`；开头几块的
//! `bitrate` / `speed` / `total_size` 可能是 `N/A`；纯音频任务没有 `frame` / `fps`。

/// 一个完整的进度块。缺失或 `N/A` 的字段为 None
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressBlock {
    pub frame: Option<u64>,
    pub fps: Option<f64>,
    pub out_time_us: Option<i64>,
    pub total_size: Option<u64>,
    pub bitrate_kbps: Option<f64>,
    pub speed: Option<f64>,
    pub dup_frames: Option<u64>,
    pub drop_frames: Option<u64>,
    /// `progress=end`：这一遍结束
    pub end: bool,
}

impl ProgressBlock {
    /// 已处理到的时间点（秒）；开头可能是负数或 N/A，按 0 处理
    pub fn out_time_sec(&self) -> Option<f64> {
        self.out_time_us.map(|us| us.max(0) as f64 / 1e6)
    }
}

#[derive(Debug, Default)]
pub struct ProgressParser {
    cur: ProgressBlock,
}

fn num<T: std::str::FromStr>(v: &str) -> Option<T> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("n/a") { None } else { v.parse().ok() }
}

impl ProgressParser {
    /// 喂一行；凑满一块时返回这一块
    pub fn feed(&mut self, line: &str) -> Option<ProgressBlock> {
        let (key, value) = line.trim().split_once('=')?;
        let c = &mut self.cur;
        match key.trim() {
            "frame" => c.frame = num(value),
            "fps" => c.fps = num(value),
            "out_time_us" => c.out_time_us = num(value),
            "total_size" => c.total_size = num(value),
            "bitrate" => c.bitrate_kbps = num(value.trim().trim_end_matches("kbits/s")),
            "speed" => c.speed = num(value.trim().trim_end_matches('x')),
            "dup_frames" => c.dup_frames = num(value),
            "drop_frames" => c.drop_frames = num(value),
            "progress" => {
                let mut block = std::mem::take(&mut self.cur);
                block.end = value.trim() == "end";
                return Some(block);
            }
            _ => {}
        }
        None
    }
}

/// 开头这么多秒内速度不可信，不给剩余时间
pub const ETA_WARMUP_SEC: f64 = 5.0;

/// 速度平滑与剩余时间估计。
///
/// 瞬时速度做指数平滑（α = 0.2）；再与全程平均速度（已处理时长 ÷ 实际耗时）按进度加权，
/// 开头偏向全程平均、后期偏向瞬时，避免场景切换让剩余时间来回跳。
#[derive(Debug, Default, Clone)]
pub struct SpeedTracker {
    smoothed: Option<f64>,
}

impl SpeedTracker {
    /// `active_sec` 是这一遍实际运行的时间（不含暂停），返回（显示用速度，这一遍的剩余秒数）
    pub fn update(
        &mut self,
        out_sec: f64,
        reported: Option<f64>,
        active_sec: f64,
        duration: f64,
    ) -> (f64, Option<f64>) {
        let average = if active_sec > 0.0 { out_sec / active_sec } else { 0.0 };
        let current = reported.filter(|s| *s > 0.0 && s.is_finite()).unwrap_or(average);
        let smoothed = match self.smoothed {
            Some(prev) => 0.8 * prev + 0.2 * current,
            None => current,
        };
        self.smoothed = Some(smoothed);
        let frac = if duration > 0.0 { (out_sec / duration).clamp(0.0, 1.0) } else { 0.0 };
        let blended = average * (1.0 - frac) + smoothed * frac;
        let eta = (active_sec >= ETA_WARMUP_SEC && blended > 0.0).then(|| ((duration - out_sec) / blended).max(0.0));
        (smoothed, eta)
    }
}

/// 两遍编码的第一遍耗时约为第二遍的 0.7（第一遍用快速设置只做分析）
pub const FIRST_PASS_SHARE: f64 = 0.7 / 1.7;

/// 合并两遍的进度百分比
pub fn overall_percent(pass: Option<u8>, frac: f64) -> f64 {
    let frac = frac.clamp(0.0, 1.0);
    100.0
        * match pass {
            Some(1) => frac * FIRST_PASS_SHARE,
            Some(_) => FIRST_PASS_SHARE + frac * (1.0 - FIRST_PASS_SHARE),
            None => frac,
        }
}

/// 两遍编码时剩余时间要加上第二遍：第二遍速度约为第一遍的 0.7
pub fn overall_eta(pass: Option<u8>, pass_eta: Option<f64>, speed: f64, duration: f64) -> Option<f64> {
    let eta = pass_eta?;
    match pass {
        Some(1) if speed > 0.0 => Some(eta + duration / (speed * 0.7)),
        _ => Some(eta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(text: &str) -> Vec<ProgressBlock> {
        let mut p = ProgressParser::default();
        text.lines().filter_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn full_blocks_are_committed_only_at_progress_lines() {
        let text = "frame=120\nfps=29.97\nstream_0_0_q=28.0\nbitrate=1523.4kbits/s\ntotal_size=786432\n\
                    out_time_us=4100000\nout_time_ms=4100000\nout_time=00:00:04.100000\ndup_frames=3\n\
                    drop_frames=0\nspeed=1.23x\nprogress=continue\nframe=240\nout_time_us=8000000\nspeed=1.5x\nprogress=end\n";
        let b = feed_all(text);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].frame, Some(120));
        assert_eq!(b[0].out_time_sec(), Some(4.1));
        assert_eq!(b[0].bitrate_kbps, Some(1523.4));
        assert_eq!((b[0].speed, b[0].dup_frames, b[0].total_size), (Some(1.23), Some(3), Some(786_432)));
        assert!(!b[0].end);
        // 第二块只带自己的字段，不继承上一块
        assert_eq!((b[1].frame, b[1].fps, b[1].dup_frames), (Some(240), None, None));
        assert!(b[1].end);
    }

    #[test]
    fn na_values_and_incomplete_blocks() {
        let b = feed_all("bitrate=N/A\ntotal_size=N/A\nout_time_us=N/A\nspeed=N/A\nprogress=continue\nframe=5\n");
        assert_eq!(b.len(), 1, "没收到 progress= 的残缺块不提交");
        assert_eq!(b[0], ProgressBlock::default());
        assert_eq!(b[0].out_time_sec(), None);
    }

    #[test]
    fn audio_only_and_negative_start_times() {
        // 纯音频任务没有 frame / fps；开头的 out_time_us 可能是负数
        let b = feed_all("out_time_us=-23220\ntotal_size=48\nspeed=0x\nprogress=continue\n");
        assert_eq!((b[0].frame, b[0].fps), (None, None));
        assert_eq!(b[0].out_time_sec(), Some(0.0));
    }

    #[test]
    fn multiple_output_streams_and_windows_line_endings() {
        let b = feed_all(
            "frame=10\r\nstream_0_0_q=23.0\r\nstream_0_1_q=-1.0\r\nout_time_us=1000000\r\nprogress=continue\r\n",
        );
        assert_eq!((b[0].frame, b[0].out_time_sec()), (Some(10), Some(1.0)));
    }

    #[test]
    fn eta_is_hidden_during_warmup_and_smooths_speed() {
        let mut t = SpeedTracker::default();
        let (s, eta) = t.update(4.0, Some(2.0), 2.0, 100.0);
        assert_eq!((s, eta), (2.0, None));
        // 一次突变只推动 20%
        let (s, _) = t.update(8.0, Some(12.0), 4.0, 100.0);
        assert!((s - 4.0).abs() < 1e-9, "{s}");
        let (_, eta) = t.update(12.0, Some(2.0), 6.0, 100.0);
        let eta = eta.expect("过了 5 秒应给出剩余时间");
        assert!((40.0..50.0).contains(&eta), "{eta}");
        // 没有 speed 字段时按全程平均
        let mut t = SpeedTracker::default();
        assert_eq!(t.update(10.0, None, 5.0, 20.0), (2.0, Some(5.0)));
    }

    #[test]
    fn two_pass_progress_is_merged() {
        assert_eq!(overall_percent(None, 0.5), 50.0);
        assert!((overall_percent(Some(1), 1.0) - FIRST_PASS_SHARE * 100.0).abs() < 1e-9);
        assert!((overall_percent(Some(2), 1.0) - 100.0).abs() < 1e-9);
        assert!(overall_percent(Some(2), 0.0) > overall_percent(Some(1), 0.99));
        assert_eq!(overall_eta(Some(2), Some(10.0), 1.0, 100.0), Some(10.0));
        assert!((overall_eta(Some(1), Some(10.0), 2.0, 70.0).unwrap() - 60.0).abs() < 1e-9);
    }
}
