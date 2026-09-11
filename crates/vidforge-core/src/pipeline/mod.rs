//! 转码决策与命令构建。全部是纯函数，环境信息只通过 `Capabilities` 传入（设计文档 1.1 原则二）。

pub mod args;
pub mod container;
pub mod encoders;
pub mod fps;
