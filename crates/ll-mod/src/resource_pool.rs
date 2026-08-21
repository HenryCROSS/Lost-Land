//! 资源池注册表——落地 `knowledge/design/resource-pools-and-rest.md` 二节
//! 「注册身份层统一」的核心形状：`ResourcePoolDef` 是一个独立内容类型,
//! 被 `TraitDef.granted_resource_pools`（[`crate::trait_def::ResourcePoolGrant`]）
//! 引用,不新开一套与 `TraitTable`/`RaceTable` 不同的存储手法——第一批
//! 只落地 [`ll_sim::resource_pool::ResourcePoolShape::Scalar`]（法力池
//! 一类的标量池）,见 [`ll_sim::resource_pool`] 模块文档「本批次范围」
//! 一节。血池不经过这张表——它就是 `Agent::health` 本身,完全排除在
//! 资源池注册表之外（`resource-pools-and-rest.md` 五节）。
//!
//! # 照抄 `race.rs`/`trait_def.rs` 已验证的模式
//!
//! 私有字段 + `ResourcePoolTable::define` 注册期完整校验（ADR 0017）+
//! `ResourcePoolView`/`ResourcePoolAttrs` 一读一写两个薄视图——本模块
//! 不是第一次证明这套模式好用,是又一次复用。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::resource_pool::{RegenRule, ResourcePoolCatalog, ResourcePoolRule, ResourcePoolShape};

/// 单条资源池声明：本体与 mod 注册资源池时共用的同一个输入形状——
/// 「本体即 Mod」在资源池层面的验收标的，理由同
/// [`crate::trait_def::TraitDef`] 文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePoolDef {
    /// 命名空间标识符，例如 `lostland:sorcery_points`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——与 `TraitDef`/`RaceDef`
    /// 同一条既有惯例。
    pub display_name_key: NamespacedId,
    /// 池的形状——本批次只支持 [`ResourcePoolShape::Scalar`]。
    pub shape: ResourcePoolShape,
    /// 恢复节奏，与 `shape` 正交（`resource-pools-and-rest.md` 四节）。
    pub regen_rule: RegenRule,
}

/// [`ResourcePoolTable::define`] 实际存进列式存储的属性子集——不含
/// `id`，理由同 [`crate::race::RaceAttrs`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePoolAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 池的形状。
    pub shape: ResourcePoolShape,
    /// 恢复节奏。
    pub regen_rule: RegenRule,
}

/// 资源池注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePoolError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for ResourcePoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourcePoolError::DuplicateDefinition(index) => {
                write!(f, "资源池索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for ResourcePoolError {}

/// 一次资源池查询命中的完整结果，理由同 [`crate::race::RaceView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourcePoolView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 池的形状。
    pub shape: ResourcePoolShape,
    /// 恢复节奏。
    pub regen_rule: RegenRule,
}

/// 资源池属性的列式存储：按 [`ContentIndex`] 下标索引，与
/// [`crate::trait_def::TraitTable`] 同一套道理——下标空间是全局
/// `ContentIndex` 号段的一部分，因此同样维护一份 `defined` 位图。
#[derive(Debug, Default, Clone)]
pub struct ResourcePoolTable {
    display_name_key: Vec<Option<NamespacedId>>,
    shape: Vec<Option<ResourcePoolShape>>,
    regen_rule: Vec<RegenRule>,
    defined: Vec<bool>,
}

impl ResourcePoolTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上资源池属性。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: ResourcePoolAttrs,
    ) -> Result<(), ResourcePoolError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.shape.resize(new_len, None);
            self.regen_rule.resize(new_len, RegenRule::None);
        }

        if self.defined[idx] {
            return Err(ResourcePoolError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.shape[idx] = Some(attrs.shape);
        self.regen_rule[idx] = attrs.regen_rule;
        Ok(())
    }

    /// 给定的资源池索引当前是否已经登记过属性。
    pub fn is_defined(&self, pool: ContentIndex) -> bool {
        self.defined
            .get(pool.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个资源池的完整属性，未注册的索引返回 `None`（ADR 0015）。
    pub fn get(&self, pool: ContentIndex) -> Option<ResourcePoolView<'_>> {
        if !self.is_defined(pool) {
            return None;
        }
        let idx = pool.get() as usize;
        Some(ResourcePoolView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            shape: self.shape[idx].expect("defined 为真时 shape 必已写入"),
            regen_rule: self.regen_rule[idx],
        })
    }
}

/// `ll_sim::resource_pool::ResourcePoolCatalog` 的真实实现——
/// `resolve_use_skill`/回合开始的自动恢复检查通过这个 impl 真正查到
/// 一个池的恢复节奏，见 `ll_sim::resource_pool` 模块文档同一套依赖
/// 倒置手法。
impl ResourcePoolCatalog for ResourcePoolTable {
    fn resource_pool(&self, pool: ContentIndex) -> Option<ResourcePoolRule> {
        self.get(pool).map(|view| ResourcePoolRule {
            regen_rule: view.regen_rule,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 新建的资源池表查询任意索引均为未注册() {
        // Arrange
        let table = ResourcePoolTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 注册后查询能拿到完整的形状与恢复节奏() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:sorcery_points").unwrap());
        let mut table = ResourcePoolTable::new();

        // Act
        table
            .define(
                index,
                ResourcePoolAttrs {
                    display_name_key: NamespacedId::parse("lostland:pool.sorcery_points").unwrap(),
                    shape: ResourcePoolShape::Scalar,
                    regen_rule: RegenRule::OnTurnStart { amount: 2 },
                },
            )
            .expect("首次定义应当成功");

        // Assert
        let view = table.get(index).expect("已注册");
        assert_eq!(view.shape, ResourcePoolShape::Scalar);
        assert_eq!(view.regen_rule, RegenRule::OnTurnStart { amount: 2 });
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:ki").unwrap());
        let mut table = ResourcePoolTable::new();
        table
            .define(
                index,
                ResourcePoolAttrs {
                    display_name_key: NamespacedId::parse("lostland:pool.ki").unwrap(),
                    shape: ResourcePoolShape::Scalar,
                    regen_rule: RegenRule::None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            ResourcePoolAttrs {
                display_name_key: NamespacedId::parse("lostland:pool.ki").unwrap(),
                shape: ResourcePoolShape::Scalar,
                regen_rule: RegenRule::None,
            },
        );

        // Assert
        assert_eq!(result, Err(ResourcePoolError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = ResourcePoolTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn resourcepoolcatalog实现查询已注册资源池返回恢复节奏() {
        // 直接验收 `impl ResourcePoolCatalog for ResourcePoolTable`——
        // resolve 真正依赖的正是这个 trait 方法，不是 `get`/`ResourcePoolView`
        // 本身。
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:sorcery_points").unwrap());
        let mut table = ResourcePoolTable::new();
        table
            .define(
                index,
                ResourcePoolAttrs {
                    display_name_key: NamespacedId::parse("lostland:pool.sorcery_points").unwrap(),
                    shape: ResourcePoolShape::Scalar,
                    regen_rule: RegenRule::OnTurnStart { amount: 1 },
                },
            )
            .expect("首次定义应当成功");

        // Act
        let rule = ResourcePoolCatalog::resource_pool(&table, index).expect("已注册");

        // Assert
        assert_eq!(rule.regen_rule, RegenRule::OnTurnStart { amount: 1 });
    }

    #[test]
    fn resourcepoolcatalog实现查询未注册资源池返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = ResourcePoolTable::new();

        // Act & Assert
        assert_eq!(
            ResourcePoolCatalog::resource_pool(&table, never_defined),
            None
        );
    }
}
