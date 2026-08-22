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
//! - ~~`equip_mask: SlotMask`~~——**P6 第三批（装备栏位）已补上**，见
//!   [`ItemDef::equip_mask`] 文档。原排除理由：`SlotMask`/`EquipSlot`
//!   当时都还没有正式定义，抢先造型会在装备批次真正设计出这两个类型
//!   之前把形状定死——批次到了，这条排除的前提本身已经不成立。
//! - ~~`stat_bonuses: Vec<StatBonus>`~~——**P6 第四批（`derive_stats`
//!   与装备属性接进战斗）已补上**，见 [`ItemDef::stat_bonuses`] 文档。
//!   原排除理由：`StatBonus` 依赖的「属性系统 `derive_stats`」尚未
//!   落地的设计——批次到了，`derive_stats` 与 `StatBonus` 在同一批次
//!   一并定形，这条排除的前提同样不成立了（与上面 `equip_mask` 那条
//!   「批次到了就该做」是同一条判断）。
//! - ~~`use_effect: Option<ContentIndex>`~~——**P6 第五批（耐久与
//!   `Intent::Use`）已补上，但形状与设计文档原文不同：`Option<SkillEffect>`，
//!   不是指向 Steel 脚本的 `ContentIndex`**，见
//!   [`ItemDef::use_effect`] 文档「与设计文档原文的偏离」一节。原排除
//!   理由：它指向的脚本要在 `Intent::Use` 结算时才会被读取，而
//!   `Intent::Use` 本身是耐久系统批次（第五批）才会新增的意图变体——
//!   批次到了，这条排除的前提同样不成立了，与上面 `equip_mask`/
//!   `stat_bonuses` 是同一条「批次到了就该做」判断。
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
use ll_sim::combat::Penetration;
use ll_sim::item::{ItemCatalog, ItemRule, SlotMask, StatBonus};
use ll_sim::skill::SkillEffect;

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
    /// 装备占位掩码（装备栏位批次，P6 第三批）——`SlotMask::EMPTY`
    /// （默认值）表示这件物品不可装备，落地
    /// `knowledge/design/equipment-slots.md`。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `set_equip_mask` 追加
    ///
    /// `register-item` 的脚本签名不能改参数个数（会破坏仓库里已有的
    /// 真实 mod 脚本，见 `crate::script_item_api` 模块文档），与
    /// `ItemDef.max_durability` 当初落地时定下的纪律不同：`max_durability`
    /// 恰好是 `register-item` 原有六个参数之一，本字段是全新追加的，
    /// 只能走 `register-race-xp-reward`/`register-trait-resource-pool`
    /// 那条「新增能力用新函数」的既有先例——脚本层对应函数是
    /// `register-item-equip-mask`（`crate::script_item_api`），Rust 层
    /// 对应方法是 [`ItemTable::set_equip_mask`]。
    pub equip_mask: SlotMask,
    /// 静态属性加成列表（P6 第四批：`derive_stats` 与装备属性接进
    /// 战斗）——落地 `knowledge/design/attribute-system.md` 七节
    /// `derive_stats(基础属性, 装备, 状态效果, 负重)` 签名里"装备"这一个
    /// 输入,空列表（默认值）表示这件物品不提供任何属性加成。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `add_stat_bonus` 追加
    ///
    /// 与 [`Self::equip_mask`] 同一条既有先例（`register-item` 的六参数
    /// 签名不能改参数个数）——脚本层对应函数是
    /// `register-item-stat-bonus`（`crate::script_item_api`），Rust 层
    /// 对应方法是 [`ItemTable::add_stat_bonus`]。**追加,不是覆盖**：与
    /// `equip_mask`（单值,覆盖）不同,`stat_bonuses` 是一个可以携带任意
    /// 多条加成的列表（一件装备可以同时加力量与护甲），语义上更接近
    /// `RaceTable::add_trait_grant`——多次调用累积,不是以最后一次为准。
    pub stat_bonuses: Vec<StatBonus>,
    /// 使用效果（P6 第五批：耐久与 `Intent::Use`）——`None`（默认值）
    /// 表示这件物品不能被 `Intent::Use` 使用（材料、装备本身……）。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `set_use_effect` 追加
    ///
    /// 与 [`Self::equip_mask`]/[`Self::stat_bonuses`] 同一条既有先例
    /// （`register-item` 的六参数签名不能改参数个数）——脚本层对应函数
    /// 是 `register-item-use-effect`（`crate::script_item_api`），Rust
    /// 层对应方法是 [`ItemTable::set_use_effect`]。**覆盖，不是追加**：
    /// 一件物品的使用效果是单个值,与 [`Self::equip_mask`] 同一种"单值
    /// 覆盖"语义，不像 `stat_bonuses` 那样天然是可以累积的列表。
    ///
    /// # 与设计文档原文的偏离：类型是 `Option<SkillEffect>`，不是
    /// `Option<ContentIndex>`
    ///
    /// `item-system.md` 八节原文把 `use_effect` 定成指向一个 Steel 脚本
    /// 的 `ContentIndex`，脚本产出 `Effect` 列表。本批次改用
    /// `Option<SkillEffect>`——`SkillEffect`（`ll_sim::skill`）已经能
    /// 表达「造成伤害/恢复资源/临时属性修正」，喝一瓶药水正是这三件
    /// 事之一：`crate::resolve::resolve_use_item`
    /// （`ll-sim`，本批次新增）对 `SkillEffect` 的 `match` 与既有的
    /// `resolve_use_skill` 逐字对应，见 [`ll_sim::item::ItemRule::use_effect`]
    /// 文档「为什么复用 `SkillEffect`」一节完整论证（ADR 0021：算法
    /// 真正可共享才抽象）。走 Steel 脚本需要脚本沙箱在结算路径上现场
    /// 求值，而 `SkillEffect` 是纯数据，`resolve` 保持纯函数（C1）不需要
    /// 为此额外引入脚本引擎依赖——这是比"跟设计文档形状对齐"更硬的
    /// 约束，因此本批次选择偏离设计文档原文的字段类型，改为复用既有
    /// 机制，而不是照抄文档新增一套平行的"物品脚本效果"通道。
    pub use_effect: Option<SkillEffect>,
    /// 穿透（武器引用与穿透接线批次，P6 第六批）——`Penetration::NONE`
    /// （默认值）表示这件物品不提供任何穿透加成。
    ///
    /// # 为什么现在也收进来了
    ///
    /// `crate::resolve::resolve_attack`（`ll-sim`）需要知道攻击者主手
    /// 武器的穿透值才能传给 `damage_after_defense`——此前（P6 第四批到
    /// 第五批）`ItemRule`/`StatBonus` 都不携带穿透字段，`resolve_attack`
    /// 只能恒传 `Penetration::NONE`。见
    /// [`ll_sim::item::ItemRule::penetration`] 文档完整论证。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `set_penetration` 追加
    ///
    /// 与 [`Self::equip_mask`]/[`Self::stat_bonuses`]/[`Self::use_effect`]
    /// 同一条既有先例（`register-item` 的六参数签名不能改参数个数）——
    /// 脚本层对应函数是 `register-item-penetration`
    /// （`crate::script_item_api`），Rust 层对应方法是
    /// [`ItemTable::set_penetration`]。**覆盖，不是追加**——与
    /// [`Self::use_effect`] 同一种"单值覆盖"语义：一件武器只有一份
    /// 穿透，不是可以累积的列表。
    pub penetration: Penetration,
    /// 这件物品显式声明的伤害公式（伤害公式引擎批次新增）——`None`
    /// （默认值）表示这件物品不指定公式，`resolve_attack` 退回全局
    /// 默认公式，见 [`ll_sim::formula::DamageFormulaCatalog`] 文档。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `set_damage_formula` 追加
    ///
    /// 与 [`Self::equip_mask`]/[`Self::stat_bonuses`]/[`Self::use_effect`]/
    /// [`Self::penetration`] 同一条既有先例（`register-item` 的六参数
    /// 签名不能改参数个数）——脚本层对应函数是
    /// `register-item-damage-formula`（`crate::script_item_api`），Rust
    /// 层对应方法是 [`ItemTable::set_damage_formula`]。**覆盖，不是
    /// 追加**——与 [`Self::penetration`] 同一种"单值覆盖"语义：一件
    /// 物品只有一份显式公式引用。
    pub damage_formula: Option<ContentIndex>,
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
    /// 装备占位掩码——`register-item` 注册时恒填 `SlotMask::EMPTY`
    /// （`do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-equip-mask` 调用 [`ItemTable::set_equip_mask`]
    /// 写入，理由同 [`ItemDef::equip_mask`] 文档。
    pub equip_mask: SlotMask,
    /// 静态属性加成列表——`register-item` 注册时恒为空列表（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-stat-bonus` 调用 [`ItemTable::add_stat_bonus`]
    /// 追加写入，理由同 [`ItemDef::stat_bonuses`] 文档。
    pub stat_bonuses: Vec<StatBonus>,
    /// 使用效果——`register-item` 注册时恒为 `None`（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-use-effect` 调用 [`ItemTable::set_use_effect`]
    /// 写入，理由同 [`ItemDef::use_effect`] 文档。
    pub use_effect: Option<SkillEffect>,
    /// 穿透——`register-item` 注册时恒为 `Penetration::NONE`（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-penetration` 调用 [`ItemTable::set_penetration`]
    /// 写入，理由同 [`ItemDef::penetration`] 文档。
    pub penetration: Penetration,
    /// 显式声明的伤害公式——`register-item` 注册时恒为 `None`（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-damage-formula` 调用
    /// [`ItemTable::set_damage_formula`] 写入，理由同
    /// [`ItemDef::damage_formula`] 文档。
    pub damage_formula: Option<ContentIndex>,
}

/// 物品注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// [`ItemTable::set_equip_mask`]/[`ItemTable::add_stat_bonus`] 的
    /// 目标索引尚未通过 `register-item` 注册，理由同
    /// [`crate::race::RaceError::NotDefined`]（ADR 0017「注册期完整
    /// 校验」）。
    NotDefined(ContentIndex),
}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemError::DuplicateDefinition(index) => {
                write!(f, "物品索引 {} 被重复定义", index.get())
            }
            ItemError::NotDefined(index) => {
                write!(
                    f,
                    "物品索引 {} 尚未定义，无法追加装备占位掩码/属性加成",
                    index.get()
                )
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
    /// 装备占位掩码。
    pub equip_mask: SlotMask,
    /// 静态属性加成列表——借用视图，不克隆，理由同
    /// [`Self::display_name_key`]（一读一写两个薄视图，读侧不复制底层
    /// 存储）。
    pub stat_bonuses: &'a [StatBonus],
    /// 使用效果。
    pub use_effect: Option<SkillEffect>,
    /// 穿透。
    pub penetration: Penetration,
    /// 显式声明的伤害公式。
    pub damage_formula: Option<ContentIndex>,
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
    equip_mask: Vec<SlotMask>,
    stat_bonuses: Vec<Vec<StatBonus>>,
    use_effect: Vec<Option<SkillEffect>>,
    penetration: Vec<Penetration>,
    damage_formula: Vec<Option<ContentIndex>>,
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
            self.equip_mask.resize(new_len, SlotMask::EMPTY);
            self.stat_bonuses.resize(new_len, Vec::new());
            self.use_effect.resize(new_len, None);
            self.penetration.resize(new_len, Penetration::NONE);
            self.damage_formula.resize(new_len, None);
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
        self.equip_mask[idx] = attrs.equip_mask;
        self.stat_bonuses[idx] = attrs.stat_bonuses;
        self.use_effect[idx] = attrs.use_effect;
        self.penetration[idx] = attrs.penetration;
        self.damage_formula[idx] = attrs.damage_formula;
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
            equip_mask: self.equip_mask[idx],
            stat_bonuses: &self.stat_bonuses[idx],
            use_effect: self.use_effect[idx],
            penetration: self.penetration[idx],
            damage_formula: self.damage_formula[idx],
        })
    }

    /// 追加声明「这件物品占用哪些装备槽位」——`register-item` 的既有
    /// 脚本签名不能改参数个数，因此装备占位掩码走这条独立的、注册后
    /// 追加的路径，与 [`crate::race::RaceTable::set_xp_reward`] 同一个
    /// 模式（见 [`ItemDef::equip_mask`] 文档）。目标索引必须已经
    /// `define` 过，否则返回 [`ItemError`]（ADR 0017「注册期完整
    /// 校验」）——本类型目前只有一种错误变体
    /// （[`ItemError::DuplicateDefinition`]），"未定义" 复用同一个
    /// 变体表达不准确，因此这里改为返回专门的
    /// [`ItemError::NotDefined`]。
    ///
    /// **覆盖，不是追加**——与 `set_xp_reward`「单值覆盖」同理：一件
    /// 物品的占位掩码是单个值，多次调用以最后一次为准，不像
    /// `RaceTable::add_trait_grant` 那样天然是一个需要累积的集合。
    pub fn set_equip_mask(&mut self, item: ContentIndex, mask: SlotMask) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.equip_mask[item.get() as usize] = mask;
        Ok(())
    }

    /// 追加一条静态属性加成（P6 第四批：`derive_stats` 与装备属性接进
    /// 战斗）——`register-item` 的既有脚本签名不能改参数个数，理由同
    /// [`Self::set_equip_mask`]。目标索引必须已经 `define` 过，否则
    /// 返回 [`ItemError::NotDefined`]，同一条 ADR 0017 纪律。
    ///
    /// **追加，不是覆盖**——与 [`Self::set_equip_mask`]「单值覆盖」
    /// 相反，与 [`crate::race::RaceTable::add_trait_grant`] 同一个模式：
    /// 一件物品可以携带任意多条加成（例如同时加力量与护甲），多次调用
    /// 累积进同一个列表，见 [`ItemDef::stat_bonuses`] 文档「为什么不是
    /// `register-item` 的参数」一节。
    pub fn add_stat_bonus(
        &mut self,
        item: ContentIndex,
        bonus: StatBonus,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.stat_bonuses[item.get() as usize].push(bonus);
        Ok(())
    }

    /// 设置「使用这件物品会发生什么」（P6 第五批：耐久与 `Intent::Use`）
    /// ——`register-item` 的既有脚本签名不能改参数个数，理由同
    /// [`Self::set_equip_mask`]。目标索引必须已经 `define` 过，否则
    /// 返回 [`ItemError::NotDefined`]，同一条 ADR 0017 纪律。
    ///
    /// **覆盖，不是追加**——与 [`Self::set_equip_mask`] 同一种"单值
    /// 覆盖"语义，见 [`ItemDef::use_effect`] 文档。
    pub fn set_use_effect(
        &mut self,
        item: ContentIndex,
        effect: SkillEffect,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.use_effect[item.get() as usize] = Some(effect);
        Ok(())
    }

    /// 设置这件物品的穿透（武器引用与穿透接线批次，P6 第六批）——
    /// `register-item` 的既有脚本签名不能改参数个数，理由同
    /// [`Self::set_equip_mask`]。目标索引必须已经 `define` 过，否则
    /// 返回 [`ItemError::NotDefined`]，同一条 ADR 0017 纪律。
    ///
    /// **覆盖，不是追加**——与 [`Self::set_use_effect`] 同一种"单值
    /// 覆盖"语义，见 [`ItemDef::penetration`] 文档。
    pub fn set_penetration(
        &mut self,
        item: ContentIndex,
        penetration: Penetration,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.penetration[item.get() as usize] = penetration;
        Ok(())
    }

    /// 设置这件物品显式声明的伤害公式（伤害公式引擎批次新增）——
    /// `register-item` 的既有脚本签名不能改参数个数，理由同
    /// [`Self::set_equip_mask`]。目标索引必须已经 `define` 过，否则
    /// 返回 [`ItemError::NotDefined`]，同一条 ADR 0017 纪律。
    ///
    /// **覆盖，不是追加**——与 [`Self::set_penetration`] 同一种"单值
    /// 覆盖"语义，见 [`ItemDef::damage_formula`] 文档。本方法不校验
    /// `formula` 是否已经通过 `register-damage-formula` 注册——与
    /// [`Self::add_stat_bonus`] 对 `StatTarget::Attribute` 不校验属性
    /// 种类是否合法同一条既有纪律，真正的存在性校验交给调用方
    /// （`crate::script_item_api::register_item_damage_formula`）在
    /// 写入前完成（ADR 0017「注册期完整校验」——校验发生在脚本绑定层，
    /// 不是本方法的职责）。
    pub fn set_damage_formula(
        &mut self,
        item: ContentIndex,
        formula: ContentIndex,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.damage_formula[item.get() as usize] = Some(formula);
        Ok(())
    }
}

/// `resolve` 侧的堆叠上限/装备占位/属性加成查询——`ll_sim::resolve::resolve_pick_up`
/// 判断「拾取时能否与背包已有堆合并」需要 `stack_limit`，
/// `resolve_equip`/`resolve_unequip` 判断占位冲突需要 `equip_mask`
/// （装备栏位批次，P6 第三批），`ll_sim::resolve::derive_stats`（P6 第
/// 四批）累加装备贡献的攻防加成需要 `stat_bonuses`，
/// `ll_sim::resolve::resolve_attack`（P6 第六批）查攻击者主手武器的
/// 穿透需要 `penetration`——见 `ll_sim::item::ItemCatalog` 文档「本模块
/// 新增」一节。与 `impl ResourcePoolCatalog for ResourcePoolTable`
/// （`crate::resource_pool` 模块）同一条既有先例：只把 `ItemView` 里
/// `resolve` 真正要读的字段搬进 [`ItemRule`]，不是把整条 `ItemView`
/// 转发出去。
impl ItemCatalog for ItemTable {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        self.get(item).map(|view| ItemRule {
            stack_limit: view.stack_limit,
            equip_mask: view.equip_mask,
            stat_bonuses: view.stat_bonuses.to_vec(),
            use_effect: view.use_effect,
            penetration: view.penetration,
            damage_formula: view.damage_formula,
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
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
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
            equip_mask: SlotMask::EMPTY,
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: Penetration::NONE,
            damage_formula: None,
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
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
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
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let rule = ItemCatalog::item(&table, index);

        // Assert
        assert_eq!(
            rule,
            Some(ItemRule {
                stack_limit: 99,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
            })
        );
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

    use ll_sim::item::EquipSlot;

    fn item_attrs() -> ItemAttrs {
        ItemAttrs {
            display_name_key: NamespacedId::parse("lostland:item.great_axe").unwrap(),
            stack_limit: 1,
            base_weight: Milli::from_whole(5),
            base_price: Milli::from_whole(80),
            max_durability: Some(120),
            equip_mask: SlotMask::EMPTY,
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: Penetration::NONE,
            damage_formula: None,
        }
    }

    #[test]
    fn 注册后追加的装备掩码能被真正查到() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:great_axe").unwrap());
        let mut table = ItemTable::new();
        table.define(index, item_attrs()).expect("首次定义应当成功");
        let two_handed = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());

        // Act
        let result = table.set_equip_mask(index, two_handed);

        // Assert
        assert_eq!(result, Ok(()));
        assert_eq!(table.get(index).unwrap().equip_mask, two_handed);
    }

    #[test]
    fn 未注册的物品追加装备掩码返回未定义错误() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let mut table = ItemTable::new();

        // Act
        let result = table.set_equip_mask(never_defined, EquipSlot::HEAD.mask());

        // Assert
        assert_eq!(result, Err(ItemError::NotDefined(never_defined)));
    }
}
