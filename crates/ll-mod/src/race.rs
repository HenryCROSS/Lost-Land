//! 种族注册表——「本体即 Mod」在种族系统上的落点，补齐
//! `knowledge/design/race-system.md`「落地状态」一节记录的缺口：
//! 该文档早已把 `Agent.race: ContentIndex` 定为「指向注册表，mod 可
//! 注册新种族」，但截至本模块之前，`race` 只是一个裸 `ContentIndex`
//! ——没有任何 `RaceDef`/`RaceTable` 存在，`Registry` 完全不知道某个
//! 种族索引背后应该有什么属性。本模块把该文档「一、核心形状」一节
//! 给出的 `RaceDef` 草图落地成真正的表。
//!
//! # 照抄 `class.rs`/`terrain.rs` 已验证的模式
//!
//! 见 `crate::class` 模块文档「照抄 `terrain.rs`/`space_profile.rs`
//! 已验证的模式」一节——私有字段 + `RaceTable::define` 注册期完整校验
//! （ADR 0017）+ `materialize_base_races` 本体注册入口 +
//! `base_race_fixture` 测试夹具，本模块走同一条路径，与职业同一个理由
//! 直接落在 `ll-mod`（种族定义本身不依赖任何「世界空间」概念）。
//!
//! # 字段形状：以设计文档为准，但显示名走既有的 `NamespacedId` 惯例
//!
//! `race-system.md`「一、核心形状」给出的草图是：
//!
//! ```text
//! pub struct RaceDef {
//!     pub id: NamespacedId,
//!     pub name_key: String,
//!     pub stat_modifiers: BaseStats,
//!     pub darkvision_floor: i32,
//!     pub footprint: (u8, u8),
//!     pub lifespan_years: u32,
//! }
//! ```
//!
//! 本模块四项数值字段（`stat_modifiers`/`darkvision_floor`/
//! `footprint`/`lifespan_years`）与草图完全一致——这些是设计文档反复
//! 论证过的实质内容（见该文档二~七节）。**唯一的偏离**：草图写的
//! `name_key: String` 换成了 `display_name_key: NamespacedId`，与
//! `ClassDef`/`SubclassDef`/`TerrainDef` 已经确立的惯例对齐（本地化是
//! 独立系统的职责，注册表只存指向 Fluent 键的标识符，不存字面字符串
//! ——见 `crate::class` 模块文档「查询接口」一节前一段）。种族系统设计
//! 文档写下草图时这条惯例尚未在代码里出现，本模块跟随代码库当前已经
//! 确立的约定，而不是逐字照抄一份先于该惯例写就的草稿。
//!
//! # 属性修正：固定加减，不是千分比
//!
//! `stat_modifiers` 的类型是 [`BaseStats`]，但语义**不是**「这个种族的
//! 主属性值」，而是「创建角色时一次性加到 `BaseStats::BASELINE`（或
//! 其他基准值）上的固定增减量」——见 `race-system.md`「三、数值形式」
//! 一节：主属性值域只有 10~30，千分比在这个值域上会被整数除法舍成 0
//! 或产生不可心算的零头，因此种族修正复用 `BaseStats` 的字段布局，但
//! 存的是**增量**而不是**绝对值**。字段本身复用 [`BaseStats`] 类型
//! （与设计文档草图一致），不是新引入一个形状相同的独立类型——六个
//! `i32` 字段的语义已经由字段名本身（`strength`/`dexterity`/……）说
//! 清楚，没有必要为了区分「绝对值」与「增量」两种用途另起一个结构体。
//!
//! # 与 `register-race-xp-reward` 的关系
//!
//! `RaceDef::xp_reward`（等级与经验系统落地批次新增）没有塞进
//! `register-race` 现有的脚本签名——`skill-requires!`/
//! `register-class-xp-curve` 已经立下的先例：不改既有 `register-*`
//! 函数的参数个数，需要新能力就加新函数（会破坏真实调用它的
//! `mods/example_mod/gameplay.scm`）。`register-race-xp-reward(id,
//! amount)` 是这条先例在种族经验值上的应用：先用 `register-race`
//! 声明种族本体，再用这个新函数追加声明「杀死它给多少经验」，两次
//! 调用不会自动同步（`register-race` 不声明经验值时默认 0），与
//! `skill-requires!`/`register-skill` 的「分类展示与强制闸门是两件
//! 独立的事」同一条设计哲学。
//!
//! # 与 `lostland:placeholder_race` 的协调
//!
//! [`crate::base_placeholder`] 已经注册了一个占位种族索引
//! （`lostland:placeholder_race`），代表「种族未知/缺失」这个降级状态
//! （见其模块文档）。**本模块不会、也不应该为这个占位索引定义任何
//! `RaceDef`**——占位索引的语义就是「没有真实种族数据」，若给它也定义
//! 一份 `RaceDef`（哪怕全填零），会让「查询到一份全零的种族属性」与
//! 「压根没有种族数据」这两种不同的情况在 [`RaceTable::get`] 的返回值
//! 上变得无法区分。[`RaceTable::get`] 对占位索引的查询因此如同任何其他
//! 未注册索引一样返回 `None`（对齐 ADR 0015「查不到就是查不到」），
//! 调用方需要显式处理「这个实体的种族是占位值」这种情况，而不是期待
//! `RaceTable` 为它兜底一份看似合法实则伪造的属性。两者共用同一个
//! `Registry`、同一段 `ContentIndex` 号段，`lostland:placeholder_race`
//! 与 `materialize_base_races` 注册的任何真实种族之间不存在也不可能
//! 存在命名冲突（不同的命名空间路径，`Registry::intern` 天然隔离）。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_sim::character::RaceStatModifierSource;
use ll_sim::traits::{TraitGrant, TraitGrantSource};
use ll_world::entity::BaseStats;
use ll_world::item::ItemStack;
use std::fmt;

/// 单条种族声明：本体与 mod 注册种族时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在种族层面的验收标的——[`materialize_base_races`]
/// 拿这个类型的值去调用外部传入的 `intern` 回调，本体的声明与未来 mod
/// 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceDef {
    /// 命名空间标识符，例如 `lostland:dwarf`、`yourmod:half_elf`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——见模块文档「字段形状」
    /// 一节。
    pub display_name_key: NamespacedId,
    /// 六项主属性的固定增减量，创建角色时一次性叠加，此后与种族脱钩
    /// ——见模块文档「属性修正」一节与 `race-system.md` 二、三两节。
    pub stat_modifiers: BaseStats,
    /// 暗视下限：`effective_light = max(实际光照, darkvision_floor)`，
    /// 见 `race-system.md`「五、暗视」一节——只改变喂给视野半径计算的
    /// 输入，不碰 FOV 算法本身（ADR 0018 归类判据第二步的又一个实例：
    /// 自由度落在算法读的数据上，不在算法本身）。
    pub darkvision_floor: i32,
    /// 占位格数（宽, 高），影响碰撞与寻路——见 `race-system.md`
    /// 「六、体型」一节。当前代码库的碰撞/寻路是否已支持大于 1×1 的
    /// 占位，该文档「十二、待验证项」标注为未核实，本字段只负责声明
    /// 数据本身。
    pub footprint: (u8, u8),
    /// 寿命（年）——只提供数据本身，`race-system.md`「七、寿命」一节
    /// 论证过为什么不需要另外的硬编折扣系数：熟练度边际递减、家族传承
    /// 摩擦、后台推进的随机波动三条既有机制已经自然抑制线性累积。
    pub lifespan_years: u32,
    /// 杀死这个种族/生物种类的实体应授予多少经验
    /// （`knowledge/design/level-and-experience-system.md` 五节）——
    /// 归并键与 `Effect::IncrementKillCount`/`crate::quest` 击杀计数完全
    /// 同一套（`victim.creature_kind.unwrap_or(victim.race)`），见模块
    /// 文档「归并键天然对齐」一节：种族注册表已经存在，用它承载这份
    /// 数据不需要新开一张表。默认 0——大多数种族/生物在 `register-race`
    /// 阶段不显式声明经验值时，杀死它不产出经验，是安全的保守默认
    /// （不会意外让某个未平衡的内容变成刷经验点）。
    pub xp_reward: i64,
    /// 这个种族授予的天赋引用列表（天赋系统落地批次，
    /// `knowledge/design/trait-system.md` 四、六节）——`register-race`
    /// 现有的脚本签名不携带这一项（不能改参数个数,见模块文档「与
    /// `register-race-xp-reward` 的关系」一节同一条先例),真正想给
    /// 种族追加天赋的 mod 作者需要额外调用
    /// [`RaceTable::add_trait_grant`]（脚本入口 `register-race-trait`,
    /// 见 `crate::script_race_api`）。空列表表示这个种族不授予任何
    /// 天赋——与 `unlocked_skills` 空列表表示零解锁同一条纪律,不需要
    /// 一个独立的哨兵值。
    pub traits: Vec<TraitGrant>,
    /// 这个种族/生物出生时随身携带的物品（NPC 生命周期批次：NPC 带
    /// 物品 → 死亡掉落 → 尸体 → 老化回收，本字段是"带物品"这一半的
    /// 落点）——`register-race` 现有的脚本签名不携带这一项（不能改
    /// 参数个数，见模块文档「与 `register-race-xp-reward` 的关系」
    /// 一节同一条先例），真正想给种族声明出生物品的 mod 作者需要额外
    /// 调用 [`RaceTable::add_starting_item`]（脚本入口
    /// `register-race-starting-item`，见 `crate::script_race_api`）。
    /// 空列表表示这个种族出生不带任何物品——与 `traits` 空列表表示
    /// 不授予任何天赋同一条纪律,不需要一个独立的哨兵值。
    ///
    /// 元素是 `(物品定义, 数量)`——只声明"带什么、带多少"这两个不依赖
    /// 任何运行期上下文就能确定的量,不声明耐久：出生装备恒是"全新"
    /// 状态（`ItemStack::new` 的 `durability: None`，见
    /// [`Self::starting_items`] 唯一的消费者
    /// [`starting_inventory`] 的实现),真要支持"某个种族天生带着一把
    /// 半磨损的祖传武器"这类设计,是该场景真正落地时再给这里加字段,
    /// 不在本批次预留（YAGNI，同一条纪律见模块文档「`Owner` 本批次
    /// 仍然不落地」一节）。
    pub starting_items: Vec<(ContentIndex, u32)>,
}

/// [`RaceTable::define`] 实际存进列式存储的属性子集——不含 `id`，理由同
/// [`crate::class::ClassAttrs`]。**必须公开**：这是 `define` 唯一的
/// 参数类型，任何想直接调用 `define`（而不是走
/// [`materialize_base_races`] 那条便捷路径）的调用方——包括未来 mod
/// 自己的种族注册函数——都需要能构造这个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 六项主属性的固定增减量。
    pub stat_modifiers: BaseStats,
    /// 暗视下限。
    pub darkvision_floor: i32,
    /// 占位格数（宽, 高）。
    pub footprint: (u8, u8),
    /// 寿命（年）。
    pub lifespan_years: u32,
    /// 击杀经验值——见 [`RaceDef::xp_reward`] 文档。`register-race`
    /// 现有的脚本签名没有携带这一项（不能改既有函数的参数个数,见模块
    /// 文档「与 `register-race-xp-reward` 的关系」一节），调用方在这里
    /// 恒传 0，真正想声明非零经验值的 mod 作者需要额外调用
    /// `register-race-xp-reward` 补一次。
    pub xp_reward: i64,
    /// 这个种族授予的天赋引用列表——见 [`RaceDef::traits`] 文档，
    /// `register-race` 现有的脚本签名同样不携带这一项，调用方在这里
    /// 恒传空列表，真正想给种族追加天赋的 mod 作者需要额外调用
    /// [`RaceTable::add_trait_grant`]。
    pub traits: Vec<TraitGrant>,
    /// 出生携带的物品列表——见 [`RaceDef::starting_items`] 文档，
    /// `register-race` 现有的脚本签名同样不携带这一项，调用方在这里
    /// 恒传空列表，真正想给种族声明出生物品的 mod 作者需要额外调用
    /// [`RaceTable::add_starting_item`]。
    pub starting_items: Vec<(ContentIndex, u32)>,
}

/// 种族注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来，而不是等到查询某个具体种族时才表现成怪行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::class::ClassError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// [`RaceTable::set_xp_reward`]/[`RaceTable::add_trait_grant`] 的
    /// 目标索引尚未经 [`RaceTable::define`] 定义——与
    /// `register-class-xp-curve` 找不到 `curve-id` 时的报错同一条纪律
    /// （ADR 0017「注册期完整校验」）：经验值/天赋都是种族属性的
    /// 追加声明，追加对象必须先存在。
    NotDefined(ContentIndex),
}

impl fmt::Display for RaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RaceError::DuplicateDefinition(index) => {
                write!(f, "种族索引 {} 被重复定义", index.get())
            }
            RaceError::NotDefined(index) => {
                write!(
                    f,
                    "种族索引 {} 尚未定义，无法追加击杀经验值/天赋引用",
                    index.get()
                )
            }
        }
    }
}

impl std::error::Error for RaceError {}

/// 一次种族查询命中的完整结果，理由同 [`crate::class::ClassView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaceView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 六项主属性的固定增减量。
    pub stat_modifiers: BaseStats,
    /// 暗视下限。
    pub darkvision_floor: i32,
    /// 占位格数（宽, 高）。
    pub footprint: (u8, u8),
    /// 寿命（年）。
    pub lifespan_years: u32,
    /// 击杀经验值，见 [`RaceDef::xp_reward`] 文档。
    pub xp_reward: i64,
    /// 这个种族授予的天赋引用列表，见 [`RaceDef::traits`] 文档。
    pub traits: &'a [TraitGrant],
    /// 出生携带的物品列表（`(物品定义, 数量)`），见
    /// [`RaceDef::starting_items`] 文档。
    pub starting_items: &'a [(ContentIndex, u32)],
}

/// 零修正的基准值——[`RaceTable::define`] 在扩容未定义槽位时使用的
/// 占位，与 `TerrainTable::move_cost` 扩容时填 0 同一个理由：未定义的
/// 槽位永远被 `defined` 位图挡住，不会被外部查询实际读到。
const ZERO_STAT_MODIFIERS: BaseStats = BaseStats {
    strength: 0,
    dexterity: 0,
    constitution: 0,
    intelligence: 0,
    willpower: 0,
    charisma: 0,
    luck: 0,
};

/// 种族属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017），与 [`crate::class::ClassTable`] 同一套道理。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分——地形、职业、技能、
/// 种族共享同一个 `Interner`/`Registry`。因此这里同样维护一份 `defined`
/// 位图，理由同 [`crate::class::ClassTable`] 文档。
#[derive(Debug, Default, Clone)]
pub struct RaceTable {
    display_name_key: Vec<Option<NamespacedId>>,
    stat_modifiers: Vec<BaseStats>,
    darkvision_floor: Vec<i32>,
    footprint: Vec<(u8, u8)>,
    lifespan_years: Vec<u32>,
    xp_reward: Vec<i64>,
    traits: Vec<Vec<TraitGrant>>,
    starting_items: Vec<Vec<(ContentIndex, u32)>>,
    defined: Vec<bool>,
}

impl RaceTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上种族属性。
    ///
    /// 唯一的校验是「不得重复定义」——种族数值字段之间没有互相矛盾的
    /// 组合需要检查（不像地形 `blocks_move`/`move_cost` 那样存在自洽性
    /// 约束）。
    pub fn define(&mut self, index: ContentIndex, attrs: RaceAttrs) -> Result<(), RaceError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.stat_modifiers.resize(new_len, ZERO_STAT_MODIFIERS);
            self.darkvision_floor.resize(new_len, 0);
            self.footprint.resize(new_len, (1, 1));
            self.lifespan_years.resize(new_len, 0);
            self.xp_reward.resize(new_len, 0);
            self.traits.resize(new_len, Vec::new());
            self.starting_items.resize(new_len, Vec::new());
        }

        if self.defined[idx] {
            return Err(RaceError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.stat_modifiers[idx] = attrs.stat_modifiers;
        self.darkvision_floor[idx] = attrs.darkvision_floor;
        self.footprint[idx] = attrs.footprint;
        self.lifespan_years[idx] = attrs.lifespan_years;
        self.xp_reward[idx] = attrs.xp_reward;
        self.traits[idx] = attrs.traits;
        self.starting_items[idx] = attrs.starting_items;
        Ok(())
    }

    /// 给定的种族索引当前是否已经登记过属性。
    pub fn is_defined(&self, race: ContentIndex) -> bool {
        self.defined
            .get(race.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个种族的完整属性，未注册的索引返回 `None`（对齐 ADR 0015
    /// 的解析纪律，同 [`crate::class::ClassTable::get`]）——占位种族
    /// 索引（[`crate::base_placeholder::PLACEHOLDER_RACE_ID`]）正是走
    /// 这条分支，见模块文档「与 `lostland:placeholder_race` 的协调」
    /// 一节。
    pub fn get(&self, race: ContentIndex) -> Option<RaceView<'_>> {
        if !self.is_defined(race) {
            return None;
        }
        let idx = race.get() as usize;
        Some(RaceView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            stat_modifiers: self.stat_modifiers[idx],
            darkvision_floor: self.darkvision_floor[idx],
            footprint: self.footprint[idx],
            lifespan_years: self.lifespan_years[idx],
            xp_reward: self.xp_reward[idx],
            traits: &self.traits[idx],
            starting_items: &self.starting_items[idx],
        })
    }

    /// 追加声明「杀死这个种族应授予多少经验」——`register-race` 的既有
    /// 脚本签名不能改参数个数（模块文档「与 `register-race-xp-reward`
    /// 的关系」一节），因此经验值走这条独立的、注册后追加的路径,与
    /// `register-class-xp-curve`/`register-race-xp-curve`「配置与定义
    /// 分离」同一个模式（`level-and-experience-system.md` 八节）。目标
    /// 索引必须已经 `define` 过，否则返回 [`RaceError::NotDefined`]
    /// （ADR 0017「注册期完整校验」）。
    pub fn set_xp_reward(&mut self, race: ContentIndex, amount: i64) -> Result<(), RaceError> {
        if !self.is_defined(race) {
            return Err(RaceError::NotDefined(race));
        }
        self.xp_reward[race.get() as usize] = amount;
        Ok(())
    }

    /// 追加声明「这个种族授予某个天赋，在什么等级」——`register-race`
    /// 的既有脚本签名同样不能改参数个数（[`RaceDef::traits`] 文档），
    /// 因此天赋引用走这条独立的、注册后追加的路径，与
    /// [`Self::set_xp_reward`] 同一个模式。**追加，不是覆盖**：一个
    /// 种族可以被多次调用授予多条不同的天赋（每次调用 push 一条
    /// `TraitGrant`），这与 `set_xp_reward`「单值覆盖」不同——经验值
    /// 只有一个数,天赋引用天然是一个集合。目标索引必须已经 `define`
    /// 过，否则返回 [`RaceError::NotDefined`]（ADR 0017）。**不校验
    /// `grant.trait_id` 是否已经在 `TraitTable` 里注册过**——与
    /// `crate::skill::do_register_skill` 对 `prerequisites` 的既有处理
    /// 方式一致（只 `intern` 不跨表校验存在性,见其文档,这是当前代码库
    /// 尚未建立跨表校验基础设施的已知简化,不是本次新引入的松懈）。
    pub fn add_trait_grant(
        &mut self,
        race: ContentIndex,
        grant: TraitGrant,
    ) -> Result<(), RaceError> {
        if !self.is_defined(race) {
            return Err(RaceError::NotDefined(race));
        }
        self.traits[race.get() as usize].push(grant);
        Ok(())
    }

    /// 追加声明「这个种族出生携带一件物品」——`register-race` 的既有
    /// 脚本签名同样不能改参数个数（[`RaceDef::starting_items`] 文档），
    /// 因此出生物品走这条独立的、注册后追加的路径，与
    /// [`Self::add_trait_grant`] 同一个模式。**追加，不是覆盖**：一个
    /// 种族可以被多次调用授予多件不同的出生物品（每次调用 push 一条
    /// `(def, count)`），与 `add_trait_grant`「追加」同一条纪律。目标
    /// 索引必须已经 `define` 过，否则返回 [`RaceError::NotDefined`]
    /// （ADR 0017）。**不校验 `def` 是否已经在 `ItemTable` 里注册过**
    /// ——与 [`Self::add_trait_grant`] 对 `grant.trait_id` 的既有处理
    /// 方式一致（只 `intern` 不跨表校验存在性）。
    pub fn add_starting_item(
        &mut self,
        race: ContentIndex,
        def: ContentIndex,
        count: u32,
    ) -> Result<(), RaceError> {
        if !self.is_defined(race) {
            return Err(RaceError::NotDefined(race));
        }
        self.starting_items[race.get() as usize].push((def, count));
        Ok(())
    }
}

/// 把一个种族的出生携带物列表算成可以直接写入
/// [`ll_world::entity::Agent::inventory`] 的 [`ItemStack`] 列表——
/// [`RaceDef::starting_items`] 唯一的消费者，供 NPC/玩家生成流程
/// （例如 `ll_game::world::spawn_player`）在拿到某个实体的种族之后
/// 调用。
///
/// 每一条 `(def, count)` 独立算成一个 `ItemStack::new(def, count)`
/// ——不做任何堆叠合并（同一次出生声明两次同种物品，就会在背包里得到
/// 两条独立的堆，而不是自动合并成一条）：合并需要查 `ItemDef.stack_limit`
/// （`ItemCatalog`），把这层规则塞进一个不持有任何目录引用的纯转换
/// 函数会让签名平白多出一个通常用不到的依赖——mod 作者只要不重复声明
/// 同一种出生物品就不会遇到这个情况，属于内容作者自己该避免的重复
/// 声明，不是引擎需要替其兜底的场景。
pub fn starting_inventory(view: &RaceView<'_>) -> Vec<ItemStack> {
    view.starting_items
        .iter()
        .map(|&(def, count)| ItemStack::new(def, count))
        .collect()
}

/// `ll_sim::traits::TraitGrantSource` 的真实实现——
/// `crate::traits::effective_traits`/`granted_skills` 通过这个 impl
/// 真正查到种族授予的天赋引用,见 `ll_sim::traits` 模块文档「天赋归谁
/// 所有」一节同一套依赖倒置手法。未注册的种族索引返回空列表——与
/// `TraitGrantSource::granted_traits` 文档「查不到就是查不到」的既有
/// 纪律一致,不是 panic 或特殊分支。
impl TraitGrantSource for RaceTable {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        self.get(owner)
            .map(|view| view.traits.to_vec())
            .unwrap_or_default()
    }
}

/// `ll_sim::character::RaceStatModifierSource` 的真实实现——
/// `ll_sim::character::bake_race_stat_modifiers` 通过这个 impl 真正查到
/// 种族声明的六项固定增减量,见 `ll_sim::character` 模块文档「为什么放在
/// `ll-sim`」一节同一套依赖倒置手法。未注册的种族索引返回全零修正——与
/// [`TraitGrantSource::granted_traits`] 文档「查不到就是查不到」的既有
/// 纪律一致,不是 panic 或特殊分支。
impl RaceStatModifierSource for RaceTable {
    fn race_stat_modifiers(&self, race: ContentIndex) -> BaseStats {
        self.get(race)
            .map(|view| view.stat_modifiers)
            .unwrap_or(ZERO_STAT_MODIFIERS)
    }
}

/// `ll_sim::vision::RaceDarkvisionSource` 的真实实现——
/// `ll_sim::vision::effective_light_for_race` 通过这个 impl 真正查到
/// 种族声明的暗视下限，见 `ll_sim::vision` 模块文档「为什么定义在
/// `ll-sim`」一节同一套依赖倒置手法。未注册的种族索引返回 `0`（无
/// 暗视）——与 [`RaceStatModifierSource::race_stat_modifiers`] 文档
/// 「查不到就是查不到」的既有纪律一致，不是 panic 或特殊分支。
impl ll_sim::vision::RaceDarkvisionSource for RaceTable {
    fn darkvision_floor(&self, race: ContentIndex) -> i32 {
        self.get(race)
            .map(|view| view.darkvision_floor)
            .unwrap_or(0)
    }
}

/// 本体基础种族在当前注册表里的索引缓存。
///
/// 只注册占位性质的少数几种基础种族——真正的种族数值平衡与内容设计
/// 不在本任务范围，与 [`crate::class::BaseClassIds`] 同一条纪律。三种
/// 族演示三种不同的修正取向：人类（无修正，`race-system.md` 惯常的
/// 「基准种族」角色）、矮人（体质向，暗视）、精灵（敏捷/智力向）。
#[derive(Debug, Clone, Copy)]
pub struct BaseRaceIds {
    /// 人类：无属性修正，暗视下限为零（无暗视）。
    pub human: ContentIndex,
    /// 矮人：体质 +2、力量 +1，暗视下限较高。
    pub dwarf: ContentIndex,
    /// 精灵：敏捷 +2、智力 +1，寿命远长于人类。
    pub elf: ContentIndex,
}

/// 本体种族注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调，理由同
/// [`crate::class::materialize_base_classes`] 文档。
pub fn materialize_base_races(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseRaceIds, RaceTable), RaceError> {
    let mut table = RaceTable::new();

    let human = define_base(
        &mut table,
        intern,
        "lostland:human",
        "lostland:race.human.display_name",
        ZERO_STAT_MODIFIERS,
        0,
        (1, 1),
        80,
    )?;
    let dwarf = define_base(
        &mut table,
        intern,
        "lostland:dwarf",
        "lostland:race.dwarf.display_name",
        BaseStats {
            constitution: 2,
            strength: 1,
            ..ZERO_STAT_MODIFIERS
        },
        // 暗视下限：取一个明显高于「完全黑暗」（0）又明显低于满光照的
        // 值，具体数值本任务不做平衡设计，只保证字段真的被本体使用到。
        4,
        (1, 1),
        250,
    )?;
    let elf = define_base(
        &mut table,
        intern,
        "lostland:elf",
        "lostland:race.elf.display_name",
        BaseStats {
            dexterity: 2,
            intelligence: 1,
            ..ZERO_STAT_MODIFIERS
        },
        0,
        (1, 1),
        400,
    )?;

    Ok((BaseRaceIds { human, dwarf, elf }, table))
}

/// [`materialize_base_races`] 的内部帮手：把一条声明的字面量字段拆开
/// 传入，换取一次 `intern` + 一次 [`RaceTable::define`]。
#[allow(clippy::too_many_arguments)]
fn define_base(
    table: &mut RaceTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    display_name_key: &str,
    stat_modifiers: BaseStats,
    darkvision_floor: i32,
    footprint: (u8, u8),
    lifespan_years: u32,
) -> Result<ContentIndex, RaceError> {
    let index = intern(NamespacedId::parse(id).expect("本体种族 id 字面量恒合法"));
    table.define(
        index,
        RaceAttrs {
            display_name_key: NamespacedId::parse(display_name_key)
                .expect("本体种族本地化键字面量恒合法"),
            stat_modifiers,
            darkvision_floor,
            footprint,
            lifespan_years,
            // 本体三种基础种族是玩家可选种族，不是设计给「打怪拿经验」
            // 用的内容，击杀经验值留空（0）——真正的怪物内容由 mod 通过
            // `register-race-xp-reward` 追加声明,见 RaceDef::xp_reward
            // 文档。
            xp_reward: 0,
            // 本体三种基础种族当前不预置任何天赋——龙裔吐息/矮人抗毒
            // 这类内容属于内容设计，不在本任务范围（模块文档「本批次
            // 范围」一节），mod 通过 `register-race-trait` 追加声明。
            traits: Vec::new(),
            starting_items: Vec::new(),
        },
    )?;
    Ok(index)
}

/// 供测试使用：现造一个空 [`Interner`]，注册本体全部基础种族，返回
/// 可用的 `(BaseRaceIds, RaceTable)`。不是生产路径，理由同
/// [`crate::class::base_class_fixture`]。
pub fn base_race_fixture() -> (BaseRaceIds, RaceTable) {
    let mut interner = Interner::new();
    materialize_base_races(&mut |id| interner.intern(id))
        .expect("本体种族声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 新建的种族表查询任意索引均为未注册() {
        // Arrange
        let table = RaceTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 人类没有任何属性修正() {
        // Arrange
        let (ids, table) = base_race_fixture();

        // Act
        let view = table.get(ids.human).expect("人类已在本体注册");

        // Assert
        assert_eq!(view.stat_modifiers, ZERO_STAT_MODIFIERS);
        assert_eq!(view.darkvision_floor, 0);
    }

    #[test]
    fn 矮人体质修正为正二且暗视下限大于零() {
        // Arrange
        let (ids, table) = base_race_fixture();

        // Act
        let view = table.get(ids.dwarf).expect("矮人已在本体注册");

        // Assert
        assert_eq!(view.stat_modifiers.constitution, 2);
        assert!(view.darkvision_floor > 0);
    }

    #[test]
    fn 精灵寿命长于人类() {
        // Arrange
        let (ids, table) = base_race_fixture();

        // Act
        let elf_view = table.get(ids.elf).expect("精灵已在本体注册");
        let human_view = table.get(ids.human).expect("人类已在本体注册");

        // Assert
        assert!(elf_view.lifespan_years > human_view.lifespan_years);
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = RaceTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:human").expect("合法"));
        let mut table = RaceTable::new();
        table
            .define(
                index,
                RaceAttrs {
                    display_name_key: NamespacedId::parse("lostland:race.human.display_name")
                        .expect("合法"),
                    stat_modifiers: ZERO_STAT_MODIFIERS,
                    darkvision_floor: 0,
                    footprint: (1, 1),
                    lifespan_years: 80,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            RaceAttrs {
                display_name_key: NamespacedId::parse("lostland:race.human.display_name")
                    .expect("合法"),
                stat_modifiers: ZERO_STAT_MODIFIERS,
                darkvision_floor: 0,
                footprint: (1, 1),
                lifespan_years: 80,
                xp_reward: 0,
                traits: Vec::new(),
                starting_items: Vec::new(),
            },
        );

        // Assert
        assert_eq!(result, Err(RaceError::DuplicateDefinition(index)));
    }

    #[test]
    fn 追加声明经验值后查询结果反映新值() {
        // Arrange
        let (ids, mut table) = base_race_fixture();

        // Act
        table
            .set_xp_reward(ids.dwarf, 25)
            .expect("矮人已经定义,追加声明应当成功");

        // Assert
        assert_eq!(
            table.get(ids.dwarf).expect("矮人已在本体注册").xp_reward,
            25
        );
    }

    #[test]
    fn 对尚未定义的索引追加声明经验值返回notdefined错误() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let mut table = RaceTable::new();

        // Act
        let result = table.set_xp_reward(never_defined, 10);

        // Assert
        assert_eq!(result, Err(RaceError::NotDefined(never_defined)));
    }

    #[test]
    fn 合法出生物品声明追加成功并写入种族表() {
        // Arrange
        let mut interner = Interner::new();
        let race_index = interner.intern(NamespacedId::parse("yourmod:goblin").unwrap());
        let mut table = RaceTable::new();
        table
            .define(
                race_index,
                RaceAttrs {
                    display_name_key: NamespacedId::parse("yourmod:goblin_display_name")
                        .expect("合法"),
                    stat_modifiers: ZERO_STAT_MODIFIERS,
                    darkvision_floor: 0,
                    footprint: (1, 1),
                    lifespan_years: 5,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("先注册种族本体");
        let item_index = interner.intern(NamespacedId::parse("yourmod:crude_dagger").unwrap());

        // Act
        let result = table.add_starting_item(race_index, item_index, 1);

        // Assert
        assert_eq!(result, Ok(()));
        let view = table.get(race_index).expect("刚注册的种族应能查到属性");
        assert_eq!(view.starting_items, &[(item_index, 1)]);
    }

    #[test]
    fn 对尚未定义的索引追加声明出生物品返回notdefined错误() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = interner.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let item_index = interner.intern(NamespacedId::parse("yourmod:crude_dagger").unwrap());
        let mut table = RaceTable::new();

        // Act
        let result = table.add_starting_item(never_defined, item_index, 1);

        // Assert
        assert_eq!(result, Err(RaceError::NotDefined(never_defined)));
    }

    #[test]
    fn 出生物品列表转换成对应数量的物品堆() {
        // starting_inventory 是 RaceDef::starting_items 唯一的消费者
        // ——验证 (def, count) 列表机械转换成同等数量的 ItemStack,不做
        // 任何堆叠合并（见其文档「不做任何堆叠合并」一节）。
        // Arrange
        let mut interner = Interner::new();
        let race_index = interner.intern(NamespacedId::parse("yourmod:goblin").unwrap());
        let mut table = RaceTable::new();
        table
            .define(
                race_index,
                RaceAttrs {
                    display_name_key: NamespacedId::parse("yourmod:goblin_display_name")
                        .expect("合法"),
                    stat_modifiers: ZERO_STAT_MODIFIERS,
                    darkvision_floor: 0,
                    footprint: (1, 1),
                    lifespan_years: 5,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("先注册种族本体");
        let dagger = interner.intern(NamespacedId::parse("yourmod:crude_dagger").unwrap());
        let torch = interner.intern(NamespacedId::parse("yourmod:torch").unwrap());
        table
            .add_starting_item(race_index, dagger, 1)
            .expect("追加出生物品应当成功");
        table
            .add_starting_item(race_index, torch, 2)
            .expect("追加出生物品应当成功");

        // Act
        let view = table.get(race_index).expect("刚注册的种族应能查到属性");
        let inventory = starting_inventory(&view);

        // Assert
        assert_eq!(
            inventory,
            vec![ItemStack::new(dagger, 1), ItemStack::new(torch, 2)]
        );
    }

    #[test]
    fn 本体种族与mod种族调用同一个公开define函数完成注册() {
        // 结构等价断言，理由同 crate::class 模块的等价测试。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径，不能证明
        // mod 脚本调得到这套 API。真正的证据在 crate::pipeline 的
        // 脚本装载测试与 mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (race_ids, mut table) =
            materialize_base_races(&mut |id| registry.intern(id)).expect("本体种族声明表内部一致");
        let mod_id = NamespacedId::parse("yourmod:half_elf").expect("合法标识符");
        let mod_index = registry.intern(mod_id);
        table
            .define(
                mod_index,
                RaceAttrs {
                    display_name_key: NamespacedId::parse("yourmod:half_elf_display_name")
                        .expect("合法"),
                    stat_modifiers: BaseStats {
                        dexterity: 1,
                        ..ZERO_STAT_MODIFIERS
                    },
                    darkvision_floor: 0,
                    footprint: (1, 1),
                    lifespan_years: 150,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("mod 种族与本体种族调用同一个公开 define 函数,理应同样成功");

        // Assert：mod 内容紧接在本体三种种族之后分配到索引，说明两者
        // 共用同一个单调递增的号段。
        assert_eq!(mod_index.get(), race_ids.elf.get() + 1);
        let view = table.get(mod_index).expect("mod 种族已通过 define 登记");
        assert_eq!(view.lifespan_years, 150);
    }

    #[test]
    fn racestatmodifiersource查询矮人返回其体质力量修正() {
        // 直接验收 impl RaceStatModifierSource for RaceTable：真实实现
        // 确实把 stat_modifiers 字段透传给了 ll_sim::character 的依赖
        // 倒置接口，不是一个只挂名字、内部恒返回零的空壳。
        // Arrange
        let (ids, table) = base_race_fixture();

        // Act
        let modifiers = RaceStatModifierSource::race_stat_modifiers(&table, ids.dwarf);

        // Assert
        assert_eq!(modifiers.constitution, 2);
        assert_eq!(modifiers.strength, 1);
    }

    #[test]
    fn racestatmodifiersource查询未注册索引返回全零修正() {
        // 反例：未注册的索引不能返回任何非零的伪造数据。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = RaceTable::new();

        // Act
        let modifiers = RaceStatModifierSource::race_stat_modifiers(&table, never_defined);

        // Assert
        assert_eq!(modifiers, ZERO_STAT_MODIFIERS);
    }

    #[test]
    fn racedarkvisionsource查询矮人返回其暗视下限() {
        // 直接验收 impl RaceDarkvisionSource for RaceTable：真实实现
        // 确实把 darkvision_floor 字段透传给了 ll_sim::vision 的依赖
        // 倒置接口，不是一个只挂名字、内部恒返回零的空壳。
        // Arrange
        let (ids, table) = base_race_fixture();

        // Act
        let floor = ll_sim::vision::RaceDarkvisionSource::darkvision_floor(&table, ids.dwarf);

        // Assert
        assert!(floor > 0);
    }

    #[test]
    fn racedarkvisionsource查询未注册索引返回零() {
        // 反例：未注册的索引不能返回任何非零的伪造暗视下限。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = RaceTable::new();

        // Act
        let floor = ll_sim::vision::RaceDarkvisionSource::darkvision_floor(&table, never_defined);

        // Assert
        assert_eq!(floor, 0);
    }

    #[test]
    fn 与占位种族共用registry时不产生索引冲突且占位索引查询为none() {
        // 直接验收模块文档「与 lostland:placeholder_race 的协调」一节：
        // 两者共用同一个 Registry，互不冲突；占位索引在 RaceTable 里
        // 查不到属性——这是刻意的，不是遗漏。
        // Arrange
        let mut registry = Registry::new();
        let placeholder = crate::base_placeholder::register_base_placeholder_content(&mut registry);

        // Act
        let (race_ids, table) =
            materialize_base_races(&mut |id| registry.intern(id)).expect("本体种族声明表内部一致");

        // Assert：占位索引与三种真实种族的索引互不相同。
        assert_ne!(placeholder, race_ids.human);
        assert_ne!(placeholder, race_ids.dwarf);
        assert_ne!(placeholder, race_ids.elf);
        // 占位索引在种族表里没有对应的 RaceDef——查询诚实返回 None。
        assert_eq!(table.get(placeholder), None);
    }
}
