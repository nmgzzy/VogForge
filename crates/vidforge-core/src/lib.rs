//! VidForge 核心库：全部业务逻辑，不依赖 Tauri（设计文档 1.1）。
//!
//! 决策模块（pipeline 下的命令构建、策略、保真度求解）是纯函数，环境信息只通过
//! [`model::Capabilities`] 传入；与外部进程打交道的部分集中在 [`ffmpeg`]。

pub mod config;
pub mod external;
pub mod ffmpeg;
pub mod model;
pub mod sysinfo;
pub mod util;
