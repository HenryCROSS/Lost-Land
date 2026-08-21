//! 实体存储：两层，各自匹配自己的访问模式。
//!
//! # 为什么两层用不同排布
//!
//! | 层 | 规模 | 排布 | 理由 |
//! |---|---|---|---|
//! | 薄层（[`ThinPopulation`]，被记住） | 数十万～数百万 | 列式 SoA | 批量遍历单一字段，可向量化 |
//! | 厚层（[`Arena`]，被模拟） | 数百，有界 | 行式 AoS | 数量少、按实体随机访问、一次读全部字段 |
//!
//! 薄层要容纳数十万到数百万 NPC，靠批量公式驱动（见
//! `knowledge/design/agent-goals-and-economy.md` 「三档精度」一节）。
//! 批量算一百万个钱包是一次列式操作——若 `Agent` 是十来个字段的结构体
//! 存在 `Vec<Agent>`（行式 / AoS），只读 `wallet` 一个字段也会把整条
//! 缓存行拉进来，浪费十倍内存带宽。厚层反过来：数量少、按实体随机
//! 访问、一次要读它的全部字段，行式排布更优。**两层用不同排布不是
//! 不一致，是各自匹配访问模式。**
//!
//! # 为什么不引入 ECS 框架
//!
//! 采用 ECS 的列式存储（SoA）思想，但不引入 ECS 框架——`hecs` 的原型
//! 动态分组是为组件动态增删设计的，薄层字段是固定模式，人人一样，动态
//! 分组只有开销没有收益；也不做「找出同时具备 A 与 B 组件的实体」这类
//! 稀疏组件查询；系统调度由已有的时间轴与 Intent 管线负责，不需要额外
//! 的调度器。引入 `hecs` 等于付了原型管理的代价却拿不到收益，还要额外
//! 跟它的序列化与迭代顺序不确定性搏斗，而后者恰是自由读档与确定性
//! 重放的地基。见 `docs/superpowers/specs/2026-08-16-lostland-design.md`
//! §3 技术栈表。

mod affiliation;
mod agent;
mod arena;
mod goal;
mod id;
mod org;
mod stats;
mod thin;

pub use affiliation::{Affiliation, AffiliationKind, OrgRef};
pub use agent::{Agent, RestState};
pub use arena::Arena;
pub use goal::Goal;
pub use id::{EntityId, FamilyId};
pub use org::OrgInstance;
pub use stats::{ActiveStatModifier, AttributeKind, BaseStats};
pub use thin::{ThinPopulation, ThinSlot};
