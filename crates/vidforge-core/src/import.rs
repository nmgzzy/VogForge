//! 文件导入：展开拖入的文件与文件夹（递归、按扩展名过滤），再并行调用 ffprobe 分析。

use std::collections::HashSet;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::ffmpeg::exec::Runner;
use crate::ffmpeg::probe::probe_file;
use crate::i18n::{Lang, pick};
use crate::model::{ImportFailure, ImportProgress, ImportResult, MediaInfo};
use crate::util::par_map;

/// 文件夹扫描时认作视频的扩展名。单独选中或拖入的文件不受此限制，按用户意图直接分析
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "ts", "m2ts", "mts", "mxf", "wmv", "flv", "3gp", "mpg", "mpeg", "vob",
    "hevc", "h265", "h264", "ivf",
];

pub fn is_video_file(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.as_str()))
}

/// 扫描文件夹时跳过的条目：
/// - 名字以 `.` 开头（Unix 隐藏；也包括 macOS 在 NAS 上留下的 `._IMG_0001.MOV` 资源分叉文件，扩展名像视频其实不是）
/// - 回收站与系统卷信息
/// - Windows 带"隐藏"或"系统"属性的条目（如用户目录下的 AppData，名字不以点开头）
fn skip_entry(e: &DirEntry) -> bool {
    let name = e.file_name().to_string_lossy().to_string();
    if name.starts_with('.')
        || name.eq_ignore_ascii_case("$RECYCLE.BIN")
        || name.eq_ignore_ascii_case("System Volume Information")
    {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(meta) = e.metadata() {
            if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
                return true;
            }
        }
    }
    false
}

/// 待分析的文件与它所属的导入根目录
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// 从文件夹导入时为该文件夹；单独导入的文件为 None
    pub root: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Expanded {
    pub entries: Vec<Entry>,
    /// 文件夹里扩展名不像视频的文件数
    pub skipped: u32,
    /// 不存在的路径、读不了的文件夹，及原因
    pub errors: Vec<ImportFailure>,
}

/// 展开输入路径：文件原样保留，文件夹递归收集视频文件。不跟随目录符号链接，避免循环
pub fn expand_paths(inputs: &[PathBuf], lang: Lang) -> Expanded {
    let mut out = Expanded::default();
    let mut seen = HashSet::new();
    for input in inputs {
        match fs::metadata(input) {
            Err(_) => {
                let reason = pick(lang, "路径不存在或无法访问", "The path does not exist or cannot be accessed");
                out.errors.push(failure(input, reason.into(), None));
            }
            Ok(meta) if meta.is_dir() => walk(input, input, &mut out, &mut seen, lang),
            Ok(_) => {
                if seen.insert(input.clone()) {
                    out.entries.push(Entry { path: input.clone(), root: None });
                }
            }
        }
    }
    out
}

fn failure(path: &Path, reason: String, detail: Option<String>) -> ImportFailure {
    ImportFailure { path: path.display().to_string(), reason, detail }
}

fn walk(dir: &Path, root: &Path, out: &mut Expanded, seen: &mut HashSet<PathBuf>, lang: Lang) {
    // 读不了的文件夹（权限、断开的网络盘）要告诉用户，不能表现成"里面什么都没有"
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            let reason = pick(lang, "无法读取文件夹", "Could not read the folder");
            out.errors.push(failure(dir, reason.into(), Some(e.to_string())));
            return;
        }
    };
    let mut items: Vec<DirEntry> = rd.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        let Ok(ft) = e.file_type() else { continue };
        if skip_entry(&e) {
            continue;
        }
        let path = e.path();
        if ft.is_dir() {
            walk(&path, root, out, seen, lang);
        } else if ft.is_file() {
            if !is_video_file(&path) {
                out.skipped += 1;
            } else if seen.insert(path.clone()) {
                out.entries.push(Entry { path, root: Some(root.to_path_buf()) });
            }
        }
    }
}

/// 导入：展开路径后用 `workers` 路并发分析，每完成一个文件回调一次进度。失败原因按 `lang` 生成
pub fn import_paths(
    inputs: &[PathBuf],
    ffprobe: &Path,
    runner: &dyn Runner,
    workers: usize,
    lang: Lang,
    progress: &(dyn Fn(ImportProgress) + Sync),
) -> ImportResult {
    let expanded = expand_paths(inputs, lang);
    let total = expanded.entries.len() as u32;
    let done = AtomicU32::new(0);
    let results: Vec<Result<MediaInfo, ImportFailure>> = par_map(
        &expanded.entries,
        workers,
        |entry| {
            let r = probe_file(ffprobe, &entry.path, runner)
                .map(|mut m| {
                    m.import_root = entry.root.as_ref().map(|r| r.display().to_string());
                    m
                })
                .map_err(|e| {
                    let (reason, detail) = e.describe(lang);
                    failure(&entry.path, reason, detail)
                });
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            let current = entry.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            progress(ImportProgress { done: n, total, current });
            r
        },
        |_| {},
    );

    let mut result = ImportResult { skipped: expanded.skipped, failures: expanded.errors, ..Default::default() };
    for r in results {
        match r {
            Ok(m) => result.media.push(m),
            Err(f) => result.failures.push(f),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::exec::ExecOutput;
    use std::io;
    use std::time::Duration;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn names(e: &Expanded) -> Vec<String> {
        e.entries.iter().map(|x| x.path.file_name().unwrap().to_string_lossy().into()).collect()
    }

    #[test]
    fn folders_are_walked_recursively_with_extension_filter() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        touch(&d.join("a.MP4"));
        touch(&d.join("sub").join("b.mkv"));
        touch(&d.join("sub").join("notes.txt"));
        touch(&d.join("sub").join("deep").join("c.mov"));
        touch(&d.join(".hidden").join("d.mp4"));
        touch(&d.join("thumb.jpg"));
        let e = expand_paths(&[d.to_path_buf()], Lang::ZhCn);
        assert_eq!(names(&e), ["a.MP4", "b.mkv", "c.mov"]);
        assert!(e.entries.iter().all(|x| x.root.as_deref() == Some(d)));
        assert_eq!(e.skipped, 2, "notes.txt 与 thumb.jpg 被跳过");
        assert!(e.errors.is_empty());
    }

    #[test]
    fn apple_double_files_on_nas_are_skipped() {
        // macOS 往非 HFS 卷复制文件时留下的资源分叉，扩展名是 .MOV 但不是视频
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("IMG_0001.MOV"));
        touch(&dir.path().join("._IMG_0001.MOV"));
        assert_eq!(names(&expand_paths(&[dir.path().to_path_buf()], Lang::ZhCn)), ["IMG_0001.MOV"]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_hidden_attribute_directories_are_skipped() {
        use std::os::windows::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let hidden = dir.path().join("AppData");
        touch(&hidden.join("cache.mp4"));
        touch(&dir.path().join("ok.mp4"));
        // attrib +h：名字不以点开头，只靠属性标记隐藏
        let status =
            std::process::Command::new("attrib").arg("+h").arg(&hidden).creation_flags(0x0800_0000).status().unwrap();
        assert!(status.success());
        assert_eq!(names(&expand_paths(&[dir.path().to_path_buf()], Lang::ZhCn)), ["ok.mp4"]);
    }

    #[test]
    fn explicit_files_bypass_the_filter_and_duplicates_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("recording.dat");
        touch(&odd);
        let e = expand_paths(&[odd.clone(), odd.clone(), dir.path().join("nope.mp4")], Lang::ZhCn);
        assert_eq!(e.entries, vec![Entry { path: odd, root: None }]);
        assert_eq!(e.errors.len(), 1);
        assert!(e.errors[0].reason.contains("不存在"));
        let en = expand_paths(&[dir.path().join("nope.mp4")], Lang::En);
        assert_eq!(en.errors[0].reason, "The path does not exist or cannot be accessed");
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folders_are_reported() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        touch(&locked.join("a.mp4"));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let e = expand_paths(&[dir.path().to_path_buf()], Lang::ZhCn);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // root 用户不受权限限制，此时读得到，跳过断言
        if e.entries.is_empty() {
            assert_eq!(e.errors.len(), 1);
            assert!(e.errors[0].reason.contains("无法读取文件夹"));
        }
    }

    #[test]
    fn a_file_given_as_folder_root_is_reported_not_silently_empty() {
        // walk 对非目录路径 read_dir 会失败，必须转成错误
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.mp4");
        touch(&file);
        let mut out = Expanded::default();
        walk(&file, dir.path(), &mut out, &mut HashSet::new(), Lang::ZhCn);
        assert_eq!(out.errors.len(), 1);
        assert!(out.errors[0].reason.contains("无法读取文件夹"));
        assert!(out.errors[0].detail.is_some(), "系统原因放进可展开的原文");
    }

    /// 文件名含 bad 的视为损坏，其余返回一段最小的有效 JSON
    struct Fake;
    impl Runner for Fake {
        fn run(&self, _p: &Path, a: &[String], _t: Duration) -> io::Result<ExecOutput> {
            let file = a.last().unwrap();
            if file.contains("bad") {
                return Ok(ExecOutput { code: Some(1), stderr: "moov atom not found".into(), ..Default::default() });
            }
            let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":640,"height":360}],
                "format":{"duration":"1"}}"#;
            Ok(ExecOutput { code: Some(0), stdout: json.into(), ..Default::default() })
        }
    }

    #[test]
    fn import_collects_media_failures_and_progress() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("good1.mp4"));
        touch(&dir.path().join("bad.mp4"));
        touch(&dir.path().join("x").join("good2.mov"));
        let seen = std::sync::Mutex::new(Vec::new());
        let r = import_paths(
            &[dir.path().to_path_buf(), dir.path().join("missing")],
            Path::new("ffprobe"),
            &Fake,
            2,
            Lang::ZhCn,
            &|p| seen.lock().unwrap().push((p.done, p.total)),
        );
        assert_eq!(r.media.len(), 2);
        assert!(r.media.iter().all(|m| m.import_root.is_some()));
        assert_eq!(r.failures.len(), 2, "损坏文件与不存在的路径：{:?}", r.failures);
        let broken = r.failures.iter().find(|f| f.reason.contains("文件不完整")).expect("损坏文件有说明");
        assert_eq!(broken.detail.as_deref(), Some("moov atom not found"), "原文可展开");
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(seen, [(1, 3), (2, 3), (3, 3)]);
    }
}
