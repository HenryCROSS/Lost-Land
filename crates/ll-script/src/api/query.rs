//! 世界只读查询：地形、时间、光照。脚本能读，但没有任何写入路径——
//! 本模块只注册返回值的函数，不存在任何一个接收"要写什么"参数的函数。
//!
//! # 活跃世界指针——为什么需要 unsafe
//!
//! Steel 的 `register_fn` 要求闭包/函数指针是 `'static` 的，但
//! `WorldState` 每一帧都在变、生命周期由宿主的游戏循环持有，不可能
//! `'static`。做法是宿主在调用脚本前用 [`set_active_world`] 记下当前
//! `WorldState` 的裸指针，脚本调用窗口结束后 [`clear_active_world`]。
//!
//! 这是 `ll-script`（本项目唯一允许出现 unsafe 的 crate）里第二处
//! unsafe——第一处是 `alloc_guard.rs` 的 `GlobalAlloc` 实现，理由相同：
//! Rust 的类型系统没有"这个引用只在这个调用窗口内有效"这种一等概念，
//! 只能用裸指针加明确写下的调用约定来表达，安全性由调用方手工维持而
//! 不是编译器保证。

use std::cell::Cell;

use ll_world::light::ambient_light;
use ll_world::state::WorldState;

use crate::host::ScriptEngine;

thread_local! {
    static ACTIVE_WORLD: Cell<*const WorldState> = const { Cell::new(std::ptr::null()) };
}

/// 设置当前调用窗口内脚本可以只读查询的世界。
///
/// # Safety
///
/// 调用方必须保证 `world` 指向的数据在「本次调用直到对应的
/// [`clear_active_world`]」这段窗口内持续有效，且窗口内没有任何代码
/// 可变借用它。本模块注册给脚本的查询函数只做只读访问，但裸指针背后
/// 的借用规则无法由编译器检查，这个不变式必须由调用方手工维持。
pub unsafe fn set_active_world(world: &WorldState) {
    ACTIVE_WORLD.with(|cell| cell.set(std::ptr::from_ref(world)));
}

/// 清空活跃世界指针。
pub fn clear_active_world() {
    ACTIVE_WORLD.with(|cell| cell.set(std::ptr::null()));
}

/// 安全包装：在 `f` 执行期间把 `world` 设为活跃世界，执行完毕后（无论
/// `f` 是否 panic 都不例外，因为清空发生在 `f()` 返回之后而不是靠
/// 调用方记得写第二行）清空。
///
/// # 为什么需要这一层，[`set_active_world`] 不够用
///
/// [`set_active_world`] 是 `unsafe fn`——本 crate 允许 `unsafe`（见
/// crate 顶层 `Cargo.toml` 的 lints 覆盖说明），但下游 crate 未必允许：
/// `ll-mod` 继承工作区 `unsafe_code = "forbid"`，`ScriptEngine` 的运行
/// 期决策来源实现（`ll_mod::script_behavior_source::ScriptBehaviorSource`）
/// 需要设置活跃世界，却没有能力写一个 `unsafe` 块。这个函数把
/// [`set_active_world`] 的安全性论证（"调用方必须保证 `world` 在设置
/// 到清空这段窗口内持续有效、不被可变借用"）**在类型签名层面兑现**：
/// `world: &WorldState`（共享借用，编译期保证不被可变借用）+ `f` 在
/// 设置之后、清空之前被调用且恰好只调用这一次——调用方不需要、也没有
/// 办法违反这个窗口，`unsafe` 因此可以完全封装在本函数内部，不需要
/// 暴露给任何调用方。
pub fn with_active_world_for<T>(world: &WorldState, f: impl FnOnce() -> T) -> T {
    // Safety: `world` 是 `&WorldState`（共享引用），编译期已经保证在
    // 本函数返回之前不会被可变借用；`clear_active_world` 紧跟在 `f()`
    // 之后无条件执行，窗口精确等于 `f` 的执行期间。
    unsafe {
        set_active_world(world);
    }
    let result = f();
    clear_active_world();
    result
}

/// 在活跃世界上执行 `f`；没有设置活跃世界时返回 `default`。
///
/// 「没有活跃世界」按调用约定不应该发生（宿主总应该在调用脚本前设置
/// 好），但防御性地返回默认值而不是 panic，是本类型对「宿主接线可能
/// 有 bug」这种情况的降级策略——查询函数本身不属于「脚本错误」四道
/// 防线覆盖的范围，但同样的降级思路仍然适用：宁可给出一个明确、可预期
/// 的默认值，也不要让整个游戏进程因为一次接线疏忽而崩溃。
///
/// `pub(crate)`——`api::state`（脚本状态存储）复用同一套活跃世界指针
/// 机制读取已提交的 `WorldState`（配额判定、`state-get!` 系列读取），
/// 不需要另起一套 `unsafe` 裸指针基础设施。
pub(crate) fn with_active_world<T>(default: T, f: impl FnOnce(&WorldState) -> T) -> T {
    ACTIVE_WORLD.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            default
        } else {
            // Safety: 非空指针只能来自 `set_active_world`，其调用方已经
            // 承诺了指针指向的数据在本次调用窗口内有效且不被可变借用。
            f(unsafe { &*ptr })
        }
    })
}

/// 注册世界只读查询函数。
pub fn register(engine: &mut ScriptEngine) {
    engine.register_fn("world-move-cost-at", world_move_cost_at);
    engine.register_fn("world-blocks-sight-at", world_blocks_sight_at);
    engine.register_fn("world-tick", world_tick);
    engine.register_fn("world-ambient-light", world_ambient_light);
}

/// `(world-move-cost-at x y)`：该格地形的移动代价。坐标未经归一化的
/// 环面坐标，走 `TorusSize::wrap`——不手写取模，环面坐标只走
/// `TorusSize` 的方法（硬性约束）。
fn world_move_cost_at(x: i64, y: i64) -> i64 {
    with_active_world(0, |world| {
        let pos = world.size.wrap(x as i32, y as i32);
        // 只读查询：脚本层只能拿到 &WorldState（见本模块文档「活跃
        // 世界指针」），不能触发 SurfaceStore 的按需生成——与
        // `ll-sim::resolve` 保持只读的理由相同，见
        // `WorldState::terrain_at` 文档。坐标所属区块尚未常驻时降级
        // 成与「没有活跃世界」相同的默认值 0，不 panic。
        world
            .terrain_at(pos)
            .map(|kind| kind.move_cost(&world.terrain_table) as i64)
            .unwrap_or(0)
    })
}

/// `(world-blocks-sight-at x y)`：该格地形是否阻挡视线。
fn world_blocks_sight_at(x: i64, y: i64) -> bool {
    with_active_world(false, |world| {
        let pos = world.size.wrap(x as i32, y as i32);
        // 同上：未常驻时降级为「不阻挡」，与「没有活跃世界」同一个
        // 默认值，不触发生成。
        world
            .terrain_at(pos)
            .map(|kind| kind.blocks_sight(&world.terrain_table))
            .unwrap_or(false)
    })
}

/// `(world-tick)`：当前世界时钟。
fn world_tick() -> i64 {
    with_active_world(0, |world| world.clock.0)
}

/// `(world-ambient-light)`：当前环境光照等级。
fn world_ambient_light() -> i64 {
    with_active_world(0, |world| ambient_light(world.clock).0 as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::generate::GenParams;
    use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};

    fn small_world() -> (WorldState, BaseTerrainIds) {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).unwrap();
        let layout = ll_world::zone::ZoneLayout::new(64, zone_count).unwrap();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .unwrap();
        (world, terrain_ids)
    }

    #[test]
    fn 没有设置活跃世界时查询返回默认值而不崩溃() {
        // Arrange
        clear_active_world();

        // Act & Assert
        assert_eq!(world_tick(), 0);
        assert!(!world_blocks_sight_at(0, 0));
    }

    #[test]
    fn 脚本能通过注册函数读到活跃世界的地形代价() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (world-move-cost-at 0 0))".to_string())
            .unwrap();
        let (mut world, terrain_ids) = small_world();
        world
            .terrain
            .set_terrain(world.size.wrap(0, 0), terrain_ids.grass);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert
        let expected = terrain_ids.grass.move_cost(&world.terrain_table) as isize;
        assert_eq!(result, Ok(steel::rvals::SteelVal::IntV(expected)));
    }

    #[test]
    fn 脚本能读到活跃世界的时钟() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (world-tick))".to_string())
            .unwrap();
        let (mut world, _terrain_ids) = small_world();
        world.advance(5);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert
        assert_eq!(
            result,
            Ok(steel::rvals::SteelVal::IntV(world.clock.0 as isize))
        );
    }
}
