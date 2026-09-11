//! 子进程执行。探测与分析都经由 [`Runner`] 调用外部程序，测试时可换成假实现。

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecOutput {
    /// 退出码；被信号杀死或超时时为 None
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }

    /// stdout 与 stderr 合并，ffmpeg 的 `-version` 等信息有的写 stdout 有的写 stderr
    pub fn combined(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

pub trait Runner: Send + Sync {
    fn run(&self, program: &Path, args: &[String], timeout: Duration) -> io::Result<ExecOutput>;
}

/// 构造子进程命令：stdin 置空；Windows 上不弹出控制台窗口。
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 真实执行器
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[String], timeout: Duration) -> io::Result<ExecOutput> {
        let mut child = command(program).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        // 两个管道各用一个线程读，避免输出过多时子进程被写阻塞
        let mut out_pipe = child.stdout.take().expect("stdout piped");
        let mut err_pipe = child.stderr.take().expect("stderr piped");
        let out_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out_pipe.read_to_end(&mut buf);
            buf
        });
        let err_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err_pipe.read_to_end(&mut buf);
            buf
        });

        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if start.elapsed() >= timeout {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };

        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        Ok(ExecOutput {
            code: status.and_then(|s| s.code()),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            timed_out,
        })
    }
}

/// 把参数列表转成 `Vec<String>`
pub fn args<I, S>(items: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    items.into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_merges_streams() {
        let o = ExecOutput { code: Some(0), stdout: "a".into(), stderr: "b".into(), timed_out: false };
        assert_eq!(o.combined(), "a\nb");
        assert!(o.success());
        let t = ExecOutput { timed_out: true, ..o };
        assert!(!t.success());
    }

    #[test]
    fn missing_program_is_io_error() {
        let r = SystemRunner.run(Path::new("definitely-not-a-real-program-vidforge"), &[], Duration::from_secs(2));
        assert!(r.is_err());
    }
}
