//! 长时间运行的 ffmpeg 进程：逐行读进度、stderr 环形缓冲、取消、暂停与继续。
//!
//! 调度器只依赖 [`Tools`] trait，测试时换成脚本化的假实现。暂停是挂起进程：Windows 上逐个挂起
//! 进程的全部线程（`SuspendThread`），类 Unix 系统发 `SIGSTOP` / `SIGCONT`。

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ffmpeg::exec::{SystemRunner, command};
use crate::ffmpeg::probe::{ProbeError, probe_file};
use crate::model::MediaInfo;

/// stderr 只保留最后这么多行
pub const LOG_LINES: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessExit {
    pub success: bool,
    pub code: Option<i32>,
    /// stderr 的最后若干行
    pub stderr: Vec<String>,
}

/// 从别的线程控制正在运行的进程
pub trait ProcessControl: Send + Sync {
    fn kill(&self);
    /// 挂起；做不到时返回 false
    fn suspend(&self) -> bool;
    fn resume(&self) -> bool;
}

pub trait Process: Send {
    /// 下一行标准输出（`-progress pipe:1` 的进度）；输出结束时为 None
    fn next_line(&mut self) -> Option<String>;
    /// 标准输出读完之后等待退出
    fn wait(&mut self) -> io::Result<ProcessExit>;
    fn control(&self) -> Arc<dyn ProcessControl>;
}

/// 调度器用到的全部外部能力
pub trait Tools: Send + Sync {
    /// 启动 ffmpeg。编码、预检、响度测量都经过它，才能被暂停、取消与随应用一起结束
    fn spawn(&self, program: &Path, args: &[String]) -> io::Result<Box<dyn Process>>;
    /// 分析输出文件，用于校验
    fn probe(&self, ffprobe: &Path, file: &Path) -> Result<MediaInfo, ProbeError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTools;

impl Tools for SystemTools {
    fn spawn(&self, program: &Path, args: &[String]) -> io::Result<Box<dyn Process>> {
        Ok(Box::new(SystemProcess::spawn(program, args)?))
    }

    fn probe(&self, ffprobe: &Path, file: &Path) -> Result<MediaInfo, ProbeError> {
        probe_file(ffprobe, file, &SystemRunner)
    }
}

struct SystemControl {
    child: Arc<Mutex<Child>>,
    pid: u32,
}

impl ProcessControl for SystemControl {
    fn kill(&self) {
        // ffmpeg 不再派生子进程，结束它本身即可；挂起中的进程也能直接结束
        let _ = self.child.lock().unwrap().kill();
    }

    fn suspend(&self) -> bool {
        os::suspend(self.pid)
    }

    fn resume(&self) -> bool {
        os::resume(self.pid)
    }
}

pub struct SystemProcess {
    child: Arc<Mutex<Child>>,
    pid: u32,
    lines: Receiver<String>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    readers: Vec<JoinHandle<()>>,
}

impl SystemProcess {
    pub fn spawn(program: &Path, args: &[String]) -> io::Result<SystemProcess> {
        let mut cmd = command(program);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        os::die_with_parent(&mut cmd);
        let mut child = cmd.spawn()?;
        os::bind(&child);
        let pid = child.id();
        let (tx, lines) = channel();
        let out = child.stdout.take().expect("stdout piped");
        let err = child.stderr.take().expect("stderr piped");
        let stderr = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        let tail = stderr.clone();
        let readers = vec![
            thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }),
            thread::spawn(move || {
                // 不按 UTF-8 严格解码：Windows 上 ffmpeg 可能按本地代码页输出路径
                let mut reader = BufReader::new(err);
                let mut buf = Vec::new();
                while reader.read_until(b'\n', &mut buf).map(|n| n > 0).unwrap_or(false) {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    buf.clear();
                    if line.is_empty() {
                        continue;
                    }
                    let mut t = tail.lock().unwrap();
                    if t.len() == LOG_LINES {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            }),
        ];
        Ok(SystemProcess { child: Arc::new(Mutex::new(child)), pid, lines, stderr, readers })
    }
}

impl Process for SystemProcess {
    fn next_line(&mut self) -> Option<String> {
        self.lines.recv().ok()
    }

    fn wait(&mut self) -> io::Result<ProcessExit> {
        // 轮询而不是阻塞 wait：取消要从别的线程拿同一把锁去 kill
        let status = loop {
            if let Some(s) = self.child.lock().unwrap().try_wait()? {
                break s;
            }
            thread::sleep(Duration::from_millis(20));
        };
        for r in self.readers.drain(..) {
            let _ = r.join();
        }
        let stderr = self.stderr.lock().unwrap().iter().cloned().collect();
        Ok(ProcessExit { success: status.success(), code: status.code(), stderr })
    }

    fn control(&self) -> Arc<dyn ProcessControl> {
        Arc::new(SystemControl { child: self.child.clone(), pid: self.pid })
    }
}

#[cfg(windows)]
mod os {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME};

    /// 对进程的每个线程执行一次操作；一个线程都没碰到时返回 false
    fn each_thread(pid: u32, op: unsafe extern "system" fn(HANDLE) -> u32) -> bool {
        // SAFETY: 只调用文档化的 Toolhelp / 线程 API；句柄在本函数内打开并关闭
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut touched = false;
            let mut more = Thread32First(snap, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        touched |= op(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            touched
        }
    }

    pub fn suspend(pid: u32) -> bool {
        each_thread(pid, SuspendThread)
    }

    pub fn resume(pid: u32) -> bool {
        each_thread(pid, ResumeThread)
    }

    struct JobHandle(HANDLE);
    // SAFETY: 作业对象句柄可以跨线程使用，这里只在创建后读取
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    /// 应用内唯一的作业对象，设置了"句柄关闭时结束其中全部进程"。句柄从不主动关闭，
    /// 应用退出、崩溃或被强杀时由系统关闭，ffmpeg 随之结束，不会变成孤儿进程继续写文件
    fn job() -> Option<&'static JobHandle> {
        static JOB: OnceLock<Option<JobHandle>> = OnceLock::new();
        JOB.get_or_init(|| {
            // SAFETY: 文档化的作业对象 API；失败时关闭句柄并放弃绑定
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(JobHandle(h))
            }
        })
        .as_ref()
    }

    pub fn bind(child: &Child) {
        if let Some(job) = job() {
            // SAFETY: 句柄来自刚启动、仍然存活的子进程
            unsafe {
                AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE);
            }
        }
    }

    pub fn die_with_parent(_cmd: &mut Command) {}
}

#[cfg(unix)]
mod os {
    use std::process::{Child, Command, Stdio};

    /// Linux：父进程退出时子进程收到 SIGKILL。macOS 没有这个机制，由 [`bind`] 起的看门狗处理
    pub fn die_with_parent(cmd: &mut Command) {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: pre_exec 里只调用异步信号安全的 prctl
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    Ok(())
                });
            }
        }
        let _ = cmd;
    }

    /// macOS 等没有 `PR_SET_PDEATHSIG` 的系统：给每个 ffmpeg 配一个看门狗，应用被强杀后一秒内结束它
    pub fn bind(child: &Child) {
        if cfg!(not(target_os = "linux")) {
            watch(std::process::id(), child.id());
        }
    }

    /// 看门狗是一个 sh 小循环：父进程与子进程都还在就每秒看一眼；父进程没了而子进程还在，就结束子进程。
    /// 它是应用的子进程，应用被杀后由 launchd 接管继续跑完这个循环。用一个线程等它退出，免得留下僵尸进程
    pub fn watch(parent: u32, child: u32) {
        let script = format!(
            "while kill -0 {parent} 2>/dev/null && kill -0 {child} 2>/dev/null; do sleep 1; done; \
             kill -0 {parent} 2>/dev/null || kill -9 {child} 2>/dev/null"
        );
        let spawned = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut dog) = spawned {
            std::thread::spawn(move || {
                let _ = dog.wait();
            });
        }
    }

    pub fn suspend(pid: u32) -> bool {
        // SAFETY: 向自己启动的子进程发信号
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) == 0 }
    }

    pub fn resume(pid: u32) -> bool {
        // SAFETY: 同上
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) == 0 }
    }
}

#[cfg(not(any(windows, unix)))]
mod os {
    pub fn die_with_parent(_cmd: &mut std::process::Command) {}

    pub fn bind(_child: &std::process::Child) {}

    pub fn suspend(_pid: u32) -> bool {
        false
    }

    pub fn resume(_pid: u32) -> bool {
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// 看门狗：假的"父进程"退出后，子进程在几秒内被结束；父进程还在时不动它
    #[test]
    fn watchdog_kills_the_child_once_the_parent_is_gone() {
        let spawn = |secs: &str| Command::new("sleep").arg(secs).stdin(Stdio::null()).spawn().unwrap();
        let mut parent = spawn("1");
        let mut child = spawn("30");
        super::os::watch(parent.id(), child.id());
        std::thread::sleep(Duration::from_millis(300));
        assert!(child.try_wait().unwrap().is_none(), "父进程还在时不能动子进程");
        // 回收父进程，kill -0 才会失败（僵尸进程对 kill -0 仍有响应）
        parent.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "父进程退出后子进程没有被结束");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
