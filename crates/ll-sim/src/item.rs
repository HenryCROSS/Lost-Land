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
//! 落地时只收敛 `stack_limit` 一个字段——`resolve_pick_up`/`resolve_drop`
//! 不需要 `base_weight`/`base_price`/`max_durability` 中的任何一个（
//! 负重与耐久扣减都是后续批次的工作，见 `ll_world::item` 模块文档
//! 「`Owner` 本批次仍然不落地」一节同一条 YAGNI 判断），与
//! `crate::skill::SkillRule` 只收敛 `resolve_use_skill` 真正要读的
//! 字段是同一个理由。
//!
//! **`max_durability` 后来被这条判据自己请了进来**（新造物品耐久批次）：
//! `resolve_craft`/`resolve_identify` 造成品时要回答「这件刚造出来的
//! 东西带多少耐久」，答案只能来自成品那条定义的耐久上限——这正是
//! 「`resolve` 真正要读的字段才收进来」，不是判据松动。`base_weight`/
//! `base_price` 照旧不收：负重与经济系统至今没有任何结算消费者。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;

pub use ll_world::item::{
    EquipSlot, GroundItemStack, ItemStack, ItemStackError, SlotMask, StatBonus, StatTarget,
    WearChannels, can_merge, merge_stacks, split_stack,
};

use crate::combat::Penetration;
use crate::rule_modifier::TypedRuleModifier;
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
    /// 这件物品声明的规则修正（抗性多来源聚合批次新增）——落地项目
    /// 所有者对抗性来源的裁定「抗性肯定会来自天赋，以及装备，还有
    /// 各种药品，或者技能」里**装备**这一路，空列表（多数物品的既有
    /// 情形）表示这件物品不改变任何规则。
    ///
    /// # 为什么复用 `RuleModifier`，不为物品另开一个枚举
    ///
    /// ADR 0021：抽象的理由是「有算法可共享」。这里共享的是
    /// [`crate::rule_modifier::resistance_damage_reduction`] 那条
    /// 「按加值类型分桶、桶内取最强、跨桶相加」的合并算法——它与
    /// 「这条抗性是天赋给的还是护符给的」完全无关，见该
    /// 模块文档「ADR 0021 复核」一节。另造一个字段与 `RuleModifier`
    /// 逐字相同、只是改了个名字的 `ItemRuleModifier`，会逼着聚合点
    /// 为两个同构枚举各写一遍同一段 `match`，正是该 ADR 要防的重复。
    ///
    /// # 与 [`Self::stat_bonuses`] 的分工
    ///
    /// `stat_bonuses` 走 `crate::resolve::derive_stats`（**无条件求和**：
    /// 两件装备各加 3 点力量就是 6 点），本字段走
    /// `crate::rule_modifier::agent_rule_modifiers`（**先按加值类型分桶,
    /// 桶内取最强，再跨桶相加**：同一类型的两条 3 点减伤还是 3 点，
    /// 不同类型的 3 点与 2 点才是 5 点）。两条通道的合并规则不同,
    /// 因此是并列的
    /// 独立字段，不是把抗性硬塞进 `StatTarget` 再多一个变体——后者会
    /// 逼着 `DerivedStats` 从「七项属性 + 护甲」这个编译期定长数组，
    /// 变成一张按开放注册的 `damage_category` 索引的动态表，代价与
    /// 收益完全不成比例。
    pub rule_modifiers: Vec<TypedRuleModifier>,
    /// 这件物品的**耐久磨损通道**集合（耐久标签批次）——由它携带的
    /// 全部标签（`ItemDef.tags`）各自声明的通道在**注册期**并起来的
    /// 结果，`crate::resolve::resolve_attack`/`resolve_craft` 直接读它
    /// 决定这件东西这一下要不要掉耐久。
    ///
    /// # 为什么这里是折算好的掩码，不是标签列表本身
    ///
    /// ADR 0016/0017：**声明式，注册期物化，运行期查表**。一件物品带
    /// 哪些标签、每个标签走哪条通道，全都是装载期就固定下来的事实,
    /// 运行期一个字都不会变。把「遍历标签 → 逐个查标签表 → 并集」这段
    /// 纯装载期常量计算搬进 `resolve_attack` 的每一次攻击、每一件已装备
    /// 物品，正是该 ADR 要避免的事；折算发生在
    /// `ll_mod::item::ItemTable::add_tag` 那一刻，结算侧只剩一次
    /// `contains`。
    ///
    /// 这也是本视图**刻意不含 `tags` 原始列表**的原因：`ItemRule` 是
    /// 「`resolve` 需要什么就给什么」的最小视图（见本类型文档），而
    /// `resolve` 需要的是"要不要掉耐久"这个答案,不是标签清单。将来
    /// 真有系统需要按标签查询（"所有带 flammable 标签的东西"），再给
    /// 那个系统开一条它自己需要的窄接口。
    pub wear_channels: WearChannels,
    /// 这件物品的**耐久上限**（新造物品耐久批次）——`None` 表示这类
    /// 东西没有耐久概念（材料、消耗品、饰品）。完整语义见
    /// `ll_mod::item::ItemDef::max_durability`。
    ///
    /// # 唯一的用途：给刚造出来的物品定初值
    ///
    /// [`crate::resolve`] 的三个产出点里的两个（制作 `resolve_craft`、
    /// 盲盒 `resolve_identify`）把它交给
    /// [`ItemStack::freshly_made`]，第三个（出生装备）在 `ll-mod` 侧走
    /// 同一个构造器。磨损扣减本身**不读它**——扣减走
    /// `crate::effect::Effect::AdjustEquipmentDurability` 直接改
    /// `ItemStack::durability`，上限只在「造出来的那一刻」被用到一次。
    ///
    /// # 为什么与 [`Self::wear_channels`] 是两个字段
    ///
    /// 它们回答两个不同的问题：`wear_channels` 是「这类东西**会不会**
    /// 磨损」（由标签决定，注册期折算成掩码），本字段是「这一件**有
    /// 多少**耐久」（由物品自己声明）。两者可以合法地一有一无：
    /// `mods/lostland/items.json5` 的琥珀坠可装备、无标签、无耐久，
    /// 而一件带标签却不声明耐久上限的东西表示「会用到磨损通道但没有
    /// 耐久可掉」——`resolve_attack` 既有的
    /// `stack.durability.is_some()` 判据本来就把它当成不磨损处理。
    pub max_durability: Option<i32>,
    /// 读这件东西能学到哪些配方（配方发现批次）——
    /// `crate::resolve::resolve_read` 唯一的输入，空列表（多数物品的
    /// 既有情形）表示这件东西**不可读**（`Intent::Read` 对它静默无效）。
    ///
    /// # 为什么「可不可读」不是一个独立的布尔字段
    ///
    /// 「空列表 = 不可读」不需要第二种表达方式，多一个 `is_readable:
    /// bool` 只会制造一个必须手动维持一致的不变式（`is_readable` 为真
    /// 但列表为空、或反之，两种都是没有定义良好行为的状态）。同一条
    /// 判断在本仓库已有先例：[`Self::equip_mask`] 用
    /// `SlotMask::EMPTY` 表达「不可装备」，没有另开一个
    /// `is_equippable`。
    ///
    /// # 为什么挂在物品上，不另开一张「书表」
    ///
    /// ADR 0021：另开一张 `BookTable` 需要它自己的 `register-book`、
    /// `GameplayTables` 字段、`ContentTableKind` 变体、哈希覆盖、审计
    /// 花名册、存档重映射——整套接线，换来的却是一张与 `ItemDef` 一一
    /// 对应的表（书**就是**物品：它有重量、有价格、能捡能丢能堆叠，
    /// 这些全是 `ItemDef` 已经回答的问题）。一件东西可读与否是它的一条
    /// 属性，与「可装备」「有使用效果」「带哪些标签」是同一类属性，走
    /// 同一张表的同一套追加式注册函数。
    pub taught_recipes: Vec<ContentIndex>,
    /// 这件物品需要先鉴定才认得（未鉴定物品批次）——
    /// `crate::resolve::resolve_identify` 的准入判断读的就是它，`false`
    /// （多数物品的既有情形）表示 `crate::intent::Intent::Identify` 对它
    /// 静默无效。完整论证见 `ll_mod::item::ItemDef::requires_identification`。
    ///
    /// # 为什么这一条**是**独立的布尔，与 `taught_recipes` 不同
    ///
    /// [`Self::taught_recipes`] 文档「为什么『可不可读』不是一个独立的
    /// 布尔字段」那条论证在这里**不成立**，因此这里的选择不是不一致：
    /// 那里有一个天然的空/非空载荷（教哪些配方）可以兼职表达可读性；
    /// 「需不需要鉴定」没有任何载荷——鉴定不产出任何列表，它揭示的正是
    /// 物品**已有的**全部字段。硬要拿 [`Self::study_experience`] 兼职
    /// （「经验非零 = 需要鉴定」）会立刻绑死两件本该独立的事：一件需要
    /// 鉴定但学不到东西的破烂就无法表达了。
    pub requires_identification: bool,
    /// 鉴定或研读这件物品一次值多少经验——`resolve_identify` 与
    /// `crate::resolve::resolve_read` 共用的同一个输入，`0` 表示研究它
    /// 学不到任何东西。完整论证见 `ll_mod::item::ItemDef::study_experience`。
    pub study_experience: i64,
    /// 盲盒产出池（盲盒批次）——空列表（多数物品的既有情形）表示这件
    /// 物品不是盲盒。完整论证见 `ll_mod::item::ItemDef::blind_box_pool`，
    /// **包括其中那条「给盲盒写配方会打开经验水龙头」的警告**。
    pub blind_box_pool: Vec<BlindBoxEntry>,
    /// 这件物品放到地上之后是不是一件**家具**（家具层批次）——完整
    /// 论证见 `ll_mod::item::ItemDef::furniture`，本字段是结算侧那一半。
    ///
    /// 两个消费者，都在 [`crate::resolve`]：
    ///
    /// - `resolve_drop` 的放置前置——为真时这次「丢弃」是一次**放置**，
    ///   要先问「这一格放得下吗」（层可建造、地形没占着、还没摆过第二
    ///   件家具），三条任一不成立就静默无效。
    /// - `resolve_craft` 的场地前置——脚下那一格摆着的家具就是
    ///   [`crate::craft::RecipeRule::required_station`] 要的东西。
    ///
    /// 第三条后果（不随时间老化回收）不在结算层，在
    /// `ll_world::state::WorldState::cleanup_aged_ground_items` 的
    /// `is_permanent` 参数上——那条不是任何一次 `Intent` 的后果，是系统
    /// 级被动演化，见该方法文档。
    pub furniture: bool,
}

/// 盲盒池里的一条候选：产出哪件物品、权重多少（盲盒批次）。
///
/// # 为什么带权重，不是均匀抽
///
/// 均匀抽等于宣布「所有候选一样常见」，而盲盒这个玩法的全部意思就是
/// 「多数时候开出普通货，偶尔开出好东西」——没有权重就没有稀有度，盒子
/// 退化成一个换皮的随机材料袋。权重的编码与选取算法**照抄仓库里既有的
/// 那一套**（[`ll_world::weather::weather_kind_at`] 的
/// `WeatherDef::season_weights`：权重求和 → `DetRng::gen_range(总和)`
/// → 沿同一顺序前缀和 walk），不另发明：那套手法已经把 C5（遍历顺序
/// 必须确定）与「权重全为 0 怎么办」两件事都答过了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlindBoxEntry {
    /// 产出哪一件物品，指向物品表。**必须已注册**（注册期校验，理由同
    /// `ll_mod::item::ItemDef::taught_recipes`「跨表引用由谁校验」：拼错一个 id 的
    /// 症状是这个盒子静默少一档产出，是最难查的一类内容缺陷）。
    pub item: ContentIndex,
    /// 一次产出几个——恒 ≥ 1，注册期校验。
    pub count: u32,
    /// 相对权重，恒 ≥ 1（注册期校验）：0 权重的候选等于没写，与其让它
    /// 静默永不出现，不如当场报错。
    pub weight: u32,
}

/// `resolve` 依赖的最小「物品定义来源」接口——与
/// [`crate::skill::SkillCatalog`]/[`crate::resource_pool::ResourcePoolCatalog`]
/// 同一套依赖倒置手法：真正的 `ItemDef`/`ItemTable` 定义在下游的
/// `ll-mod`，本 crate 只声明「给我一个物品索引，还我它的堆叠上限」
/// 这个接口。
pub trait ItemCatalog {
    /// 查询一条物品定义；未注册的索引返回 `None`（ADR 0015）。
    fn item(&self, item: ContentIndex) -> Option<ItemRule>;

    /// 这个**种族/生物种类**的尸体是哪件物品；查不到就是 `None`
    /// （ADR 0015）。
    ///
    /// # 为什么这条查询在物品目录上
    ///
    /// `crate::resolve` 的 `append_corpse_drop` 需要把
    /// `victim.creature_kind.unwrap_or(victim.race)` 这个**归并键**翻译
    /// 成一个真正的**物品**索引（此前它把种族索引直接塞进
    /// [`ItemStack::def`]，那是一次类型混淆，见
    /// `ll_mod::corpse_item` 模块文档）。`ll-sim` 不能依赖 `ll-mod`
    /// （依赖方向，规格 §5），拿不到注册表，只能靠一条依赖倒置的查询；
    /// 而 `ItemCatalog` 已经是那个函数所在调用链上现成的物品目录参数，
    /// 不需要为此新开一个只有一个实现的 trait（ADR 0021）。
    ///
    /// # 为什么有默认实现
    ///
    /// 本 crate 内外有十几处 `impl ItemCatalog`，绝大多数是只关心
    /// 「查一条规则」的测试夹具。给它们强加一条必填方法只会让每一处都
    /// 抄一遍 `None`。默认实现 `None` 的含义与 [`NoItems`] 一致：**没有
    /// 尸体物品注册表**，调用方按自己的兜底纪律处理。
    ///
    /// 真正的实现只有一处：`ll_mod::item::ItemTable`，数据由
    /// `ll_mod::corpse_item::register_corpse_items` 在全部 mod 装载完
    /// 之后填进去。
    fn corpse_of(&self, _kind: ContentIndex) -> Option<ContentIndex> {
        None
    }
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

/// 一件物品登记的装备占位掩码；**查不到定义按 [`SlotMask::EMPTY`]
/// 处理**（视为不占用任何槽位）。
///
/// 抽成函数是因为这一句「查目录 → 取掩码 → 查不到当空」在
/// [`crate::resolve`] 的装备/卸下两条路径与 [`outfit_from_inventory`]
/// 里各出现一次，而那个 `map_or` 的**兜底方向**是一条被反复论证过的
/// 裁定（见 `resolve_equip` 文档「占位冲突」一节：老物品的冲突判定
/// 退化不应该无端阻塞新物品的装备），不该散成三处各写一遍。
pub fn equip_mask_of(def: ContentIndex, items: &dyn ItemCatalog) -> SlotMask {
    items
        .item(def)
        .map_or(SlotMask::EMPTY, |rule| rule.equip_mask)
}

/// `equipment` 里与 `new_mask` **占位冲突**的全部锚点槽位，按槽位升序
/// （`BTreeMap` 的遍历序，确定性见约束 C5）。
///
/// 「一条规则覆盖所有特例」（`knowledge/design/equipment-slots.md`）：
/// 双手武器、全身甲、连体装的冲突判定全部就是这一个掩码相交测试，没有
/// 任何逐类特例。[`crate::resolve`] 的 `resolve_equip` 与
/// [`outfit_from_inventory`] 共用本函数，因此「什么算冲突」在仓库里
/// 只有一个定义。
pub fn conflicting_anchors(
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    new_mask: SlotMask,
    items: &dyn ItemCatalog,
) -> Vec<EquipSlot> {
    equipment
        .iter()
        .filter(|(_, stack)| equip_mask_of(stack.def, items).intersects(new_mask))
        .map(|(anchor, _)| *anchor)
        .collect()
}

/// 把一份**刚生成出来的**背包按装备规则拆成「穿在身上的」与「留在
/// 背包里的」两半。
///
/// # 等谁来用：NPC 生成期的穿戴决策（今天**没有生产调用点**）
///
/// 本函数落地时（`3191884`）的消费者是
/// `ll_game::world::build_player_agent`——玩家角色的出生装备自动穿上。
/// 项目所有者随后推翻了那条裁定：
///
/// > 「这个如果是 NPC 就是根据 NPC 自行决策，人的话就等玩家自己装备吧」
///
/// 「人」指玩家角色，那条调用点因此已经撤掉（出生装备回到只进
/// `Agent::inventory`）。**函数本身刻意保留**：所有者裁定的另一半——
/// NPC 自行决策穿什么——需要的正是这一步「把一份背包按装备规则拆成
/// 两半」，删掉只会让做那一批的人原样重写一遍。它今天的调用点只有下面
/// 那四条单元测试，这是**有意的等待状态**，不是遗漏的死代码。
///
/// NPC 那一批还要在本函数之上加一层「这个 NPC 会挑哪件穿」的决策
/// （职业/体型/偏好），本函数只提供它下面那一层规则：**哪些穿得上、
/// 哪些互相冲突**。下面三条裁定描述的就是这一层，与那层决策正交。
///
/// # 三条裁定（生成期没有「玩家刚刚请求装备哪一件」这个输入）
///
/// **哪些该穿：`equip_mask` 非空的就穿。** 不另发明一个「出生时要不要
/// 穿上」的内容字段：`equip_mask` 本身就是内容作者写下的「这件东西是
/// 穿戴在身上的」，再要求写第二遍等于把同一件事声明两次，两处迟早
/// 分叉。这也正是 [`crate::resolve`] 的 `resolve_equip` 用的同一道闸门
/// （掩码为空即静默不装备）。
///
/// **槽位冲突：先到先得，后来者留在背包，不报错。** 与 `resolve_equip`
/// 的「后来者顶掉先来者」**刻意相反**，因为两者的输入不是一回事：
/// `resolve_equip` 服务的是一次**玩家请求**（「现在给我换上这件」），
/// 顶掉旧的正是玩家要的；世界生成没有任何请求，只有一份列表。若照搬
/// 顶替语义，内容作者写下的列表顺序就变成了一条**倒过来读**的优先级
/// （最后一件赢），而人读一份清单默认是从上往下的优先级。先到先得让
/// 列表顺序就是优先级顺序。
///
/// 也不报错：两件抢同一个槽位是**合法内容**（行囊里多带一件披风换洗
/// 完全正常），装载期拒绝它等于禁止一个种族携带两件同槽位衣物。
///
/// **穿上的不再留在背包。** 与 `resolve_equip` 逐字一致（那边是
/// `RemoveFromInventory` + `Equip` 一对效果）——两处都留一份就是凭空
/// 复制了一件物品。
///
/// # 整堆搬运，不拆堆
///
/// 一条 `count > 1` 的堆若可装备，整堆进槽位，与 `resolve_equip` 的
/// `Effect::Equip { stack }` 逐字一致（那里搬的也是整条堆）。本函数不
/// 在世界生成期发明一套「装一件、剩下的留背包」的拆堆规则——那会是一条
/// 只在这里成立的语义。本体全部可装备物品的 `stack_limit` 都是 1，
/// 这条差别今天在本体内容上不可观察。
pub fn outfit_from_inventory(
    inventory: Vec<ItemStack>,
    items: &dyn ItemCatalog,
) -> (BTreeMap<EquipSlot, ItemStack>, Vec<ItemStack>) {
    let mut equipment: BTreeMap<EquipSlot, ItemStack> = BTreeMap::new();
    let mut carried = Vec::new();

    for stack in inventory {
        let mask = equip_mask_of(stack.def, items);
        let Some(anchor) = mask.anchor_slot() else {
            // 掩码为空（不可装备）或查不到定义——留在背包。
            carried.push(stack);
            continue;
        };
        if conflicting_anchors(&equipment, mask, items).is_empty() {
            equipment.insert(anchor, stack);
        } else {
            carried.push(stack);
        }
    }

    (equipment, carried)
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

    /// 一份最小物品目录：只回答堆叠上限与装备掩码，其余字段取默认。
    struct MaskCatalog(BTreeMap<ContentIndex, SlotMask>);

    impl ItemCatalog for MaskCatalog {
        fn item(&self, item: ContentIndex) -> Option<ItemRule> {
            self.0.get(&item).map(|mask| ItemRule {
                stack_limit: 1,
                equip_mask: *mask,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                max_durability: None,
                wear_channels: WearChannels::default(),
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                furniture: false,
                taught_recipes: Vec::new(),
            })
        }
    }

    fn outfit_fixture() -> (MaskCatalog, ContentIndex, ContentIndex, ContentIndex) {
        let mut interner = Interner::new();
        let mut id = |raw: &str| interner.intern(NamespacedId::parse(raw).unwrap());
        let apron = id("lostland:forge_apron");
        let mantle = id("lostland:fur_mantle");
        let meat = id("lostland:roast_meat");
        let catalog = MaskCatalog(BTreeMap::from([
            (apron, EquipSlot::OUTER.mask()),
            (mantle, EquipSlot::OUTER.mask()),
            (meat, SlotMask::EMPTY),
        ]));
        (catalog, apron, mantle, meat)
    }

    #[test]
    fn 出生装备里可装备的穿上不可装备的留在背包() {
        // Arrange
        let (catalog, apron, _mantle, meat) = outfit_fixture();
        let inventory = vec![ItemStack::new(apron, 1), ItemStack::new(meat, 3)];

        // Act
        let (equipment, carried) = outfit_from_inventory(inventory, &catalog);

        // Assert：穿上的那件**不再**留在背包（两处都留就是凭空复制了
        // 一件物品，与 resolve_equip 的 RemoveFromInventory + Equip 一致）。
        assert_eq!(equipment.len(), 1);
        assert_eq!(equipment[&EquipSlot::OUTER].def, apron);
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].def, meat);
    }

    #[test]
    fn 两件抢同一槽位时先到先得后来者留在背包() {
        // Arrange：围裙与毛皮披风都占 outer。
        let (catalog, apron, mantle, _meat) = outfit_fixture();

        // Act
        let (equipment, carried) = outfit_from_inventory(
            vec![ItemStack::new(apron, 1), ItemStack::new(mantle, 1)],
            &catalog,
        );

        // Assert：列表顺序就是优先级顺序——**不是** resolve_equip 的
        // 「后来者顶掉先来者」，理由见 outfit_from_inventory 文档。
        assert_eq!(equipment[&EquipSlot::OUTER].def, apron);
        assert_eq!(
            carried.iter().map(|s| s.def).collect::<Vec<_>>(),
            vec![mantle]
        );
    }

    #[test]
    fn 同一份出生装备换个顺序穿上的就是另一件() {
        // Arrange：上一条的反面——先到先得这条裁定必须是**可观察**的，
        // 否则它与「后来者顶掉」在本体内容上区分不出来。
        let (catalog, apron, mantle, _meat) = outfit_fixture();

        // Act
        let (equipment, carried) = outfit_from_inventory(
            vec![ItemStack::new(mantle, 1), ItemStack::new(apron, 1)],
            &catalog,
        );

        // Assert
        assert_eq!(equipment[&EquipSlot::OUTER].def, mantle);
        assert_eq!(
            carried.iter().map(|s| s.def).collect::<Vec<_>>(),
            vec![apron]
        );
    }

    #[test]
    fn 查不到定义的物品留在背包不当作可装备() {
        // Arrange：与 resolve_equip 的同一条纪律——「新物品必须证明
        // 自己能装备」，查不到规则就没有任何证据。
        let mut interner = Interner::new();
        let unknown = interner.intern(NamespacedId::parse("yourmod:unknown").unwrap());

        // Act
        let (equipment, carried) =
            outfit_from_inventory(vec![ItemStack::new(unknown, 1)], &NoItems);

        // Assert
        assert!(equipment.is_empty());
        assert_eq!(carried.len(), 1);
    }
}
