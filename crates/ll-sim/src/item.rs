//! 物品结算侧接口——运行时实例本身（[`ItemStack`]/[`GroundItemStack`]/
//! [`can_merge`]/[`merge_stacks`]/[`split_stack`]）已挪到
//! `ll_world::item`（P6 第二批），本模块现在 `pub use` 它们，不再维护
//! 一份会漂移的副本——与 `ll_mod::trait_def` 现在 `pub use`
//! `ll_sim::resource_pool::{CapacityFormula, CapacityValue,
//! ResourcePoolGrant}` 是同一条先例（见其文档），只是这次挪动方向
//! 相反：从更上游的 `ll-sim` 挪到更下游的 `ll-world`，因为
//! [`ll_world::entity::Agent::inventory`]/
//! [`ll_world::state::WorldState::ground_items`] 都定义在 `ll-world`，
//! `ll-world` 不能反过来依赖 `ll-sim`（依赖方向，规格 §5）。
//!
//! 挪动的完整理由见 [`ll_world::item`] 模块文档「为什么从 `ll-sim`
//! 挪到本模块」一节；`Owner` 为什么本批次仍然不落地见该模块文档
//! 「`Owner` 本批次仍然不落地」一节——两节论证不在本文件重复。
//!
//! # 本模块新增：[`ItemCatalog`]（P6 第二批，resolve 侧依赖倒置）
//!
//! `crate::resolve::resolve_pick_up` 拾取时若背包已有同种可堆叠物品，
//! 需要知道这个 `def` 的堆叠上限（`ItemDef.stack_limit`）才能调用
//! [`merge_stacks`]——真正的 `ItemDef`/`ItemTable` 定义在下游的
//! `ll-mod::item`，`ll-sim` 不能反过来依赖它（依赖方向）。与
//! `crate::skill::SkillCatalog`/`crate::resource_pool::ResourcePoolCatalog`
//! 同一套依赖倒置手法：本模块只声明「给我一个物品索引，还我它的堆叠
//! 上限」这个最小接口，真正的实现（`ll_mod::item::ItemTable`）在
//! `ll-mod` 侧补上 `impl ItemCatalog for ItemTable`。
//!
//! 只收敛 `stack_limit` 一个字段——`resolve_pick_up`/`resolve_drop`
//! 不需要 `base_weight`/`base_price`/`max_durability` 中的任何一个（
//! 负重与耐久扣减都是后续批次的工作，见 `ll_world::item` 模块文档
//! 「`Owner` 本批次仍然不落地」一节同一条 YAGNI 判断），与
//! `crate::skill::SkillRule` 只收敛 `resolve_use_skill` 真正要读的
//! 字段是同一个理由。

use ll_core::ident::ContentIndex;

pub use ll_world::item::{
    EquipSlot, GroundItemStack, ItemStack, ItemStackError, SlotMask, can_merge, merge_stacks,
    split_stack,
};

/// `resolve` 侧需要的一条物品定义的最小只读视图——堆叠上限与装备占位
/// 掩码，见模块文档「本模块新增」一节。
///
/// # `equip_mask` 为什么现在也收进来了（装备栏位批次，P6 第三批）
///
/// `resolve_equip`/`resolve_unequip`（`crate::resolve`）需要知道一件
/// 物品占用哪些槽位才能判断占位冲突（`knowledge/design/equipment-slots.md`
/// 「一条规则覆盖所有特例」一节）——与 `stack_limit` 当初被收进来的
/// 理由完全一样：真正的 `ItemDef` 在下游的 `ll-mod`，本 crate 只收敛
/// `resolve` 真正要读的字段，不整条转发 `ItemView`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRule {
    /// 堆叠上限，即 [`merge_stacks`] 的 `stack_limit` 参数。
    pub stack_limit: u32,
    /// 装备占位掩码——`SlotMask::EMPTY` 表示这件物品不可装备。
    pub equip_mask: SlotMask,
}

/// `resolve` 依赖的最小「物品定义来源」接口——与
/// [`crate::skill::SkillCatalog`]/[`crate::resource_pool::ResourcePoolCatalog`]
/// 同一套依赖倒置手法：真正的 `ItemDef`/`ItemTable` 定义在下游的
/// `ll-mod`，本 crate 只声明「给我一个物品索引，还我它的堆叠上限」
/// 这个接口。
pub trait ItemCatalog {
    /// 查询一条物品定义；未注册的索引返回 `None`（ADR 0015）。
    fn item(&self, item: ContentIndex) -> Option<ItemRule>;
}

/// 空物品目录：查询任何索引恒返回 `None`——理由同 [`crate::skill::NoSkills`]。
///
/// `crate::resolve::resolve_pick_up` 查不到堆叠上限时按「不限量」
/// 处理（`u32::MAX`），不是拒绝拾取——见其文档：这与
/// `crate::resource_pool::effective_scalar_capacity` 「查不到就按零
/// 处理」的既有纪律方向相反，但理由对称：查不到目录本身就意味着调用
/// 方没有提供真实的物品注册表（多数只测试移动/开门这类不涉及内容
/// 注册表的既有测试场景），不应该让"没传目录"这件事表现成"这件物品
/// 堆叠上限异常地低"。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoItems;

impl ItemCatalog for NoItems {
    fn item(&self, _item: ContentIndex) -> Option<ItemRule> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    #[test]
    fn 空物品目录查询任意索引恒返回none() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:arrow").unwrap());

        // Act & Assert
        assert_eq!(NoItems.item(index), None);
    }
}
