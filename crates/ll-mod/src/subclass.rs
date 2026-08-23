//! 副职注册表——「本体即 Mod」在副职系统上的落点（P5-B 任务 4）。
//!
//! # 与 `ClassTable`/`SkillTable` 同一套模式
//!
//! 见 [`crate::class`] 模块文档「照抄 `terrain.rs`/`space_profile.rs`
//! 已验证的模式」一节——副职的定义/存储/查询走完全相同的思路：私有
//! 字段 + `SubclassTable::define` 注册期校验。
//!
//! # 本体两个基础副职的定义已经搬进 `mods/lostland/subclasses.scm`
//!
//! 本模块此前还有一对 `materialize_base_subclasses`/`base_subclass_fixture`
//! ——与 [`crate::class`] 的那一对处境完全相同（都不在生产装载路径上，
//! 见其模块文档同名一节），一并删除，剑舞者/学徒两条改由脚本注册。
//! 留下来的是 [`BaseSubclassIds`] 与 [`resolve_base_subclasses`]。
//!
//! # 裁定 P5-4：副职与主职共享技能命名空间——为什么 `SubclassDef` 本身
//! 不需要携带任何命名空间字段
//!
//! `knowledge/design/class-skill-quest-system.md` 第三节已经把这条裁定
//! 定为正式设计：**主职与副职共享同一份技能 `ContentIndex` 命名空间**。
//! 技能就是技能；「谁能学」（主职决定的可学习集合、副职决定的可学习
//! 集合，或某个技能干脆是通用技能）是另一道闸，不是命名空间该管的事。
//!
//! **共享的理由**（完整论证见设计文档第三节，这里摘要复述，供只看
//! 代码不看设计文档的读者也能理解这条判断）：
//!
//! 1. 若不共享，同一个技能被两个职业共同拥有时（例如「基础格挡」这类
//!    几乎所有近战职业都该有的技能），要么复制成两份定义——内容漂移
//!    风险（两份定义迟早在数值上不同步），要么发明一套跨命名空间的
//!    「技能引用」机制——复杂度只是从命名空间转移到了引用层，没有真正
//!    省掉。
//! 2. 不共享还会导致 mod 无法让副职复用主职技能——一个 mod 想设计
//!    「盗贼副职可以使用战士主职的部分技能作为副职技能树的一部分」这类
//!    玩法，若命名空间隔离，mod 完全没有公开 API 能表达这种复用，只能
//!    被迫复制一份技能定义。
//! 3. 共享命名空间后，`SkillDef.owning_class: Option<ContentIndex>`
//!    本身已经足够表达「这个技能主要属于哪个职业」（展示/分类用途），
//!    而「某个实体的主职/副职是否能学这个技能」是运行期判定（比对
//!    `Agent.profession`/`Agent.subclasses` 与技能的 `owning_class`，
//!    或者技能本身是 `owning_class: None` 的通用技能），不需要命名空间
//!    层面的物理隔离来保证。
//!
//! 因此 `SubclassDef` 本身只需要 `id`/`display_name_key` 两个字段，与
//! [`crate::class::ClassDef`] 去掉 `primary_attribute` 后的形状几乎
//! 一致——副职复用主职已有的技能，或者技能声明 `owning_class` 指向某个
//! 副职，两种用法都直接可行，不需要额外的命名空间字段承载这条关系。
//!
//! **这条裁定推翻了实施计划文档给出的保守默认**（`docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md`
//! 任务 1 撰写时给出的保守默认是「不共享」，标注为待裁定）——项目所有者
//! 与执行者在任务 1 实际讨论后拍板为共享，记入设计文档与
//! `.superpowers/sdd/2026-08-19-p5-gameplay-systems/progress.md`「沿用
//! 的既有裁定」一节，本模块遵循这条最终裁定，不是计划文档里的保守
//! 默认。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::subclass::{CraftUnlockRule, SubclassUnlockCatalog};

use crate::base_contract::{BaseContractError, BaseContractResolver};
use crate::registry::Registry;

/// 单条副职声明：本体与 mod 注册副职时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在副职层面的验收标的——本体的声明与未来 mod 的
/// 声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
///
/// **不携带任何技能命名空间字段**——见本模块文档「裁定 P5-4」一节：
/// 副职的技能与主职共享同一份 `ContentIndex` 命名空间，`SubclassDef`
/// 只需要回答「这是哪个副职、展示名叫什么」，不需要回答「它的技能存在
/// 哪个号段」这个不存在的问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubclassDef {
    /// 命名空间标识符，例如 `lostland:duelist`、`yourmod:shadowdancer`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——与 [`crate::class::ClassDef`]
    /// 同一条纪律：本地化是独立系统的职责，不是内容注册表的。
    pub display_name_key: NamespacedId,
}

/// [`SubclassTable::define`] 实际存进列式存储的属性子集——不含 `id`，
/// 理由同 [`crate::class::ClassAttrs`]。**必须公开**：这是 `define`
/// 唯一的参数类型，任何想直接调用 `define`（而不是走
/// `register-subclass` 那条脚本路径）的调用方——包括未来 mod 自己的
/// 副职注册函数——都需要能构造这个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubclassAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
}

/// 副职注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubclassError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::class::ClassError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// 往一个从未 `register-subclass` 过的索引上追加获得条件——
    /// ADR 0017「注册期完整校验」：目标必须已存在。
    UnknownSubclass(ContentIndex),
    /// 同一个副职声明了两条获得条件，见
    /// [`SubclassTable::set_craft_unlock`] 文档「一个副职只能有一条」。
    DuplicateUnlock(ContentIndex),
    /// 阈值为零——「做满 0 次就获得」等价于「注册即获得」，那不是
    /// 使用计数想表达的东西，是内容作者写错了数。
    ZeroThreshold(ContentIndex),
}

impl fmt::Display for SubclassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubclassError::DuplicateDefinition(index) => {
                write!(f, "副职索引 {} 被重复定义", index.get())
            }
            SubclassError::UnknownSubclass(index) => {
                write!(f, "副职索引 {} 尚未注册，不能给它追加获得条件", index.get())
            }
            SubclassError::DuplicateUnlock(index) => {
                write!(
                    f,
                    "副职索引 {} 已经声明过获得条件，一个副职只能有一条",
                    index.get()
                )
            }
            SubclassError::ZeroThreshold(index) => {
                write!(
                    f,
                    "副职索引 {} 的获得条件阈值为 0，至少要 1 次",
                    index.get()
                )
            }
        }
    }
}

impl std::error::Error for SubclassError {}

/// 一次副职查询命中的完整结果，理由同 [`crate::class::ClassView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubclassView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
}

/// 副职属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0016/0017），与 [`crate::class::ClassTable`]/[`crate::skill::SkillTable`]
/// 同一套道理。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分（与地形、职业、技能
/// 共享同一个 `Interner`/`Registry`），因此这里同样维护一份 `defined`
/// 位图，理由同 [`crate::class::ClassTable`] 文档。
#[derive(Debug, Default, Clone)]
pub struct SubclassTable {
    display_name_key: Vec<Option<NamespacedId>>,
    /// 「做满 N 次某个配方类别就获得这个副职」——`register-subclass-unlock`
    /// 注册后追加的第二列，见 [`SubclassTable::set_craft_unlock`] 文档。
    /// `None` = 这个副职没有声明任何获得条件（合法：它可能靠任务奖励或
    /// 世界生成时写死的初始副职拿到，两条路径都还没落地）。
    craft_unlock: Vec<Option<CraftUnlockRule>>,
    defined: Vec<bool>,
}

impl SubclassTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上副职属性。
    ///
    /// 唯一的校验是「不得重复定义」——副职不像技能那样有前置关系需要
    /// 校验（副职之间没有 DAG，「持有哪些副职」是 `Agent.subclasses`
    /// 这个 `Vec` 的职责，不是副职定义本身的属性）。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: SubclassAttrs,
    ) -> Result<(), SubclassError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.craft_unlock.resize(new_len, None);
        }

        if self.defined[idx] {
            return Err(SubclassError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        Ok(())
    }

    /// 注册后追加：给一个**已注册**的副职声明它的「制作计数」获得
    /// 条件（`register-subclass-unlock` 的写入目标）。
    ///
    /// # 为什么是独立函数，不是 `define` 的第三个参数
    ///
    /// 与已落地的 [`crate::recipe_category::RecipeCategoryTable::add_required_subclass`]
    /// 完全同一条理由：「这个副职叫什么」与「它怎么拿到」是两件独立
    /// 的事，混进同一个位置参数列表会在将来某天有人想「只声明副职、
    /// 获得条件由另一个 mod 补」时变成一处隐藏耦合。副职**没有**获得
    /// 条件是合法且常见的（任务奖励、世界生成时写死的初始副职两条
    /// 路径都还没落地，但形状已经清楚，见 `ll_sim::subclass` 模块
    /// 文档「唯一出口」一节）。
    ///
    /// # 一个副职只能有一条
    ///
    /// 与 `register-class-xp-curve`「一个职业只能绑一条曲线」同一条
    /// 纪律：一个副职若有多条互相竞争的解锁路径，「我还差多少」这句
    /// UI 文案就没法唯一地展示。重复声明在注册期直接报错
    /// （[`SubclassError::DuplicateUnlock`]），不是静默覆盖。
    /// # 为什么存的直接就是结算侧的 [`CraftUnlockRule`]
    ///
    /// 本来「表里存的形状」与「结算侧读到的形状」各有一个类型是本仓库
    /// 的常态（`RecipeDef` 对 `RecipeRule`、`ItemDef` 对 `ItemRule`），
    /// 理由是结算只读其中一部分字段。这里**不是**那种情形：获得条件
    /// 一共三个字段，结算三个全要读，两个类型会逐字段一模一样。ADR 0021
    /// 的反向那半（「拦住把同一份东西复制两遍」）在这里适用，因此这一
    /// 列直接存 `CraftUnlockRule`，`craft_unlocks()` 只是把它们倒出来。
    pub fn set_craft_unlock(
        &mut self,
        index: ContentIndex,
        category: ContentIndex,
        category_id: NamespacedId,
        threshold: u32,
    ) -> Result<(), SubclassError> {
        if !self.is_defined(index) {
            return Err(SubclassError::UnknownSubclass(index));
        }
        if threshold == 0 {
            return Err(SubclassError::ZeroThreshold(index));
        }
        let idx = index.get() as usize;
        if self.craft_unlock[idx].is_some() {
            return Err(SubclassError::DuplicateUnlock(index));
        }
        self.craft_unlock[idx] = Some(CraftUnlockRule {
            subclass: index,
            category,
            category_id,
            threshold,
        });
        Ok(())
    }

    /// 查询一个副职声明的「制作计数」获得条件；没声明过（或索引未
    /// 注册）时返回 `None`。
    pub fn craft_unlock(&self, subclass: ContentIndex) -> Option<&CraftUnlockRule> {
        if !self.is_defined(subclass) {
            return None;
        }
        self.craft_unlock[subclass.get() as usize].as_ref()
    }

    /// 给定的副职索引当前是否已经登记过属性。
    pub fn is_defined(&self, subclass: ContentIndex) -> bool {
        self.defined
            .get(subclass.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个副职的完整属性，未注册的索引返回 `None`（对齐 ADR 0015
    /// 的解析纪律，同 [`crate::class::ClassTable::get`]）。
    pub fn get(&self, subclass: ContentIndex) -> Option<SubclassView<'_>> {
        if !self.is_defined(subclass) {
            return None;
        }
        let idx = subclass.get() as usize;
        Some(SubclassView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
        })
    }
}

/// [`SubclassTable`] 直接充当结算侧的副职获得条件目录——与
/// `impl ExperienceCatalog for RaceTable`（种族表本就登记了 `xp_reward`，
/// 直接当经验目录用）完全同一个手法：获得条件本来就按副职索引存在这张
/// 表里，不需要再造一个把它包起来的中间类型。
///
/// # 为什么是「全部倒出来」而不是「按类别查询」
///
/// 见 `ll_sim::subclass::SubclassUnlockCatalog::craft_unlocks` 文档：与
/// [`crate::quest::RegisteredQuests`] 交付全部 `KillCount` 规则、由
/// `kill_progress_effects` 自己过滤是同一条既有纪律。规则总数是「副职
/// 数量」这个小量级（上限约束下的内容体量，见
/// `ll_sim::subclass::MAX_SUBCLASSES` 文档），一次线性扫描比给注册表
/// 再维护一份反向索引便宜。
///
/// **遍历顺序是 `ContentIndex` 升序**（列式存储的下标顺序），不依赖
/// 任何哈希表迭代顺序——约束 C5。
impl SubclassUnlockCatalog for SubclassTable {
    fn craft_unlocks(&self) -> Vec<CraftUnlockRule> {
        self.craft_unlock.iter().flatten().cloned().collect()
    }
}

/// 本体基础副职在当前注册表里的索引缓存——**句柄，不是内容**。
///
/// 两条副职的字段值已经搬进 `mods/lostland/subclasses.scm`，本结构体
/// 只保住使用点的编译期安全，填充由 [`resolve_base_subclasses`] 在装载
/// 完成后按 id 逐字段解析完成，理由完整见 [`crate::class::BaseClassIds`]
/// 与 [`crate::base_contract`] 两处文档。
///
/// 只有剑舞者/学徒两条——真正的副职数值平衡与内容设计不在本任务范围
/// （与 [`crate::class::BaseClassIds`] 同一条纪律）。同一个脚本文件里
/// 还注册着四个制作类副职（工匠/裁缝/炼金术士/厨师），它们**不进**
/// 本结构体：Rust 侧没有任何代码按名字引用它们（它们只通过
/// `register-subclass-unlock` 与配方类别挂钩，全程走 `ContentIndex`），
/// 判据同 [`crate::class::BaseClassIds`] 文档「哪些内容进」一节。
#[derive(Debug, Clone, Copy)]
pub struct BaseSubclassIds {
    /// 剑舞者：轻装近战副职。
    pub duelist: ContentIndex,
    /// 学徒：可搭配任意主职的通用魔法副职。
    pub apprentice: ContentIndex,
}

/// 本体两个基础副职的 id 字面量——[`resolve_base_subclasses`] 的契约
/// 清单，理由同 [`crate::class`] 的 `BASE_CLASS_IDS`。
const BASE_SUBCLASS_IDS: [(&str, &str); 2] = [
    ("BaseSubclassIds::duelist", "lostland:duelist"),
    ("BaseSubclassIds::apprentice", "lostland:apprentice"),
];

/// 装载完成后解析本体副职契约：按 id 逐字段填充 [`BaseSubclassIds`]，
/// 缺任何一条就整批失败。取代原先的 `materialize_base_subclasses`/
/// `base_subclass_fixture`，理由同 [`crate::class::resolve_base_classes`]。
pub fn resolve_base_subclasses(
    registry: &Registry,
    table: &SubclassTable,
) -> Result<BaseSubclassIds, BaseContractError> {
    let mut resolver = BaseContractResolver::new("本体副职", registry);
    let mut resolved = BASE_SUBCLASS_IDS
        .iter()
        .map(|(field, id)| resolver.require(field, id, |index| table.is_defined(index)));
    let duelist = resolved.next().expect("BASE_SUBCLASS_IDS 恒有两条");
    let apprentice = resolved.next().expect("BASE_SUBCLASS_IDS 恒有两条");
    drop(resolved);
    resolver.finish()?;

    Ok(BaseSubclassIds {
        duelist,
        apprentice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_contract::MissingReason;
    use crate::skill::{ResourceCost, ResourceKind, SkillAttrs, SkillEffect, SkillTable};

    /// 一张现造的、与本体内容无关的副职表，理由同
    /// [`crate::class`] 测试里的 `sample_table`。
    fn sample_table() -> (Registry, ContentIndex, ContentIndex, SubclassTable) {
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();

        let define = |registry: &mut Registry, table: &mut SubclassTable, id: &str| {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            table
                .define(
                    index,
                    SubclassAttrs {
                        display_name_key: NamespacedId::parse("testmod:display_name")
                            .expect("合法标识符"),
                    },
                )
                .expect("首次定义应当成功");
            index
        };

        let blademaster = define(&mut registry, &mut table, "testmod:blademaster");
        let acolyte = define(&mut registry, &mut table, "testmod:acolyte");

        (registry, blademaster, acolyte, table)
    }

    /// 把 [`BASE_SUBCLASS_IDS`] 两条全部注册进一张表，字段值填测试占位
    /// 值——理由同 `crate::class` 测试里的 `registry_with_all_base_classes`。
    fn registry_with_all_base_subclasses() -> (Registry, SubclassTable) {
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();
        for (_, id) in BASE_SUBCLASS_IDS {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            table
                .define(
                    index,
                    SubclassAttrs {
                        display_name_key: NamespacedId::parse("testmod:display_name")
                            .expect("合法标识符"),
                    },
                )
                .expect("首次定义应当成功");
        }
        (registry, table)
    }

    #[test]
    fn 新建的副职表查询任意索引均为未注册() {
        // Arrange
        let table = SubclassTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 已定义的副职能查回它声明的显示名键() {
        // Arrange
        let (_registry, blademaster, _acolyte, table) = sample_table();

        // Act
        let view = table.get(blademaster).expect("已定义");

        // Assert
        assert_eq!(
            view.display_name_key,
            &NamespacedId::parse("testmod:display_name").expect("合法")
        );
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let (_registry, blademaster, _acolyte, mut table) = sample_table();

        // Act
        let result = table.define(
            blademaster,
            SubclassAttrs {
                display_name_key: NamespacedId::parse("testmod:other_display_name").expect("合法"),
            },
        );

        // Assert
        assert_eq!(result, Err(SubclassError::DuplicateDefinition(blademaster)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined =
            registry.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = SubclassTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 本体副职与mod副职共用同一个单调递增号段没有预留区间() {
        // 结构等价断言，理由同 crate::class 里的同名测试。
        // Arrange
        let (mut registry, _blademaster, acolyte, mut table) = sample_table();

        // Act
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:shadowdancer").expect("合法标识符"));
        table
            .define(
                mod_index,
                SubclassAttrs {
                    display_name_key: NamespacedId::parse("yourmod:shadowdancer_display_name")
                        .expect("合法"),
                },
            )
            .expect("mod 副职与先注册的副职调用同一个公开 define 函数,理应同样成功");

        // Assert
        assert_eq!(mod_index.get(), acolyte.get() + 1);
        let view = table.get(mod_index).expect("mod 副职已通过 define 登记");
        assert_eq!(
            view.display_name_key,
            &NamespacedId::parse("yourmod:shadowdancer_display_name").expect("合法")
        );
    }

    #[test]
    fn 副职可以复用主职已定义的技能而不需要重新登记() {
        // 裁定 P5-4 的直接验收：技能与副职共享同一份 ContentIndex
        // 命名空间——一个已经由主职声明的技能，副职可以直接在自己的
        // 技能列表里引用同一个索引，不需要为副职复制一份技能定义，也
        // 不需要任何「跨命名空间引用」机制。
        // Arrange
        let (mut registry, blademaster, acolyte, _table) = sample_table();
        let bruiser = registry.intern(NamespacedId::parse("testmod:bruiser").expect("合法"));
        let mut skill_table = SkillTable::new();
        let heavy_swing =
            registry.intern(NamespacedId::parse("testmod:heavy_swing").expect("合法"));
        skill_table
            .define(
                heavy_swing,
                SkillAttrs {
                    owning_class: Some(bruiser),
                    prerequisites: Vec::new(),
                    cooldown_ticks: 20,
                    resource_cost: ResourceCost::Amount(ResourceKind::Stamina, 10),
                    effect: SkillEffect::DealDamage { base: 12 },
                },
            )
            .expect("主职技能注册应当成功");

        // Act
        let view = skill_table
            .get(heavy_swing)
            .expect("heavy_swing 应当仍然可查询");

        // Assert
        assert_eq!(view.owning_class, Some(bruiser));
        assert_ne!(heavy_swing.get(), blademaster.get());
        assert_ne!(heavy_swing.get(), acolyte.get());
    }

    #[test]
    fn 两条本体副职都在时契约解析成功且返回真实索引() {
        // Arrange
        let (registry, table) = registry_with_all_base_subclasses();

        // Act
        let ids = resolve_base_subclasses(&registry, &table).expect("两条都在，解析应当成功");

        // Assert
        assert_eq!(
            registry.resolve(ids.duelist).map(|id| id.to_string()),
            Some("lostland:duelist".to_string())
        );
        assert_eq!(
            registry.resolve(ids.apprentice).map(|id| id.to_string()),
            Some("lostland:apprentice".to_string())
        );
    }

    #[test]
    fn 本体副职一条都没注册时契约解析一次列出全部两条() {
        // Arrange
        let registry = Registry::new();
        let table = SubclassTable::new();

        // Act
        let error = resolve_base_subclasses(&registry, &table).expect_err("空注册表必须解析失败");

        // Assert
        assert_eq!(error.contract, "本体副职");
        assert_eq!(error.required, 2);
        assert_eq!(
            error
                .missing
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            vec!["lostland:duelist", "lostland:apprentice"]
        );
    }

    #[test]
    fn 副职id只被intern没被define时契约解析报notdefined() {
        // Arrange
        let mut registry = Registry::new();
        for (_, id) in BASE_SUBCLASS_IDS {
            registry.intern(NamespacedId::parse(id).expect("合法标识符"));
        }
        let table = SubclassTable::new();

        // Act
        let error =
            resolve_base_subclasses(&registry, &table).expect_err("只 intern 未 define 必须失败");

        // Assert
        assert!(
            error
                .missing
                .iter()
                .all(|entry| entry.reason == MissingReason::NotDefined)
        );
    }
}
