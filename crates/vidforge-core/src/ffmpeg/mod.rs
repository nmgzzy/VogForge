//! 与 ffmpeg / ffprobe 进程打交道的部分：定位、能力探测、执行。

pub mod capability;
pub mod classify;
pub mod errors;
pub mod exec;
pub mod locate;
pub mod parse;
pub mod probe;
pub mod progress;
