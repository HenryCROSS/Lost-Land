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

/// 在活跃世界上执行 `f`；没有设置活跃世界时返回 `default`。
///
/// 「没有活跃世界」按调用约定不应该发生（宿主总应该在调用脚本前设置
/// 好），但防御性地返回默认值而不是 panic，是本类型对「宿主接线可能
/// 有 bug」这种情况的降级策略——查询函数本身不属于「脚本错误」四道
/// 防线覆盖的范围，但同样的降级思路仍然适用：宁可给出一个明确、可预期
/// 的默认值，也不要让整个游戏进程因为一次接线疏忽而崩溃。
fn with_active_world<T>(default: T, f: impl FnOnce(&WorldState) -> T) -> T {
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
        world.terrain.terrain_at(pos).move_cost() as i64
    })
}

/// `(world-blocks-sight-at x y)`：该格地形是否阻挡视线。
fn world_blocks_sight_at(x: i64, y: i64) -> bool {
    with_active_world(false, |world| {
        let pos = world.size.wrap(x as i32, y as i32);
        world.terrain.terrain_at(pos).blocks_sight()
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
    use ll_world::terrain::TerrainKind;

    fn small_world() -> WorldState {
        let size = ll_core::torus::TorusSize::new(64, 64).unwrap();
        WorldState::new(size, &GenParams::default()).unwrap()
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
        let mut world = small_world();
        world
            .terrain
            .set_terrain(world.size.wrap(0, 0), TerrainKind::GRASS);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert
        let expected = TerrainKind::GRASS.move_cost() as isize;
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
        let mut world = small_world();
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
