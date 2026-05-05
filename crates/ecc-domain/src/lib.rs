//! ecc-domain — 核心领域模型 + Repository trait。
//!
//! 零外部依赖（仅 serde + chrono），所有其他 crate 依赖此 crate。

pub mod mapping;
pub mod pricing;
pub mod provider;
pub mod repository;
