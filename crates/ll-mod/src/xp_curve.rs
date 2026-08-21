//! 经验曲线注册表：`register-xp-curve`/`register-class-xp-curve`/
//! `register-race-xp-curve` 的存储落点
//! （`knowledge/design/level-and-experience-system.md` 八节）。
//!
//! # 为什么不是列式存储（不照抄 `RaceTable`/`ClassTable`）
//!
//! `RaceTable`/`ClassTable` 走列式存储是因为它们服务战斗/属性结算这类
//! 高频查询路径（ADR 0017 的判据）。经验曲线的查询频率与升级事件同
//! 数量级——一场战斗里一个实体最多触发几次，不是逐 tick 路径（见
//! `ll_sim::xp_curve::XpCurveCatalog` 文档「为什么按值返回」一节同一个
//! 频率判断）。`BTreeMap<ContentIndex, XpCurveDef>` 的 `O(log n)` 查询
//! 对这个频率完全足够，不需要为一张低频小表引入列式存储的额外复杂度
//! （YAGNI）。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;
use ll_sim::experience::ExperienceCatalog;
use ll_sim::xp_curve::{XpCurveCatalog, XpCurveDef};

use crate::race::RaceTable;

/// 经验曲线注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpCurveError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
    /// `register-class-xp-curve`/`register-race-xp-curve` 引用的
    /// `curve-id` 在当前注册表里找不到——ADR 0017「注册期完整校验」，
    /// 不允许绑定一条不存在的曲线、留到升级那一刻才查询失败。
    UnknownCurve(ContentIndex),
}

impl fmt::Display for XpCurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XpCurveError::DuplicateDefinition(index) => {
                write!(f, "经验曲线索引 {} 被重复定义", index.get())
            }
            XpCurveError::UnknownCurve(index) => {
                write!(f, "经验曲线索引 {} 未注册，无法绑定", index.get())
            }
        }
    }
}

impl std::error::Error for XpCurveError {}

/// 经验曲线定义表：`ContentIndex`（曲线自身的命名空间标识符）→
/// [`XpCurveDef`]。
#[derive(Debug, Default, Clone)]
pub struct XpCurveTable {
    curves: BTreeMap<ContentIndex, XpCurveDef>,
}

impl XpCurveTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条曲线定义。
    pub fn define(&mut self, index: ContentIndex, def: XpCurveDef) -> Result<(), XpCurveError> {
        if self.curves.contains_key(&index) {
            return Err(XpCurveError::DuplicateDefinition(index));
        }
        self.curves.insert(index, def);
        Ok(())
    }

    /// 查询一条曲线定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&XpCurveDef> {
        self.curves.get(&index)
    }
}

/// 职业/种族 → 曲线的绑定表（`register-class-xp-curve`/
/// `register-race-xp-curve`，均是一档纯绑定，见设计文档八节）。
#[derive(Debug, Default, Clone)]
pub struct XpCurveBindings {
    class: BTreeMap<ContentIndex, ContentIndex>,
    race: BTreeMap<ContentIndex, ContentIndex>,
}

impl XpCurveBindings {
    /// 建立空绑定表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 把 `class_id` 绑定到 `curve_id`——`curve_id` 必须已经在
    /// `curves` 里定义过（注册期完整校验，见 [`XpCurveError::UnknownCurve`]
    /// 文档）。同一个职业重复绑定按最后一次生效（不是错误）——与
    /// `active_stat_modifiers` 的覆盖语义同一类「重复声明取最后一次」，
    /// 绑定关系不像曲线定义那样需要「只能有一份权威声明」的强约束。
    pub fn bind_class(
        &mut self,
        curves: &XpCurveTable,
        class_id: ContentIndex,
        curve_id: ContentIndex,
    ) -> Result<(), XpCurveError> {
        if curves.get(curve_id).is_none() {
            return Err(XpCurveError::UnknownCurve(curve_id));
        }
        self.class.insert(class_id, curve_id);
        Ok(())
    }

    /// 把 `race_id` 绑定到 `curve_id`，理由同 [`Self::bind_class`]。
    pub fn bind_race(
        &mut self,
        curves: &XpCurveTable,
        race_id: ContentIndex,
        curve_id: ContentIndex,
    ) -> Result<(), XpCurveError> {
        if curves.get(curve_id).is_none() {
            return Err(XpCurveError::UnknownCurve(curve_id));
        }
        self.race.insert(race_id, curve_id);
        Ok(())
    }

    /// 查询职业绑定的曲线索引，未绑定返回 `None`。
    pub fn class_curve(&self, class_id: ContentIndex) -> Option<ContentIndex> {
        self.class.get(&class_id).copied()
    }

    /// 查询种族绑定的曲线索引，未绑定返回 `None`。
    pub fn race_curve(&self, race_id: ContentIndex) -> Option<ContentIndex> {
        self.race.get(&race_id).copied()
    }
}

/// [`ll_sim::apply::apply_with_xp_curves`] 消费的真实曲线目录：组合
/// [`XpCurveTable`]（曲线定义）、[`XpCurveBindings`]（职业/种族 → 曲线
/// 的绑定）与一个保底默认曲线索引。
///
/// # 优先级：职业绑定 > 种族绑定 > 默认曲线
///
/// 设计文档八节只定案了「未绑定的职业/种族退回默认曲线」，没有明说
/// 「职业与种族都绑定了、且指向不同曲线」时听谁的——这是本实现需要
/// 补的一个真实缺口，选择"职业优先"：`profession` 是 `Agent` 的单值
/// 主职字段，在这个项目里最接近"这个角色是干什么的"这个问题的权威
/// 答案（`race-system.md` 定位种族修正为"创建角色时一次性叠加，此后
/// 与种族脱钩"，成长节奏更适合由职业决定），种族绑定因此只在职业没有
/// 显式绑定曲线时才生效,不是两者取其一的随机选择。
pub struct RegistryXpCurves<'a> {
    /// 曲线定义表。
    pub curves: &'a XpCurveTable,
    /// 职业/种族 → 曲线的绑定表。
    pub bindings: &'a XpCurveBindings,
    /// 未绑定时的保底曲线索引——必须已经在 `curves` 里定义过，找不到
    /// 时 [`XpCurveCatalog::curve_for`] 退化到
    /// [`ll_sim::xp_curve::FlatXpCurve::DEFAULT`]（防御性兜底：装载
    /// 期若真的漏掉默认曲线的注册，运行期也不应该 panic）。
    pub default_curve: ContentIndex,
}

impl XpCurveCatalog for RegistryXpCurves<'_> {
    fn curve_for(&self, profession: ContentIndex, race: ContentIndex) -> XpCurveDef {
        let resolved = self
            .bindings
            .class_curve(profession)
            .or_else(|| self.bindings.race_curve(race))
            .unwrap_or(self.default_curve);
        self.curves
            .get(resolved)
            .cloned()
            .unwrap_or_else(|| ll_sim::xp_curve::FlatXpCurve::DEFAULT.curve_for(profession, race))
    }
}

/// 让 [`RaceTable`] 直接充当 [`ExperienceCatalog`]——种族表本就登记了
/// 每个种族的 `xp_reward`（[`crate::race::RaceDef::xp_reward`] 文档），
/// 不需要另开一张表，见本任务「生物值多少经验落在哪」的落点判断。
impl ExperienceCatalog for RaceTable {
    fn xp_reward_for(&self, kind: ContentIndex) -> i64 {
        self.get(kind).map(|view| view.xp_reward).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_sim::xp_curve::{XpCurveOp, XpCurveOperand};

    /// 造一批彼此不同的测试用索引——`ContentIndex` 没有公开的裸整数
    /// 构造函数（[`ContentIndex`] 模块文档：合法索引只能来自
    /// `Interner::intern`），本函数登记 `count` 个各不相同的占位标识符，
    /// 换取一批已知互不相同的索引。
    fn distinct_indices(count: usize) -> Vec<ContentIndex> {
        let mut interner = Interner::new();
        (0..count)
            .map(|i| interner.intern(NamespacedId::parse(&format!("test:slot_{i}")).unwrap()))
            .collect()
    }

    fn sample_curve(seed: i64) -> XpCurveDef {
        XpCurveDef {
            id: ContentIndex::default(),
            base_requirement: seed,
            instructions: vec![XpCurveOp::Ref(XpCurveOperand::Const(seed))],
        }
    }

    #[test]
    fn 定义后可以查到同一条曲线() {
        // Arrange
        let mut table = XpCurveTable::new();
        let [index] = distinct_indices(1)[..] else {
            unreachable!()
        };

        // Act
        table
            .define(index, sample_curve(140))
            .expect("首次定义应当成功");

        // Assert
        assert_eq!(table.get(index).unwrap().base_requirement, 140);
    }

    #[test]
    fn 重复定义同一个曲线索引返回错误() {
        // Arrange
        let mut table = XpCurveTable::new();
        let [index] = distinct_indices(1)[..] else {
            unreachable!()
        };
        table
            .define(index, sample_curve(140))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, sample_curve(80));

        // Assert
        assert_eq!(result, Err(XpCurveError::DuplicateDefinition(index)));
    }

    #[test]
    fn 绑定一条不存在的曲线返回错误() {
        // Arrange
        let table = XpCurveTable::new();
        let mut bindings = XpCurveBindings::new();
        let [class_id, missing_curve] = distinct_indices(2)[..] else {
            unreachable!()
        };

        // Act
        let result = bindings.bind_class(&table, class_id, missing_curve);

        // Assert
        assert_eq!(result, Err(XpCurveError::UnknownCurve(missing_curve)));
    }

    #[test]
    fn 职业绑定优先于种族绑定() {
        // Arrange
        let mut table = XpCurveTable::new();
        let indices = distinct_indices(5);
        let [
            class_curve_id,
            race_curve_id,
            profession,
            race,
            default_curve,
        ] = indices[..]
        else {
            unreachable!()
        };
        table
            .define(class_curve_id, sample_curve(140))
            .expect("定义应当成功");
        table
            .define(race_curve_id, sample_curve(80))
            .expect("定义应当成功");
        let mut bindings = XpCurveBindings::new();
        bindings
            .bind_class(&table, profession, class_curve_id)
            .expect("绑定应当成功");
        bindings
            .bind_race(&table, race, race_curve_id)
            .expect("绑定应当成功");
        table
            .define(default_curve, sample_curve(1))
            .expect("定义应当成功");
        let resolver = RegistryXpCurves {
            curves: &table,
            bindings: &bindings,
            default_curve,
        };

        // Act
        let curve = resolver.curve_for(profession, race);

        // Assert：职业绑定的种子值（140）胜出，不是种族的（80）。
        assert_eq!(curve.base_requirement, 140);
    }

    #[test]
    fn 都未绑定时退回默认曲线() {
        // Arrange
        let mut table = XpCurveTable::new();
        let indices = distinct_indices(3);
        let [default_curve, profession, race] = indices[..] else {
            unreachable!()
        };
        table
            .define(default_curve, sample_curve(99))
            .expect("定义应当成功");
        let bindings = XpCurveBindings::new();
        let resolver = RegistryXpCurves {
            curves: &table,
            bindings: &bindings,
            default_curve,
        };

        // Act
        let curve = resolver.curve_for(profession, race);

        // Assert
        assert_eq!(curve.base_requirement, 99);
    }

    #[test]
    fn racetable对未注册的种类查询击杀经验值返回零() {
        // Arrange
        let table = RaceTable::new();

        // Act
        let reward = table.xp_reward_for(ContentIndex::default());

        // Assert
        assert_eq!(reward, 0);
    }
}
