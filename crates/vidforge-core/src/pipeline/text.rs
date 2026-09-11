//! 给界面文案用的数字格式化。与前端 `src/lib/format.ts` 的同名函数输出一致（JS 的 toFixed / toLocaleString 语义）。

/// JS `Number.prototype.toFixed`：按二进制的精确十进制值舍入，恰好落在一半时远离零舍入。
/// Rust 的格式化同样按精确值舍入，只有"恰好一半"时向偶数舍入，这里单独处理这种情况
pub fn to_fixed(v: f64, digits: usize) -> String {
    let long = format!("{:.*}", digits + 25, v);
    let tail = long.find('.').map(|dot| &long[dot + 1 + digits..]).unwrap_or("");
    if tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0') {
        let scale = 10f64.powi(digits as i32);
        let up = (v.abs() * scale).floor() + 1.0;
        return format!("{:.*}", digits, v.signum() * up / scale);
    }
    format!("{:.*}", digits, v)
}

/// 整数不带小数点，与 JS 的 `String(number)` 一致
pub fn plain(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// 千分位，与 JS 默认区域的 `toLocaleString()` 一致：`3959` → `3,959`
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 29.97 保留两位，30 显示整数
pub fn format_fps(fps: f64) -> String {
    if !fps.is_finite() || fps <= 0.0 {
        return "—".into();
    }
    let r = fps.round();
    if (fps - r).abs() < 0.005 { plain(r) } else { to_fixed(fps, 2) }
}

pub fn format_percent(ratio: f64) -> String {
    if !ratio.is_finite() {
        return "—".into();
    }
    format!("{}%", plain((ratio * 100.0).round()))
}

pub fn format_bitrate(bps: f64) -> String {
    if bps <= 0.0 || !bps.is_finite() {
        return "—".into();
    }
    if bps >= 1_000_000.0 {
        format!("{} Mbps", to_fixed(bps / 1_000_000.0, if bps >= 10_000_000.0 { 0 } else { 1 }))
    } else {
        format!("{} kbps", plain((bps / 1000.0).round()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_js_formatting() {
        assert_eq!(to_fixed(4.25, 1), "4.3", "JS toFixed 恰好一半向上");
        assert_eq!(to_fixed(1.005, 2), "1.00", "1.005 的二进制略小于 1.005");
        assert_eq!(to_fixed(1.45, 1), "1.4", "1.45 的二进制也略小于 1.45");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(29.97002997, 2), "29.97");
        assert_eq!(thousands(3959), "3,959");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
        assert_eq!(format_fps(30.0), "30");
        assert_eq!(format_fps(30000.0 / 1001.0), "29.97");
        assert_eq!(format_percent(0.456), "46%");
        assert_eq!(format_bitrate(45_200_000.0), "45 Mbps");
        assert_eq!(format_bitrate(4_520_000.0), "4.5 Mbps");
        assert_eq!(format_bitrate(256_000.0), "256 kbps");
    }
}
