//! 输出路径规划（需求 F-6.6）：输出目录、命名模板、保留源目录结构。
//!
//! 同名冲突与临时文件在执行阶段处理（queue 模块），这里只算出"想要的最终路径"。

use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::model::{MediaInfo, Scenario, StreamAction, TranscodePlan};
use crate::pipeline::args::{display_size, target_dimensions};

/// 默认输出目录名：源文件旁边的 VidForge 文件夹
pub const DEFAULT_DIR_NAME: &str = "VidForge";

fn scenario_word(s: Scenario) -> &'static str {
    match s {
        Scenario::Archive => "归档",
        Scenario::Collection => "收藏",
        Scenario::Streaming => "流媒体",
        Scenario::Mobile => "手机",
        Scenario::Social => "社交",
        Scenario::Smallest => "最小",
        Scenario::Editing => "剪辑",
        Scenario::Remux => "封装",
    }
}

/// 输出画面的短边像素数（原样封装时是源的短边）
fn output_height(media: &MediaInfo, plan: &TranscodePlan) -> u32 {
    let Some(v) = media.video.first() else { return 0 };
    if plan.video.action == StreamAction::Encode {
        if let Some(d) = target_dimensions(v, plan.video.resolution) {
            return d.w.min(d.h);
        }
    }
    let (w, h) = display_size(v);
    w.min(h)
}

/// 文件名里不能出现的字符换成下划线（Windows 最严格，按它来）
pub fn sanitize(name: &str) -> String {
    let cleaned: String =
        name.chars()
            .map(|c| {
                if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() { "output".to_string() } else { trimmed.to_string() }
}

/// 展开命名模板：`{name}` `{height}` `{codec}` `{scenario}` `{date}`。原样封装时 `{codec}` 是 `remux`
pub fn render_template(template: &str, media: &MediaInfo, plan: &TranscodePlan, date: &str) -> String {
    let stem = Path::new(&media.name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let codec = if plan.video.action == StreamAction::Copy {
        "remux".to_string()
    } else {
        format!("{:?}", plan.video.codec).to_lowercase()
    };
    let rendered = template
        .replace("{name}", &stem)
        .replace("{height}", &output_height(media, plan).to_string())
        .replace("{codec}", &codec)
        .replace("{scenario}", scenario_word(plan.scenario))
        .replace("{date}", date);
    sanitize(&rendered)
}

// 路径按字符串处理，`/` 与 `\` 都认作分隔符：这段代码也编译成 WebAssembly 在界面里运行，
// wasm 上的 std::path 只认 `/`，会把 `D:\素材\a.mov` 当成一个没有父目录的文件名

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// 父目录；没有分隔符时为空
fn parent(path: &str) -> &str {
    path.rfind(is_sep).map_or("", |i| &path[..i])
}

/// 在 `base` 后追加一段，沿用 `base` 的分隔符风格
fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        return name.to_string();
    }
    let sep = if base.contains('\\') && !base.contains('/') {
        '\\'
    } else if base.contains('/') {
        '/'
    } else {
        std::path::MAIN_SEPARATOR
    };
    format!("{}{sep}{name}", base.trim_end_matches(is_sep))
}

fn components(path: &str) -> Vec<&str> {
    path.split(is_sep).filter(|s| !s.is_empty()).collect()
}

/// `dir` 相对于 `root` 的各级子目录；不在 `root` 之下时为 None。Windows 路径不区分大小写
fn relative<'a>(dir: &'a str, root: &str) -> Option<Vec<&'a str>> {
    let (d, r) = (components(dir), components(root));
    let windows = cfg!(windows) || dir.contains('\\') || dir.as_bytes().get(1) == Some(&b':');
    let same = |a: &str, b: &str| if windows { a.to_lowercase() == b.to_lowercase() } else { a == b };
    (d.len() >= r.len() && d.iter().zip(&r).all(|(a, b)| same(a, b))).then(|| d[r.len()..].to_vec())
}

/// 输出目录：设置里指定的目录，否则源文件旁的 VidForge 文件夹；保留目录结构时再拼上相对于导入根目录的子路径
pub fn output_dir(media: &MediaInfo, settings: &Settings) -> String {
    let source_dir = parent(&media.path);
    let Some(base) = settings.output_dir.as_deref() else {
        // 没指定输出目录：直接放在源文件旁，天然保持了目录结构
        return join(source_dir, DEFAULT_DIR_NAME);
    };
    let mut dir = base.to_string();
    if settings.keep_tree {
        if let Some(rel) = media.import_root.as_deref().and_then(|root| relative(source_dir, root)) {
            for part in rel {
                dir = join(&dir, part);
            }
        }
    }
    dir
}

/// 期望的最终输出路径
pub fn output_path(media: &MediaInfo, plan: &TranscodePlan, settings: &Settings, date: &str) -> PathBuf {
    let file = format!("{}.{}", render_template(&settings.naming_template, media, plan, date), plan.container.ext());
    PathBuf::from(join(&output_dir(media, settings), &file))
}

/// 今天的日期（UTC），用于 `{date}`
pub fn today() -> String {
    crate::util::now_iso()[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capabilities, EnvStatus, Scenario};
    use crate::pipeline::recommend_plan;

    fn media(path: &str, root: Option<&str>) -> MediaInfo {
        let json = format!(
            r#"{{"id":"m","path":{path:?},"name":{name:?},"container":"mov","durationSec":10,"sizeBytes":1000,"bitrate":800,
            "video":[{{"index":0,"codec":"hevc","width":3840,"height":2160,"fpsAvg":30,"fpsNominal":30,"isVfr":false,
            "bitDepth":10,"pixFmt":"yuv420p10le","color":{{"primaries":"bt2020","transfer":"arib-std-b67","space":"bt2020nc",
            "range":"tv","hdrKind":"hlg"}},"hdr10plus":false,"rotation":0}}],"audio":[],"subtitle":[],"chapters":0,
            "attachments":0,"sourceHint":"iphone"{root}}}"#,
            name = path.rsplit(['/', '\\']).next().unwrap(),
            root = root.map(|r| format!(r#","importRoot":{r:?}"#)).unwrap_or_default(),
        );
        serde_json::from_str(&json).unwrap()
    }

    fn caps() -> Capabilities {
        Capabilities::placeholder(EnvStatus::Probing, "")
    }

    fn out(m: &MediaInfo, s: &Settings) -> String {
        let plan = recommend_plan(m, Scenario::Archive, &caps());
        output_path(m, &plan, s, "d").to_string_lossy().to_string()
    }

    #[test]
    fn default_goes_next_to_the_source() {
        let m = media("/clips/trip/IMG_1.MOV", None);
        assert_eq!(out(&m, &Settings::default()), "/clips/trip/VidForge/IMG_1_2160p_hevc.mkv");
    }

    #[test]
    fn windows_paths_work_on_every_target() {
        // 引擎也在 WebAssembly 里跑，那里的 std::path 不认反斜杠；这组断言在任何平台上都必须成立
        let m = media(r"D:\素材\trip\IMG_1.MOV", None);
        assert_eq!(out(&m, &Settings::default()), r"D:\素材\trip\VidForge\IMG_1_2160p_hevc.mkv");
        let m = media(r"\\nas\in\2026\08\IMG_1.MOV", Some(r"\\NAS\in"));
        let s = Settings { output_dir: Some(r"E:\out\".into()), keep_tree: true, ..Settings::default() };
        assert_eq!(out(&m, &s), r"E:\out\2026\08\IMG_1_2160p_hevc.mkv");
        // 不在导入根目录之下（例如单独拖进来的文件）时直接放在输出目录
        let m = media(r"C:\other\IMG_1.MOV", Some(r"\\nas\in"));
        assert_eq!(out(&m, &s), r"E:\out\IMG_1_2160p_hevc.mkv");
        // 输出目录用正斜杠时沿用正斜杠
        let s = Settings { output_dir: Some("E:/out".into()), keep_tree: false, ..Settings::default() };
        assert_eq!(out(&m, &s), "E:/out/IMG_1_2160p_hevc.mkv");
    }

    #[test]
    fn template_variables_and_remux() {
        let m = media("/clips/IMG_1.MOV", None);
        let mut plan = recommend_plan(&m, Scenario::Mobile, &caps());
        let s =
            Settings { naming_template: "{date}-{scenario}-{name}-{height}p-{codec}".into(), ..Settings::default() };
        assert_eq!(render_template(&s.naming_template, &m, &plan, "2026-09-11"), "2026-09-11-手机-IMG_1-1080p-h264");
        plan.video.action = StreamAction::Copy;
        assert_eq!(render_template("{name}_{codec}", &m, &plan, ""), "IMG_1_remux");
    }

    #[test]
    fn keep_tree_rebuilds_subfolders_under_the_output_dir() {
        let m = media("/nas/in/2026/08/IMG_1.MOV", Some("/nas/in"));
        let s = Settings { output_dir: Some("/nas/out".into()), keep_tree: true, ..Settings::default() };
        assert_eq!(out(&m, &s), "/nas/out/2026/08/IMG_1_2160p_hevc.mkv");
        let flat = Settings { keep_tree: false, ..s };
        assert_eq!(out(&m, &flat), "/nas/out/IMG_1_2160p_hevc.mkv");
    }

    #[test]
    fn unsafe_characters_are_replaced() {
        assert_eq!(sanitize("a:b*c?"), "a_b_c_");
        assert_eq!(sanitize("  ..  "), "output");
        assert_eq!(sanitize("名字 1."), "名字 1");
    }
}
