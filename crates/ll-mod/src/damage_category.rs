//! 伤害类别定义表：`register-damage-category` 的存储落点
//! （`knowledge/design/damage-formula-mod-api.md` 十七、二十一节）。
//!
//! # 与武器类别、`DamageSchool` 都是独立的轴
//!
//! 十七节「是不是同一种东西：不是」——伤害类别（物理/火/冰……）回答
//! "这一下造成哪种伤害"，服务两个目的：挂公式（十八/十九节，本批次不
//! 落地，见「本批次范围」一节）与挂抗性（二十节，本批次落地）。
//! [`crate::weapon_category::WeaponCategoryTable`] 回答的是完全独立的
//! 另一个问题（"用什么打"），`DamageSchool`（`ll_world::item` 模块文档
//! 已核实：本仓库尚未落地这个类型，`resolve_attack` 目前仍是纯物理
//! 近战占位实现,见 `crates/ll-world/src/item.rs`「为什么不现在就加
//! 魔抗/意志抗性两个变体」一节）描述的又是第三个问题（"读哪组防御
//! 字段"）——三者不合并，见十七节「与既有 `DamageSchool` 的关系：
//! 正交，不合并」一节完整论证。
//!
//! # 为什么是 `BTreeMap`，不是列式存储
//!
//! 与 [`crate::formula::FormulaTable`] 同一条理由（其模块文档「为什么
//! 不是列式存储」一节）：伤害类别的查询发生在装载期（`register-item-damage-category`
//! 校验类别是否已注册）与一次攻击同数量级（`resistance_multiplier_permille`
//! 每次攻击查一次），不是逐 tick 高频路径。设计文档十七节「表达方式」
//! 一节进一步指出：与 `SurfaceKindTable` 刻意的一处不同是本表**不**
//! 分配稠密位下标——地表分类是高频运行期位测试，伤害类别的消费场景
//! 是一次性查表，`BTreeMap<ContentIndex, _>` 已经足够。
//!
//! # 本批次范围：注册表 + 校验，不接四层默认公式解析链条
//!
//! `default_formula` 字段按设计文档十七节的形状声明（`register-damage-category`
//! 的第二个参数），注册期校验它若非空则必须已经通过
//! `register-damage-formula` 注册过（见 `damage_categories.json5`
//! 文档）——但十九节「默认公式的挂载层级与优先级」这条完整的四层
//! 解析链条（分项自身 → 伤害类别默认 → 武器类别默认 → 全局默认）本批次
//! 不接线：`resolve_attack` 仍然只用
//! `ll_sim::formula::DamageFormulaCatalog` 现有的两层（显式引用 → 全局
//! 默认）挑公式，见 `damage_formulas.json5` 模块文档「本批次
//! 排除」一节同一条 YAGNI 判断——四层解析链条服务的是"分项相加"
//! （十八节），而分项列表本身（`DamageComponent`）依赖 `WeaponDef`/
//! `SkillDef`（P6 范畴，见该文档二十三节前置依赖清单第 4 项），两者都
//! 不在本批次范围内。`default_formula` 字段因此现在只是"声明先行"，
//! 与 `TraitDef.rule_modifiers` 里其余三个 `RuleModifier` 变体、
//! `RaceDef.xp_reward` 早期状态是同一条既有纪律——先把形状定下来，接
//! 消费者留给挂载链条真正落地的批次，不假装它已经在装载期之外的任何
//! 地方生效。
//!
//! # 显示名字段：从「呈现层现拼键」改成真字段
//!
//! 本表此前**只有** `default_formula` 一个字段。角色面板的规则修正一段
//! （`ll_ui::hud::character_panel`）落地时要显示「火焰抗性 6」，其中
//! 「火焰」两个字没有字段可查，于是那一批用了一条约定拼键
//! （`命名空间:damage_category.路径.display_name`），并在当时就写下代价：
//! **mod 作者漏在 `locales/` 里补这条键，看到的是键名本身**——因为没有
//! 任何一处会告诉他这条键该存在。
//!
//! 现在 [`DamageCategoryDef::display_name_key`] 是一个真字段：漏写在
//! 装载期当场报错（serde 缺必填字段），而不是等到玩家打开面板。呈现层
//! 因此也不再拼键，改成读这个字段——见
//! `ll_sim::rule_modifier::rule_modifier_displays` 的 `subject_name_key`
//! 回调。
//!
//! 与它并列的 [`crate::weapon_category::WeaponCategoryDef`] **没有**跟着
//! 加：武器类别至今一个 UI 落点都没有，为了对称给它加一个没人读的字段，
//! 正是本仓库反复拒绝的「声明了没人读」。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::damage_category::DamageCategoryCatalog;

/// 伤害类别的注册表条目——「物理」「火」「冰」这一类。
///
/// **不是 `Copy`**：`display_name_key` 是 [`NamespacedId`]（内部持一个
/// `String`），与 [`crate::recipe_category::RecipeCategoryDef`] 同样只
/// 派生 `Clone`。这条差别是随显示名字段一起来的，没有别处依赖过本类型
/// 的 `Copy`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageCategoryDef {
    /// 指向 Fluent 本地化键，不存字面字符串——与
    /// [`crate::recipe_category::RecipeCategoryDef::display_name_key`]
    /// 同一条纪律，形状也逐字相同（`命名空间:damage_category.路径.display_name`
    /// 是本体两条的写法，但**这只是本体的约定，不是本字段的格式要求**：
    /// 内容作者写什么键，呈现层就查什么键）。
    ///
    /// # 为什么它现在存在，而 `WeaponCategoryDef` 仍然没有
    ///
    /// [`crate::recipe_category`] 模块文档「与那两张表的两处不同」一节
    /// 当初写下「武器/伤害类别至今没有任何 UI 落点，所以它们没有这个
    /// 字段」——**这句话对伤害类别已经不成立**：角色面板的规则修正一段
    /// 要显示「火焰抗性 6」，其中「火焰」两个字就是本字段。武器类别
    /// 至今仍然没有任何 UI 落点，因此那一半照旧不加。
    ///
    /// # 为什么是必填，不是 `Option`
    ///
    /// 本仓库全部十来处 `display_name_key` 都是必填的 [`NamespacedId`]
    /// （`ItemDef`/`RaceDef`/`ClassDef`/`RecipeDef`/`RecipeCategoryDef`
    /// …），照最一致的那个做。代价与收益是同一件事：mod 作者**漏写
    /// 字段会在装载期当场报错**，而不是等到玩家打开角色面板才看见一行
    /// 键名——这正是本字段替换掉「呈现层按约定现拼键」那条旧做法要买
    /// 到的东西。
    pub display_name_key: NamespacedId,

    /// 这个伤害类别没有被具体分项覆盖时使用的默认公式（十九节，本批次
    /// 不接线，见模块文档「本批次范围」一节）——`None` 表示不声明类别
    /// 默认，继续下探到全局默认。
    pub default_formula: Option<ContentIndex>,
}

/// 伤害类别注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageCategoryError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for DamageCategoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DamageCategoryError::DuplicateDefinition(index) => {
                write!(f, "伤害类别索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for DamageCategoryError {}

/// 伤害类别定义表：`ContentIndex`（类别自身的命名空间标识符）→
/// [`DamageCategoryDef`]，理由见模块文档「为什么是 `BTreeMap`」一节。
#[derive(Debug, Default, Clone)]
pub struct DamageCategoryTable {
    entries: BTreeMap<ContentIndex, DamageCategoryDef>,
}

impl DamageCategoryTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条伤害类别定义。
    pub fn define(
        &mut self,
        index: ContentIndex,
        def: DamageCategoryDef,
    ) -> Result<(), DamageCategoryError> {
        if self.entries.contains_key(&index) {
            return Err(DamageCategoryError::DuplicateDefinition(index));
        }
        self.entries.insert(index, def);
        Ok(())
    }

    /// 查询一条伤害类别定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&DamageCategoryDef> {
        self.entries.get(&index)
    }

    /// 给定的伤害类别索引当前是否已经登记过定义——供
    /// [`crate::content_hash::classify_index`] 判定表归属。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.entries.contains_key(&index)
    }
}

/// `ll_sim::resolve::resolve_attack` 消费的真实伤害类别目录：把
/// [`crate::base_damage_category::register_base_damage_category`] 产出的
/// **全局默认伤害类别索引**包成一个
/// [`DamageCategoryCatalog`]。
///
/// # 为什么不是 `impl DamageCategoryCatalog for DamageCategoryTable`
///
/// 因为答案不在表里。[`DamageCategoryTable`] 知道的是「有哪几类」
/// （`register-damage-category` 的落点，本体与 mod 共用同一条
/// [`DamageCategoryTable::define`]）；而本 trait 唯一要回答的是「**没有
/// 任何声明时退回哪一类**」——那是引擎在任何 mod 装载之前就定下的另一
/// 件事（见 [`crate::base_damage_category`] 模块文档）。让存储表凭空多
/// 出一个 mod 永远不该写、也写不出的「我是默认」字段，是把装载会话的
/// 产物塞进存储层，这条边界与 `FormulaTable`/[`crate::formula::RegistryFormulas`]
/// 的划法一致。
///
/// # 为什么不像 [`crate::formula::RegistryFormulas`] 那样也拿着表
///
/// 那是本类型与它唯一的形状差别，理由是**目前没有一个方法需要读表**：
/// `RegistryFormulas` 拿着 `FormulaTable` 是因为
/// [`ll_sim::formula::DamageFormulaCatalog::formula_for`] 要**从表里取出
/// 一条 `FormulaDef` 返回**；[`DamageCategoryCatalog::default_category`]
/// 返回的只是一个索引，一次查表都不需要。多挂一个从不被读的
/// `&DamageCategoryTable` 字段，正是本仓库反复拒绝的「声明了没人读」
/// ——`scripts/ci/check_field_consumers.py` 存在的理由就是这个。
///
/// 这不是永久结论：`damage-formula-mod-api.md` 十九节那条四层默认公式
/// 下探链条（分项 → **伤害类别默认** → 武器类别默认 → 全局默认）真正
/// 落地时，本 trait 会多出一个「这一类的默认公式是什么」的方法，那时
/// 表才第一次有人读，照 `RegistryFormulas` 的先例加一个字段即可——
/// 生产侧的构造点只有 `ll_game::content::RuntimeCatalogs::new` 一处。
///
/// # 名字不是新起的
///
/// [`ll_sim::damage_category::NoDamageCategories`] 的文档早就点名了
/// 「调用方没有接好真正的 `RegistryDamageCategories`」——本类型就是那句
/// 话一直缺席的那一半：在此之前**仓库里唯一的实现是那个空实现**，
/// 于是引擎注册的全局默认类别在真实游戏里从来没被当默认用过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryDamageCategories {
    /// 引擎注册的全局默认伤害类别索引（本体是 `lostland:physical`，见
    /// [`crate::base_damage_category::DEFAULT_DAMAGE_CATEGORY_ID`]）。
    pub default_category: ContentIndex,
}

impl DamageCategoryCatalog for RegistryDamageCategories {
    fn default_category(&self) -> ContentIndex {
        self.default_category
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    fn key(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一条内部自洽的伤害类别定义——测试不关心显示名时用它，避免每处
    /// 都重复拼一遍必填的 `display_name_key`（同
    /// [`crate::recipe_category`] 测试里的 `key` 辅助函数）。
    fn def(display_name: &str) -> DamageCategoryDef {
        DamageCategoryDef {
            display_name_key: key(display_name),
            default_formula: None,
        }
    }

    #[test]
    fn 定义后可以查到同一条伤害类别() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:fire");
        let mut table = DamageCategoryTable::new();

        // Act
        table
            .define(index, def("lostland:damage_category.fire.display_name"))
            .expect("首次定义应当成功");

        // Assert
        assert_eq!(
            table.get(index).map(|def| def.display_name_key.to_string()),
            Some("lostland:damage_category.fire.display_name".to_string())
        );
        assert!(table.is_defined(index));
    }

    #[test]
    fn 重复定义同一个伤害类别索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:physical");
        let mut table = DamageCategoryTable::new();
        table
            .define(index, def("lostland:damage_category.physical.display_name"))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, def("lostland:damage_category.physical.display_name"));

        // Assert
        assert_eq!(result, Err(DamageCategoryError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的伤害类别索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let table = DamageCategoryTable::new();

        // Act & Assert
        assert_eq!(table.get(never_defined), None);
        assert!(!table.is_defined(never_defined));
    }

    #[test]
    fn 真实伤害类别目录返回引擎注册的全局默认类别() {
        // Arrange
        let mut interner = Interner::new();
        // 先 intern 一条别的内容，把索引 0 占掉——`ContentIndex::default()`
        // **不是**保留哨兵，它就是第一个被 intern 的东西（见
        // `ll_core::ident::ContentIndex::default` 文档），这一行确保下面
        // 那条 `assert_ne!` 断的是真事。
        let _first = index(&mut interner, "lostland:grass");
        let physical = index(&mut interner, "lostland:physical");
        let catalog = RegistryDamageCategories {
            default_category: physical,
        };

        // Act & Assert：这一条是「不显式声明 damage_category 的武器退回
        // lostland:physical」这句承诺的最小验收——此前生产路径接的是空
        // 实现 NoDamageCategories，返回的是 ContentIndex::default()。
        assert_eq!(catalog.default_category(), physical);
        assert_ne!(catalog.default_category(), ContentIndex::default());
    }
}
