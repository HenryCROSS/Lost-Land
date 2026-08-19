//! 副职注册表——「本体即 Mod」在副职系统上的落点（P5-B 任务 4）。
//!
//! # 与 `ClassTable`/`SkillTable` 同一套模式
//!
//! 见 [`crate::class`] 模块文档「照抄 `terrain.rs`/`space_profile.rs`
//! 已验证的模式」一节——副职的定义/存储/查询走完全相同的思路：私有
//! 字段 + `SubclassTable::define` 注册期校验 + `materialize_base_subclasses`
//! 本体注册入口 + `base_subclass_fixture` 测试夹具。
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

use ll_core::ident::{ContentIndex, Interner, NamespacedId};

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
/// [`materialize_base_subclasses`] 那条便捷路径）的调用方——包括未来
/// mod 自己的副职注册函数——都需要能构造这个类型。
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
}

impl fmt::Display for SubclassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubclassError::DuplicateDefinition(index) => {
                write!(f, "副职索引 {} 被重复定义", index.get())
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
        }

        if self.defined[idx] {
            return Err(SubclassError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        Ok(())
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

/// 本体基础副职在当前注册表里的索引缓存。
///
/// 只注册占位性质的少数几种基础副职——真正的副职数值平衡与内容设计
/// 不在本任务范围（与 [`crate::class::BaseClassIds`] 同一条纪律）。
#[derive(Debug, Clone, Copy)]
pub struct BaseSubclassIds {
    /// 剑舞者：轻装近战副职。
    pub duelist: ContentIndex,
    /// 学徒：可搭配任意主职的通用魔法副职。
    pub apprentice: ContentIndex,
}

/// 本体副职注册的唯一入口：本体与 mod 共用的注册路径，理由同
/// [`crate::class::materialize_base_classes`]。
pub fn materialize_base_subclasses(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseSubclassIds, SubclassTable), SubclassError> {
    let mut table = SubclassTable::new();

    let duelist = define_base(
        &mut table,
        intern,
        "lostland:duelist",
        "lostland:subclass.duelist.display_name",
    )?;
    let apprentice = define_base(
        &mut table,
        intern,
        "lostland:apprentice",
        "lostland:subclass.apprentice.display_name",
    )?;

    Ok((
        BaseSubclassIds {
            duelist,
            apprentice,
        },
        table,
    ))
}

/// [`materialize_base_subclasses`] 的内部帮手：把一条声明的字面量字段
/// 拆开传入，换取一次 `intern` + 一次 [`SubclassTable::define`]。
fn define_base(
    table: &mut SubclassTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    display_name_key: &str,
) -> Result<ContentIndex, SubclassError> {
    let index = intern(NamespacedId::parse(id).expect("本体副职 id 字面量恒合法"));
    table.define(
        index,
        SubclassAttrs {
            display_name_key: NamespacedId::parse(display_name_key)
                .expect("本体副职本地化键字面量恒合法"),
        },
    )?;
    Ok(index)
}

/// 供测试使用：现造一个空 [`Interner`]，注册本体全部基础副职，返回
/// 可用的 `(BaseSubclassIds, SubclassTable)`。不是生产路径，理由同
/// [`crate::class::base_class_fixture`]。
pub fn base_subclass_fixture() -> (BaseSubclassIds, SubclassTable) {
    let mut interner = Interner::new();
    materialize_base_subclasses(&mut |id| interner.intern(id))
        .expect("本体副职声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::skill::{ResourceCost, SkillAttrs, SkillEffect, SkillTable};

    #[test]
    fn 新建的副职表查询任意索引均为未注册() {
        // Arrange
        let table = SubclassTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 剑舞者副职注册成功且可查询() {
        // Arrange
        let (ids, table) = base_subclass_fixture();

        // Act
        let view = table.get(ids.duelist).expect("剑舞者已在本体注册");

        // Assert
        assert_eq!(
            view.display_name_key,
            &NamespacedId::parse("lostland:subclass.duelist.display_name").expect("合法")
        );
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:duelist").expect("合法"));
        let mut table = SubclassTable::new();
        table
            .define(
                index,
                SubclassAttrs {
                    display_name_key: NamespacedId::parse("lostland:subclass.duelist.display_name")
                        .expect("合法"),
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            SubclassAttrs {
                display_name_key: NamespacedId::parse("lostland:subclass.other.display_name")
                    .expect("合法"),
            },
        );

        // Assert
        assert_eq!(result, Err(SubclassError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = SubclassTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 本体副职通过与mod副职完全相同的intern调用路径注册() {
        // 本任务最核心的一条断言，理由同 crate::class/crate::skill 模块
        // 的等价测试：本体副职与 mod 副职都只是往同一个
        // Registry::intern 里塞一个 NamespacedId，再用完全相同的公开
        // SubclassTable::define 函数登记属性。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (base_ids, mut table) = materialize_base_subclasses(&mut |id| registry.intern(id))
            .expect("本体副职声明表内部一致");
        let mod_id = NamespacedId::parse("yourmod:shadowdancer").expect("合法标识符");
        let mod_index = registry.intern(mod_id);
        table
            .define(
                mod_index,
                SubclassAttrs {
                    display_name_key: NamespacedId::parse("yourmod:shadowdancer_display_name")
                        .expect("合法"),
                },
            )
            .expect("mod 副职与本体副职调用同一个公开 define 函数,理应同样成功");

        // Assert：mod 副职紧接在本体两种副职之后分配到索引，说明两者
        // 共用同一个单调递增的号段。
        assert_eq!(mod_index.get(), base_ids.apprentice.get() + 1);
        let view = table.get(mod_index).expect("mod 副职已通过 define 登记");
        assert_eq!(
            view.display_name_key,
            &NamespacedId::parse("yourmod:shadowdancer_display_name").expect("合法")
        );
    }

    #[test]
    fn 副职可以复用主职已定义的技能而不需要重新登记() {
        // 裁定 P5-4 的直接验收：技能与副职共享同一份 ContentIndex
        // 命名空间——一个已经由主职（战士）声明的技能，副职（剑舞者）
        // 可以直接在自己的技能列表里引用同一个索引，不需要为副职复制
        // 一份技能定义，也不需要任何「跨命名空间引用」机制。
        // Arrange：先注册技能表（含一个 owning_class 指向战士的技能），
        // 再注册副职表——两者共用同一个 Registry，因此索引天然落在同
        // 一段号空间。
        let mut registry = Registry::new();
        let warrior = registry.intern(NamespacedId::parse("lostland:warrior").expect("合法"));
        let mut skill_table = SkillTable::new();
        let power_strike =
            registry.intern(NamespacedId::parse("lostland:power_strike").expect("合法"));
        skill_table
            .define(
                power_strike,
                SkillAttrs {
                    owning_class: Some(warrior),
                    prerequisites: Vec::new(),
                    cooldown_ticks: 20,
                    resource_cost: ResourceCost::Stamina(10),
                    effect: SkillEffect::DealDamage { base: 12 },
                },
            )
            .expect("战士技能注册应当成功");
        let (base_ids, _subclass_table) =
            materialize_base_subclasses(&mut |id| registry.intern(id))
                .expect("本体副职声明表内部一致");

        // Act：剑舞者副职「复用」战士的 power_strike——这里只是证明
        // power_strike 这个 ContentIndex 在副职注册完毕后依然能在同一张
        // 技能表里查到、且属性不变，没有因为副职注册流程而发生任何
        // 命名空间冲突或索引偏移。
        let view = skill_table
            .get(power_strike)
            .expect("power_strike 应当仍然可查询");

        // Assert
        assert_eq!(view.owning_class, Some(warrior));
        assert_ne!(power_strike.get(), base_ids.duelist.get());
        assert_ne!(power_strike.get(), base_ids.apprentice.get());
    }
}
