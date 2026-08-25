//! 内容数据文件（JSON5）的反序列化 schema——**`register-*` 那套脚本
//! API 的字段化重写**。
//!
//! # 为什么内容离开脚本，去的是数据文件而不是 Rust 常量
//!
//! 脚本系统要拆掉：`steel-core 0.8.2` 有一个查不出根因的内存破坏缺陷
//! （[ADR 0028]），完整测试套件被它以 17–33% 的概率打断，六条假说逐条
//! 排除后仍然定位不到根因。但「玩家下载即用的 mod」不该跟着一起丢掉
//! ——项目所有者的裁定原话是「内容用数据文件（JSON5），行为用 Rust」
//! 「这样也能有数据驱动的方式编写」。
//!
//! 实测（`2b4f6dc` 时的仓库，只数非注释非空行）：全部 `.scm` 共 261 行
//! 有效代码，其中 **211 行是纯静态声明**（本体 54 行 + example_mod 157
//! 行），只有 50 行是真逻辑（行为树 38 行 + 事件回调 12 行，两者都已
//! 经搬进引擎，见 `ll_mod::native_behavior`）。声明那一份 JSON5 能原样
//! 表达：玩家直装、零虚拟机、
//! 零沙箱问题；真逻辑那一份走 Rust，需要重编译。这是本类型的主流形态
//! ——矮人要塞的 raws、RimWorld 的 XML defs 都是「声明走数据、逻辑走
//! 本体」。
//!
//! 换句话说，**那台虚拟机在为不到两成的内容承担全部的风险**。
//!
//! [ADR 0028]: ../../../knowledge/decisions/0028-steel-engine-construction-memory-corruption.md
//!
//! # 具名字段顺手消灭了一整类失败模式
//!
//! `register-race` 是 **13 个裸整数位置参数**，上一批专门为它写了一条
//! 「整体错位一格就变红」的测试——因为错位之后每个数字仍然合法、不
//! 报任何错，症状是矮人的寿命变成了暗视格数。**JSON5 字段是具名的，
//! 这个失败模式结构上不可能存在。**
//!
//! 为了不把它换成别的静默失败，本模块的三条硬要求：
//!
//! 1. **未知字段报错**，不是静默忽略——全部 `Raw*` 结构体都带
//!    `#[serde(deny_unknown_fields)]`，拼错字段名会当场变红而不是
//!    「这个字段没生效，但没人知道」。
//! 2. **缺必填字段报错**——没标 `#[serde(default)]` 的字段就是必填，
//!    serde 自带这条。
//! 3. **错误带文件名与位置**——`json5` 1.3.1 会给反序列化错误附上
//!    `Position`（`Display` 输出 `... at line N column M`），
//!    [`crate::content_data`] 再在外面拼上文件路径。这一条有实测钉子，
//!    见本模块测试 `未知字段报错且错误信息带行列位置`。
//!
//! # 两阶段解析：先字符串，后 `ContentIndex`
//!
//! 磁盘上的跨表引用是命名空间字符串（`"lostland:warrior"`），内容表里
//! 是 [`ContentIndex`]。本模块的 `Raw*` 结构体只负责「字符 ↔ 结构」
//! 这一层，全部引用都还是 `String`；把它们 intern／查表换成
//! `ContentIndex` 是 `apply_*` 那一步的事。
//!
//! 这与 [`crate::manifest`] 的 `RawManifest`／[`ModManifest`] 分工逐字
//! 同源，也是 ADR 0015「内容 id 注册是解析，不是不变量」的直接落点：
//! **不能**直接给 `RaceAttrs` 这类内容表结构体派生 `Deserialize`——它
//! 们的字段已经是解析完成后的形态（`ContentIndex`、`NamespacedId`），
//! 让 serde 直接产出它们等于把「解析」这一步偷偷跳过。
//!
//! [`ModManifest`]: crate::manifest::ModManifest
//!
//! # 只 intern 还是必须已定义
//!
//! 逐条沿用脚本 API 当时的语义，一个字都不改——这是内容值哈希逐位
//! 不变的前提之一：
//!
//! | 引用 | 语义 | 理由 |
//! |---|---|---|
//! | 技能 `owning_class` / `prerequisites` | intern | 允许前向引用，成环由装载末尾的整表检查兜底 |
//! | 任务 `prerequisites` / `kill-count` 目标 | intern | 同上；目标类型至今没有注册表（`UntypedIdSpace` 豁免） |
//! | 种族／职业天赋 `id` | intern | 天赋表可以后到 |
//! | 副职获得条件的配方类别 | **必须已定义** | 只 get 不 intern，拼错当场报错 |
//! | 配方类别的 `required_subclasses` | **必须已定义** | 同上 |
//! | 资源池（`pool` / `slot-tier`） | **必须已定义** | 同上 |
//! | 种族出生装备的物品 | **必须已定义** | 同上 |

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::item::WearChannels;
use ll_sim::traits::TraitGrant;
use ll_world::entity::{AttributeKind, BaseStats};
use serde::Deserialize;

use crate::class::{ClassAttrs, ClassTable};
use crate::quest::{QuestAttrs, QuestCondition, QuestTable};
use crate::race::{RaceAttrs, RaceTable};
use crate::recipe_category::RecipeCategoryTable;
use crate::registry::Registry;
use crate::skill::{ResourceCost, ResourceKind, SkillAttrs, SkillEffect, SkillTable};
use crate::subclass::{SubclassAttrs, SubclassTable};
use crate::tag::{TagDef, TagTable};
use crate::xp_curve::{XpCurveBindings, XpCurveTable};

/// `apply_*` 系列的返回类型——错误是一句面向 mod 作者的人话，调用方
/// （[`crate::content_data`]）负责在外面拼上文件名。
pub(crate) type Applied = Result<(), String>;

/// 把一个命名空间字符串解析成 [`NamespacedId`]，失败时报出原文。
pub(crate) fn parse_id(raw: &str, what: &str) -> Result<NamespacedId, String> {
    NamespacedId::parse(raw).map_err(|err| format!("非法{what} {raw:?}：{err}"))
}

/// 解析并 intern：允许前向引用（被引用的内容可以还没定义）。
pub(crate) fn intern_id(
    registry: &mut Registry,
    raw: &str,
    what: &str,
) -> Result<ContentIndex, String> {
    Ok(registry.intern(parse_id(raw, what)?))
}

/// 解析并**要求已注册**：只 get 不 intern，拼错当场报错。
pub(crate) fn required_id(
    registry: &Registry,
    raw: &str,
    what: &str,
) -> Result<ContentIndex, String> {
    let parsed = parse_id(raw, what)?;
    registry
        .get(&parsed)
        .ok_or_else(|| format!("{what} {raw:?} 尚未注册——它必须在引用它的内容之前定义"))
}

/// 六项主属性 + 幸运的名字 → [`AttributeKind`]。取值集合与此前
/// `register-class` / `register-skill` 认的那一份逐字相同。
pub(crate) fn attribute_kind_from_str(name: &str) -> Result<AttributeKind, String> {
    Ok(match name {
        "strength" => AttributeKind::Strength,
        "dexterity" => AttributeKind::Dexterity,
        "constitution" => AttributeKind::Constitution,
        "intelligence" => AttributeKind::Intelligence,
        "willpower" => AttributeKind::Willpower,
        "charisma" => AttributeKind::Charisma,
        "luck" => AttributeKind::Luck,
        other => {
            return Err(format!(
                "未知的属性名 {other:?}（只认 strength/dexterity/constitution/\
                 intelligence/willpower/charisma/luck）"
            ));
        }
    })
}

// ───────────────────────────── 标签 ─────────────────────────────

/// `tags.json5` 的顶层形状。
///
/// 顶层是一个**具名键的对象**而不是裸数组：键名参与
/// `deny_unknown_fields` 检查，于是「文件放对了、键名拼错了」也是一条
/// 当场报出来的错，而不是「解析成功，一条内容都没有」。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagFile {
    /// 标签名册，按文件里的书写顺序注册（数组天然有序，满足 C5）。
    pub tags: Vec<RawTag>,
}

/// 一条标签声明——对应此前的 `(register-tag id wear-channels)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTag {
    /// 完整命名空间标识符。
    pub id: String,
    /// 耐久磨损通道名（`"on-hit"` / `"on-use"`）。空列表表示这个标签
    /// 与耐久无关——纯分类标签，合法且预期常见。
    #[serde(default)]
    pub wear_channels: Vec<String>,
}

/// 把一批标签写进注册表与标签表。
pub fn apply_tags(registry: &mut Registry, table: &mut TagTable, tags: &[RawTag]) -> Applied {
    for tag in tags {
        let index = intern_id(registry, &tag.id, "标签标识符")?;
        let mut wear = WearChannels::NONE;
        for name in &tag.wear_channels {
            let channel = WearChannels::from_name(name).ok_or_else(|| {
                format!("未知的耐久磨损通道名称 {name:?}（只认 \"on-hit\" 与 \"on-use\"）")
            })?;
            wear = wear.union(channel);
        }
        table
            .define(index, TagDef { wear })
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

// ───────────────────────────── 种族 ─────────────────────────────

/// `races.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceFile {
    /// 种族名册，按书写顺序注册。
    pub races: Vec<RawRace>,
}

/// 七项属性的**固定增减量**（可为负），不是千分比。整条缺省即全零
/// ——「无任何修正」是种族设计里惯常的「基准种族」形态，写成
/// `stat_modifiers: {}` 或整条不写都合法。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStatModifiers {
    /// 力量修正。
    #[serde(default)]
    pub strength: i32,
    /// 敏捷修正。
    #[serde(default)]
    pub dexterity: i32,
    /// 体质修正。
    #[serde(default)]
    pub constitution: i32,
    /// 智力修正。
    #[serde(default)]
    pub intelligence: i32,
    /// 意志修正。
    #[serde(default)]
    pub willpower: i32,
    /// 魅力修正。
    #[serde(default)]
    pub charisma: i32,
    /// 幸运修正。
    #[serde(default)]
    pub luck: i32,
}

impl From<&RawStatModifiers> for BaseStats {
    fn from(raw: &RawStatModifiers) -> BaseStats {
        BaseStats {
            strength: raw.strength,
            dexterity: raw.dexterity,
            constitution: raw.constitution,
            intelligence: raw.intelligence,
            willpower: raw.willpower,
            charisma: raw.charisma,
            luck: raw.luck,
        }
    }
}

/// 占地格数，缺省 1×1。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawFootprint {
    /// 占地宽度（格）。
    pub width: u8,
    /// 占地高度（格）。
    pub height: u8,
}

impl Default for RawFootprint {
    fn default() -> Self {
        RawFootprint {
            width: 1,
            height: 1,
        }
    }
}

/// 一条天赋授予——对应此前的
/// `(register-race-trait / register-class-trait 拥有者 天赋 解锁等级)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTraitGrant {
    /// 天赋的完整命名空间标识符（intern，允许天赋表后到）。
    pub id: String,
    /// 解锁等级，不允许为负。
    pub unlock_level: i32,
}

impl RawTraitGrant {
    fn resolve(&self, registry: &mut Registry) -> Result<TraitGrant, String> {
        if self.unlock_level < 0 {
            return Err(format!("解锁等级不允许为负数：{}", self.unlock_level));
        }
        Ok(TraitGrant {
            trait_id: intern_id(registry, &self.id, "天赋标识符")?,
            unlock_level: self.unlock_level,
        })
    }
}

/// 一件出生装备——对应此前的
/// `(register-race-starting-item 种族 物品 数量)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStartingItem {
    /// 物品的完整命名空间标识符。**必须已注册**——拼错的症状是「这个
    /// 种族静默少一件出生装备」，正是最难查的那一类内容缺陷。
    pub id: String,
    /// 件数。
    pub count: u32,
}

/// 一条种族声明——对应此前 `register-race` 的十三个位置参数外加
/// `register-race-xp-reward` / `-trait` / `-starting-item` 三条追加指令。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRace {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 七项属性的固定增减量，缺省全零。
    #[serde(default)]
    pub stat_modifiers: RawStatModifiers,
    /// **夜间视野格数下限**：0 表示未声明（按常人处理，落回默认的
    /// 4 格），非 0 就是这个种族夜里实际看得见的格数（白天基准 12
    /// 格）。见 `ll_world::light::sight_radius_at`。
    #[serde(default)]
    pub darkvision_cells: u32,
    /// 占地，缺省 1×1。
    #[serde(default)]
    pub footprint: RawFootprint,
    /// 自然寿命（年）。
    pub lifespan_years: u32,
    /// 击杀基准经验值——`ll_sim::experience::kill_experience` 的公式
    /// 输入，不是玩家最终拿到的数字。缺省 0。
    #[serde(default)]
    pub xp_reward: i64,
    /// 种族天赋，缺省无。
    #[serde(default)]
    pub traits: Vec<RawTraitGrant>,
    /// 出生装备，缺省无。
    #[serde(default)]
    pub starting_items: Vec<RawStartingItem>,
    /// 这个种族用哪条经验曲线，**必须已注册**；整条不写表示落回默认
    /// 曲线。对应此前的 `(register-race-xp-curve 种族 曲线)`——那条
    /// 追加指令要把种族 id 重复写一遍，字段则不会。
    #[serde(default)]
    pub xp_curve: Option<String>,
}

/// 把一批种族写进注册表与种族表。
pub fn apply_races(
    registry: &mut Registry,
    table: &mut RaceTable,
    curves: &XpCurveTable,
    bindings: &mut XpCurveBindings,
    races: &[RawRace],
) -> Applied {
    for race in races {
        if race.xp_reward < 0 {
            return Err(format!(
                "种族 {:?} 的击杀经验值不允许为负数：{}",
                race.id, race.xp_reward
            ));
        }
        let index = intern_id(registry, &race.id, "种族标识符")?;
        let display_name_key = parse_id(&race.display_name_key, "本地化键标识符")?;

        let mut traits = Vec::with_capacity(race.traits.len());
        for grant in &race.traits {
            traits.push(grant.resolve(registry)?);
        }
        let mut starting_items = Vec::with_capacity(race.starting_items.len());
        for item in &race.starting_items {
            starting_items.push((required_id(registry, &item.id, "物品")?, item.count));
        }

        table
            .define(
                index,
                RaceAttrs {
                    display_name_key,
                    stat_modifiers: BaseStats::from(&race.stat_modifiers),
                    darkvision_cells: race.darkvision_cells,
                    footprint: (race.footprint.width, race.footprint.height),
                    lifespan_years: race.lifespan_years,
                    xp_reward: race.xp_reward,
                    traits,
                    starting_items,
                },
            )
            .map_err(|err| err.to_string())?;

        if let Some(raw) = &race.xp_curve {
            let curve = required_id(registry, raw, "经验曲线")?;
            bindings
                .bind_race(curves, index, curve)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

// ───────────────────────────── 职业 ─────────────────────────────

/// `classes.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassFile {
    /// 职业名册，按书写顺序注册。
    pub classes: Vec<RawClass>,
}

/// 一条职业声明——对应此前的 `(register-class id 显示名键 主属性)`
/// 外加 `register-class-trait`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClass {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 主属性倾向，七选一。
    pub primary_attribute: String,
    /// 职业天赋，缺省无。
    #[serde(default)]
    pub traits: Vec<RawTraitGrant>,
    /// 这个职业用哪条经验曲线，**必须已注册**；整条不写表示落回默认
    /// 曲线。理由同 [`RawRace::xp_curve`]。
    #[serde(default)]
    pub xp_curve: Option<String>,
}

/// 把一批职业写进注册表与职业表。
pub fn apply_classes(
    registry: &mut Registry,
    table: &mut ClassTable,
    curves: &XpCurveTable,
    bindings: &mut XpCurveBindings,
    classes: &[RawClass],
) -> Applied {
    for class in classes {
        let index = intern_id(registry, &class.id, "职业标识符")?;
        let display_name_key = parse_id(&class.display_name_key, "本地化键标识符")?;
        let primary_attribute = attribute_kind_from_str(&class.primary_attribute)?;
        let mut traits = Vec::with_capacity(class.traits.len());
        for grant in &class.traits {
            traits.push(grant.resolve(registry)?);
        }
        table
            .define(
                index,
                ClassAttrs {
                    display_name_key,
                    primary_attribute,
                    traits,
                },
            )
            .map_err(|err| err.to_string())?;

        if let Some(raw) = &class.xp_curve {
            let curve = required_id(registry, raw, "经验曲线")?;
            bindings
                .bind_class(curves, index, curve)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

// ───────────────────────────── 技能 ─────────────────────────────

/// `skills.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFile {
    /// 技能名册，按书写顺序注册。
    pub skills: Vec<RawSkill>,
}

/// 施放代价。
///
/// # 为什么是「`kind` + 可选字段 + 手写校验」，不是 serde 的内部标签枚举
///
/// `#[serde(tag = "kind")]` 读起来最漂亮，但它与
/// `#[serde(deny_unknown_fields)]` 不能共存（serde 已知限制：内部标签
/// 枚举会把 `kind` 自己当成未知字段）——而未知字段必须报错是本模块的
/// 硬要求，它优先。手写校验因此不是偷懒，是拿回了一条 serde 给不了的
/// 检查：**填了与 `kind` 不搭的字段也报错**（`kind: "none"` 却写了
/// `amount` 会当场变红，而不是静默忽略）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawResourceCost {
    /// `"none"` / `"mana"` / `"stamina"` / `"blood"` / `"pool"` /
    /// `"slot-tier"`。
    pub kind: String,
    /// `mana`/`stamina`/`blood`/`pool` 的消耗量。
    #[serde(default)]
    pub amount: Option<u32>,
    /// `pool`/`slot-tier` 指向的标量资源池 id，**必须已注册**。
    #[serde(default)]
    pub pool: Option<String>,
    /// `slot-tier` 要求的最低法术位阶。
    #[serde(default)]
    pub min_tier: Option<u8>,
}

impl RawResourceCost {
    fn resolve(&self, registry: &Registry) -> Result<ResourceCost, String> {
        // 与 `kind` 不搭的字段一律报错——这条检查替代了 serde 内部标签
        // 枚举本来会给的那一半，见结构体文档。
        let reject = |present: bool, field: &str| -> Result<(), String> {
            if present {
                Err(format!(
                    "施放代价 kind {:?} 不接受字段 {field:?}",
                    self.kind
                ))
            } else {
                Ok(())
            }
        };
        let amount = |field: &str| -> Result<u32, String> {
            self.amount
                .ok_or_else(|| format!("施放代价 kind {:?} 缺少必填字段 {field:?}", self.kind))
        };
        let pool = |registry: &Registry| -> Result<ContentIndex, String> {
            let raw = self
                .pool
                .as_deref()
                .ok_or_else(|| format!("施放代价 kind {:?} 缺少必填字段 \"pool\"", self.kind))?;
            required_id(registry, raw, "资源池")
        };

        match self.kind.as_str() {
            "none" => {
                reject(self.amount.is_some(), "amount")?;
                reject(self.pool.is_some(), "pool")?;
                reject(self.min_tier.is_some(), "min_tier")?;
                Ok(ResourceCost::None)
            }
            kind @ ("mana" | "stamina") => {
                reject(self.pool.is_some(), "pool")?;
                reject(self.min_tier.is_some(), "min_tier")?;
                let resource = if kind == "mana" {
                    ResourceKind::Mana
                } else {
                    ResourceKind::Stamina
                };
                Ok(ResourceCost::Amount(resource, amount("amount")?))
            }
            "blood" => {
                reject(self.pool.is_some(), "pool")?;
                reject(self.min_tier.is_some(), "min_tier")?;
                Ok(ResourceCost::Blood(amount("amount")?))
            }
            "pool" => {
                reject(self.min_tier.is_some(), "min_tier")?;
                Ok(ResourceCost::PoolAmount(pool(registry)?, amount("amount")?))
            }
            "slot-tier" => {
                reject(self.amount.is_some(), "amount")?;
                let min_tier = self.min_tier.ok_or_else(|| {
                    format!("施放代价 kind {:?} 缺少必填字段 \"min_tier\"", self.kind)
                })?;
                Ok(ResourceCost::SlotTier(pool(registry)?, min_tier))
            }
            other => Err(format!(
                "未知的施放代价 kind {other:?}（只认 none/mana/stamina/blood/pool/slot-tier）"
            )),
        }
    }
}

/// 技能效果——形状与理由同 [`RawResourceCost`]。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkillEffect {
    /// `"deal-damage"` / `"restore-resource"` / `"temporary-stat-modifier"`。
    pub kind: String,
    /// 三种 kind 都用它：伤害量／恢复量／属性增减量。
    pub amount: i32,
    /// `restore-resource` 恢复哪种资源（`"mana"` / `"stamina"`）。
    #[serde(default)]
    pub resource: Option<String>,
    /// `temporary-stat-modifier` 改哪一项属性。
    #[serde(default)]
    pub attribute: Option<String>,
    /// `temporary-stat-modifier` 的持续 tick 数。
    #[serde(default)]
    pub duration_ticks: Option<u32>,
}

impl RawSkillEffect {
    pub(crate) fn resolve(&self) -> Result<SkillEffect, String> {
        let reject = |present: bool, field: &str| -> Result<(), String> {
            if present {
                Err(format!(
                    "技能效果 kind {:?} 不接受字段 {field:?}",
                    self.kind
                ))
            } else {
                Ok(())
            }
        };
        match self.kind.as_str() {
            "deal-damage" => {
                reject(self.resource.is_some(), "resource")?;
                reject(self.attribute.is_some(), "attribute")?;
                reject(self.duration_ticks.is_some(), "duration_ticks")?;
                Ok(SkillEffect::DealDamage { base: self.amount })
            }
            "restore-resource" => {
                reject(self.attribute.is_some(), "attribute")?;
                reject(self.duration_ticks.is_some(), "duration_ticks")?;
                let raw = self.resource.as_deref().ok_or_else(|| {
                    format!("技能效果 kind {:?} 缺少必填字段 \"resource\"", self.kind)
                })?;
                let resource = match raw {
                    "mana" => ResourceKind::Mana,
                    "stamina" => ResourceKind::Stamina,
                    other => {
                        return Err(format!(
                            "未知的资源种类 {other:?}（只认 \"mana\" 与 \"stamina\"）"
                        ));
                    }
                };
                Ok(SkillEffect::RestoreResource {
                    resource,
                    base: self.amount,
                })
            }
            "temporary-stat-modifier" => {
                reject(self.resource.is_some(), "resource")?;
                let raw = self.attribute.as_deref().ok_or_else(|| {
                    format!("技能效果 kind {:?} 缺少必填字段 \"attribute\"", self.kind)
                })?;
                let duration_ticks = self.duration_ticks.ok_or_else(|| {
                    format!(
                        "技能效果 kind {:?} 缺少必填字段 \"duration_ticks\"",
                        self.kind
                    )
                })?;
                Ok(SkillEffect::TemporaryStatModifier {
                    attribute: attribute_kind_from_str(raw)?,
                    amount: self.amount,
                    duration_ticks,
                })
            }
            other => Err(format!(
                "未知的技能效果 kind {other:?}（只认 deal-damage/restore-resource/\
                 temporary-stat-modifier）"
            )),
        }
    }
}

/// 一条技能声明——对应此前 `register-skill` 的十个位置参数。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkill {
    /// 完整命名空间标识符。
    pub id: String,
    /// 所属职业的完整标识符；整条不写表示**通用技能**（不专属任何
    /// 职业）。此前脚本里用空串 `""` 表达同一件事——空串是个容易与
    /// 「忘了填」混淆的哨兵值，数据文件里换成「字段缺席」。
    #[serde(default)]
    pub owning_class: Option<String>,
    /// 前置技能的完整标识符，缺省无前置。
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// 冷却（tick），缺省 0。
    #[serde(default)]
    pub cooldown_ticks: u32,
    /// 施放代价。
    pub resource_cost: RawResourceCost,
    /// 效果。
    pub effect: RawSkillEffect,
}

/// 把一批技能写进注册表与技能表。
///
/// 环检查**不在这里跑**：前置成环是整张表的性质，不是「某一个 mod 那
/// 几条」的性质，它挂在 `ll_game::content::load_content` 上（全部 mod
/// 装载完毕之后跑一次）。
pub fn apply_skills(
    registry: &mut Registry,
    table: &mut SkillTable,
    skills: &[RawSkill],
) -> Applied {
    for skill in skills {
        let index = intern_id(registry, &skill.id, "技能标识符")?;
        let owning_class = match skill.owning_class.as_deref() {
            None => None,
            Some(raw) => Some(intern_id(registry, raw, "owning_class 标识符")?),
        };
        let mut prerequisites = Vec::with_capacity(skill.prerequisites.len());
        for raw in &skill.prerequisites {
            prerequisites.push(intern_id(registry, raw, "前置技能标识符")?);
        }
        let resource_cost = skill.resource_cost.resolve(registry)?;
        let effect = skill.effect.resolve()?;
        table
            .define(
                index,
                SkillAttrs {
                    owning_class,
                    prerequisites,
                    cooldown_ticks: skill.cooldown_ticks,
                    resource_cost,
                    effect,
                },
            )
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

// ───────────────────────────── 任务 ─────────────────────────────

/// `quests.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestFile {
    /// 任务名册，按书写顺序注册。
    pub quests: Vec<RawQuest>,
}

/// 任务完成条件——形状与理由同 [`RawResourceCost`]。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawQuestCondition {
    /// `"kill-count"` / `"script"`。
    pub kind: String,
    /// `kill-count`：目标敌人类型的标识符（**开放标识符空间**，代码库
    /// 至今没有敌人类型注册表，因此只 intern 不要求已定义）。
    /// `script`：脚本回调标识符。
    pub target: String,
    /// `kill-count` 的击杀数量。
    #[serde(default)]
    pub count: Option<u32>,
}

impl RawQuestCondition {
    fn resolve(&self, registry: &mut Registry) -> Result<QuestCondition, String> {
        match self.kind.as_str() {
            "kill-count" => {
                let count = self.count.ok_or_else(|| {
                    format!("任务完成条件 kind {:?} 缺少必填字段 \"count\"", self.kind)
                })?;
                Ok(QuestCondition::KillCount {
                    target_kind: intern_id(registry, &self.target, "目标类型标识符")?,
                    count,
                })
            }
            "script" => {
                if self.count.is_some() {
                    return Err(format!(
                        "任务完成条件 kind {:?} 不接受字段 \"count\"",
                        self.kind
                    ));
                }
                Ok(QuestCondition::Script(parse_id(
                    &self.target,
                    "脚本回调标识符",
                )?))
            }
            other => Err(format!(
                "未知的任务完成条件 kind {other:?}（只认 kill-count 与 script）"
            )),
        }
    }
}

/// 一条任务声明——对应此前 `register-quest` 的五个位置参数。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawQuest {
    /// 完整命名空间标识符。
    pub id: String,
    /// 前置任务的完整标识符，缺省无前置。
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// 完成条件。
    pub condition: RawQuestCondition,
}

/// 把一批任务写进注册表与任务表。环检查不在这里跑，理由同
/// [`apply_skills`]。
pub fn apply_quests(
    registry: &mut Registry,
    table: &mut QuestTable,
    quests: &[RawQuest],
) -> Applied {
    for quest in quests {
        let index = intern_id(registry, &quest.id, "任务标识符")?;
        let mut prerequisites = Vec::with_capacity(quest.prerequisites.len());
        for raw in &quest.prerequisites {
            prerequisites.push(intern_id(registry, raw, "前置任务标识符")?);
        }
        let condition = quest.condition.resolve(registry)?;
        table
            .define(
                index,
                QuestAttrs {
                    prerequisites,
                    condition,
                },
            )
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

// ─────────────────────────── 配方类别 ───────────────────────────

/// `crafting.json5` 的顶层形状——配方类别与配方两张名册。
///
/// 本体至今只写了类别（配方要引用**物品**，而本体的物品内容一条都
/// 没有，见 `mods/lostland/crafting.json5` 文件头），`recipes` 因此
/// 带 `#[serde(default)]`；示例 mod 两张都写。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CraftingFile {
    /// 配方类别名册，按书写顺序注册。
    pub recipe_categories: Vec<RawRecipeCategory>,
    /// 配方名册，按书写顺序注册，缺省无。
    #[serde(default)]
    pub recipes: Vec<crate::content_schema_gear::RawRecipe>,
}

/// 一条配方类别声明——对应此前的
/// `(register-recipe-category id 显示名键)` 外加
/// `recipe-category-requires-subclass!`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRecipeCategory {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 副职闸门：列在这里的副职**必须已注册**，缺省为空（人人可做）。
    #[serde(default)]
    pub required_subclasses: Vec<String>,
}

/// 把一批配方类别**定义**写进注册表与配方类别表。
///
/// `required_subclasses` 那一半**不在这里**——见
/// [`apply_recipe_category_subclass_gates`]。
pub fn apply_recipe_categories(
    registry: &mut Registry,
    table: &mut RecipeCategoryTable,
    categories: &[RawRecipeCategory],
) -> Applied {
    for category in categories {
        let display_name_key = parse_id(&category.display_name_key, "本地化键标识符")?;
        let index = intern_id(registry, &category.id, "配方类别标识符")?;
        table
            .define(index, display_name_key)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 把配方类别的**副职闸门**写进配方类别表——`crafting.json5` 的第二
/// 遍应用。
///
/// # 为什么要分两遍
///
/// 配方类别与副职**互相引用**，且两边都是「只 get 不 intern」：
///
/// - 副职的获得条件指向一个配方类别（`RawSubclassUnlock::target`）；
/// - 配方类别的闸门指向一个副职（[`RawRecipeCategory::required_subclasses`]）。
///
/// 这是内容里真实存在的一处环，不是文件顺序没排好——任何单一的文件
/// 先后顺序都满足不了它。脚本时代靠内容作者把四条 `register-*` 手工
/// 交错着写来打破；现在由引擎侧固定的两遍应用打破，mod 作者写不错、
/// 也改不动（顺序表在 [`crate::content_data`]）。
///
/// 两遍都读同一个文件。多解析一次几十行 JSON5 的代价，换掉的是一条
/// 「作者必须知道要把哪四条声明交错着写」的隐形约束。
pub fn apply_recipe_category_subclass_gates(
    registry: &Registry,
    table: &mut RecipeCategoryTable,
    categories: &[RawRecipeCategory],
) -> Applied {
    for category in categories {
        if category.required_subclasses.is_empty() {
            continue;
        }
        let index = required_id(registry, &category.id, "配方类别")?;
        for raw in &category.required_subclasses {
            let subclass = required_id(registry, raw, "副职")?;
            table
                .add_required_subclass(index, subclass)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

// ───────────────────────────── 副职 ─────────────────────────────

/// `subclasses.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubclassFile {
    /// 副职名册，按书写顺序注册。
    pub subclasses: Vec<RawSubclass>,
}

/// 副职获得条件——对应此前的
/// `(register-subclass-unlock 副职 触发器 目标 阈值)`。
///
/// `kind` 目前只接受 `"items-crafted"`，`target` 是一个**已经注册过**
/// 的配方类别 id。传别的 kind 会当场报错并列出支持的取值，不会被静默
/// 当成制作。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSubclassUnlock {
    /// 触发器种类。
    pub kind: String,
    /// 触发目标（配方类别 id）。
    pub target: String,
    /// 阈值（累计件数）。
    pub threshold: u32,
}

/// 一条副职声明——对应此前的 `(register-subclass id 显示名键)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSubclass {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 获得条件。整条不写表示**这个副职目前拿不到**——这是一个合法且
    /// 有意的状态（宁可让「拿不到」是一条写下来、可查的缺口，也不要一条
    /// 语义不对的获得条件），见 `mods/lostland/subclasses.json5` 文件头。
    #[serde(default)]
    pub unlock: Option<RawSubclassUnlock>,
    /// 副职天赋，缺省无——`SubclassDef.traits`，与 [`RawClass::traits`]/
    /// [`RawRace::traits`] 共用同一个 [`RawTraitGrant`] 形状与同一段
    /// 解析。天赋表可以后到（`resolve` 走 `intern_id`），与职业那一路
    /// 完全一致：`CONTENT_FILES` 里 `Subclasses` 排在 `Traits` 之前。
    ///
    /// **与 `unlock` 并存、互不相干**：`unlock` 回答「怎么拿到这个副
    /// 职」，`traits` 回答「拿到之后给什么」。两条都可以单独不写。
    #[serde(default)]
    pub traits: Vec<RawTraitGrant>,
}

/// 副职获得条件当前唯一支持的触发器种类。
const TRIGGER_ITEMS_CRAFTED: &str = "items-crafted";

/// 把一批副职写进注册表与副职表。
///
/// **必须排在 [`apply_recipe_categories`] 之后**：获得条件的 `target`
/// 只 get 不 intern。这条顺序由 [`crate::content_data`] 里那张固定的
/// 文件表钉死。
pub fn apply_subclasses(
    registry: &mut Registry,
    table: &mut SubclassTable,
    subclasses: &[RawSubclass],
) -> Applied {
    for subclass in subclasses {
        let index = intern_id(registry, &subclass.id, "副职标识符")?;
        let display_name_key = parse_id(&subclass.display_name_key, "本地化键标识符")?;
        let mut traits = Vec::with_capacity(subclass.traits.len());
        for grant in &subclass.traits {
            traits.push(grant.resolve(registry)?);
        }
        table
            .define(
                index,
                SubclassAttrs {
                    display_name_key,
                    traits,
                },
            )
            .map_err(|err| err.to_string())?;

        let Some(unlock) = &subclass.unlock else {
            continue;
        };
        if unlock.kind != TRIGGER_ITEMS_CRAFTED {
            return Err(format!(
                "未知的副职获得条件触发器 {:?}，当前支持的取值只有 {TRIGGER_ITEMS_CRAFTED:?}",
                unlock.kind
            ));
        }
        let category_id = parse_id(&unlock.target, "配方类别标识符")?;
        let category_index = registry.get(&category_id).ok_or_else(|| {
            format!(
                "配方类别 {:?} 尚未注册——它必须在引用它的副职之前定义",
                unlock.target
            )
        })?;
        table
            .set_craft_unlock(index, category_index, category_id, unlock.threshold)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未知字段报错且错误信息带行列位置() {
        // 这是任务书三条错误质量要求里最容易空口宣称的一条：json5
        // 1.3.1 会不会给 **serde 层**的错误（未知字段是 serde 报的，
        // 不是词法分析报的）附上位置？本测试是那个问题的实测答案。
        // Arrange
        let source = "{\n  tags: [\n    { id: \"lostland:armor\", wear_channelz: [] },\n  ],\n}";

        // Act
        let error = json5::from_str::<TagFile>(source).expect_err("未知字段必须报错");
        let text = error.to_string();

        // Assert
        assert!(
            text.contains("wear_channelz"),
            "错误应当点名那个字段：{text}"
        );
        assert!(text.contains("line"), "错误应当带行号：{text}");
        assert!(text.contains("column"), "错误应当带列号：{text}");
    }

    #[test]
    fn 缺必填字段报错() {
        // Arrange：种族缺 lifespan_years。
        let source = "{ races: [ { id: \"m:a\", display_name_key: \"m:a.name\" } ] }";

        // Act
        let error = json5::from_str::<RaceFile>(source).expect_err("缺必填字段必须报错");

        // Assert
        assert!(
            error.to_string().contains("lifespan_years"),
            "错误应当点名缺的那个字段：{error}"
        );
    }

    #[test]
    fn 填了与kind不搭的字段报错而不是静默忽略() {
        // 这条检查替代了 serde 内部标签枚举给不了的那一半，见
        // RawResourceCost 文档。
        // Arrange
        let cost = RawResourceCost {
            kind: "none".to_string(),
            amount: Some(10),
            pool: None,
            min_tier: None,
        };

        // Act
        let result = cost.resolve(&Registry::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("amount")));
    }

    #[test]
    fn 只get不intern的引用拼错时当场报错() {
        // Arrange：副职的获得条件指向一个谁都没注册过的配方类别。
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();
        let subclasses = [RawSubclass {
            id: "m:artisan".to_string(),
            display_name_key: "m:artisan.name".to_string(),
            unlock: Some(RawSubclassUnlock {
                kind: "items-crafted".to_string(),
                target: "m:frogingg".to_string(),
                threshold: 20,
            }),
            traits: Vec::new(),
        }];

        // Act
        let result = apply_subclasses(&mut registry, &mut table, &subclasses);

        // Assert
        assert!(result.is_err_and(|err| err.contains("m:frogingg")));
    }

    #[test]
    fn 副职的traits字段被解析进副职表且天赋表可以后到() {
        // 副职天赋接线批次：`traits` 与 `unlock` 并存、互不相干,
        // 且天赋 id 走 `intern_id`（`CONTENT_FILES` 里 Subclasses 排在
        // Traits 之前，天赋表此刻还是空的）。
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();
        let subclasses = [RawSubclass {
            id: "m:shadowdancer".to_string(),
            display_name_key: "m:shadowdancer.name".to_string(),
            unlock: None,
            traits: vec![RawTraitGrant {
                id: "m:shadow_dance".to_string(),
                unlock_level: 1,
            }],
        }];

        // Act
        apply_subclasses(&mut registry, &mut table, &subclasses).expect("解析应当成功");

        // Assert
        let index = registry
            .get(&NamespacedId::parse("m:shadowdancer").expect("合法"))
            .expect("副职应当已注册");
        let shadow_dance = registry
            .get(&NamespacedId::parse("m:shadow_dance").expect("合法"))
            .expect("天赋 id 应当被 intern 出来");
        let view = table.get(index).expect("已定义");
        assert_eq!(
            view.traits,
            &[ll_sim::traits::TraitGrant {
                trait_id: shadow_dance,
                unlock_level: 1,
            }]
        );
    }

    #[test]
    fn 副职天赋的解锁等级为负时报错() {
        // 与职业/种族那两路共用同一段 `RawTraitGrant::resolve` 校验——
        // 本条守的是「副职这一路真的走了那段校验」，不是又抄了一份。
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();
        let subclasses = [RawSubclass {
            id: "m:shadowdancer".to_string(),
            display_name_key: "m:shadowdancer.name".to_string(),
            unlock: None,
            traits: vec![RawTraitGrant {
                id: "m:shadow_dance".to_string(),
                unlock_level: -1,
            }],
        }];

        // Act
        let result = apply_subclasses(&mut registry, &mut table, &subclasses);

        // Assert
        assert!(result.is_err_and(|err| err.contains("-1")));
    }

    #[test]
    fn 技能的owning_class缺席表示通用技能() {
        // 此前脚本里用空串表达「通用技能」，数据文件换成字段缺席——
        // 本测试钉住换法没有把语义弄丢。
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();
        let skills = [RawSkill {
            id: "m:focus".to_string(),
            owning_class: None,
            prerequisites: Vec::new(),
            cooldown_ticks: 10,
            resource_cost: RawResourceCost {
                kind: "none".to_string(),
                amount: None,
                pool: None,
                min_tier: None,
            },
            effect: RawSkillEffect {
                kind: "restore-resource".to_string(),
                amount: 8,
                resource: Some("mana".to_string()),
                attribute: None,
                duration_ticks: None,
            },
        }];

        // Act
        apply_skills(&mut registry, &mut table, &skills).expect("合法声明应当注册成功");

        // Assert
        let index = registry
            .get(&NamespacedId::parse("m:focus").expect("合法标识符"))
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).expect("已注册").owning_class, None);
    }
}
