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
use ll_sim::item::{BlindBoxEntry, ItemCatalog, ItemRule, SlotMask, StatBonus, WearChannels};
use ll_sim::rule_modifier::TypedRuleModifier;
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
    /// 真实 mod 脚本，见 `items.json5` 模块文档），与
    /// `ItemDef.max_durability` 当初落地时定下的纪律不同：`max_durability`
    /// 恰好是 `register-item` 原有六个参数之一，本字段是全新追加的，
    /// 只能走 `register-race-xp-reward`/`register-trait-resource-pool`
    /// 那条「新增能力用新函数」的既有先例——脚本层对应函数是
    /// `register-item-equip-mask`（`items.json5`），Rust 层
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
    /// `register-item-stat-bonus`（`items.json5`），Rust 层
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
    /// 是 `register-item-use-effect`（`items.json5`），Rust
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
    /// （`items.json5`），Rust 层对应方法是
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
    /// `register-item-damage-formula`（`items.json5`），Rust
    /// 层对应方法是 [`ItemTable::set_damage_formula`]。**覆盖，不是
    /// 追加**——与 [`Self::penetration`] 同一种"单值覆盖"语义：一件
    /// 物品只有一份显式公式引用。
    pub damage_formula: Option<ContentIndex>,
    /// 这件物品显式声明的伤害类别（伤害类别/抗性接线批次新增）——
    /// `None`（默认值）表示这件物品不指定伤害类别，`resolve_attack`
    /// 退回全局默认伤害类别，见
    /// [`ll_sim::damage_category::DamageCategoryCatalog`] 文档。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `set_damage_category` 追加
    ///
    /// 与 [`Self::damage_formula`] 同一条既有先例（`register-item` 的
    /// 六参数签名不能改参数个数）——脚本层对应函数是
    /// `register-item-damage-category`（`items.json5`），
    /// Rust 层对应方法是 [`ItemTable::set_damage_category`]。**覆盖，
    /// 不是追加**——一件物品只有一份显式伤害类别引用,与
    /// [`Self::damage_formula`] 同一种"单值覆盖"语义。
    ///
    /// # 与武器类别、`damage_formula` 都是独立的轴
    ///
    /// `damage-formula-mod-api.md` 十七节「是不是同一种东西：不是」——
    /// 伤害类别描述"造成哪种伤害"（挂公式、查抗性），与"这件物品显式
    /// 声明哪条公式"（[`Self::damage_formula`]）、"这件武器算哪一类
    /// 武器"（`register-weapon-category`，本批次未给 `ItemDef` 加对应
    /// 字段,见 `crate::weapon_category` 模块文档「本批次范围」一节）
    /// 是三件独立的事,本字段只回答第一个问题。
    pub damage_category: Option<ContentIndex>,
    /// 这件物品声明的规则修正（抗性多来源聚合批次新增）——落地项目
    /// 所有者对抗性来源的裁定「抗性肯定会来自天赋，以及装备，还有
    /// 各种药品，或者技能」里**装备**这一路。空列表（默认值）表示
    /// 这件物品不改变任何规则；载荷形状与消费路径见
    /// [`ll_sim::item::ItemRule::rule_modifiers`] 文档。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `add_rule_modifier` 追加
    ///
    /// 与 [`Self::stat_bonuses`] 同一条既有先例（`register-item` 的六
    /// 参数签名不能改参数个数）——脚本层对应函数是
    /// `register-item-resistance`（`items.json5`），Rust 层
    /// 对应方法是 [`ItemTable::add_rule_modifier`]。**追加，不是
    /// 覆盖**：一件装备可以同时声明对多个伤害类别的抗性，语义与
    /// [`Self::stat_bonuses`]/[`crate::trait_def::TraitDef::rule_modifiers`]
    /// 一致，不是 [`Self::equip_mask`] 那种单值覆盖。
    ///
    /// # 脚本层目前只开放了抗性一个变体
    ///
    /// `RuleModifier` 有五个变体，本批次只给物品开放
    /// `register-item-resistance` 一条注册入口——所有者的裁定谈的是
    /// 抗性；其余变体在天赋那一路的现状同样是「重骰/优势/劣势没有任何
    /// 消费者，偷袭有消费者但没有内容需求」。Rust 层的字段本身不限制
    /// 变体（聚合点对五个变体一视同仁，见
    /// [`ll_sim::rule_modifier::equipment_rule_modifiers`]），需要时照
    /// `register-trait-sneak-attack` 相对 `register-trait-resistance`
    /// 的先例再加一个注册函数即可，不改本字段形状。
    pub rule_modifiers: Vec<TypedRuleModifier>,
    /// 这件物品携带的**标签**列表（耐久标签批次）——项目所有者裁定
    /// 「每个物品可以有个标签的列表，带有多个标签」的落点，空列表
    /// （默认值）表示这件物品没有任何标签。每一条都是
    /// [`crate::tag::TagTable`] 里已经登记过的标签索引，
    /// `register-item-tag` 在注册期校验这一点（引用未注册的标签当场
    /// 报错，见 `items.json5` 里该函数的文档）。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `add_tag` 追加
    ///
    /// 与 [`Self::equip_mask`]/[`Self::stat_bonuses`] 同一条既有先例
    /// （`register-item` 的六参数签名不能改参数个数）——脚本层对应函数
    /// 是 `register-item-tag`，Rust 层对应方法是 [`ItemTable::add_tag`]。
    /// **追加，不是覆盖**：一件物品可以带多个标签，这正是所有者原话
    /// 里「带有多个标签」那半。
    ///
    /// # 决策层怎么消费它（为什么字段门禁里有一条豁免）
    ///
    /// 结算侧读的是 [`ItemTable::add_tag`] 在**注册期**把本列表折算出来
    /// 的 [`ll_sim::item::ItemRule::wear_channels`]，不是本字段本身
    /// ——ADR 0016/0017「注册期物化，运行期查表」，完整论证见
    /// `ItemRule::wear_channels` 文档。`scripts/ci/check_field_consumers.py`
    /// 的字段级正则抓不到这条间接路径（它头注释「已知局限」第 2 条点名
    /// 的那一类），因此那份清单里有一条写明这条路径的豁免。
    pub tags: Vec<ContentIndex>,
    /// 读这件物品一次能学到哪些配方（配方发现批次）——项目所有者裁定
    /// 「菜谱就是……阅读书籍的时候获取」里「书籍」那一半的落点。空列表
    /// （默认值）表示这件物品**不可读**，`ll_sim::intent::Intent::Read`
    /// 对它静默无效。
    ///
    /// # 为什么挂在 `ItemDef` 上，不另开一张「书表」
    ///
    /// 完整论证见 [`ll_sim::item::ItemRule::taught_recipes`] 文档
    /// 「为什么挂在物品上」一节，一句话版本：书**就是**物品（有重量、
    /// 有价格、能捡能丢），另开一张表要付整套接线代价（`register-book`
    /// 加 `GameplayTables` 字段加 `ContentTableKind` 变体加哈希覆盖加
    /// 审计花名册加存档重映射），换来一张与 `ItemDef` 一一对应的表。
    ///
    /// # 为什么不是 `register-item` 的参数，走 `add_taught_recipe` 追加
    ///
    /// 与 [`Self::equip_mask`]/[`Self::stat_bonuses`]/[`Self::tags`] 同
    /// 一条既有先例（`register-item` 的六参数签名不能改参数个数，会
    /// 破坏仓库里已有的真实 mod 脚本，见 `items.json5` 模块
    /// 文档）——脚本层对应函数是 `register-item-teaches-recipe`，Rust
    /// 层对应方法是 [`ItemTable::add_taught_recipe`]。**追加，不是
    /// 覆盖**：一本书可以教多条配方，语义与 [`Self::tags`] 一致。
    ///
    /// # 跨表引用由谁校验：两道，注册期一道 + 装载后一道
    ///
    /// 与 [`Self::tags`] 同一条形状（**不**同于 `RecipeDef::product`
    /// 那条「只 intern 不跨表校验」）：脚本层的
    /// `register-item-teaches-recipe` 在**注册期**就要求目标是一条真的
    /// 登记在配方表里的配方。这道校验值得付出「书必须写在配方之后」这
    /// 点顺序耦合，理由与 `register-item-tag` 校验标签 id 逐字相同——
    /// 拼错一个配方 id 的症状是**这本书静默什么都不教**，是最难查的一
    /// 类内容缺陷。
    ///
    /// 装载全部完成后 `crate::content_audit` 的
    /// `ItemAttrs::taught_recipes` 引用检查是第二道独立防线（本方法
    /// [`ItemTable::add_taught_recipe`] 自身不做跨表校验，因此绕过脚本
    /// 层直接调用 Rust API 的路径由它兜住）。
    pub taught_recipes: Vec<ContentIndex>,
    /// 这件物品**需要先鉴定**才认得（未鉴定物品批次）——项目所有者裁定
    /// 「可以加入未鉴定物品，通过鉴定获取属性和说明」里「未鉴定」那一半
    /// 的落点。`false`（默认值）表示这件东西一眼就认得，
    /// `ll_sim::intent::Intent::Identify` 对它静默无效。
    ///
    /// # 为什么是内容作者逐条声明，不是「一律需要鉴定」
    ///
    /// 「全部物品都要鉴定」会立刻把生肉、铁锭、亚麻布这类东西一起卷进
    /// 来——没有人需要「鉴定」一块铁锭，那不是神秘，那是杂务。判据与
    /// [`crate::recipe::RecipeDef::requires_discovery`] 那条
    /// **逐字同构**（本字段的命名也刻意与它对齐）：值得被「发现」的是
    /// 少数带信息的条目，默认值必须是「不需要」，否则每加一件普通材料
    /// 都要记得关掉它。
    ///
    /// # 未鉴定**不改变任何结算**
    ///
    /// 一把没鉴定的剑照样按真实属性打人，见
    /// [`ll_world::entity::Agent::identified_items`] 文档
    /// 「未鉴定不影响任何结算」一节：这个字段的全部效力是**呈现层**
    /// （`ll_ui::hud::item_display_name` 把名字换成「未鉴定的物品」）
    /// 加一道 `resolve_identify` 的准入判断，不进任何伤害/防御公式。
    pub requires_identification: bool,
    /// 鉴定或研读这件物品一次值多少经验（未鉴定物品批次）——项目所有者
    /// 把研究类经验**收窄**成两条来源「通过未鉴定物品和书籍获取经验」
    /// 之后，两条来源共用的就是这一个字段。`0`（默认值）表示这件东西
    /// 研究起来学不到任何东西。恒非负，注册期硬校验（负数当场报错，
    /// 与 [`crate::race::RaceDef::xp_reward`] 逐字同一条先例）。
    ///
    /// # 一个字段服务两条路径，不是两个
    ///
    /// `ll_sim::resolve::resolve_identify`（鉴定出一个新种类）与
    /// `ll_sim::resolve::resolve_read`（读一本书真的教到了新配方）产出
    /// 的是同一条 `Effect::GrantExperience`，读的也是同一个字段。给两条
    /// 路径各开一个字段（`identify_experience` / `read_experience`）需要
    /// 先回答「同一件东西两条路径的值凭什么不同」，而没有任何内容需求
    /// 提出过这个区分——正是 ADR 0021 与 YAGNI 同时点名的投机式抽象。
    /// 一本**同时**需要鉴定的书因此确实能拿两次经验，但那是两次不同的
    /// 一次性事件（认出这是本书 / 读懂里面的配方），不是重复计费。
    ///
    /// # 防刷靠「一次性事件」，不靠数值
    ///
    /// 两条路径都只在**真的学到新东西**时才产出效果：鉴定只对
    /// `identified_items` 里还没有的种类生效，读书只在真的教到新配方时
    /// 才产出。重复做恒零收益，因此本字段可以放心取任意大的值，不需要
    /// 为「刷」预留任何数值上的余地。**唯一的例外是盲盒**
    /// （[`Self::blind_box_pool`]），见该字段文档。
    pub study_experience: i64,
    /// 这件物品是一个**盲盒**：鉴定它会把它消耗掉，并从这个池子里随机
    /// 产出一件物品（盲盒批次）——项目所有者裁定「我希望能加入盲盒这种
    /// 物品，鉴定了可以获取经验，同时会随机获得一件物品或者武器装备」
    /// 的落点。空列表（默认值）表示这件物品**不是**盲盒。
    ///
    /// # 与普通鉴定的本质区别：转化，不是揭示
    ///
    /// | | 普通鉴定 | 盲盒 |
    /// |---|---|---|
    /// | 物品去向 | 留着（只是你现在认识它了） | **被消耗** |
    /// | 产出 | 无 | **一件随机物品** |
    /// | 性质 | 揭示 | **转化** |
    ///
    /// 正因如此，一个盲盒**必须同时**声明
    /// [`Self::requires_identification`]（否则没有任何动作能打开它），
    /// 这条由注册期硬校验（见 `content_schema_gear::define_one_item`）。
    ///
    /// # ⚠ 盲盒是「只有学到新东西才给经验」那条原则的**有意例外**
    ///
    /// 项目所有者的裁定，原话：**「开盲盒都给吧，轻松点，这是游戏」**。
    /// 每开一个盒子给一次 [`Self::study_experience`]，不查「产出物认不
    /// 认识」、也不查「这种盒子开过没有」。这**不是**没想到防刷，是明确
    /// 的取舍：
    ///
    /// 1. 那条原则仍然完整适用于普通鉴定与读书两条路径，一个字都没改；
    /// 2. 产出经验的**上限由「世界里有多少盒子」决定**，不由玩家按了几
    ///    次决定——盒子目前是纯粹的世界产物：没有任何配方产出它，也没有
    ///    交易系统能买到它。
    ///
    /// **⚠ 给盲盒写配方（或让它可购买）的人请读这一段**：第 2 条那个
    /// 上限**当场消失**，盲盒会变成一台经验机器。真要那么做，需要先把
    /// 「开盒给多少经验」重新拿去裁定，而不是默默加一条配方。这句话写
    /// 在这里，是为了让加配方的人在写下 `product: "…blind_box"` 之前就
    /// 看见它。
    ///
    /// # 为什么是「一串候选」而不是引用一张掉落表
    ///
    /// 另开一张「战利品表」要付整套接线代价（新 `ContentTableKind` 变体、
    /// `ContentValueTables` 新字段、哈希覆盖、审计花名册、存档重映射），
    /// 换来的第一个用户是一张只有几条候选的列表——判据与
    /// [`Self::taught_recipes`]「为什么挂在物品上，不另开一张书表」逐字
    /// 相同。等到真的出现「多个盒子共用同一张表」的内容需求时，那才是
    /// 抽出一张表的时机（ADR 0021）。
    pub blind_box_pool: Vec<BlindBoxEntry>,
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
    /// 显式声明的伤害类别——`register-item` 注册时恒为 `None`（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-damage-category` 调用
    /// [`ItemTable::set_damage_category`] 写入，理由同
    /// [`ItemDef::damage_category`] 文档。
    pub damage_category: Option<ContentIndex>,
    /// 规则修正列表——`register-item` 注册时恒为空列表（同上，
    /// `do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-resistance` 调用 [`ItemTable::add_rule_modifier`]
    /// 追加写入，理由同 [`ItemDef::rule_modifiers`] 文档。
    pub rule_modifiers: Vec<TypedRuleModifier>,
    /// 标签列表——`register-item` 注册时恒为空列表（`do_register_item`
    /// 不接受这个参数），真正的取值由后续 `register-item-tag` 调用
    /// [`ItemTable::add_tag`] 追加写入，理由同 [`ItemDef::tags`] 文档。
    pub tags: Vec<ContentIndex>,
    /// 可教授的配方列表——`register-item` 注册时恒为空列表
    /// （`do_register_item` 不接受这个参数），真正的取值由后续
    /// `register-item-teaches-recipe` 调用
    /// [`ItemTable::add_taught_recipe`] 追加写入，理由同
    /// [`ItemDef::taught_recipes`] 文档。
    pub taught_recipes: Vec<ContentIndex>,
    /// 需要先鉴定才认得——与上面几条不同，这一条**在 `define` 那一刻
    /// 就有真实取值**（它是纯布尔，没有跨表引用要校验，因此不需要一条
    /// 单独的 `set_*` 入口），理由见 [`ItemDef::requires_identification`]。
    pub requires_identification: bool,
    /// 鉴定/研读一次值多少经验，同上：`define` 那一刻就有真实取值，
    /// 理由见 [`ItemDef::study_experience`]。
    pub study_experience: i64,
    /// 盲盒产出池——`define` 注册时恒为空列表，真正的取值由后续
    /// [`ItemTable::add_blind_box_entry`] 追加写入（它要跨表校验候选
    /// 物品真的已注册），理由同 [`ItemDef::blind_box_pool`] 文档。
    pub blind_box_pool: Vec<BlindBoxEntry>,
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
    /// [`ItemTable::add_tag`] 把同一个标签重复挂到同一件物品上——见该
    /// 方法文档「追加，不是覆盖」一段：重复声明没有任何意义,只可能是
    /// 复制粘贴的笔误,注册期拒绝而不是静默去重。
    DuplicateTag {
        /// 被重复挂标签的物品。
        item: ContentIndex,
        /// 重复的那个标签。
        tag: ContentIndex,
    },
    /// [`ItemTable::add_taught_recipe`] 把同一条配方重复挂到同一件物品
    /// 上——理由与 [`ItemError::DuplicateTag`] 逐字相同。
    DuplicateTaughtRecipe {
        /// 被重复挂配方的物品。
        item: ContentIndex,
        /// 重复的那条配方。
        recipe: ContentIndex,
    },
    /// [`ItemTable::add_blind_box_entry`] 收到一条权重或数量为零的候选
    /// ——「写了等于没写」，注册期拒绝而不是静默吞掉，理由同
    /// [`ItemError::DuplicateTag`]。
    DegenerateBlindBoxEntry {
        /// 声明这条候选的盲盒。
        item: ContentIndex,
        /// 那条候选想产出的物品。
        product: ContentIndex,
    },
}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemError::DuplicateDefinition(index) => {
                write!(f, "物品索引 {} 被重复定义", index.get())
            }
            ItemError::DuplicateTag { item, tag } => {
                write!(
                    f,
                    "物品索引 {} 已经带有标签索引 {}，不能重复声明",
                    item.get(),
                    tag.get()
                )
            }
            ItemError::DegenerateBlindBoxEntry { item, product } => {
                write!(
                    f,
                    "盲盒索引 {} 的产出候选（物品索引 {}）权重或数量为零，写了等于没写",
                    item.get(),
                    product.get()
                )
            }
            ItemError::DuplicateTaughtRecipe { item, recipe } => {
                write!(
                    f,
                    "物品索引 {} 已经声明能教授配方索引 {}，不能重复声明",
                    item.get(),
                    recipe.get()
                )
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
    /// 显式声明的伤害类别。
    pub damage_category: Option<ContentIndex>,
    /// 规则修正列表——借用视图，不克隆，理由同 [`Self::stat_bonuses`]。
    pub rule_modifiers: &'a [TypedRuleModifier],
    /// 标签列表——借用视图，不克隆，理由同 [`Self::stat_bonuses`]。
    pub tags: &'a [ContentIndex],
    /// 可教授的配方列表（配方发现批次），见
    /// [`ItemDef::taught_recipes`]。
    pub taught_recipes: &'a [ContentIndex],
    /// 需要先鉴定才认得，见 [`ItemDef::requires_identification`]。
    pub requires_identification: bool,
    /// 鉴定/研读一次值多少经验，见 [`ItemDef::study_experience`]。
    pub study_experience: i64,
    /// 盲盒产出池——借用视图，不克隆，理由同 [`Self::stat_bonuses`]。
    pub blind_box_pool: &'a [BlindBoxEntry],
    /// 由 [`Self::tags`] 在注册期折算出的耐久磨损通道集合，见
    /// [`ItemTable::add_tag`] 文档「为什么在这里折算」一节。
    pub wear_channels: WearChannels,
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
    damage_category: Vec<Option<ContentIndex>>,
    rule_modifiers: Vec<Vec<TypedRuleModifier>>,
    tags: Vec<Vec<ContentIndex>>,
    /// 由 `tags` 折算出的派生列（不是独立声明的内容）——见
    /// [`ItemTable::add_tag`] 文档。
    wear_channels: Vec<WearChannels>,
    /// 每件物品读一次能教会哪些配方（配方发现批次）——见
    /// [`ItemDef::taught_recipes`]。
    taught_recipes: Vec<Vec<ContentIndex>>,
    /// 每件物品要不要先鉴定（未鉴定物品批次）——见
    /// [`ItemDef::requires_identification`]。
    requires_identification: Vec<bool>,
    /// 每件物品鉴定/研读一次值多少经验——见 [`ItemDef::study_experience`]。
    study_experience: Vec<i64>,
    /// 每件盲盒的产出池（盲盒批次）——见 [`ItemDef::blind_box_pool`]。
    blind_box_pool: Vec<Vec<BlindBoxEntry>>,
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
            self.damage_category.resize(new_len, None);
            self.rule_modifiers.resize(new_len, Vec::new());
            self.tags.resize(new_len, Vec::new());
            self.taught_recipes.resize(new_len, Vec::new());
            self.requires_identification.resize(new_len, false);
            self.study_experience.resize(new_len, 0);
            self.blind_box_pool.resize(new_len, Vec::new());
            self.wear_channels.resize(new_len, WearChannels::NONE);
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
        self.damage_category[idx] = attrs.damage_category;
        self.rule_modifiers[idx] = attrs.rule_modifiers;
        self.tags[idx] = attrs.tags;
        self.taught_recipes[idx] = attrs.taught_recipes;
        self.requires_identification[idx] = attrs.requires_identification;
        self.study_experience[idx] = attrs.study_experience;
        self.blind_box_pool[idx] = attrs.blind_box_pool;
        // 派生列：`define` 恒写空——`attrs.tags` 在 `register-item` 那一刻
        // 恒是空列表，真正的取值由后续 `add_tag` 逐条折算。
        self.wear_channels[idx] = WearChannels::NONE;
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
            damage_category: self.damage_category[idx],
            rule_modifiers: &self.rule_modifiers[idx],
            tags: &self.tags[idx],
            wear_channels: self.wear_channels[idx],
            taught_recipes: &self.taught_recipes[idx],
            requires_identification: self.requires_identification[idx],
            study_experience: self.study_experience[idx],
            blind_box_pool: &self.blind_box_pool[idx],
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
    /// （`items.json5`）在
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

    /// 设置这件物品显式声明的伤害类别（伤害类别/抗性接线批次新增）——
    /// `register-item` 的既有脚本签名不能改参数个数，理由同
    /// [`Self::set_equip_mask`]。目标索引必须已经 `define` 过，否则
    /// 返回 [`ItemError::NotDefined`]，同一条 ADR 0017 纪律。
    ///
    /// **覆盖，不是追加**——与 [`Self::set_damage_formula`] 同一种"单值
    /// 覆盖"语义，见 [`ItemDef::damage_category`] 文档。本方法不校验
    /// `category` 是否已经通过 `register-damage-category` 注册——与
    /// [`Self::set_damage_formula`] 同一条既有纪律，真正的存在性校验
    /// 交给调用方（`items.json5`）
    /// 在写入前完成。
    pub fn set_damage_category(
        &mut self,
        item: ContentIndex,
        category: ContentIndex,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.damage_category[item.get() as usize] = Some(category);
        Ok(())
    }

    /// 追加一条规则修正（抗性多来源聚合批次新增）——`register-item` 的
    /// 既有脚本签名不能改参数个数，理由同 [`Self::set_equip_mask`]。
    /// 目标索引必须已经 `define` 过，否则返回 [`ItemError::NotDefined`]，
    /// 同一条 ADR 0017 纪律。
    ///
    /// **追加，不是覆盖**——与 [`Self::add_stat_bonus`] 同一个模式，见
    /// [`ItemDef::rule_modifiers`] 文档。本方法不校验
    /// `RuleModifier::Resistance` 里的 `damage_category`、也不校验
    /// `TypedRuleModifier::modifier_type` 是否已经注册——与
    /// [`Self::set_damage_category`]
    /// 同一条既有纪律，真正的存在性校验交给调用方
    /// （`items.json5`）在写入前
    /// 完成，与 [`crate::trait_def::TraitTable::add_rule_modifier`] 完全
    /// 对称。
    pub fn add_rule_modifier(
        &mut self,
        item: ContentIndex,
        modifier: TypedRuleModifier,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        self.rule_modifiers[item.get() as usize].push(modifier);
        Ok(())
    }

    /// 追加声明「这件物品带有某个标签」（耐久标签批次）——脚本层对应
    /// 函数是 `register-item-tag`，见 [`ItemDef::tags`] 文档。目标索引
    /// 必须已经 `define` 过（ADR 0017），否则返回
    /// [`ItemError::NotDefined`]。
    ///
    /// **追加，不是覆盖**——一件物品带多个标签正是所有者裁定的形状。
    /// 同一个标签重复挂到同一件物品上返回
    /// [`ItemError::DuplicateTag`]：那不是一个有意义的声明，只可能是
    /// 内容作者复制粘贴出的笔误，注册期直接拒绝而不是静默去重，与
    /// `register-item` 拒绝矛盾配置同一条纪律。
    ///
    /// # 为什么在这里折算 `wear_channels`
    ///
    /// `wear` 参数是**调用方从标签表里查好的**这条标签自己声明的磨损
    /// 通道（[`crate::tag::TagDef::wear`]）；本方法把它并进这件物品的
    /// `wear_channels` 派生列。ADR 0016/0017：声明式内容在**注册期
    /// 物化**，运行期只查表。一件物品带哪些标签、每个标签走哪条通道，
    /// 全是装载期就固定的事实，把「遍历标签 → 逐个查标签表 → 求并集」
    /// 搬进 `resolve_attack` 的每一次攻击 × 每一件已装备物品，正是该
    /// ADR 要避免的事。
    ///
    /// 本方法**不自己查标签表**：`ItemTable` 不持有、也不该持有对
    /// `TagTable` 的引用（两张表的生命周期在 `GameplayTables` 里是并列
    /// 的可变借用，互相引用会立刻撞上借用检查），查表发生在
    /// `items.json5` —— 那里两张表
    /// 同时在手。
    pub fn add_tag(
        &mut self,
        item: ContentIndex,
        tag: ContentIndex,
        wear: WearChannels,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        let idx = item.get() as usize;
        if self.tags[idx].contains(&tag) {
            return Err(ItemError::DuplicateTag { item, tag });
        }
        self.tags[idx].push(tag);
        self.wear_channels[idx] = self.wear_channels[idx].union(wear);
        Ok(())
    }

    /// 追加声明「读这件物品能学会某条配方」（配方发现批次）——脚本层
    /// 对应函数是 `register-item-teaches-recipe`，见
    /// [`ItemDef::taught_recipes`] 文档。目标物品必须已经 `define` 过
    /// （ADR 0017），否则返回 [`ItemError::NotDefined`]。
    ///
    /// **追加，不是覆盖**——一本书教多条配方正是这个字段存在的意义。
    /// 同一条配方重复挂到同一本书上返回
    /// [`ItemError::DuplicateTaughtRecipe`]：那不是一个有意义的声明，
    /// 只可能是内容作者复制粘贴出的笔误，注册期直接拒绝而不是静默去重
    /// ——与 [`Self::add_tag`] 对重复标签的处理逐字同构。
    ///
    /// **不校验 `recipe` 是不是一条已注册的配方**：跨表强校验会让注册
    /// 顺序产生耦合，完整性由 `crate::content_audit` 兜住，见
    /// [`ItemDef::taught_recipes`] 文档「跨表引用由谁校验」一节。
    pub fn add_taught_recipe(
        &mut self,
        item: ContentIndex,
        recipe: ContentIndex,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        let idx = item.get() as usize;
        if self.taught_recipes[idx].contains(&recipe) {
            return Err(ItemError::DuplicateTaughtRecipe { item, recipe });
        }
        self.taught_recipes[idx].push(recipe);
        Ok(())
    }

    /// 追加一条盲盒产出候选（盲盒批次）——**追加，不是覆盖**，语义同
    /// [`Self::add_taught_recipe`]：一个盒子可以有多档产出。
    ///
    /// # 为什么这一条走 `add_*` 而 `requires_identification`/
    /// `study_experience` 直接进 `define`
    ///
    /// 判据是「这个字段有没有需要在注册期校验的东西」，不是「它是不是
    /// 新增的」：布尔与整数各自只需要一条本地校验（后者的非负判断在
    /// `content_schema_gear::define_one_item` 里，与 `stack_limit >= 1`
    /// 并排），而本方法要做两件 `define` 做不到的事——拒绝权重/数量为
    /// 零的候选，以及（在 `apply_items` 那一层）确认候选物品真的已经
    /// 注册过。
    ///
    /// 权重与数量的下限都是 `1`：权重 0 的候选永远抽不中、数量 0 的
    /// 候选开出个空气，两者都是「写了等于没写」，与其静默吞掉不如当场
    /// 报错（ADR 0017「注册期完整校验」）。
    pub fn add_blind_box_entry(
        &mut self,
        item: ContentIndex,
        entry: BlindBoxEntry,
    ) -> Result<(), ItemError> {
        if !self.is_defined(item) {
            return Err(ItemError::NotDefined(item));
        }
        if entry.weight == 0 || entry.count == 0 {
            return Err(ItemError::DegenerateBlindBoxEntry {
                item,
                product: entry.item,
            });
        }
        self.blind_box_pool[item.get() as usize].push(entry);
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
            damage_category: view.damage_category,
            rule_modifiers: view.rule_modifiers.to_vec(),
            wear_channels: view.wear_channels,
            max_durability: view.max_durability,
            taught_recipes: view.taught_recipes.to_vec(),
            requires_identification: view.requires_identification,
            study_experience: view.study_experience,
            blind_box_pool: view.blind_box_pool.to_vec(),
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
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                    tags: Vec::new(),
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
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
            damage_category: None,
            rule_modifiers: Vec::new(),
            tags: Vec::new(),
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
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
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                    tags: Vec::new(),
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
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
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                    tags: Vec::new(),
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                },
            )
            .expect("首次定义应当成功");

        // Act
        let rule = ItemCatalog::item(&table, index);

        // Assert
        assert_eq!(
            rule,
            Some(ItemRule {
                wear_channels: WearChannels::NONE,
                max_durability: None,
                taught_recipes: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                stack_limit: 99,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
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
            damage_category: None,
            rule_modifiers: Vec::new(),
            tags: Vec::new(),
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
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
