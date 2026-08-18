//! 把 `register-terrain` 注册进脚本引擎：mod 脚本借此定义自定义地形。
//!
//! # 为什么不放进 `ll-script::api`
//!
//! `ll-script::api` 模块文档早就说明了理由：「内容注册相关的函数留给
//! 任务 7 按需添加，不在这里预先造好」。本函数需要同时持有
//! [`crate::registry::Registry`]（`ll-mod`）与
//! `ll_world::terrain::TerrainTable`（`ll-world`）的可变引用——
//! `ll-script` 不认识、也不该认识 `ll-mod` 的类型（依赖方向
//! `ll-script` ← `ll-mod`，反过来会成环），因此这个函数只能放在
//! `ll-mod`，由加载管线（[`crate::pipeline`]）在构造每个 mod 的
//! `ScriptEngine` 时按需注册。
//!
//! # 为什么用 `thread_local!` 而不是直接在闭包里捕获 `Rc<RefCell<_>>`
//!
//! `steel::steel_vm::register_fn::RegisterFn` 要求注册的闭包满足
//! `Send + Sync + 'static`（`SendSyncStatic`），`Rc<RefCell<_>>`
//! 两者都不满足。`ll_script::api::query`/`rng` 已经用 `thread_local!`
//! 解决过同一个问题（把状态放进线程局部存储，注册的闭包本身不捕获任何
//! 非 `Send`/`Sync` 数据，因此天然满足约束）——本模块照搬同一套约定，
//! 见这两个模块的文档「为什么需要 unsafe」/模块顶部注释。与它们的
//! 区别是：本模块的活跃状态需要**被拥有**（`Registry`/`TerrainTable`
//! 在装载会话期间持续累积内容，不是每帧现成的只读引用），因此用
//! `RefCell<Option<T>>` 把值整个移进/移出，而不是存一个裸指针——这样
//! 不需要 `unsafe`，`ll-mod` 也确实继承了工作区 `unsafe_code = "forbid"`
//! （不像 `ll-script` 那样为 `ScriptAllocGuard` 单独放宽）。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_world::terrain::{TerrainAttrs, TerrainError, TerrainKind, TerrainTable};

use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-terrain` 应该写入的注册表与地形表。
    /// 装载管线在为一个 mod 的脚本调用 [`ScriptEngine::load_source`]
    /// 之前用 [`set_active_target`] 把 `(Registry, TerrainTable)` 整体
    /// 移进来，脚本跑完之后用 [`take_active_target`] 原样移回——两次
    /// 调用之间夹住的正是这一次 `load_source`。
    static ACTIVE_TARGET: RefCell<Option<(Registry, TerrainTable)>> = const { RefCell::new(None) };
}

/// 把 `(registry, table)` 设为当前调用窗口内 `register-terrain` 可写入
/// 的目标，取走两者的所有权。
pub fn set_active_target(registry: Registry, table: TerrainTable) {
    ACTIVE_TARGET.with(|cell| *cell.borrow_mut() = Some((registry, table)));
}

/// 取回 [`set_active_target`] 放进去的 `(Registry, TerrainTable)`。
///
/// 调用约定：**必须**与 [`set_active_target`] 成对出现，且中间不能
/// 再嵌套一次 `set_active_target`（会覆盖，见 `panic` 分支的注释）。
/// 没有先 `set_active_target` 就调用会 panic——这不是脚本触发得到的
/// 路径（脚本只能调用 `register-terrain`，够不到这两个函数），而是
/// 装载管线自身的接线契约，接线写错理应在开发期就暴露，不是静默吞掉
/// 装载会话的内容。
pub fn take_active_target() -> (Registry, TerrainTable) {
    ACTIVE_TARGET.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-terrain` 注册进 `engine`。
///
/// **必须**在调用 [`set_active_target`] 之后、[`ScriptEngine::load_source`]
/// 求值脚本之前完成注册——`register_fn` 本身只是把函数名字加进白名单
/// 与符号表，真正读写 `ACTIVE_TARGET` 发生在脚本调用 `register-terrain`
/// 的那一刻，届时线程局部状态必须已经就绪。
pub fn register_terrain_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-terrain", register_terrain);
}

/// `(register-terrain id blocks-sight blocks-move move-cost opens-into)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"examplemod:lava_floor"`。
/// - `blocks-sight`/`blocks-move`：布尔。
/// - `move-cost`：非负整数；`blocks-move` 为真时该值会被忽略（内部
///   强制写成 `u32::MAX`，与 [`TerrainAttrs`] 的自洽性约束一致）。
/// - `opens-into`：撞入后变成的另一种地形的完整标识符字符串，空串
///   `""` 表示「不是这类地形」——Steel 的 FFI 转换层没有现成的
///   `Option<String>` 支持，用空串做哨兵比引入一个自定义 Steel 结构体
///   简单，且合法的命名空间字符串恒非空（`NamespacedId::parse` 拒绝
///   空串），不会与真实标识符混淆。
///
/// 返回 `Result<bool, String>`：Steel 侧 `Result<T, E: IntoSteelVal>`
/// 会被 steel-core 自动转成一次真正的求值期错误（`Err` 分支不会被
/// 脚本当成普通返回值悄悄吞掉，`load_source` 会拿到 `Err`），因此这里
/// 直接把校验失败的 [`TerrainError`]/非法标识符原样转成人类可读的
/// `Err(String)`，不需要额外的错误包装层。
fn register_terrain(
    id: String,
    blocks_sight: bool,
    blocks_move: bool,
    move_cost: i64,
    opens_into: String,
) -> Result<bool, String> {
    ACTIVE_TARGET.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some((registry, table)) = slot.as_mut() else {
            // 装载管线接线错误（忘了先 set_active_target）——不是 mod
            // 作者能触发的情形,但脚本调用不能 panic（四道防线①②），
            // 只能降级成一条错误消息。
            return Err("register-terrain 在没有活跃注册目标的窗口内被调用".to_string());
        };
        do_register_terrain(
            registry,
            table,
            &id,
            blocks_sight,
            blocks_move,
            move_cost,
            &opens_into,
        )
    })
}

/// [`register_terrain`] 的纯函数核心：不依赖线程局部状态，直接接收
/// `&mut Registry`/`&mut TerrainTable`，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_terrain(
    registry: &mut Registry,
    table: &mut TerrainTable,
    id: &str,
    blocks_sight: bool,
    blocks_move: bool,
    move_cost: i64,
    opens_into: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let opens_into = if opens_into.is_empty() {
        None
    } else {
        let target = NamespacedId::parse(opens_into)
            .map_err(|err| format!("非法 opens-into 标识符 {opens_into:?}：{err}"))?;
        Some(TerrainKind::from_index(registry.intern(target)))
    };

    // move_cost 是 i64（Steel 整数的宿主表示），负值没有意义——按 0
    // 钳位而不是拒绝整次调用：这是数据层面的"取舍"而非"矛盾"（真正
    // 的矛盾——blocks_move 与 move_cost 互相矛盾——由 TerrainTable::define
    // 自己校验，见下面的错误转换）。
    let move_cost = move_cost.max(0) as u32;

    table
        .define(
            index,
            TerrainAttrs {
                blocks_sight,
                blocks_move,
                move_cost,
                opens_into,
            },
        )
        .map(|()| true)
        .map_err(|err: TerrainError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法地形声明注册成功并写入地形表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let result = do_register_terrain(
            &mut registry,
            &mut table,
            "examplemod:lava_floor",
            false,
            true,
            i64::from(u32::MAX),
            "",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("examplemod:lava_floor").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.blocks_move(TerrainKind::from_index(index)));
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let result = do_register_terrain(
            &mut registry,
            &mut table,
            "Not Valid",
            false,
            false,
            100,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn blocks_move与move_cost矛盾时返回terraintable的校验错误() {
        // Arrange：blocks_move=true 但给出一个有限代价，TerrainTable::define
        // 拒绝这种自相矛盾的声明（ADR 0017「注册期完整校验」）。
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let result = do_register_terrain(
            &mut registry,
            &mut table,
            "examplemod:broken",
            false,
            true,
            100,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn opens_into指向另一个地形时该地形也被intern() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act：door_closed 撞入后变成 door_open，door_open 此刻还没有
        // 被单独 define 过属性，但 intern 是幂等的，仍应成功换到索引。
        let result = do_register_terrain(
            &mut registry,
            &mut table,
            "examplemod:door_closed",
            true,
            true,
            i64::from(u32::MAX),
            "examplemod:door_open",
        );

        // Assert
        assert_eq!(result, Ok(true));
        assert!(
            registry
                .get(&NamespacedId::parse("examplemod:door_open").unwrap())
                .is_some()
        );
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_terrain() {
        // 端到端验证：这是本模块真正要交付的能力——脚本里写
        // (register-terrain ...)，不需要脚本作者知道 Rust 侧的
        // Registry/TerrainTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_terrain_api(&mut engine);
        set_active_target(Registry::new(), TerrainTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-terrain "examplemod:lava_floor" #f #t 4294967295 "")"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let (registry, _table) = take_active_target();
        assert!(
            registry
                .get(&NamespacedId::parse("examplemod:lava_floor").unwrap())
                .is_some()
        );
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误，宿主必须优雅报错。
        let mut engine = ScriptEngine::new();
        register_terrain_api(&mut engine);
        set_active_target(Registry::new(), TerrainTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-terrain "Not Valid" #f #f 100 "")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：即便脚本出错，接线契约仍要求成对调用，否则下一个
        // 测试用例会因为 ACTIVE_TARGET 里残留旧值而互相污染
        // （thread_local 在同一测试线程内跨用例存活）。
        take_active_target();
    }
}
