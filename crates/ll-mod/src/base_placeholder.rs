//! 本体占位内容注册——补上批次 C 记录的既知债务：本项目此前从未注册
//! 过任何「占位/未知」内容，`ll_content::degrade::DegradeAction::FallbackToPlaceholder`
//! 因此在生产读档管线里永远拿不到一个真实索引（`ll-content::save_file::load_full`
//! 只能把 `placeholder` 参数硬编码成 `None`，NPC 种族缺失的占位降级
//! 分支在完整读档管线里不可达）。本模块补上这一半：把本体的占位内容
//! 注册进 [`Registry`]。
//!
//! # 为什么走与 [`crate::base_terrain`] 完全相同的模式
//!
//! ADR 0015/0016「本体即 Mod」检验点在这里同样成立——占位内容与未来
//! mod 可能注册的自定义内容走**完全相同**的 [`Registry::intern`] 调用，
//! 没有任何本体专属的特权通道。占位内容没有 [`crate::base_terrain`]
//! 那样的属性表需要物化（种族/职业当前就是裸的 `ContentIndex`，见
//! `ll_world::entity::Agent::race` 文档「种族是内容,走注册表索引,不是
//! 封闭本体枚举」），因此不需要 `materialize_*` 那层回调间接——直接
//! [`Registry::intern`] 一个固定标识符即可。
//!
//! # 为什么只注册一个共享占位值，不分别注册「未知种族」与「未知职业」
//!
//! [`ll_world::entity::ThinPopulation::try_remap_content_indices`]
//! 与 `ll_content::remap::Remapper`（读档重映射的实现）已经把「种族」
//! 与「职业」两列按同一条规则处理——`ThinPopulation` 模块文档原话：
//! 「两列走同一条规则……因此一个闭包足够，不必对两列各暴露一个方法」。
//! 这是既有架构（P5 批次 A/C）的刻意选择，不是本次任务引入的简化。
//! 跟着这条既定设计走：只注册一个共享占位值，不为「职业」单独开一条
//! 路——那需要先改 `Remapper`/`ThinPopulation` 的接口形状（把单个
//! `Option<ContentIndex>` 拆成按属性区分的两个），超出本次「让占位
//! 降级分支在生产管线里可达」的范围，如实记录在此，不是被忽略的缺口。

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::registry::Registry;

/// 本体占位内容的固定标识符。
///
/// 命名沿用 P5 验收 demo（`crates/ll-content/examples/p5_save_acceptance.rs`
/// 早期版本）摸索出的约定——demo 当时只能在测试代码里临时 `intern`
/// 这个字符串来绕过 `load_full` 的硬编码 `None`，本模块把它转正为一条
/// 真实的本体注册。
pub const PLACEHOLDER_RACE_ID: &str = "lostland:placeholder_race";

/// 把本体占位内容注册进 `registry`，返回它拿到的 [`ContentIndex`]。
///
/// **这是本体占位内容唯一的生产注册入口**：内部只是一次
/// [`Registry::intern`] 调用——本体占位内容因此与未来 mod 注册的自定义
/// 内容走同一条通道，`Registry` 内部完全无法区分某次 `intern` 调用来自
/// 占位内容还是任何其他内容。
///
/// 调用方应在启动时、紧跟在其余本体内容（地形/空间层属性）注册之后
/// 调用一次；返回的索引此后按值使用,不会把注册表查询带进任何热路径。
pub fn register_base_placeholder_content(registry: &mut Registry) -> ContentIndex {
    let id = NamespacedId::parse(PLACEHOLDER_RACE_ID).expect("固定字面量标识符恒合法");
    registry.intern(id)
}

/// 查询 `registry` 是否已经注册过本体占位内容，返回其索引。
///
/// 与 [`register_base_placeholder_content`] 的区别：本函数只**解析**
/// （[`Registry::get`]，不创建新记录），供只持有 `&Registry`（不是
/// `&mut Registry`）的调用方使用——最典型的例子是
/// `ll-content::save_file::load_full`：读档这一刻不应该、也没有能力
/// 反过来往当前会话的注册表里塞新内容（那是启动时装载阶段的职责）。
///
/// 若调用方传入的 `registry` 从未调用过 [`register_base_placeholder_content`]
/// （例如测试特意构造的最小注册表），返回 `None`——这不是错误，见
/// `ll_content::degrade` 模块文档「`ContentIndex` 缺占位值的既知债务」：
/// 拿不到占位索引时,降级决策会诚实退化为拒绝,不会伪造一个索引。
pub fn base_placeholder_index(registry: &Registry) -> Option<ContentIndex> {
    let id = NamespacedId::parse(PLACEHOLDER_RACE_ID).expect("固定字面量标识符恒合法");
    registry.get(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 本体占位内容与mod内容共用registry同一段连续递增的索引号段() {
        // 本测试只证明本体占位内容与 mod 内容走同一条 Registry::intern
        // 调用路径、共用同一个单调递增的号段——不为占位内容预留任何
        // 特殊区间。这是「结构等价」，不是「mod 脚本调得到这套 API」的
        // 证据；后者的证据在 crate::pipeline 的脚本装载测试与
        // mods/example_mod/ 的内容文件。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let placeholder_index = register_base_placeholder_content(&mut registry);
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:crystal").expect("合法标识符"));

        // Assert：mod 内容紧接在占位内容之后分配到索引,说明两者共用
        // 同一个单调递增的号段,没有为占位内容预留任何特殊区间。
        assert_eq!(mod_index.get(), placeholder_index.get() + 1);
    }

    #[test]
    fn 已注册的占位内容可以通过base_placeholder_index查到() {
        // Arrange
        let mut registry = Registry::new();
        let registered = register_base_placeholder_content(&mut registry);

        // Act
        let looked_up = base_placeholder_index(&registry);

        // Assert
        assert_eq!(looked_up, Some(registered));
    }

    #[test]
    fn 未注册占位内容的registry查询返回none而非伪造索引() {
        // 呼应 degrade 模块文档「ContentIndex 缺占位值的既知债务」：
        // 拿不到占位索引时必须诚实返回 None,不能伪造一个可能指向错误
        // 内容的索引。
        // Arrange
        let registry = Registry::new();

        // Act
        let looked_up = base_placeholder_index(&registry);

        // Assert
        assert_eq!(looked_up, None);
    }

    #[test]
    fn 占位内容重复注册返回相同索引() {
        // Registry::intern 本身的幂等性（见其文档）在占位内容这里同样
        // 成立——多次调用 register_base_placeholder_content 不应该产生
        // 两条不同的记录。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let first = register_base_placeholder_content(&mut registry);
        let second = register_base_placeholder_content(&mut registry);

        // Assert
        assert_eq!(first, second);
    }
}
