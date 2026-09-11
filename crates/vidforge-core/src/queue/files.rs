//! 输出文件安全（设计文档 4.7）：先写 `.vidforge-part` 临时文件，成功后才改名；同名冲突按设置处理；
//! 取消、失败或崩溃后清理临时文件与两遍编码的统计文件，目标目录不留半成品。源文件绝不删除。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::ConflictPolicy;

pub const PART_SUFFIX: &str = ".vidforge-part";

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// 最终文件对应的临时文件
pub fn temp_path(final_path: &Path) -> PathBuf {
    with_suffix(final_path, PART_SUFFIX)
}

/// 两个路径是否指向同一个文件：分隔符统一、Windows 上不分大小写；都存在时再比规范化后的真实路径
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    if norm(a) == norm(b) {
        return true;
    }
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// `名字 (1).mkv`、`名字 (2).mkv`……第一个不存在、没有同名临时文件、也没被占用的路径
pub fn next_free(path: &Path, busy: &dyn Fn(&Path) -> bool) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    // 按字符串替换文件名，保留原路径的分隔符风格（with_file_name 在 Windows 上会混进反斜杠）
    let full = path.to_string_lossy();
    let dir = &full[..full.rfind(['/', '\\']).map_or(0, |i| i + 1)];
    (1..)
        .map(|n| PathBuf::from(format!("{dir}{stem} ({n}){ext}")))
        .find(|p| !p.exists() && !temp_path(p).exists() && !busy(p))
        .expect("总能找到空闲的名字")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// 写到这个路径
    Write(PathBuf),
    /// 目标已存在且策略是跳过
    Skip,
}

/// 决定写到哪里。`busy` 标出不能写的路径：源文件本身、别的任务正在写的目标。
/// 被占用的路径即使策略是覆盖也不写，改用序号（跳过策略仍然跳过）
pub fn resolve_target(desired: &Path, policy: ConflictPolicy, busy: &dyn Fn(&Path) -> bool) -> Target {
    let locked = busy(desired);
    if !locked && !desired.exists() {
        return Target::Write(desired.to_path_buf());
    }
    match policy {
        ConflictPolicy::Skip => Target::Skip,
        ConflictPolicy::Overwrite if !locked => Target::Write(desired.to_path_buf()),
        _ => Target::Write(next_free(desired, busy)),
    }
}

/// 编码成功后把临时文件改名为最终文件。转码期间目标位置可能又出现了同名文件，按策略再处理一次；
/// 返回实际的最终路径，策略是跳过时删除临时文件并返回 None
pub fn finalize(
    temp: &Path,
    target: &Path,
    policy: ConflictPolicy,
    busy: &dyn Fn(&Path) -> bool,
) -> io::Result<Option<PathBuf>> {
    let target = match resolve_target(target, policy, busy) {
        Target::Skip => {
            let _ = fs::remove_file(temp);
            return Ok(None);
        }
        Target::Write(p) => p,
    };
    if policy == ConflictPolicy::Overwrite && target.exists() && !busy(&target) {
        fs::remove_file(&target)?;
    }
    fs::rename(temp, &target)?;
    Ok(Some(target))
}

/// 两遍编码的统计文件：`<临时文件>.2pass-0.log`，x264 另有 `.mbtree`，x265 另有 `.cutree`
pub fn passlog_files(temp: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (temp.parent(), temp.file_name()) else { return Vec::new() };
    let prefix = format!("{}.2pass", name.to_string_lossy());
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// 删除临时文件与统计文件；文件不在时不算错
pub fn cleanup(temp: &Path) {
    let _ = fs::remove_file(temp);
    for f in passlog_files(temp) {
        let _ = fs::remove_file(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free(_: &Path) -> bool {
        false
    }

    #[test]
    fn temp_names_and_free_names() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.mkv");
        assert_eq!(temp_path(&out), dir.path().join("a.mkv.vidforge-part"));
        assert_eq!(next_free(&out, &free), dir.path().join("a (1).mkv"));
        fs::write(dir.path().join("a (1).mkv"), b"x").unwrap();
        // 别的任务正在写 a (2) 的临时文件，也不能占用
        fs::write(dir.path().join("a (2).mkv.vidforge-part"), b"x").unwrap();
        assert_eq!(next_free(&out, &free), dir.path().join("a (3).mkv"));
        // 原路径用正斜杠时，改名后的路径也用正斜杠
        let slash = PathBuf::from(format!("{}/a.mkv", dir.path().to_string_lossy().replace('\\', "/")));
        assert!(!next_free(&slash, &free).to_string_lossy().ends_with("\\a (3).mkv"));
        assert!(next_free(&slash, &free).to_string_lossy().ends_with("/a (3).mkv"));
    }

    #[test]
    fn conflict_policies() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.mkv");
        assert_eq!(resolve_target(&out, ConflictPolicy::Skip, &free), Target::Write(out.clone()));
        fs::write(&out, b"old").unwrap();
        assert_eq!(resolve_target(&out, ConflictPolicy::Skip, &free), Target::Skip);
        assert_eq!(resolve_target(&out, ConflictPolicy::Overwrite, &free), Target::Write(out.clone()));
        assert_eq!(resolve_target(&out, ConflictPolicy::Rename, &free), Target::Write(dir.path().join("a (1).mkv")));
    }

    #[test]
    fn finalize_renames_and_rechecks_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.mkv");
        let tmp = temp_path(&out);
        fs::write(&tmp, b"new").unwrap();
        assert_eq!(finalize(&tmp, &out, ConflictPolicy::Rename, &free).unwrap(), Some(out.clone()));
        assert!(!tmp.exists());

        // 转码期间目标出现了同名文件
        fs::write(&tmp, b"newer").unwrap();
        let renamed = finalize(&tmp, &out, ConflictPolicy::Rename, &free).unwrap().unwrap();
        assert_eq!(renamed, dir.path().join("a (1).mkv"));
        assert_eq!(fs::read(&out).unwrap(), b"new");

        fs::write(&tmp, b"newest").unwrap();
        assert_eq!(finalize(&tmp, &out, ConflictPolicy::Overwrite, &free).unwrap(), Some(out.clone()));
        assert_eq!(fs::read(&out).unwrap(), b"newest");

        fs::write(&tmp, b"skipped").unwrap();
        assert_eq!(finalize(&tmp, &out, ConflictPolicy::Skip, &free).unwrap(), None);
        assert!(!tmp.exists());
        assert_eq!(fs::read(&out).unwrap(), b"newest", "跳过时不动已有文件");
    }

    #[test]
    fn cleanup_removes_part_and_passlogs_only() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.mkv");
        let tmp = temp_path(&out);
        for name in ["a.mkv.vidforge-part", "a.mkv.vidforge-part.2pass-0.log", "a.mkv.vidforge-part.2pass-0.log.cutree"]
        {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        fs::write(dir.path().join("b.mkv.vidforge-part.2pass-0.log"), b"x").unwrap();
        fs::write(&out, b"keep").unwrap();
        assert_eq!(passlog_files(&tmp).len(), 2);
        cleanup(&tmp);
        let left: Vec<String> =
            fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(out.exists());
    }

    #[test]
    fn busy_paths_are_never_written_even_when_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clip.mkv");
        fs::write(&src, b"source").unwrap();
        // 输出目录与命名模板恰好让目标等于源文件：覆盖策略也不能写
        let is_src = |p: &Path| same_path(p, &src);
        assert_eq!(
            resolve_target(&src, ConflictPolicy::Overwrite, &is_src),
            Target::Write(dir.path().join("clip (1).mkv"))
        );
        assert_eq!(resolve_target(&src, ConflictPolicy::Skip, &is_src), Target::Skip);
        // 别的任务正在写的目标（文件还不存在）同样算占用
        let other = dir.path().join("out.mkv");
        let reserved = |p: &Path| same_path(p, &other);
        assert_eq!(
            resolve_target(&other, ConflictPolicy::Rename, &reserved),
            Target::Write(dir.path().join("out (1).mkv"))
        );
        assert_eq!(
            resolve_target(&other, ConflictPolicy::Overwrite, &reserved),
            Target::Write(dir.path().join("out (1).mkv"))
        );
        // 改名那一步也守住源文件
        let tmp = temp_path(&src);
        fs::write(&tmp, b"new").unwrap();
        let done = finalize(&tmp, &src, ConflictPolicy::Overwrite, &is_src).unwrap().unwrap();
        assert_eq!(done, dir.path().join("clip (1).mkv"));
        assert_eq!(fs::read(&src).unwrap(), b"source");
    }

    #[test]
    fn same_path_ignores_separators_and_windows_case() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("A.mkv");
        let b = PathBuf::from(a.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
        assert!(same_path(&a, &b));
        assert_eq!(same_path(&a, &dir.path().join("a.mkv")), cfg!(windows));
        assert!(!same_path(&a, &dir.path().join("b.mkv")));
    }
}
