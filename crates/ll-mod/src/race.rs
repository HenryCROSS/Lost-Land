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
use ll_world::entity::BaseStats;
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
}

/// 种族注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来，而不是等到查询某个具体种族时才表现成怪行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::class::ClassError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// [`RaceTable::set_xp_reward`] 的目标索引尚未经 [`RaceTable::define`]
    /// 定义——与 `register-class-xp-curve` 找不到 `curve-id` 时的报错
    /// 同一条纪律（ADR 0017「注册期完整校验」）：经验值是种族属性的
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
                write!(f, "种族索引 {} 尚未定义，无法追加击杀经验值", index.get())
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
