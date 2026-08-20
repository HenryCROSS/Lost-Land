//! 装载会话内唯一共享的活跃 [`Registry`]——供全部脚本注册函数
//! （`register-terrain`/`register-class`/`register-skill`/
//! `register-subclass`/`register-quest`/`register-race`）在同一次
//! `ScriptEngine::load_source` 调用窗口内共同读写。
//!
//! # 为什么 `Registry` 需要单独拆出来，不能照抄 `script_terrain_api.rs`
//! 「把 `(Registry, Table)` 整体move进一个 thread_local」的写法
//!
//! `script_terrain_api.rs`（Task 11/12）最初把 `Registry` 与
//! `TerrainTable` 打包成一个元组，一起 move 进它自己私有的
//! `thread_local!`——当时脚本唯一能注册的内容就是地形，这样写没有
//! 问题。但一个 mod 脚本现在可能在同一个文件里既调用 `register-terrain`
//! 又调用 `register-class`（甚至更多），而这些调用**必须落在同一个
//! `Registry` 实例上**——`ContentIndex` 的分配依赖 `Interner` 内部单调
//! 递增的计数器，若给地形一份 `Registry`、给职业另一份 `Registry`，
//! 两类内容会各自从索引 0 开始编号，`ContentIndex` 会撞车（地形的
//! `lostland:grass`（索引 3）与某个职业撞到同一个索引 3），彻底破坏
//! 「全部内容共享同一段 `ContentIndex` 号段」这条已经在多处模块文档里
//! 反复确认过的前提（`crate::class`/`crate::skill` 等模块文档「下标
//! 空间是全局 `ContentIndex` 号段的一部分」）。
//!
//! 因此 `Registry` 必须是**唯一**的一份共享状态，被全部注册函数共用；
//! 各类内容各自的表（`TerrainTable`/`ClassTable`/……）才按类型分别持有
//! 在各自模块的 `thread_local!` 里（见 `crate::script_terrain_api`/
//! `crate::script_class_api` 等模块文档）。两者的生命周期由装载管线
//! （[`crate::pipeline`]）在同一个调用窗口内成对管理：先
//! [`set_active_registry`]，再对每张表分别 `set_active_target`，脚本跑
//! 完后按相反顺序依次取回。
//!
//! # 与 `RefCell<Option<T>>` 而非裸指针的选择
//!
//! 理由与 `script_terrain_api.rs` 模块文档「为什么用 `thread_local!`
//! 而不是直接在闭包里捕获 `Rc<RefCell<_>>`」一节完全相同：`Registry`
//! 需要在装载会话期间被**拥有**（跨多次脚本调用持续累积内容），不是
//! 每次调用现成的只读引用，因此用 `RefCell<Option<Registry>>` 把值
//! 整个移进/移出，不需要 `unsafe`——`ll-mod` 继承了工作区
//! `unsafe_code = "forbid"`。

use std::cell::RefCell;

use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，全部注册函数应当写入的 `Registry`。
    static ACTIVE_REGISTRY: RefCell<Option<Registry>> = const { RefCell::new(None) };
}

/// 把 `registry` 设为当前调用窗口内全部注册函数可写入的目标，取走其
/// 所有权。
pub fn set_active_registry(registry: Registry) {
    ACTIVE_REGISTRY.with(|cell| *cell.borrow_mut() = Some(registry));
}

/// 取回 [`set_active_registry`] 放进去的 `Registry`。
///
/// 调用约定与 `script_terrain_api::take_active_target` 完全相同：必须
/// 与 [`set_active_registry`] 成对出现，没有先 `set_active_registry`
/// 就调用会 panic——这不是脚本触发得到的路径（脚本只能调用具体的
/// `register-*` 函数，够不到这两个函数本身），而是装载管线自身的接线
/// 契约，接线写错理应在开发期就暴露。
pub fn take_active_registry() -> Registry {
    ACTIVE_REGISTRY.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_registry 必须与 set_active_registry 成对调用")
    })
}

/// 在当前活跃的 `Registry` 上执行 `f`，供各类型的 `register-*` 函数
/// 共用。没有活跃 `Registry`（装载管线接线错误，忘了先
/// `set_active_registry`）时返回一条错误消息而不是 panic——与
/// `script_terrain_api::register_terrain` 处理同一种情形的方式一致
/// （四道防线①②要求脚本调用路径不能 panic）。
pub(crate) fn with_active_registry<T>(
    f: impl FnOnce(&mut Registry) -> Result<T, String>,
) -> Result<T, String> {
    ACTIVE_REGISTRY.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(registry) = slot.as_mut() else {
            return Err("register-* 在没有活跃 Registry 的窗口内被调用".to_string());
        };
        f(registry)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 设置后可以取回同一个registry实例的内容() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse("lostland:grass").expect("合法标识符"));
        set_active_registry(registry);

        // Act
        let restored = take_active_registry();

        // Assert
        assert!(
            restored
                .get(&NamespacedId::parse("lostland:grass").expect("合法标识符"))
                .is_some()
        );
    }

    #[test]
    fn 没有活跃registry时with_active_registry返回错误而不panic() {
        // Arrange：确保没有残留状态（测试线程内 thread_local 可能被
        // 前一个用例设置过）。
        // 由于本模块没有暴露"清空但不 take"的方法，这里直接
        // set + take 一次，确保处于"未设置"状态。
        set_active_registry(Registry::new());
        take_active_registry();

        // Act
        let result = with_active_registry(|_registry| Ok::<(), String>(()));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn with_active_registry能读写当前活跃的registry() {
        // Arrange
        set_active_registry(Registry::new());

        // Act
        let index = with_active_registry(|registry| {
            Ok::<_, String>(registry.intern(NamespacedId::parse("lostland:grass").expect("合法")))
        })
        .expect("活跃窗口内调用应当成功");

        // Assert
        let registry = take_active_registry();
        assert_eq!(
            registry.get(&NamespacedId::parse("lostland:grass").expect("合法")),
            Some(index)
        );
    }
}
