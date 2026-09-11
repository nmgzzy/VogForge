//! 前后端共享的数据模型（设计文档第 3 章）。
//!
//! 全部类型通过 ts-rs 导出到 `src/bindings/`（`cargo test` 时生成），前端不再手写。
//! 字段一律 camelCase，可选字段序列化时省略，对应 TS 的 `field?: T`。

mod caps;
mod encoder;
mod media;
mod plan;
mod queue;

pub use caps::*;
pub use encoder::*;
pub use media::*;
pub use plan::*;
pub use queue::*;
