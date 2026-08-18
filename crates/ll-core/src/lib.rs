//! 迷途大陆的纯数据基础层。
//!
//! 本 crate 被项目中所有其他 crate 依赖，因此**必须保持零运行时依赖**。
//! 任何引入这里的第三方依赖都会传染给整个项目。
//!
//! 设计约束（源自总纲规格）：
//! - 世界状态禁止浮点数。跨平台浮点差异会摧毁确定性存档与重放。
//! - 随机性只能来自按实体 ID 派生的确定性流，禁止全局 RNG。
//! - 环面距离只能通过本 crate 的类型计算，禁止在别处手写。

pub mod error;
pub mod hashing;
pub mod ident;
pub mod light;
pub mod rng;
pub mod scaled;
pub mod time;
pub mod torus;
