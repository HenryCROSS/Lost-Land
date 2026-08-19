//! 职业注册表——「本体即 Mod」在职业系统上的落点（P5-B 任务 2）。
//!
//! # 照抄 `terrain.rs`/`space_profile.rs` 已验证的模式
//!
//! `ll_world::terrain`/`ll_world::space_profile` 验证过一套「私有字段 +
//! `Table::define` 注册期校验 + `materialize_base_*` 本体注册入口 +
//! `*_fixture` 测试夹具」的模式（见 `ll_world::terrain` 模块文档）。
//! 本模块走同一条路径。
//!
//! # 为什么定义本身直接落在 `ll-mod`，不像地形那样拆成两处
//!
//! 地形/层属性的定义（`TerrainDef`/`SpaceProfileDef`）之所以落在
//! `ll-world`、只让 `ll-mod` 做一层 `Registry::intern` 的薄封装，是
//! 因为它们要与 `ChunkGrid`/`Space` 等世界存储结构直接打交道，必须
//! 留在 `ll-world` 内部才不会让 `ll-world` 反向依赖 `ll-mod`（依赖顺序
//! `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`，规格 §5）。
//!
//! `ClassDef` 不依赖任何「世界空间」概念——它只是一条静态声明（主属性
//! 倾向、本地化键），`ll-mod` 本身就是可以直接持有
//! [`crate::registry::Registry`] 的那一层，没有必要为了套用地形那套
//! 「定义在下游、封装在上游」的分层，平白多出一次跨 crate 的间接。
//! 见 `knowledge/design/class-skill-quest-system.md`「与既有架构的
//! 接线点」一节。
//!
//! # 与 `Agent.profession` 的关系
//!
//! `ClassDef` 不是给 `Agent` 添加第二套「职业」概念，而是给 P3 阶段就
//! 已经建好的 `Agent.profession: ContentIndex` 字段配一张真正的注册表
//! ——`society-and-affiliation.md` 早已把 `AffiliationKind::Profession`
//! 定为「恒为 `Def`」，职业本身是类型不是实例，与 `ContentIndex` 语义
//! 完全吻合。见设计文档第一节。
//!
//! # 查询接口：`Option`，不是「安全兜底默认值」
//!
//! [`TerrainTable::blocks_sight`](ll_world::terrain::TerrainTable::blocks_sight)
//! 一类查询对未注册索引返回一个「安全默认值」（如 `false`），因为地形
//! 判定是逐格逐帧的热路径，不能为了一次查询就返回 `Option` 强迫调用方
//! 到处 `unwrap_or`。职业查询不在任何逐帧热路径上（建档选择职业、UI
//! 展示，都是低频调用），因此本模块选择更明确的 [`ClassTable::get`]
//! ——查不到就是查不到，返回 `None`，不用一个可能被误当成「真实数据」
//! 的默认值掩盖「这个索引其实没注册」这个事实（呼应 ADR 0015「注册
//! 校验是解析」——`Registry::get` 同样是查不到就返回 `None`，不创建、
//! 不兜底）。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_world::entity::AttributeKind;
use std::fmt;

/// 单条职业声明：本体与 mod 注册职业时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在职业层面的验收标的——[`materialize_base_classes`]
/// 拿这个类型的值去调用外部传入的 `intern` 回调，本体的声明与未来 mod
/// 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    /// 命名空间标识符，例如 `lostland:warrior`、`yourmod:necromancer`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——本地化是独立系统的职责，
    /// 不是内容注册表的。
    pub display_name_key: NamespacedId,
    /// 主属性倾向：六项主属性之一，供职业选择界面展示、以及后续批次
    /// 职业相关数值计算的输入。P5 阶段只是分类字段，不驱动结算逻辑。
    pub primary_attribute: AttributeKind,
}

/// [`ClassTable::define`] 实际存进列式存储的属性子集——不含 `id`（`id`
/// 只在注册那一刻用于换取 [`ContentIndex`]，换到之后就不再需要）。
///
/// **必须公开**：这是 [`ClassTable::define`] 唯一的参数类型，任何想
/// 直接调用 `define`（而不是走 [`materialize_base_classes`] 那条便捷
/// 路径）的调用方——包括未来 mod 自己的职业注册函数——都需要能构造这个
/// 类型。地形迁移时曾把等价类型写成模块私有，导致公开的 `define`
/// 事实上无法从模块外调用（见 `ll_world::terrain::TerrainAttrs`
/// 模块文档「必须公开」一节），这里直接吸取那次教训。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 主属性倾向。
    pub primary_attribute: AttributeKind,
}

/// 职业注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来，而不是等到查询某个具体职业时才表现成怪行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassError {
    /// 同一个内容索引被定义了两次——与
    /// [`ll_world::terrain::TerrainError::DuplicateDefinition`] 同一条
    /// 纪律：`intern` 对同一个 `NamespacedId` 重复调用是幂等的（返回
    /// 同一个索引），但幂等的是「索引分配」，不是「这个索引对应的职业
    /// 属性」——两个不同的 mod（或某 mod 与本体）若都尝试给同一个 `id`
    /// 定义职业，第二次必须报错，不能静默覆盖第一次的结果。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for ClassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClassError::DuplicateDefinition(index) => {
                write!(f, "职业索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for ClassError {}

/// 一次职业查询命中的完整结果——把 [`ClassAttrs`] 的字段按引用/值
/// 打包，避免调用方对每个字段分别处理「查不到怎么办」（见模块文档
/// 「查询接口」一节，本类型只在 [`ClassTable::get`] 命中时才会出现，
/// 因此内部字段本身不需要再包一层 `Option`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 主属性倾向。
    pub primary_attribute: AttributeKind,
}

/// 职业属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017）。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分，不是「职业专属」的
/// 连续编号——地形、空间层属性、职业、技能共享同一个
/// `Interner`/`Registry`。因此这里额外维护一份 `defined` 位图：数组
/// 下标落在表范围内不代表「这是一个职业」，只有 `defined[idx]` 为真
/// 才是。
#[derive(Debug, Default, Clone)]
pub struct ClassTable {
    display_name_key: Vec<Option<NamespacedId>>,
    primary_attribute: Vec<AttributeKind>,
    defined: Vec<bool>,
}

impl ClassTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上职业属性。
    ///
    /// 唯一的校验是「不得重复定义」（见 [`ClassError::DuplicateDefinition`]
    /// 文档）——职业不像地形那样有 `blocks_move`/`move_cost` 之间的
    /// 自洽性约束需要检查，主属性倾向本身没有互相矛盾的组合。
    pub fn define(&mut self, index: ContentIndex, attrs: ClassAttrs) -> Result<(), ClassError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            // AttributeKind 没有语义上的「默认值」，这里用 Strength
            // 只是一个占位——与 TerrainTable::move_cost 在扩容时填 0
            // 同一个理由：未定义的槽位永远被 `defined` 位图挡住，不会
            // 被外部查询实际读到。
            self.primary_attribute
                .resize(new_len, AttributeKind::Strength);
        }

        if self.defined[idx] {
            return Err(ClassError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.primary_attribute[idx] = attrs.primary_attribute;
        Ok(())
    }

    /// 给定的职业索引当前是否已经登记过属性。
    pub fn is_defined(&self, class: ContentIndex) -> bool {
        self.defined
            .get(class.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个职业的完整属性，未注册的索引返回 `None`（见模块文档
    /// 「查询接口」一节，对齐 ADR 0015 的解析纪律：`Registry::get`
    /// 同样是查不到就返回 `None`）。
    pub fn get(&self, class: ContentIndex) -> Option<ClassView<'_>> {
        if !self.is_defined(class) {
            return None;
        }
        let idx = class.get() as usize;
        Some(ClassView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            primary_attribute: self.primary_attribute[idx],
        })
    }
}

/// 本体基础职业在当前注册表里的索引缓存。
///
/// 只注册占位性质的少数几种基础职业——真正的职业数值平衡与内容设计
/// 不在本任务范围（`knowledge/design/class-skill-quest-system.md`
/// 文档开篇已声明：本文档与本任务只交付系统骨架，不交付具体职业该有
/// 什么数值）。
#[derive(Debug, Clone, Copy)]
pub struct BaseClassIds {
    /// 战士：力量倾向。
    pub warrior: ContentIndex,
    /// 法师：智力倾向。
    pub mage: ContentIndex,
    /// 游侠：敏捷倾向。
    pub ranger: ContentIndex,
}

/// 本体职业注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调（生产路径是 `|id| registry.intern(id)`
/// ，测试路径是本模块的 [`base_class_fixture`]）——本函数只管「拿到一个
/// 索引后，声明它的职业属性」，不关心索引从哪个具体类型来，与
/// `ll_world::terrain::materialize_base_terrain` 同一个理由（该函数
/// 文档「与 Registry 的关系」一节）。
pub fn materialize_base_classes(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseClassIds, ClassTable), ClassError> {
    let mut table = ClassTable::new();

    let warrior = define_base(
        &mut table,
        intern,
        "lostland:warrior",
        "lostland:class.warrior.display_name",
        AttributeKind::Strength,
    )?;
    let mage = define_base(
        &mut table,
        intern,
        "lostland:mage",
        "lostland:class.mage.display_name",
        AttributeKind::Intelligence,
    )?;
    let ranger = define_base(
        &mut table,
        intern,
        "lostland:ranger",
        "lostland:class.ranger.display_name",
        AttributeKind::Dexterity,
    )?;

    Ok((
        BaseClassIds {
            warrior,
            mage,
            ranger,
        },
        table,
    ))
}

/// [`materialize_base_classes`] 的内部帮手：把一条声明的字面量字段
/// 拆开传入，换取一次 `intern` + 一次 [`ClassTable::define`]。
fn define_base(
    table: &mut ClassTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    display_name_key: &str,
    primary_attribute: AttributeKind,
) -> Result<ContentIndex, ClassError> {
    let index = intern(NamespacedId::parse(id).expect("本体职业 id 字面量恒合法"));
    table.define(
        index,
        ClassAttrs {
            display_name_key: NamespacedId::parse(display_name_key)
                .expect("本体职业本地化键字面量恒合法"),
            primary_attribute,
        },
    )?;
    Ok(index)
}

/// 供测试使用：现造一个空 [`Interner`]，注册本体全部基础职业，返回
/// 可用的 `(BaseClassIds, ClassTable)`。
///
/// **不是生产路径**——生产路径必须经过 [`crate::registry::Registry::intern`]（见模块
/// 文档）。这个函数只是让本 crate 的单元测试不必先搭一整套 mod 加载
/// 流程就能拿到一份内部自洽的职业表。
pub fn base_class_fixture() -> (BaseClassIds, ClassTable) {
    let mut interner = Interner::new();
    materialize_base_classes(&mut |id| interner.intern(id))
        .expect("本体职业声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 新建的职业表查询任意索引均为未注册() {
        // Arrange
        let table = ClassTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 战士的主属性倾向是力量() {
        // Arrange
        let (ids, table) = base_class_fixture();

        // Act
        let view = table.get(ids.warrior).expect("战士已在本体注册");

        // Assert
        assert_eq!(view.primary_attribute, AttributeKind::Strength);
    }

    #[test]
    fn 法师的主属性倾向是智力() {
        // Arrange
        let (ids, table) = base_class_fixture();

        // Act
        let view = table.get(ids.mage).expect("法师已在本体注册");

        // Assert
        assert_eq!(view.primary_attribute, AttributeKind::Intelligence);
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // 对齐 ADR 0015 的解析纪律：查不到就是查不到，不返回一个可能
        // 被误当成真实数据的兜底值。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = ClassTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:warrior").expect("合法"));
        let mut table = ClassTable::new();
        table
            .define(
                index,
                ClassAttrs {
                    display_name_key: NamespacedId::parse("lostland:class.warrior.display_name")
                        .expect("合法"),
                    primary_attribute: AttributeKind::Strength,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            ClassAttrs {
                display_name_key: NamespacedId::parse("lostland:class.warrior.display_name")
                    .expect("合法"),
                primary_attribute: AttributeKind::Intelligence,
            },
        );

        // Assert
        assert_eq!(result, Err(ClassError::DuplicateDefinition(index)));
    }

    #[test]
    fn 本体职业通过与mod职业完全相同的intern调用路径注册() {
        // 本任务最核心的一条断言：本体注册与 mod 注册除了命名空间
        // 字符串不同之外，没有任何结构性差异——都只是往同一个
        // Registry::intern 里塞一个 NamespacedId，再用完全相同的公开
        // ClassTable::define 函数登记属性。用「本体注册完之后，再拿
        // 同一个 Registry 直接注册一个 mod 风格的职业，两者分配到的
        // 索引连续递增」证明它们走的是完全相同的通道，没有任何一条
        // 只对本体开放的旁路。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (class_ids, mut table) = materialize_base_classes(&mut |id| registry.intern(id))
            .expect("本体职业声明表内部一致");
        let mod_id = NamespacedId::parse("yourmod:necromancer").expect("合法标识符");
        let mod_index = registry.intern(mod_id);
        table
            .define(
                mod_index,
                ClassAttrs {
                    display_name_key: NamespacedId::parse("yourmod:necromancer_display_name")
                        .expect("合法"),
                    primary_attribute: AttributeKind::Willpower,
                },
            )
            .expect("mod 职业与本体职业调用同一个公开 define 函数,理应同样成功");

        // Assert：mod 内容紧接在本体三种职业之后分配到索引，说明两者
        // 共用同一个单调递增的号段，没有为本体预留任何特殊区间；且
        // mod 注册的职业确实能通过 get 查到正确属性。
        assert_eq!(mod_index.get(), class_ids.ranger.get() + 1);
        let view = table.get(mod_index).expect("mod 职业已通过 define 登记");
        assert_eq!(view.primary_attribute, AttributeKind::Willpower);
    }
}
