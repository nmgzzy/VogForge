//! ffmpeg / ffprobe 定位（设计文档 5.1）。
//!
//! 顺序：用户指定 → 应用目录 → 进程 PATH → 注册表 PATH（仅 Windows）→ 平台常见安装位置。
//! Windows 上刚装完 ffmpeg 时注册表 PATH 已更新、但正在运行的进程环境未刷新，所以要读注册表。
//! 用户指定的路径总是被采用；其余来源优先选满足最低版本的那一个。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::model::{LocateSource, Platform};

use super::exec::{Runner, args};
use super::parse::{VersionInfo, parse_version};

/// 定位所需的环境访问，测试时换成假实现
pub trait Env: Send + Sync {
    fn var(&self, key: &str) -> Option<String>;
    /// 注册表中 Machine 与 User 两级的 PATH 条目（已展开环境变量）。非 Windows 返回空
    fn registry_path_entries(&self) -> Vec<String>;
    fn is_file(&self, p: &Path) -> bool;
    /// 列出目录下的子目录
    fn subdirs(&self, p: &Path) -> Vec<PathBuf>;
    fn home(&self) -> Option<PathBuf>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn registry_path_entries(&self) -> Vec<String> {
        registry_path_entries()
    }

    fn is_file(&self, p: &Path) -> bool {
        p.is_file()
    }

    fn subdirs(&self, p: &Path) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(p) else { return Vec::new() };
        let mut out: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        out.sort();
        out
    }

    fn home(&self) -> Option<PathBuf> {
        #[allow(deprecated)]
        std::env::home_dir()
    }
}

#[cfg(windows)]
fn registry_path_entries() -> Vec<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let sources = [
        (HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
        (HKEY_CURRENT_USER, "Environment"),
    ];
    let mut out = Vec::new();
    for (root, sub) in sources {
        let Ok(key) = RegKey::predef(root).open_subkey(sub) else { continue };
        let Ok(value) = key.get_value::<String, _>("Path") else { continue };
        let expanded = expand_env_vars(&value, |k| std::env::var(k).ok());
        out.extend(split_path_list(&expanded, Platform::Windows));
    }
    out
}

#[cfg(not(windows))]
fn registry_path_entries() -> Vec<String> {
    Vec::new()
}

/// 展开 Windows 风格的 `%VAR%`。未定义的变量原样保留。
pub fn expand_env_vars(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn split_path_list(s: &str, platform: Platform) -> Vec<String> {
    let sep = if platform == Platform::Windows { ';' } else { ':' };
    s.split(sep).map(|p| p.trim().trim_matches('"').to_string()).filter(|p| !p.is_empty()).collect()
}

#[derive(Debug, Clone)]
pub struct LocateOptions {
    /// 用户在设置里指定的目录或可执行文件
    pub user_path: Option<PathBuf>,
    /// 应用目录下存放下载构建的位置
    pub bundled_dir: Option<PathBuf>,
    pub platform: Platform,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub dir: PathBuf,
    pub source: LocateSource,
}

#[derive(Debug, Clone)]
pub struct Located {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub source: LocateSource,
    pub version: VersionInfo,
    /// ffmpeg `-version` 的原始输出，后续解析 configuration 用
    pub version_text: String,
    pub ffprobe_version: Option<VersionInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct LocateReport {
    pub found: Option<Located>,
    /// 查找过的目录（去重后，按顺序）
    pub searched: Vec<String>,
    /// 途中遇到的问题，如"只有 ffmpeg 没有 ffprobe""无法运行"
    pub problems: Vec<String>,
    /// 有 ffmpeg 可执行文件却用不了的目录及原因。没有找到可用构建时据此区分"没装"与"装坏了"
    pub broken: Vec<String>,
}

fn exe_names(platform: Platform) -> (&'static str, &'static str) {
    if platform == Platform::Windows { ("ffmpeg.exe", "ffprobe.exe") } else { ("ffmpeg", "ffprobe") }
}

fn dedupe_key(p: &Path, platform: Platform) -> String {
    let s = p.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    if platform == Platform::Windows { s.to_lowercase().replace('/', "\\") } else { s }
}

/// 按优先级列出候选目录（已去重）
pub fn candidate_dirs(opts: &LocateOptions, env: &dyn Env) -> Vec<Candidate> {
    let mut list: Vec<Candidate> = Vec::new();
    let (ffmpeg_name, _) = exe_names(opts.platform);

    if let Some(p) = &opts.user_path {
        // 用户可能选的是 ffmpeg.exe 本身，也可能是它所在的目录
        let is_exe = p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(ffmpeg_name));
        let dir =
            if is_exe || env.is_file(p) { p.parent().map(Path::to_path_buf).unwrap_or_default() } else { p.clone() };
        list.push(Candidate { dir, source: LocateSource::User });
    }
    if let Some(d) = &opts.bundled_dir {
        list.push(Candidate { dir: d.clone(), source: LocateSource::Bundled });
    }
    if let Some(path) = env.var("PATH") {
        for p in split_path_list(&path, opts.platform) {
            list.push(Candidate { dir: PathBuf::from(p), source: LocateSource::Path });
        }
    }
    for p in env.registry_path_entries() {
        list.push(Candidate { dir: PathBuf::from(p), source: LocateSource::Registry });
    }
    for dir in common_dirs(opts.platform, env) {
        list.push(Candidate { dir, source: LocateSource::Common });
    }

    let mut seen = HashSet::new();
    list.retain(|c| !c.dir.as_os_str().is_empty() && seen.insert(dedupe_key(&c.dir, opts.platform)));
    list
}

fn common_dirs(platform: Platform, env: &dyn Env) -> Vec<PathBuf> {
    let mut out = Vec::new();
    match platform {
        Platform::Windows => {
            if let Some(local) = env.var("LOCALAPPDATA") {
                let winget = Path::new(&local).join("Microsoft").join("WinGet");
                out.push(winget.join("Links"));
                // winget 的包目录形如 Packages\Gyan.FFmpeg_Microsoft.Winget.Source_xxx\ffmpeg-7.1-full_build\bin
                for pkg in env.subdirs(&winget.join("Packages")) {
                    let name = pkg.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if name.contains("ffmpeg") {
                        for sub in env.subdirs(&pkg) {
                            out.push(sub.join("bin"));
                        }
                    }
                }
            }
            if let Some(home) = env.home() {
                out.push(home.join("scoop").join("apps").join("ffmpeg").join("current").join("bin"));
                out.push(home.join("scoop").join("shims"));
            }
            let program_data = env.var("ProgramData").unwrap_or_else(|| r"C:\ProgramData".to_string());
            out.push(Path::new(&program_data).join("chocolatey").join("bin"));
            out.push(PathBuf::from(r"C:\ffmpeg\bin"));
            let program_files = env.var("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".to_string());
            out.push(Path::new(&program_files).join("ffmpeg").join("bin"));
        }
        Platform::Macos => {
            // 从 Finder 启动的应用不继承 shell 的 PATH，这几个位置必须显式找
            for d in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
                out.push(PathBuf::from(d));
            }
        }
        Platform::Linux => {
            for d in ["/usr/bin", "/usr/local/bin"] {
                out.push(PathBuf::from(d));
            }
        }
    }
    out
}

const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// 检查一个候选目录的结果
enum Attempt {
    /// 这里没有 ffmpeg
    Absent,
    /// 有 ffmpeg，但用不了（缺 ffprobe、无法运行、输出不可识别）
    Broken(String),
    Found(Box<Located>),
}

fn try_candidate(c: &Candidate, platform: Platform, env: &dyn Env, runner: &dyn Runner) -> Attempt {
    let (ffmpeg_name, ffprobe_name) = exe_names(platform);
    let ffmpeg = c.dir.join(ffmpeg_name);
    if !env.is_file(&ffmpeg) {
        return Attempt::Absent;
    }
    let ffprobe = c.dir.join(ffprobe_name);
    if !env.is_file(&ffprobe) {
        return Attempt::Broken(format!("{} 里只有 ffmpeg，没有 ffprobe", c.dir.display()));
    }
    let out = match runner.run(&ffmpeg, &args(["-hide_banner", "-version"]), VERSION_TIMEOUT) {
        Ok(o) => o,
        Err(e) => return Attempt::Broken(format!("无法运行 {}：{e}", ffmpeg.display())),
    };
    let text = out.combined();
    let Some(version) = parse_version(&text) else {
        return Attempt::Broken(format!("{} 没有输出可识别的版本信息", ffmpeg.display()));
    };
    let ffprobe_version = runner
        .run(&ffprobe, &args(["-hide_banner", "-version"]), VERSION_TIMEOUT)
        .ok()
        .and_then(|o| parse_version(&o.combined()));
    if ffprobe_version.is_none() {
        return Attempt::Broken(format!("无法运行 {}", ffprobe.display()));
    }
    Attempt::Found(Box::new(Located {
        ffmpeg,
        ffprobe,
        source: c.source,
        version,
        version_text: text,
        ffprobe_version,
    }))
}

pub fn locate(opts: &LocateOptions, env: &dyn Env, runner: &dyn Runner) -> LocateReport {
    let candidates = candidate_dirs(opts, env);
    let mut report = LocateReport {
        searched: candidates.iter().map(|c| c.dir.display().to_string()).collect(),
        ..Default::default()
    };
    let mut fallback: Option<Located> = None;
    for c in &candidates {
        let found = match try_candidate(c, opts.platform, env, runner) {
            Attempt::Found(found) => *found,
            Attempt::Absent => {
                if c.source == LocateSource::User {
                    report.problems.insert(0, format!("设置里指定的 {} 里没有 ffmpeg", c.dir.display()));
                }
                continue;
            }
            Attempt::Broken(reason) => {
                if c.source == LocateSource::User {
                    report.problems.insert(0, format!("设置里指定的 {} 不可用", c.dir.display()));
                }
                report.problems.push(reason.clone());
                report.broken.push(reason);
                continue;
            }
        };
        if c.source == LocateSource::User || found.version.meets_minimum() {
            report.found = Some(found);
            return report;
        }
        // 版本过低：先记下，继续找更新的
        fallback.get_or_insert(found);
    }
    report.found = fallback;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::exec::ExecOutput;
    use std::collections::HashMap;
    use std::io;

    #[derive(Default)]
    struct FakeEnv {
        vars: HashMap<String, String>,
        registry: Vec<String>,
        files: HashSet<PathBuf>,
        dirs: HashMap<PathBuf, Vec<PathBuf>>,
        home: Option<PathBuf>,
    }

    impl Env for FakeEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
        fn registry_path_entries(&self) -> Vec<String> {
            self.registry.clone()
        }
        fn is_file(&self, p: &Path) -> bool {
            self.files.contains(p)
        }
        fn subdirs(&self, p: &Path) -> Vec<PathBuf> {
            self.dirs.get(p).cloned().unwrap_or_default()
        }
        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    /// 按可执行文件路径返回预设的 -version 输出
    struct VersionRunner(HashMap<PathBuf, String>);

    impl Runner for VersionRunner {
        fn run(&self, program: &Path, _args: &[String], _t: Duration) -> io::Result<ExecOutput> {
            match self.0.get(program) {
                Some(text) => Ok(ExecOutput { code: Some(0), stdout: text.clone(), ..Default::default() }),
                None => Err(io::Error::new(io::ErrorKind::NotFound, "no such program")),
            }
        }
    }

    fn version_text(v: &str) -> String {
        format!("ffmpeg version {v} Copyright (c) 2000-2026 the FFmpeg developers\n")
    }

    fn install(env: &mut FakeEnv, runner: &mut VersionRunner, dir: &str, version: &str) {
        for exe in ["ffmpeg.exe", "ffprobe.exe"] {
            let p = Path::new(dir).join(exe);
            env.files.insert(p.clone());
            runner.0.insert(p, version_text(version));
        }
    }

    fn win_opts() -> LocateOptions {
        LocateOptions { user_path: None, bundled_dir: None, platform: Platform::Windows }
    }

    #[test]
    fn order_is_user_bundled_path_registry_common() {
        let mut env = FakeEnv::default();
        env.vars.insert("PATH".into(), r"C:\a;C:\b".into());
        env.registry = vec![r"C:\reg".into()];
        let opts = LocateOptions {
            user_path: Some(PathBuf::from(r"D:\user")),
            bundled_dir: Some(PathBuf::from(r"C:\app\ffmpeg\bin")),
            platform: Platform::Windows,
        };
        let c = candidate_dirs(&opts, &env);
        let sources: Vec<LocateSource> = c.iter().map(|c| c.source).collect();
        assert_eq!(
            &sources[..5],
            &[
                LocateSource::User,
                LocateSource::Bundled,
                LocateSource::Path,
                LocateSource::Path,
                LocateSource::Registry
            ]
        );
        assert!(sources[5..].iter().all(|s| *s == LocateSource::Common));
    }

    #[test]
    fn duplicates_are_removed_case_insensitively_on_windows() {
        let mut env = FakeEnv::default();
        env.vars.insert("PATH".into(), r"C:\FFmpeg\bin;c:\ffmpeg\bin\".into());
        env.registry = vec![r"C:\ffmpeg\bin".into()];
        let c = candidate_dirs(&win_opts(), &env);
        let n = c.iter().filter(|c| c.dir.to_string_lossy().to_lowercase().starts_with(r"c:\ffmpeg\bin")).count();
        assert_eq!(n, 1, "PATH、注册表与常见位置里的同一目录只保留一次");
        assert_eq!(c[0].source, LocateSource::Path);
    }

    #[test]
    fn registry_path_is_used_when_process_path_is_stale() {
        // 开发期真实遇到的情况：刚装完 ffmpeg，进程 PATH 里还没有
        let mut env = FakeEnv::default();
        let mut runner = VersionRunner(HashMap::new());
        env.vars.insert("PATH".into(), r"C:\Windows\System32".into());
        env.registry = vec![r"C:\Program1\ffmpeg\bin".into()];
        install(&mut env, &mut runner, r"C:\Program1\ffmpeg\bin", "9.0.1-full_build-www.gyan.dev");
        let r = locate(&win_opts(), &env, &runner);
        let f = r.found.expect("应通过注册表 PATH 找到");
        assert_eq!(f.source, LocateSource::Registry);
        assert_eq!(f.ffmpeg, Path::new(r"C:\Program1\ffmpeg\bin\ffmpeg.exe"));
    }

    #[test]
    fn user_path_may_point_to_the_exe() {
        let mut env = FakeEnv::default();
        let mut runner = VersionRunner(HashMap::new());
        install(&mut env, &mut runner, r"E:\tools\ff", "7.1");
        let opts = LocateOptions { user_path: Some(PathBuf::from(r"E:\tools\ff\ffmpeg.exe")), ..win_opts() };
        let r = locate(&opts, &env, &runner);
        assert_eq!(r.found.unwrap().source, LocateSource::User);
    }

    #[test]
    fn user_path_is_respected_even_if_old() {
        let mut env = FakeEnv::default();
        let mut runner = VersionRunner(HashMap::new());
        install(&mut env, &mut runner, r"E:\old", "6.0");
        install(&mut env, &mut runner, r"C:\ffmpeg\bin", "9.0.1");
        let opts = LocateOptions { user_path: Some(PathBuf::from(r"E:\old")), ..win_opts() };
        let f = locate(&opts, &env, &runner).found.unwrap();
        assert_eq!(f.source, LocateSource::User);
        assert!(!f.version.meets_minimum());
    }

    #[test]
    fn prefers_a_new_enough_build_over_an_earlier_old_one() {
        let mut env = FakeEnv::default();
        let mut runner = VersionRunner(HashMap::new());
        env.vars.insert("PATH".into(), r"C:\old\bin;C:\new\bin".into());
        install(&mut env, &mut runner, r"C:\old\bin", "6.0");
        install(&mut env, &mut runner, r"C:\new\bin", "7.1.1");
        let f = locate(&win_opts(), &env, &runner).found.unwrap();
        assert_eq!(f.ffmpeg, Path::new(r"C:\new\bin\ffmpeg.exe"));
    }

    #[test]
    fn falls_back_to_old_build_when_nothing_better() {
        let mut env = FakeEnv::default();
        let mut runner = VersionRunner(HashMap::new());
        env.vars.insert("PATH".into(), r"C:\old\bin".into());
        install(&mut env, &mut runner, r"C:\old\bin", "6.0");
        let f = locate(&win_opts(), &env, &runner).found.unwrap();
        assert_eq!(f.version.release, Some(crate::ffmpeg::parse::Ver(6, 0, 0)));
    }

    #[test]
    fn ffmpeg_without_ffprobe_is_skipped_with_a_problem() {
        let mut env = FakeEnv::default();
        env.vars.insert("PATH".into(), r"C:\half".into());
        env.files.insert(PathBuf::from(r"C:\half\ffmpeg.exe"));
        let r = locate(&win_opts(), &env, &VersionRunner(HashMap::new()));
        assert!(r.found.is_none());
        assert!(r.problems.iter().any(|p| p.contains("没有 ffprobe")));
    }

    #[test]
    fn unrunnable_ffmpeg_is_reported_as_broken() {
        let mut env = FakeEnv::default();
        env.vars.insert("PATH".into(), r"C:\bad".into());
        env.files.insert(PathBuf::from(r"C:\bad\ffmpeg.exe"));
        env.files.insert(PathBuf::from(r"C:\bad\ffprobe.exe"));
        // 执行器里没有登记这两个程序，模拟"文件在但跑不起来"
        let r = locate(&win_opts(), &env, &VersionRunner(HashMap::new()));
        assert!(r.found.is_none());
        assert_eq!(r.broken.len(), 1);
        assert!(r.broken[0].contains("无法运行"));
    }

    #[test]
    fn not_found_lists_searched_dirs() {
        let mut env = FakeEnv::default();
        env.vars.insert("PATH".into(), r"C:\x".into());
        let r = locate(&win_opts(), &env, &VersionRunner(HashMap::new()));
        assert!(r.found.is_none());
        assert!(r.searched.contains(&r"C:\x".to_string()));
        assert!(r.searched.contains(&r"C:\ffmpeg\bin".to_string()));
    }

    #[test]
    fn winget_package_dirs_are_discovered() {
        let mut env = FakeEnv::default();
        env.vars.insert("LOCALAPPDATA".into(), r"C:\Users\u\AppData\Local".into());
        let pkgs = PathBuf::from(r"C:\Users\u\AppData\Local\Microsoft\WinGet\Packages");
        let pkg = pkgs.join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe");
        env.dirs.insert(pkgs.clone(), vec![pkg.clone(), pkgs.join("Other.Tool")]);
        env.dirs.insert(pkg.clone(), vec![pkg.join("ffmpeg-7.1-full_build")]);
        let c = candidate_dirs(&win_opts(), &env);
        assert!(c.iter().any(|c| c.dir == pkg.join("ffmpeg-7.1-full_build").join("bin")));
    }

    #[test]
    fn macos_common_locations() {
        let env = FakeEnv::default();
        let opts = LocateOptions { user_path: None, bundled_dir: None, platform: Platform::Macos };
        let dirs: Vec<String> = candidate_dirs(&opts, &env).iter().map(|c| c.dir.display().to_string()).collect();
        assert_eq!(dirs, ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]);
    }

    #[test]
    fn expand_env() {
        let lookup = |k: &str| (k == "USERPROFILE").then(|| r"C:\Users\u".to_string());
        assert_eq!(expand_env_vars(r"%USERPROFILE%\bin;%NOPE%\x;50%", lookup), r"C:\Users\u\bin;%NOPE%\x;50%");
    }
}
