//! 时间轴调度器的黑箱测试：只经由 `ll_sim::timeline` 的公开 API 驱动，
//! 覆盖 Task 3 简报要求的全部用例与三条属性测试。

use ll_core::time::Tick;
use ll_sim::timeline::{Timeline, action_cost};
use ll_world::entity::{Arena, EntityId};
use proptest::prelude::*;

/// 建一批互不相同的 [`EntityId`]，下标小的实体号也更小（同一个空
/// [`Arena`] 里连续 `spawn`，下标严格递增），供打破平局的测试使用。
///
/// `Arena<T>` 是本 crate 唯一能从外部拿到 `EntityId` 的公开入口——
/// `EntityId` 的构造函数是 `ll-world` 内部私有的，见其字段文档。
fn entities(count: usize) -> Vec<EntityId> {
    let mut arena: Arena<()> = Arena::new();
    (0..count).map(|_| arena.spawn(())).collect()
}

#[test]
fn 最早的条目先弹出() {
    // Arrange
    let ids = entities(2);
    let mut timeline = Timeline::new();
    timeline.schedule(ids[0], Tick(50));
    timeline.schedule(ids[1], Tick(10));

    // Act
    let popped = timeline.pop_next().expect("非空队列必有条目");

    // Assert
    assert_eq!(popped.actor, ids[1]);
}

#[test]
fn 同刻条目按实体号升序弹出() {
    // Arrange：ids[0] 下标更小，实体号也更小；故意先排入 ids[1]，
    // 若弹出顺序依赖插入顺序而非实体号，这里会先弹出 ids[1]。
    let ids = entities(2);
    let mut timeline = Timeline::new();
    timeline.schedule(ids[1], Tick(10));
    timeline.schedule(ids[0], Tick(10));

    // Act
    let popped = timeline.pop_next().expect("非空队列必有条目");

    // Assert
    assert_eq!(popped.actor, ids[0]);
}

#[test]
fn 敏捷翻倍则行动耗时减半() {
    // Arrange
    let base_cost = 1000;

    // Act
    let normal = action_cost(base_cost, 1000);
    let doubled_speed = action_cost(base_cost, 2000);

    // Assert
    assert_eq!(doubled_speed, normal / 2);
}

#[test]
fn 敏捷为零时不会除零() {
    // Act：敏捷为零应等价于敏捷为一（`max(1, …)` 的钳制）。
    let cost = action_cost(1000, 0);

    // Assert
    assert_eq!(cost, action_cost(1000, 1));
}

#[test]
fn 移除某实体后其条目不再弹出() {
    // Arrange
    let ids = entities(2);
    let mut timeline = Timeline::new();
    timeline.schedule(ids[0], Tick(5));
    timeline.schedule(ids[1], Tick(10));

    // Act
    timeline.remove(ids[0]);
    let popped = timeline.pop_next().expect("剩余条目应仍可弹出");

    // Assert
    assert_eq!(popped.actor, ids[1]);
}

#[test]
fn 空队列弹出返回空值() {
    // Arrange
    let mut timeline = Timeline::new();

    // Act & Assert
    assert!(timeline.pop_next().is_none());
}

#[test]
fn 序列化往返后弹出顺序不变() {
    // Arrange
    let ids = entities(3);
    let mut timeline = Timeline::new();
    timeline.schedule(ids[0], Tick(30));
    timeline.schedule(ids[1], Tick(10));
    timeline.schedule(ids[2], Tick(20));
    let json = serde_json::to_string(&timeline).expect("时间轴必可序列化");
    let mut decoded: Timeline = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

    // Act
    let mut original_order = Vec::new();
    while let Some(entry) = timeline.pop_next() {
        original_order.push(entry.actor);
    }
    let mut decoded_order = Vec::new();
    while let Some(entry) = decoded.pop_next() {
        decoded_order.push(entry.actor);
    }

    // Assert
    assert_eq!(decoded_order, original_order);
}

proptest! {
    #[test]
    fn 弹出顺序恒按时刻单调不减(
        schedules in prop::collection::vec((0i64..1000, 0usize..8), 0..64),
    ) {
        // Arrange
        let ids = entities(8);
        let mut timeline = Timeline::new();
        for (tick, idx) in &schedules {
            timeline.schedule(ids[*idx], Tick(*tick));
        }

        // Act & Assert
        let mut last_tick: Option<i64> = None;
        while let Some(entry) = timeline.pop_next() {
            if let Some(prev) = last_tick {
                prop_assert!(entry.at.0 >= prev);
            }
            last_tick = Some(entry.at.0);
        }
    }

    #[test]
    fn 任意调度序列下弹出总数等于调度总数(
        schedules in prop::collection::vec((0i64..1000, 0usize..8), 0..64),
    ) {
        // Arrange
        let ids = entities(8);
        let mut timeline = Timeline::new();
        for (tick, idx) in &schedules {
            timeline.schedule(ids[*idx], Tick(*tick));
        }

        // Act
        let mut popped = 0usize;
        while timeline.pop_next().is_some() {
            popped += 1;
        }

        // Assert
        prop_assert_eq!(popped, schedules.len());
    }

    #[test]
    fn 序列化往返前后弹出顺序完全一致(
        schedules in prop::collection::vec((0i64..1000, 0usize..8), 0..64),
    ) {
        // Arrange
        let ids = entities(8);
        let mut timeline = Timeline::new();
        for (tick, idx) in &schedules {
            timeline.schedule(ids[*idx], Tick(*tick));
        }
        let json = serde_json::to_string(&timeline).expect("时间轴必可序列化");
        let mut decoded: Timeline = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Act
        let mut original_order = Vec::new();
        while let Some(entry) = timeline.pop_next() {
            original_order.push((entry.at, entry.actor));
        }
        let mut decoded_order = Vec::new();
        while let Some(entry) = decoded.pop_next() {
            decoded_order.push((entry.at, entry.actor));
        }

        // Assert
        prop_assert_eq!(decoded_order, original_order);
    }
}
