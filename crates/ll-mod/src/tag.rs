//! 标签定义表：`register-tag` 的存储落点（耐久标签批次）。
//!
//! # 这张表为什么存在
//!
//! 项目所有者裁定「每个物品可以有个标签的列表，带有多个标签」，用来
//! 回答此前被错误地交给槽位回答的那个问题——**这件东西是什么**。
//! 原话点出了槽位判据错在哪：「副手也可能拿着武器,例如双刀,双盾」，
//! 副手不等于盾，槽位携带不了这个信息。
//!
//! 标签本身必须**先注册**：[`crate::script_item_api`] 的
//! `register-item-tag` 只接受已经登记在本表里的标签，引用一个没注册过的
//! 标签当场报错、整个 mod 装载失败（ADR 0017「注册期完整校验」）。
//! 这条纪律拦的是 `"lostlan:armor"` 这类拼写错误——它的症状是「标签
//! 静默不生效」，一件甲从此再也不掉耐久却没有任何报错，是最难查的一类
//! 内容缺陷。判据与做法照抄 `register-recipe-category` +
//! `recipe-category-requires-subclass!` 那一对既有先例。
//!
//! # 结构手法照抄 `weapon_category`/`damage_category`
//!
//! `BTreeMap<ContentIndex, TagDef>` + `define` 查重 + `Display` 错误
//! 类型，与 [`crate::weapon_category::WeaponCategoryTable`]/
//! [`crate::damage_category::DamageCategoryTable`] 逐条同构——同一类
//! 「可扩展项没有自然上限的开放集合」，不为这一张表另发明一套写法。
//! 用 `BTreeMap` 而不是物品表那样的列式存储：标签数量是几十的量级、
//! 且查询发生在**注册期**（`add_tag` 折算磨损通道）而不是结算热路径,
//! 稀疏表比按 `ContentIndex` 下标的稠密列更合适,与那两张表同一条判断。
//!
//! # 为什么叫 `register-tag` 而不是 `register-item-tag`
//!
//! `register-item-tag` 这个名字留给「**给某件物品挂上**一个标签」那半
//! （在 [`crate::script_item_api`]，形状照 `register-item-stat-bonus`/
//! `register-item-resistance`）。两半的命名关系与
//! `register-damage-category` / `register-item-damage-category` 完全
//! 一致：`register-<东西>` 声明这个东西存在，`register-item-<东西>` 把它
//! 挂到某件物品上。
//!
//! 名字里不带 `item` 也是诚实的：标签本身没有任何一处是物品专属的。
//! 今天唯一的消费者确实是 `ItemDef.tags`（本模块不假装有别的），但
//! 将来生物/地形要分类时复用同一张表不需要改名——**这不是为将来预留
//! 字段**（YAGNI 管的是字段与机制，不是名字），只是不给一个通用概念
//! 起一个会立刻过时的名字。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;
use ll_sim::item::WearChannels;

/// 单条标签声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagDef {
    /// 带有这个标签的物品**因此获得**哪些耐久磨损通道——
    /// [`WearChannels::NONE`] 表示这个标签与耐久无关（纯分类标签，
    /// 例如将来的「可燃」「金属」），是完全合法且预期常见的取值。
    ///
    /// 这是标签今天在引擎侧**唯一**的后果。将来标签有了第二种后果
    /// （比如「火焰会点燃带可燃标签的东西」），照
    /// `register-recipe-category` + `recipe-category-requires-subclass!`
    /// 的先例追加一条独立的 `tag-*!` 函数与一个独立字段，不是把这个
    /// 字段改造成一个万能的「标签效果」联合体。
    pub wear: WearChannels,
}

/// 标签注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagError::DuplicateDefinition(index) => {
                write!(f, "标签索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for TagError {}

/// 标签定义表：`ContentIndex`（标签自身的命名空间标识符）→ [`TagDef`]。
#[derive(Debug, Default, Clone)]
pub struct TagTable {
    entries: BTreeMap<ContentIndex, TagDef>,
}

impl TagTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条标签定义。
    pub fn define(&mut self, index: ContentIndex, def: TagDef) -> Result<(), TagError> {
        if self.entries.contains_key(&index) {
            return Err(TagError::DuplicateDefinition(index));
        }
        self.entries.insert(index, def);
        Ok(())
    }

    /// 这个索引当前是否已经登记成一个标签——`register-item-tag` 用它
    /// 判断「这个 id 真的是个标签」，而不是只用
    /// `Registry::get` 判断「这个 id 被 intern 过」：后者对**任何**已注册
    /// 内容都为真，把物品 id 当标签传进来同样能通过，那条校验就等于没写。
    pub fn is_defined(&self, tag: ContentIndex) -> bool {
        self.entries.contains_key(&tag)
    }

    /// 查询一条标签定义，未注册的索引返回 `None`（ADR 0015）。
    pub fn get(&self, tag: ContentIndex) -> Option<TagDef> {
        self.entries.get(&tag).copied()
    }

    /// 按索引升序遍历全部已登记标签——内容值哈希遍历用（约束 C5：
    /// `BTreeMap` 有序，遍历顺序确定）。
    pub fn iter(&self) -> impl Iterator<Item = (ContentIndex, TagDef)> + '_ {
        self.entries.iter().map(|(index, def)| (*index, *def))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    #[test]
    fn 登记后可以查到同一条标签定义() {
        // Arrange
        let mut interner = Interner::new();
        let armor = index(&mut interner, "lostland:armor");
        let mut table = TagTable::new();

        // Act
        let result = table.define(
            armor,
            TagDef {
                wear: WearChannels::ON_HIT,
            },
        );

        // Assert
        assert_eq!(result, Ok(()));
        assert!(table.is_defined(armor));
        assert_eq!(
            table.get(armor).map(|def| def.wear),
            Some(WearChannels::ON_HIT)
        );
    }

    #[test]
    fn 重复登记同一个索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let armor = index(&mut interner, "lostland:armor");
        let mut table = TagTable::new();
        table
            .define(
                armor,
                TagDef {
                    wear: WearChannels::NONE,
                },
            )
            .expect("首次登记应当成功");

        // Act
        let result = table.define(
            armor,
            TagDef {
                wear: WearChannels::ON_USE,
            },
        );

        // Assert
        assert_eq!(result, Err(TagError::DuplicateDefinition(armor)));
    }

    #[test]
    fn 未登记的索引查不到也不算已定义() {
        // Arrange
        let mut interner = Interner::new();
        let never = index(&mut interner, "lostland:never_registered");
        let table = TagTable::new();

        // Act & Assert
        assert!(!table.is_defined(never));
        assert_eq!(table.get(never), None);
    }
}
