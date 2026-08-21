//! 物品注册表——落地 `knowledge/design/item-system.md` 一节「定义与
//! 实例分离」的静态一半：[`ItemDef`] 是本体与 mod 注册物品时共用的
//! 输入形状，运行时实例（数量、耐久……）是
//! [`ll_sim::item::ItemStack`]（`ll-sim` 不能依赖 `ll-mod`，依赖方向
//! 见 `crate::trait_def` 模块文档同一条约束，因此两个类型分处两个
//! crate，不是同一个模块里的两个字段）。
//!
//! # 照抄 `race.rs`/`trait_def.rs`/`resource_pool.rs` 已验证的模式
//!
//! 私有字段 + `ItemTable::define` 注册期完整校验（ADR 0017）+
//! `ItemView`/`ItemAttrs` 一读一写两个薄视图——与
//! [`crate::resource_pool`] 同一套列式存储手法，本模块不是第一次证明
//! 这套模式好用，是又一次复用。
//!
//! # 本批次范围：只定形注册与堆叠所需的字段，装备/耐久机制/使用效果
//! 三类字段本批次不放进来
//!
//! `item-system.md` 一节给出的完整 `ItemDef` 左列还有三类本批次故意
//! 不声明的字段：
//!
//! - `equip_mask: SlotMask`——`SlotMask` 类型本身尚未落地（见
//!   `knowledge/design/equipment-slots.md`「落地状态：纯设计」），且
//!   该文档连 `EquipSlot`（单槽位类型，与 `SlotMask` 多槽位集合是两个
//!   不同类型）都还没有正式定义（见其「左右分离的设计价值」一节前的
//!   冲突记录）——本批次若为了" `ItemDef` 形状定形"而抢先造一个
//!   `SlotMask`，等于在装备批次（第三批）真正设计 `EquipSlot`/22 槽位
//!   表之前就把类型定死，一旦两者对不上就是本批次亲手挖的返工坑，
//!   与项目任务书「一次只打通一条完整链路」的裁定直接冲突。
//! - `stat_bonuses: Vec<StatBonus>`——`StatBonus` 类型同样尚未落地
//!   （`attribute-system.md`「衍生属性绝不进存档」一节只给出了
//!   `derive_stats` 的函数签名，没有定义 `StatBonus` 具体长什么样），
//!   同一条理由排除。
//! - `use_effect: Option<ContentIndex>`——类型上不需要发明新东西
//!   （`ContentIndex` 早已存在），但这个字段没有任何意义：它指向的
//!   Steel 脚本要在 `Intent::Use` 结算时才会被读取
//!   （`item-system.md` 八节），而 `Intent::Use` 本身是耐久系统批次
//!   （第五批）才会新增的意图变体——现在声明这个字段只是给内容作者
//!   一个填了也没有任何效果的选项，不填一样，不是"形状先定好、消费者
//!   以后接"（`stat_modifiers`/`rule_modifiers` 那种情形），是纯粹的
//!   死字段，YAGNI。
//!
//! `max_durability: Option<i32>` **保留**——不需要发明新类型
//! （`Option<i32>` 已经是本代码库到处在用的形状），且直接支撑本批次
//! 「同一个 `def` 的两个 `ItemStack` 各自携带独立耐久」这条区分验收
//! （见 `ll_sim::item` 模块测试）：一件物品"有没有耐久上限"是它的类型
//! 属性（剑有，材料没有），这条判断本批次就该能表达，不必等到耐久
//! 扣减机制（第五批）才补——与 `TraitDef.rule_modifiers` 先定形、后接
//! 消费者是同一条先例，区别只是 `max_durability` 已经在本批次就有一
//! 个真实读者（堆叠比较逻辑需要知道"这件物品是否该有耐久"才能决定
//! 初始 `ItemStack` 该不该带 `Some`），不是纯粹的占位声明。
//! **不通过 `register-item` 暴露给脚本**——本批次的两个示例物品
//! （箭矢/铁剑）用 `stack_limit` 就能完整表达"能不能堆叠"这条区别,
//! 没有必要在两个示例都用不上的情况下现在就为 `max_durability` 发明
//! 脚本编码约定,真正需要的批次（耐久系统落地批次）再照
//! `register-trait-resource-pool` 相对 `register-trait`「新增能力用
//! 新函数」的先例补一个 `register-item-durability`。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::scaled::Milli;
use ll_sim::item::{ItemCatalog, ItemRule};

/// 单条物品声明：本体与 mod 注册物品时共用的同一个输入形状——
/// 「本体即 Mod」在物品层面的验收标的，理由同 [`crate::race::RaceDef`]
/// 文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDef {
    /// 命名空间标识符，例如 `lostland:iron_sword`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——与 `TraitDef`/`RaceDef`
    /// 同一条既有惯例。
    pub display_name_key: NamespacedId,
    /// 堆叠上限——`ll_sim::item::merge_stacks` 的 `stack_limit`
    /// 参数就是这个字段，恒 ≥ 1（`register-item` 拒绝 0，见其文档）。
    /// `1` 表示不可堆叠（武器、装备……），`merge_stacks` 会把这类物品
    /// 的合并结果算成"两堆数量原样不变"，不需要在这里另开一个布尔
    /// 字段表达"能不能堆叠"，见该函数文档「为什么不用三条特判分支」
    /// 一节。
    pub stack_limit: u32,
    /// 基础重量，`Milli` 千分之一为单位——[负重系统](../../../knowledge/design/item-system.md)
    /// 七节的输入，本批次不接线（背包/负重是后续批次的工作），这里
    /// 只落地形状。
    pub base_weight: Milli,
    /// 基础价格，`Milli` 千分之一为单位——[经济系统](../../../knowledge/design/agent-goals-and-economy.md)
    /// 的输入之一（换算关系未定，见 `item-system.md` 五节「总索引
    /// 冲突清单」），本批次不接线。
    pub base_price: Milli,
    /// 耐久上限——`None` 表示这件物品没有耐久概念（材料、消耗品），
    /// `Some` 表示有（武器、装备）。扣减耐久的具体规则是耐久系统落地
    /// 批次（第五批）的工作，见模块文档「`max_durability` 保留」
    /// 一节。
    pub max_durability: Option<i32>,
}

/// [`ItemTable::define`] 实际存进列式存储的属性子集——不含 `id`，
/// 理由同 [`crate::race::RaceAttrs`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 堆叠上限。
    pub stack_limit: u32,
    /// 基础重量。
    pub base_weight: Milli,
    /// 基础价格。
    pub base_price: Milli,
    /// 耐久上限。
    pub max_durability: Option<i32>,
}

/// 物品注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemError::DuplicateDefinition(index) => {
                write!(f, "物品索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for ItemError {}

/// 一次物品查询命中的完整结果，理由同 [`crate::race::RaceView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 堆叠上限。
    pub stack_limit: u32,
    /// 基础重量。
    pub base_weight: Milli,
    /// 基础价格。
    pub base_price: Milli,
    /// 耐久上限。
    pub max_durability: Option<i32>,
}

/// 物品属性的列式存储：按 [`ContentIndex`] 下标索引，与
/// [`crate::resource_pool::ResourcePoolTable`] 同一套道理——下标空间
/// 是全局 `ContentIndex` 号段的一部分，因此同样维护一份 `defined`
/// 位图。
#[derive(Debug, Default, Clone)]
pub struct ItemTable {
    display_name_key: Vec<Option<NamespacedId>>,
    stack_limit: Vec<u32>,
    base_weight: Vec<Milli>,
    base_price: Vec<Milli>,
    max_durability: Vec<Option<i32>>,
    defined: Vec<bool>,
}

impl ItemTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上物品属性。
    pub fn define(&mut self, index: ContentIndex, attrs: ItemAttrs) -> Result<(), ItemError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.stack_limit.resize(new_len, 0);
            self.base_weight.resize(new_len, Milli::ZERO);
            self.base_price.resize(new_len, Milli::ZERO);
            self.max_durability.resize(new_len, None);
        }

        if self.defined[idx] {
            return Err(ItemError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.stack_limit[idx] = attrs.stack_limit;
        self.base_weight[idx] = attrs.base_weight;
        self.base_price[idx] = attrs.base_price;
        self.max_durability[idx] = attrs.max_durability;
        Ok(())
    }

    /// 给定的物品索引当前是否已经登记过属性。
    pub fn is_defined(&self, item: ContentIndex) -> bool {
        self.defined
            .get(item.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个物品的完整属性，未注册的索引返回 `None`（ADR 0015）。
    pub fn get(&self, item: ContentIndex) -> Option<ItemView<'_>> {
        if !self.is_defined(item) {
            return None;
        }
        let idx = item.get() as usize;
        Some(ItemView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            stack_limit: self.stack_limit[idx],
            base_weight: self.base_weight[idx],
            base_price: self.base_price[idx],
            max_durability: self.max_durability[idx],
        })
    }
}

/// `resolve` 侧的堆叠上限查询——`ll_sim::resolve::resolve_pick_up` 判断
/// 「拾取时能否与背包已有堆合并」需要它，见
/// `ll_sim::item::ItemCatalog` 文档「本模块新增」一节。与
/// `impl ResourcePoolCatalog for ResourcePoolTable`
/// （`crate::resource_pool` 模块）同一条既有先例：只把 `ItemView` 里
/// `resolve` 真正要读的那一个字段（`stack_limit`）搬进
/// [`ItemRule`]，不是把整条 `ItemView` 转发出去。
impl ItemCatalog for ItemTable {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        self.get(item).map(|view| ItemRule {
            stack_limit: view.stack_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 新建的物品表查询任意索引均为未注册() {
        // Arrange
        let table = ItemTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 注册后查询能拿到完整的堆叠上限与价格() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:arrow").unwrap());
        let mut table = ItemTable::new();

        // Act
        table
            .define(
                index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("lostland:item.arrow").unwrap(),
                    stack_limit: 99,
                    base_weight: Milli::from_whole(0),
                    base_price: Milli::from_whole(2),
                    max_durability: None,
                },
            )
            .expect("首次定义应当成功");

        // Assert
        let view = table.get(index).expect("已注册");
        assert_eq!(view.stack_limit, 99);
        assert_eq!(view.base_price, Milli::from_whole(2));
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:iron_sword").unwrap());
        let mut table = ItemTable::new();
        let attrs = || ItemAttrs {
            display_name_key: NamespacedId::parse("lostland:item.iron_sword").unwrap(),
            stack_limit: 1,
            base_weight: Milli::from_whole(3),
            base_price: Milli::from_whole(50),
            max_durability: Some(100),
        };
        table.define(index, attrs()).expect("首次定义应当成功");

        // Act
        let result = table.define(index, attrs());

        // Assert
        assert_eq!(result, Err(ItemError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = ItemTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 不可堆叠物品的堆叠上限为一() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:iron_sword").unwrap());
        let mut table = ItemTable::new();

        // Act
        table
            .define(
                index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("lostland:item.iron_sword").unwrap(),
                    stack_limit: 1,
                    base_weight: Milli::from_whole(3),
                    base_price: Milli::from_whole(50),
                    max_durability: Some(100),
                },
            )
            .expect("首次定义应当成功");

        // Assert
        assert_eq!(table.get(index).unwrap().stack_limit, 1);
        assert_eq!(table.get(index).unwrap().max_durability, Some(100));
    }

    #[test]
    fn itemcatalog实现对已注册物品返回真实堆叠上限() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:arrow").unwrap());
        let mut table = ItemTable::new();
        table
            .define(
                index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("lostland:item.arrow").unwrap(),
                    stack_limit: 99,
                    base_weight: Milli::from_whole(0),
                    base_price: Milli::from_whole(2),
                    max_durability: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let rule = ItemCatalog::item(&table, index);

        // Assert
        assert_eq!(rule, Some(ItemRule { stack_limit: 99 }));
    }

    #[test]
    fn itemcatalog实现对未注册物品返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = ItemTable::new();

        // Act
        let rule = ItemCatalog::item(&table, never_defined);

        // Assert
        assert_eq!(rule, None);
    }
}
