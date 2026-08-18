//! 非线性时间轴调度器。
//!
//! # 为什么不是「每个实体一轮」的传统回合制
//!
//! 本游戏的核心手感是敏捷高的角色能在慢角色行动一次的时间里行动两次
//! 甚至三次——而不是「所有实体各行动一次」的严格轮换。做法是给每个
//! 实体维护一个「下次可行动的世界时刻」，行动后按其敏捷与本次行动的
//! 代价重新计算下一次可行动的时刻，再放回队列；每次只弹出全队列里
//! 最早的那一条。谁的敏捷越高，同一段时间窗口内被弹出的次数就越多。
//!
//! # 为什么队列只装 `(Tick, EntityId)`
//!
//! 约束 C2：队列不得装闭包、trait 对象、函数指针——存档要能把整条
//! 队列序列化下来，而这些都不可序列化。真正「弹出后要做什么」（读取
//! 该实体的意图、跑 `resolve`）由调用方在拿到 [`TimelineEntry`] 之后
//! 另行决定，不是这个类型的职责。
//!
//! # 同刻打破平局
//!
//! 两个实体在同一 `Tick` 都要行动时，弹出顺序若依赖 `BinaryHeap` 的
//! 内部堆结构（进而依赖插入历史），存档读回后重建的堆哪怕装的是同一
//! 批条目，弹出顺序也可能与原来不同——重放就此分叉。这里选择按
//! `EntityId` 升序打破平局：[`TimelineEntry`] 派生的 `Ord`
//! 先比较 `at` 再比较 `actor`，是一个不依赖任何构造历史、只由字段值
//! 决定的稳定全序。

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ll_core::time::Tick;
use ll_world::entity::EntityId;
use serde::{Deserialize, Serialize};

/// 时间轴队列里的一条待行动记录：某实体计划在某个世界时刻行动。
///
/// 只装「谁、什么时候」——见模块文档「为什么队列只装 `(Tick,
/// EntityId)`」一节。`Ord` 按字段声明顺序派生（先 `at` 后 `actor`），
/// 这既是队列的弹出顺序，也直接是「同刻按实体号升序」的打破平局
/// 规则本身，不需要另外的比较逻辑。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// 计划行动的世界时刻。
    pub at: Tick,
    /// 计划行动的实体。
    pub actor: EntityId,
}

/// 非线性时间轴：按「下次可行动时刻」排出全部待行动实体的弹出顺序。
///
/// 内部用 `BinaryHeap<Reverse<TimelineEntry>>`——标准库的
/// `BinaryHeap` 是大顶堆，包一层 [`Reverse`] 使得 `TimelineEntry` 自身
/// 「更小」（更早、同刻实体号更小）的条目变成堆序意义上的「更大」，
/// 从而被优先弹出，不需要为堆单独写一份反向 `Ord`。
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    heap: BinaryHeap<Reverse<TimelineEntry>>,
}

impl Timeline {
    /// 建一个空的时间轴。
    pub fn new() -> Self {
        Timeline {
            heap: BinaryHeap::new(),
        }
    }

    /// 把某个实体排入队列，计划在 `at` 时刻行动。
    ///
    /// 不检查该实体是否已在队列中——重复排入是合法用法（例如效果系统
    /// 想让某实体额外多行动一次），去重是调用方的职责，不是时间轴的
    /// 职责。
    pub fn schedule(&mut self, actor: EntityId, at: Tick) {
        self.heap.push(Reverse(TimelineEntry { at, actor }));
    }

    /// 弹出全队列里最早的一条记录；队列为空时返回 [`None`]。
    pub fn pop_next(&mut self) -> Option<TimelineEntry> {
        self.heap.pop().map(|Reverse(entry)| entry)
    }

    /// 移除某实体在队列中的全部条目。
    ///
    /// 用于实体死亡：时间轴可能残留它此前排入的行动，若不清理，
    /// 死后队列弹出到它时会对一个已不存在的实体执行动作。一次移除全部
    /// 条目而非只移除一条——[`Self::schedule`] 允许同一实体重复排入，
    /// 死亡应当清空它全部待行动的记录，而不只是最早的一条。
    pub fn remove(&mut self, actor: EntityId) {
        self.heap.retain(|Reverse(entry)| entry.actor != actor);
    }

    /// 查看队首（最早）记录的时刻，不弹出。队列为空时返回 [`None`]。
    pub fn peek_next_tick(&self) -> Option<Tick> {
        self.heap.peek().map(|Reverse(entry)| entry.at)
    }

    /// 队列中待行动的记录数。
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

/// 序列化为排序后的 `Vec<TimelineEntry>`，而不是 `BinaryHeap` 的内部
/// 数组顺序。
///
/// `BinaryHeap` 的内部数组顺序是堆结构的实现细节，不是弹出顺序本身，
/// 若直接把内部数组写进存档，存档的字节内容会依赖插入历史（同一批
/// 条目以不同顺序插入，内部数组可能不同），即使弹出顺序其实一致。
/// 显式排序后再写出，保证同一逻辑状态恒产出同一份存档字节，也让
/// 存档本身就是可读的「计划行动列表」。
impl Serialize for Timeline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut entries: Vec<TimelineEntry> =
            self.heap.iter().map(|Reverse(entry)| *entry).collect();
        entries.sort();
        entries.serialize(serializer)
    }
}

/// 反序列化时重新入堆：见 [`Serialize`] 实现的文档——存档存的是排序后
/// 的 `Vec`，这里通过 [`Timeline::schedule`] 逐条重新排入，重建出的堆
/// 弹出顺序与序列化前一致（弹出顺序只由条目的值决定，见
/// [`TimelineEntry`] 的 `Ord`，与入堆顺序无关）。
impl<'de> Deserialize<'de> for Timeline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let entries = Vec::<TimelineEntry>::deserialize(deserializer)?;
        let mut timeline = Timeline::new();
        for entry in entries {
            timeline.schedule(entry.actor, entry.at);
        }
        Ok(timeline)
    }
}

/// 算出一次行动的耗时（以 tick 计）。
///
/// 公式（来自属性系统）：`基础代价 × 1000 / max(1, 有效敏捷)`，整数
/// 除法。`max(1, …)` 防止敏捷被临时减到零时除零——即使敏捷归零，行动
/// 依然按「敏捷为 1」计耗时，而不是崩溃或耗时无穷大。
pub fn action_cost(base_cost: u32, effective_speed: u32) -> u32 {
    let speed = u64::from(effective_speed.max(1));
    let cost = u64::from(base_cost) * 1000 / speed;
    // 正常游戏数值下 cost 远小于 u32::MAX；这里用 u64 只是为了让乘法
    // 本身不因中间结果溢出而在 debug 构建下 panic，不是期望这个值真的
    // 逼近上限。
    cost as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 行动代价公式在基准敏捷下等于基础代价() {
        // Arrange：有效敏捷为 1000 时，公式退化为 base_cost * 1000 / 1000。
        // Act
        let cost = action_cost(1000, 1000);

        // Assert
        assert_eq!(cost, 1000);
    }
}
