//! `Effect`：描述「发生了什么」的纯数据，是 `resolve`（批次 C）与
//! [`crate::apply::apply`] 之间的唯一接口。
//!
//! `resolve` 读世界、算规则、决定判定结果，但**绝不直接改世界**——它
//! 的产出是一串 `Effect` 值，纯数据，不含任何执行逻辑。真正的写入
//! 全部交给 [`crate::apply::apply`] 一处完成（见该函数文档的「三条
//! 纪律」）。这个分离是并行结算的前提：成千上万个 AI 的 `resolve` 可以
//! 同时跑（各自只读世界，互不冲突），产出的 `Effect` 收集起来后再单
//! 线程依次 `apply`，读写从不交织。

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::entity::EntityId;
use ll_world::terrain::TerrainKind;

/// 「发生了什么」的纯数据描述。
///
/// 不要求可序列化（不像 [`crate::intent::Intent`]）：`Effect` 是
/// `resolve` 到 `apply` 之间同一进程内、同一次结算里的瞬时产物，算完
/// 立刻被 `apply` 消费掉，不需要跨进程/跨存档留存——真正要长期保留、
/// 用于重放的是产生它的 [`crate::intent::Intent`]。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// 把某实体的位置设为 `pos`。
    MoveTo {
        /// 被移动的实体。
        actor: EntityId,
        /// 目标位置。
        pos: TorusPos,
    },
    /// 对某实体造成 `amount` 点伤害（从其生命值里减去）。
    Damage {
        /// 受创的实体。
        target: EntityId,
        /// 伤害量。是否致死等规则判断不在这里——`apply` 只做减法。
        amount: i32,
    },
    /// 销毁某实体。
    Kill {
        /// 被销毁的实体。
        target: EntityId,
    },
    /// 把某实体下一次可行动的时刻设为 `at`。
    ///
    /// 只写 [`ll_world::entity::Agent::next_action_at`] 这个字段本身，
    /// 不触碰任何时间轴队列——真正把该实体重新排入时间轴（调用
    /// `ll_sim::timeline::Timeline::schedule`）是调用方在 `apply`
    /// 返回之后另行要做的事：`apply` 的签名只有 `&mut WorldState`，
    /// 拿不到调用方持有的 `Timeline`（`Timeline` 定义在本 crate，是
    /// 运行期的调度缓存，不是存档的一部分，因此不在 `WorldState`
    /// 内——见 `timeline` 模块文档）。
    ScheduleNext {
        /// 被重新安排的实体。
        actor: EntityId,
        /// 下一次可行动的世界时刻。
        at: Tick,
    },
    /// 把某位置的地形设为 `kind`。
    SetTerrain {
        /// 目标位置。
        pos: TorusPos,
        /// 目标地形。
        kind: TerrainKind,
    },
    /// 调整某实体的钱包，`delta` 可正可负。
    AdjustWallet {
        /// 被调整的实体。
        actor: EntityId,
        /// 调整量。
        delta: i64,
    },
}
