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
    EquipSlot, GroundItemStack, ItemStack, ItemStackError, SlotMask, StatBonus, StatTarget,
    can_merge, merge_stacks, split_stack,
};

use crate::combat::Penetration;
use crate::skill::SkillEffect;

/// `resolve` 侧需要的一条物品定义的最小只读视图——堆叠上限、装备占位
/// 掩码与静态属性加成，见模块文档「本模块新增」一节。
///
/// # `equip_mask` 为什么现在也收进来了（装备栏位批次，P6 第三批）
///
/// `resolve_equip`/`resolve_unequip`（`crate::resolve`）需要知道一件
/// 物品占用哪些槽位才能判断占位冲突（`knowledge/design/equipment-slots.md`
/// 「一条规则覆盖所有特例」一节）——与 `stack_limit` 当初被收进来的
/// 理由完全一样：真正的 `ItemDef` 在下游的 `ll-mod`，本 crate 只收敛
/// `resolve` 真正要读的字段，不整条转发 `ItemView`。
///
/// # `stat_bonuses` 为什么现在也收进来了（P6 第四批：`derive_stats`
/// 与装备属性接进战斗）
///
/// `crate::resolve::derive_stats` 需要逐件已装备物品累加它的
/// `stat_bonuses` 才能算出装备贡献的攻防加成——同一条「resolve 真正要
/// 读的字段才收进 `ItemRule`」的理由。
///
/// # 为什么不再是 `Copy`
///
/// `stat_bonuses: Vec<StatBonus>` 不满足 `Copy`（`Vec` 需要堆分配），
/// 本类型因此从 `Copy` 降级为只 `Clone`——`stack_limit`/`equip_mask`
/// 两个既有字段本身仍是 `Copy`，但整体类型的 `Copy` 能力由最"重"的
/// 那个字段决定,加一个 `Vec` 字段后整体必须跟着降级,不是可以只给
/// 新字段单独开小灶的选择。全部既有调用点（`items.item(def)` 返回
/// `Option<ItemRule>` 后直接 `.map`/`if let` 解构使用,或在测试夹具的
/// `BTreeMap<ContentIndex, ItemRule>` 里从 `.copied()` 改为 `.cloned()`）
/// 已经同步改过,不存在遗留的 `Copy` 依赖。
///
/// # `penetration` 为什么现在也收进来了（武器引用与穿透接线批次，P6 第
/// 六批）
///
/// `crate::resolve::resolve_attack` 需要知道攻击者主手武器的穿透值才能
/// 传给 [`crate::combat::damage_after_defense`]——此前（P6 第四批）
/// `StatBonus`/`ItemRule` 都不携带穿透字段，`resolve_attack` 因此只能
/// 恒传 [`Penetration::NONE`]。与 `stat_bonuses` 不同，穿透不是"目标 +
/// 增量"列表形状——`Penetration` 本身已经是"固定值 + 千分比"两个分量
/// 的完整类型（`combat.rs`），一件武器只有一份穿透（不像 `stat_bonuses`
/// 那样一件装备可以同时加力量与护甲两条），因此这里是单个 `Penetration`
/// 字段，不是 `Vec<Penetration>`。
///
/// # `use_effect` 为什么复用 `SkillEffect`，不是一个新的 `ItemEffect`
/// 类型（耐久与 `Intent::Use` 落地批次，P6 第五批）
///
/// 喝一瓶药水，效果无非「造成伤害/恢复资源/临时属性修正」——这恰好是
/// [`SkillEffect`] 已经能表达的全部三种效果。技能与物品的**触发条件**
/// 不同（技能有冷却/资源消耗/可学条件，物品有数量/耐久），但**效果
/// 本身**的算法完全相同：`crate::resolve::resolve_use_item` 对
/// `SkillEffect` 三个变体的 `match` 与 `resolve_use_skill` 逐字对应
/// （`DealDamage` → `Effect::Damage`+可能的 `Effect::Kill`，
/// `RestoreResource` → `Effect::AdjustResource`，
/// `TemporaryStatModifier` → `Effect::ApplyStatModifier`）。ADR 0021：
/// 只有算法真正可共享才抽象——这里不是"表面相似的两件事"，是同一个
/// 算法被两种不同的触发路径复用，另造一个字段完全相同、只是改了个
/// 名字的 `ItemEffect` 才是真正的重复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRule {
    /// 堆叠上限，即 [`merge_stacks`] 的 `stack_limit` 参数。
    pub stack_limit: u32,
    /// 装备占位掩码——`SlotMask::EMPTY` 表示这件物品不可装备。
    pub equip_mask: SlotMask,
    /// 静态属性加成列表——`crate::resolve::derive_stats` 汇总"装备"这
    /// 一个输入的数据来源，空列表表示这件物品不提供任何加成（多数消耗
    /// 品/材料的既有情形）。
    pub stat_bonuses: Vec<StatBonus>,
    /// 使用效果——`None` 表示这件物品不能被 `Intent::Use`（材料、装备
    /// 本身……），`Some` 时 `crate::resolve::resolve_use_item` 用它产出
    /// 对应的 `Effect`，见本类型文档「`use_effect` 为什么复用
    /// `SkillEffect`」一节。
    pub use_effect: Option<SkillEffect>,
    /// 穿透——`crate::resolve::resolve_attack` 用攻击者主手武器的这个
    /// 值传给 `damage_after_defense`，见本类型文档「`penetration` 为
    /// 什么现在也收进来了」一节。`Penetration::NONE`（多数物品的既有
    /// 默认值）表示这件物品不提供任何穿透。
    pub penetration: Penetration,
    /// 这件物品显式声明的伤害公式（伤害公式引擎批次新增）——
    /// `crate::resolve::resolve_attack` 用它作为
    /// `crate::formula::DamageFormulaCatalog::formula_for` 的
    /// `explicit` 参数；`None` 表示这件物品没有显式声明，退回全局
    /// 默认公式（两层下探的第二层，见 `crate::formula` 模块文档
    /// 「公式只算『攻击力』」一节与
    /// `knowledge/design/damage-formula-mod-api.md` 十九节——本批次
    /// 没有武器类别/伤害类别，四层下探退化成两层）。
    pub damage_formula: Option<ContentIndex>,
    /// 这件物品显式声明的伤害类别（伤害类别/抗性接线批次新增）——
    /// `None` 表示这件物品不指定伤害类别，`resolve_attack` 退回
    /// [`crate::damage_category::DamageCategoryCatalog::default_category`]，
    /// 见其文档「为什么只有『默认类别』这一个方法」一节。伤害类别与
    /// 伤害公式是两条独立的轴（`damage-formula-mod-api.md` 十七节
    /// 「与既有 `DamageSchool` 的关系：正交，不合并」——伤害类别本身
    /// 也与武器类别正交），因此是与 [`Self::damage_formula`] 并列的
    /// 独立字段，不是复用同一个 `ContentIndex`。
    pub damage_category: Option<ContentIndex>,
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
