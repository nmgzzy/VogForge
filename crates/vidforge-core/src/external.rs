//! 可选外部工具探测（dovi_tool / hdr10plus_tool / mkvmerge）。缺失时功能降级，不报错。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ffmpeg::exec::{Runner, args};
use crate::ffmpeg::locate::{Env, LocateOptions, candidate_dirs};
use crate::model::{ExternalTool, Platform};

struct ToolSpec {
    name: &'static str,
    purpose: &'static str,
}

const TOOLS: [ToolSpec; 3] = [
    ToolSpec { name: "dovi_tool", purpose: "杜比视界 P7 转换（v2）" },
    ToolSpec { name: "hdr10plus_tool", purpose: "HDR10+ 动态元数据（v2）" },
    ToolSpec { name: "mkvmerge", purpose: "MKV 高级封装（可选）" },
];

fn extra_dirs(platform: Platform, env: &dyn Env) -> Vec<PathBuf> {
    match platform {
        Platform::Windows => {
            let pf = env.var("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".to_string());
            vec![Path::new(&pf).join("MKVToolNix")]
        }
        _ => Vec::new(),
    }
}

pub fn probe_external(platform: Platform, env: &dyn Env, runner: &dyn Runner) -> Vec<ExternalTool> {
    let opts = LocateOptions { user_path: None, bundled_dir: None, platform };
    let mut dirs: Vec<PathBuf> = candidate_dirs(&opts, env).into_iter().map(|c| c.dir).collect();
    dirs.extend(extra_dirs(platform, env));
    let ext = if platform == Platform::Windows { ".exe" } else { "" };

    TOOLS
        .iter()
        .map(|spec| {
            let exe = format!("{}{ext}", spec.name);
            let path = dirs.iter().map(|d| d.join(&exe)).find(|p| env.is_file(p));
            let version = path.as_ref().and_then(|p| {
                runner
                    .run(p, &args(["--version"]), Duration::from_secs(5))
                    .ok()
                    .map(|o| o.combined().lines().next().unwrap_or("").trim().to_string())
                    .filter(|v| !v.is_empty())
            });
            ExternalTool {
                name: spec.name.to_string(),
                found: path.is_some(),
                purpose: spec.purpose.to_string(),
                path: path.map(|p| p.display().to_string()),
                version,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::exec::ExecOutput;
    use std::collections::{HashMap, HashSet};
    use std::io;

    struct OneFileEnv(HashSet<PathBuf>);
    impl Env for OneFileEnv {
        fn var(&self, k: &str) -> Option<String> {
            (k == "PATH").then(|| r"C:\bin".to_string())
        }
        fn registry_path_entries(&self) -> Vec<String> {
            Vec::new()
        }
        fn is_file(&self, p: &Path) -> bool {
            self.0.contains(p)
        }
        fn subdirs(&self, _: &Path) -> Vec<PathBuf> {
            Vec::new()
        }
        fn home(&self) -> Option<PathBuf> {
            None
        }
    }

    struct Echo(HashMap<PathBuf, String>);
    impl Runner for Echo {
        fn run(&self, p: &Path, _: &[String], _: Duration) -> io::Result<ExecOutput> {
            Ok(ExecOutput { code: Some(0), stdout: self.0.get(p).cloned().unwrap_or_default(), ..Default::default() })
        }
    }

    #[test]
    fn finds_tools_in_path_and_program_files() {
        let dovi = PathBuf::from(r"C:\bin").join("dovi_tool.exe");
        let mkv = PathBuf::from(r"C:\Program Files").join("MKVToolNix").join("mkvmerge.exe");
        let env = OneFileEnv([dovi.clone(), mkv.clone()].into_iter().collect());
        let runner = Echo(
            [(dovi, "dovi_tool 2.1.2\n".into()), (mkv, "mkvmerge v88.0 ('All I Know') 64-bit\n".into())]
                .into_iter()
                .collect(),
        );
        let tools = probe_external(Platform::Windows, &env, &runner);
        let get = |n: &str| tools.iter().find(|t| t.name == n).unwrap();
        assert!(get("dovi_tool").found);
        assert_eq!(get("dovi_tool").version.as_deref(), Some("dovi_tool 2.1.2"));
        assert!(!get("hdr10plus_tool").found);
        assert!(get("mkvmerge").found);
    }
}
