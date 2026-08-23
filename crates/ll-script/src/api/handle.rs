//! 脚本持有的不透明实体句柄——只限厚层 `Arena<Agent>`。
//!
//! 落地 `knowledge/design/script-entity-handles-and-batch-queries.md`
//! 三、2/3 节给出的确切形状（该文档整体标注「纯设计，不要求本次
//! 实现」，但本批次的脚本状态存储需要一个可以安全跨越 Steel FFI
//! 边界的实体引用表示——`ScriptValue::Entity(EntityId)` 要存进
//! `WorldState`，脚本侧读写它时不能直接把裸 `EntityId` 的字段（下标、
//! 世代号）暴露给脚本，否则脚本可以拼出任意实体标识伪造引用——因此
//! 这里按该设计文档已经给出的形状把它落地，不是重新设计）。
//!
//! # 防伪造的三层论证（设计文档 3.3 节，原样适用）
//!
//! 1. **语法层面拿不到**：`SteelVal::Custom`（[`steel::rvals::Custom`]
//!    trait 的 blanket 实现落点）没有 Scheme 字面语法能直接构造，脚本写不出
//!    "我要一个 `ScriptEntityHandle`" 这样的表达式，只能等宿主给。
//! 2. **字段层面读不到**：字段私有，脚本即使拿到一个 `SteelVal::Custom`
//!    也无法读出内部的 `index`/`generation`。
//! 3. **类型层面伪造不了**：`FromSteelVal` 按 `downcast_ref::<ScriptEntityHandle>()`
//!    的 `TypeId` 精确匹配，类型不符返回 `ConversionError`（走
//!    `Result`，不会 panic），不会被其他 `Custom` 类型张冠李戴。

use ll_world::entity::EntityId;

/// 脚本持有的不透明厚层实体句柄。
///
/// 字段私有——脚本没有任何路径读到内部的 `EntityId`，只能把句柄整个
/// 传回宿主注册的函数。`Copy`：`EntityId` 本身是 8 字节（两个
/// `u32`），包一层不增加任何有意义的开销。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptEntityHandle(EntityId);

impl ScriptEntityHandle {
    /// 仅供本 crate 内部构造——句柄只应该由「宿主已经从 `Arena` 里查到
    /// 的一个真实 `EntityId`」换取，不暴露给外部任意构造。
    pub(crate) fn new(id: EntityId) -> Self {
        ScriptEntityHandle(id)
    }

    /// 取出内部的 `EntityId`，供宿主侧查询/写入函数使用。
    ///
    /// # 为什么是 `pub` 而不是 `pub(crate)`（盗贼被动两分批次改）
    ///
    /// 落地第一天写成 `pub(crate)` 只是因为当时全部宿主侧消费者都住在
    /// 本 crate（`crate::api::actor` 的 `direction-toward`/
    /// `actor-stealthed?`）。`ll_mod::script_behavior_api` 的
    /// `actor-inspection-suspicion` 是第一个**住在下游 crate** 的
    /// 消费者——它必须住在那里，因为它要读 `ll_mod::race::RaceTable`
    /// 这类表（见该模块文档「为什么这一个函数单独落在 `ll-mod`」
    /// 一节），而 `ll-script` 不允许反过来依赖 `ll-mod`（规格 §5）。
    ///
    /// **这不削弱脚本沙箱**：真正的隔离来自
    /// [`ScriptEntityHandle::new`] 仍然是 `pub(crate)`——脚本没有任何
    /// 路径伪造一个句柄，也读不到内部的 `EntityId`（字段私有、
    /// `display` 不输出数值，见类型文档）。本方法只对**宿主侧的
    /// Rust 代码**开放，而宿主侧代码本来就持有整个 `WorldState`
    /// 的引用，能不能把句柄换回 `EntityId` 不改变它的能力边界。
    pub fn entity_id(&self) -> EntityId {
        self.0
    }
}

// 空实现即可获得 `CustomType`/`IntoSteelVal`/`FromSteelVal`（steel-core
// 的 blanket 实现，见设计文档二节「核实到的机制」）。不覆盖 `display`：
// 默认输出 `#<ScriptEntityHandle>`，不含 index/generation 数值，避免
// 脚本靠打印句柄的字符串表示旁敲侧击猜测编码。
impl steel::rvals::Custom for ScriptEntityHandle {}

#[cfg(test)]
mod tests {
    use steel::rvals::{FromSteelVal, IntoSteelVal};

    use super::*;
    use ll_world::entity::Arena;

    fn some_entity_id() -> EntityId {
        let mut arena: Arena<()> = Arena::new();
        arena.spawn(())
    }

    #[test]
    fn 句柄可以经过steelval往返取回同一个实体标识() {
        // Arrange
        let id = some_entity_id();
        let handle = ScriptEntityHandle::new(id);

        // Act
        let steelval = handle.into_steelval().expect("Custom 类型转换恒成功");
        let restored =
            ScriptEntityHandle::from_steelval(&steelval).expect("同一类型的 downcast 恒成功");

        // Assert
        assert_eq!(restored.entity_id(), id);
    }

    #[test]
    fn 非custom的steelval无法转换成句柄() {
        // Arrange：一个普通整数——不是 SteelVal::Custom，不该被误判成
        // 句柄。
        let value = steel::rvals::SteelVal::IntV(42);

        // Act
        let result = ScriptEntityHandle::from_steelval(&value);

        // Assert
        assert!(result.is_err());
    }
}
