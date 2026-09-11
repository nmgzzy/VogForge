//! ffmpeg 自报信息的解析：`-version`、`-buildconf`、`-encoders`、`-filters` 等。
//!
//! 全部是纯函数，输入是 ffmpeg 的原始输出（可能是 CRLF），测试用真实采集的 fixture。

use std::fmt;

/// 三段式版本号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ver(pub u32, pub u32, pub u32);

impl fmt::Display for Ver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.2 == 0 { write!(f, "{}.{}", self.0, self.1) } else { write!(f, "{}.{}.{}", self.0, self.1, self.2) }
    }
}

/// 最低支持版本：杜比视界保留（`libx265 -dolbyvision`）从 7.1 开始
pub const MIN_VERSION: Ver = Ver(7, 1, 0);
/// 7.1 对应的 libavcodec 版本，用于判断没有发行版本号的 git 构建
pub const MIN_LAVC: Ver = Ver(61, 19, 0);

/// 发行版本与 libavcodec 版本的对应，用于给 git 构建推断"相当于哪个版本之后"
const RELEASE_LAVC: [(Ver, Ver); 8] = [
    (Ver(5, 0, 0), Ver(59, 18, 0)),
    (Ver(5, 1, 0), Ver(59, 37, 0)),
    (Ver(6, 0, 0), Ver(60, 3, 0)),
    (Ver(6, 1, 0), Ver(60, 31, 0)),
    (Ver(7, 0, 0), Ver(61, 3, 0)),
    (Ver(7, 1, 0), Ver(61, 19, 0)),
    (Ver(8, 0, 0), Ver(62, 11, 0)),
    (Ver(9, 0, 0), Ver(63, 1, 0)),
];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VersionInfo {
    /// `ffmpeg version` 之后的原始版本串
    pub raw: String,
    /// 发行版本号；git 构建为 None
    pub release: Option<Ver>,
    pub libavcodec: Option<Ver>,
    /// `built with` 行
    pub built_with: String,
    /// `configuration:` 行里的全部参数
    pub configuration: String,
}

impl VersionInfo {
    /// 是否满足最低版本
    pub fn meets_minimum(&self) -> bool {
        match (self.release, self.libavcodec) {
            (Some(r), _) => r >= MIN_VERSION,
            (None, Some(l)) => l >= MIN_LAVC,
            (None, None) => false,
        }
    }

    /// 面向用户的版本号：发行版直接给出，git 构建给出"X.Y+"
    pub fn display_number(&self) -> String {
        if let Some(r) = self.release {
            return r.to_string();
        }
        match self.libavcodec {
            Some(l) => RELEASE_LAVC
                .iter()
                .rev()
                .find(|(_, lavc)| l >= *lavc)
                .map(|(rel, _)| format!("{rel}+（开发版）"))
                .unwrap_or_else(|| "未知（开发版）".to_string()),
            None => "未知".to_string(),
        }
    }
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.starts_with("Exiting with exit code"))
}

/// 从版本串开头解析 `9.0.1` / `n7.1` / `6.0`。日期开头的 git 版本（2024-10-13-git）返回 None。
pub fn parse_release(raw: &str) -> Option<Ver> {
    let s = raw.strip_prefix('n').unwrap_or(raw);
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if c == '.' && !cur.is_empty() {
            parts.push(cur.clone());
            cur.clear();
        } else {
            break;
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    // 必须至少是 "主.次"，否则像 2024-10-13 这样的日期会被误读
    if parts.len() < 2 {
        return None;
    }
    let n = |i: usize| parts.get(i).and_then(|p| p.parse().ok()).unwrap_or(0);
    let v = Ver(n(0), n(1), n(2));
    (v.0 < 100).then_some(v)
}

/// 解析 `libavcodec     63.  1.101 / 63.  1.101` 这类库版本行
fn parse_lib_line(line: &str, lib: &str) -> Option<Ver> {
    let rest = line.trim_start().strip_prefix(lib)?;
    let first = rest.split('/').next()?;
    let compact: String = first.chars().filter(|c| !c.is_whitespace()).collect();
    let mut it = compact.split('.').map(|p| p.parse::<u32>().ok());
    Some(Ver(it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn parse_version(text: &str) -> Option<VersionInfo> {
    let mut info = VersionInfo::default();
    let mut found = false;
    for line in lines(text) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("ffmpeg version ").or_else(|| t.strip_prefix("ffprobe version ")) {
            info.raw = rest.split_whitespace().next().unwrap_or("").to_string();
            info.release = parse_release(&info.raw);
            found = true;
        } else if let Some(rest) = t.strip_prefix("built with ") {
            info.built_with = rest.to_string();
        } else if let Some(rest) = t.strip_prefix("configuration:") {
            info.configuration = rest.trim().to_string();
        } else if t.starts_with("libavcodec") {
            info.libavcodec = parse_lib_line(t, "libavcodec");
        }
    }
    found.then_some(info)
}

/// 取所有 `--enable-xxx` 的 xxx，来源可以是 `-buildconf` 或 `-version` 的 configuration 行
pub fn parse_enabled_flags(text: &str) -> Vec<String> {
    let mut out: Vec<String> =
        text.split_whitespace().filter_map(|t| t.strip_prefix("--enable-")).map(|s| s.to_string()).collect();
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecEntry {
    pub name: String,
    /// V / A / S
    pub kind: char,
}

/// 解析 `-encoders` / `-decoders`：分隔线 `------` 之后每行是 `标志位 名称 描述`
pub fn parse_codec_list(text: &str) -> Vec<CodecEntry> {
    let mut out = Vec::new();
    let mut body = false;
    for line in lines(text) {
        let t = line.trim();
        if !body {
            body = t.starts_with("------");
            continue;
        }
        let mut it = t.split_whitespace();
        if let (Some(flags), Some(name)) = (it.next(), it.next()) {
            if let Some(kind) = flags.chars().next().filter(|c| matches!(c, 'V' | 'A' | 'S')) {
                out.push(CodecEntry { name: name.to_string(), kind });
            }
        }
    }
    out
}

/// 解析 `-filters`：分隔线之后每行是 `标志位 名称 输入->输出 描述`
pub fn parse_filters(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut body = false;
    for line in lines(text) {
        let t = line.trim();
        if !body {
            body = t.starts_with("------");
            continue;
        }
        let mut it = t.split_whitespace();
        if let (Some(_flags), Some(name), Some(io)) = (it.next(), it.next(), it.next()) {
            if io.contains("->") {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// 解析 `-bsfs` / `-hwaccels` / `-protocols`：标题行（以冒号结尾）之后每行一个名称
pub fn parse_name_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut body = false;
    for line in lines(text) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.ends_with(':') {
            body = true;
            continue;
        }
        if body && !t.contains(' ') {
            out.push(t.to_string());
        }
    }
    out
}

/// 从 `-h encoder=xxx` 取 `Supported pixel formats:` 列表
pub fn parse_pix_fmts(help: &str) -> Vec<String> {
    for line in lines(help) {
        if let Some(rest) = line.trim().strip_prefix("Supported pixel formats:") {
            return rest.split_whitespace().map(|s| s.to_string()).collect();
        }
    }
    Vec::new()
}

/// `-h encoder=libx265` 里是否有某个私有选项，如 `-dolbyvision`
pub fn help_has_option(help: &str, option: &str) -> bool {
    lines(help).any(|l| l.split_whitespace().next() == Some(option))
}

/// 从版本信息推断构建来源，用于环境页展示与引导下载
pub fn detect_build_source(v: &VersionInfo) -> String {
    let raw = v.raw.to_ascii_lowercase();
    let conf = v.configuration.to_ascii_lowercase();
    let built = v.built_with.to_ascii_lowercase();
    if raw.contains("www.gyan.dev") {
        if raw.contains("essentials") {
            return "gyan.dev essentials".into();
        }
        if raw.contains("full") {
            return "gyan.dev full".into();
        }
        return "gyan.dev".into();
    }
    if raw.contains("jellyfin") {
        return "jellyfin-ffmpeg".into();
    }
    if raw.contains("tessus") || conf.contains("evermeet") {
        return "evermeet.cx".into();
    }
    if conf.contains("/opt/homebrew") || conf.contains("/usr/local/cellar") || conf.contains("homebrew") {
        return "Homebrew".into();
    }
    if built.contains("crosstool-ng") {
        return "BtbN".into();
    }
    if raw.contains("ubuntu") || raw.contains("debian") {
        return "Linux 发行版".into();
    }
    "未知来源".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_VERSION: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/version.txt");
    const FULL_BUILDCONF: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/buildconf.txt");
    const FULL_ENCODERS: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/encoders.txt");
    const FULL_FILTERS: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/filters.txt");
    const FULL_BSFS: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/bsfs.txt");
    const FULL_HWACCELS: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/hwaccels.txt");
    const FULL_X265: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-full-9.0.1/h-libx265.txt");
    const ESS_VERSION: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-essentials-9.0.1/version.txt");
    const ESS_BUILDCONF: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-essentials-9.0.1/buildconf.txt");
    const OLD_VERSION: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-essentials-6.0/version.txt");
    const OLD_X265: &str = include_str!("../../tests/fixtures/ffmpeg/gyan-essentials-6.0/h-libx265.txt");

    #[test]
    fn version_gyan_full() {
        let v = parse_version(FULL_VERSION).unwrap();
        assert_eq!(v.raw, "9.0.1-full_build-www.gyan.dev");
        assert_eq!(v.release, Some(Ver(9, 0, 1)));
        assert_eq!(v.libavcodec, Some(Ver(63, 1, 101)));
        assert!(v.meets_minimum());
        assert_eq!(v.display_number(), "9.0.1");
        assert_eq!(detect_build_source(&v), "gyan.dev full");
        assert!(v.configuration.contains("--enable-libplacebo"));
    }

    #[test]
    fn version_essentials_and_old() {
        let e = parse_version(ESS_VERSION).unwrap();
        assert_eq!(detect_build_source(&e), "gyan.dev essentials");
        assert!(e.meets_minimum());

        let o = parse_version(OLD_VERSION).unwrap();
        assert_eq!(o.release, Some(Ver(6, 0, 0)));
        assert!(!o.meets_minimum(), "6.0 低于 7.1");
        assert_eq!(o.display_number(), "6.0");
    }

    #[test]
    fn release_strings_from_various_builds() {
        assert_eq!(parse_release("n7.1-153-gaeb8631048-20241016"), Some(Ver(7, 1, 0)));
        assert_eq!(parse_release("7.1"), Some(Ver(7, 1, 0)));
        assert_eq!(parse_release("7.0.2-Jellyfin"), Some(Ver(7, 0, 2)));
        assert_eq!(parse_release("7.1-tessus"), Some(Ver(7, 1, 0)));
        assert_eq!(parse_release("6.1.1-3ubuntu5"), Some(Ver(6, 1, 1)));
        // git 构建：master 与日期开头的都不是发行版
        assert_eq!(parse_release("N-117474-g3f6e3ba3d4-20241016"), None);
        assert_eq!(parse_release("2024-10-13-git-e347b4ff31-full_build-www.gyan.dev"), None);
    }

    #[test]
    fn git_build_uses_libavcodec() {
        let text = "ffmpeg version N-117474-g3f6e3ba3d4-20241016 Copyright (c) 2000-2024 the FFmpeg developers\r\n\
                    built with gcc 14.2.0 (crosstool-NG 1.26.0.120_4d36f27)\r\n\
                    libavcodec     61. 22.100 / 61. 22.100\r\n";
        let v = parse_version(text).unwrap();
        assert_eq!(v.release, None);
        assert_eq!(v.libavcodec, Some(Ver(61, 22, 100)));
        assert!(v.meets_minimum());
        assert_eq!(v.display_number(), "7.1+（开发版）");
        assert_eq!(detect_build_source(&v), "BtbN");

        let old = "ffmpeg version N-1-gabc\nlibavcodec     61.  3.100 / 61.  3.100\n";
        assert!(!parse_version(old).unwrap().meets_minimum(), "7.0 之后、7.1 之前的开发版不满足");
    }

    #[test]
    fn build_source_homebrew_and_others() {
        let brew = VersionInfo {
            raw: "7.1".into(),
            configuration: "--prefix=/opt/homebrew/Cellar/ffmpeg/7.1_3 --enable-shared".into(),
            ..Default::default()
        };
        assert_eq!(detect_build_source(&brew), "Homebrew");
        let jf = VersionInfo { raw: "7.0.2-Jellyfin".into(), ..Default::default() };
        assert_eq!(detect_build_source(&jf), "jellyfin-ffmpeg");
        let ev = VersionInfo { raw: "7.1-tessus".into(), ..Default::default() };
        assert_eq!(detect_build_source(&ev), "evermeet.cx");
    }

    #[test]
    fn buildconf_flags() {
        let full = parse_enabled_flags(FULL_BUILDCONF);
        for f in ["libx265", "libx264", "libsvtav1", "libplacebo", "vulkan", "opencl", "libzimg", "libvpl"] {
            assert!(full.contains(&f.to_string()), "full 应含 {f}");
        }
        let ess = parse_enabled_flags(ESS_BUILDCONF);
        assert!(ess.contains(&"libx265".to_string()));
        for f in ["libsvtav1", "libplacebo", "opencl", "vulkan"] {
            assert!(!ess.contains(&f.to_string()), "essentials 不应含 {f}");
        }
    }

    #[test]
    fn encoders_list() {
        let list = parse_codec_list(FULL_ENCODERS);
        let has = |n: &str| list.iter().any(|e| e.name == n && e.kind == 'V');
        for n in ["libx264", "libx265", "libsvtav1", "hevc_qsv", "av1_qsv", "hevc_nvenc", "hevc_amf", "vp9_qsv"] {
            assert!(has(n), "应解析出 {n}");
        }
        assert!(list.iter().any(|e| e.name == "aac" && e.kind == 'A'));
        // 标题说明行（" V..... = Video"）不能被当成编码器
        assert!(!list.iter().any(|e| e.name == "="));
    }

    #[test]
    fn filters_list() {
        let f = parse_filters(FULL_FILTERS);
        for n in ["libplacebo", "tonemap_opencl", "zscale", "tonemap", "aresample", "pan", "alimiter", "loudnorm"] {
            assert!(f.contains(&n.to_string()), "应含滤镜 {n}");
        }
        assert!(!f.contains(&"=".to_string()));
    }

    #[test]
    fn name_lists() {
        let bsfs = parse_name_list(FULL_BSFS);
        assert!(bsfs.contains(&"dovi_rpu".to_string()));
        assert!(bsfs.contains(&"hevc_mp4toannexb".to_string()));
        let hw = parse_name_list(FULL_HWACCELS);
        assert_eq!(hw, ["cuda", "vaapi", "dxva2", "qsv", "d3d11va", "opencl", "vulkan", "d3d12va", "amf"]);
    }

    #[test]
    fn protocols_skip_section_headers() {
        let text = "Supported file protocols:\r\nInput:\r\n  file\r\n  http\r\nOutput:\r\n  file\r\n  pipe\r\n";
        assert_eq!(parse_name_list(text), ["file", "http", "file", "pipe"]);
    }

    #[test]
    fn dolbyvision_option() {
        assert!(help_has_option(FULL_X265, "-dolbyvision"));
        assert!(!help_has_option(OLD_X265, "-dolbyvision"), "6.0 的 libx265 没有 -dolbyvision");
    }

    #[test]
    fn pix_fmts() {
        let help = "Encoder h264_qsv [H.264]:\n    Supported pixel formats: nv12 qsv\n";
        assert_eq!(parse_pix_fmts(help), ["nv12", "qsv"]);
        assert!(parse_pix_fmts("nothing").is_empty());
    }
}
