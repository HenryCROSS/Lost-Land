//! 物品运行时实例——落地 `knowledge/design/item-system.md` 一节「定义与
//! 实例分离」的另一半：`ItemDef`（命名空间 ID、堆叠上限、价格……）是
//! 静态定义，注册表持有，定义在下游的 `ll-mod::item`（`ll-sim` 不能
//! 依赖 `ll-mod`，依赖方向见 `crate::skill` 模块文档「为什么这里重新
//! 声明了一遍」一节同一条约束）；[`ItemStack`] 是运行时实例，本模块
//! 定义——数量、耐久这类"千支箭共享一份定义，但各自库存不同"的状态
//! 落在这里，不落在 `ItemDef` 上。
//!
//! # 本批次范围：只有 `count`/`durability`，`owner`/`quality`/`modifiers`
//! 留给后续批次
//!
//! `item-system.md` 一节给出的完整 `ItemStack` 还有 `owner: Owner`（归属）
//! 与 `quality: Quality`（品质）、`modifiers: Vec<ContentIndex>`（附魔
//! 词条）三个字段——本批次（P6 第一批：物品基础）明确排除背包/地面
//! 物品/归属（见项目任务书「本批次范围」一节），这三个字段目前没有
//! 任何消费者，也没有必须提前敲定的形状依赖（`Owner`/`Quality` 两个
//! 类型本身都还没有 Rust 定形），提前声明只会制造不知道该怎么处理的
//! 死字段——与 `ll_mod::resource_pool` 模块文档「第一批范围」一节同一
//! 条 YAGNI 判断：先打通一条完整链路，下一批需要归属/品质时再补，
//! [`can_merge`] 的比较逻辑届时随字段一起扩展（见该函数文档「为什么
//! 只比较这两个字段」一节）。
//!
//! # 堆叠与合并
//!
//! [`can_merge`]/[`merge_stacks`]/[`split_stack`] 落地 `item-system.md`
//! 二节「堆叠规则」——`merge_stacks` 用同一条公式覆盖「合并后不超过
//! 上限」「合并后超过上限，一满一余」「`stack_limit == 1` 的物品合并
//! 后两堆数量原样不变（等价于"不能堆叠"）」三种结果，不是三条独立的
//! 特判分支，见其文档「为什么不用三条特判分支」一节。

use ll_core::ident::ContentIndex;
use std::fmt;

/// 物品的运行时实例——一堆同一种物品，`item-system.md` 一节表格右列。
///
/// 与 `ll_mod::item::ItemDef`（本模块不能直接引用它，依赖方向不
/// 允许，这里只能点名、不能用 intra-doc link 指过去）的关系正是
/// 「类型 ID / 实例 ID 分离」
/// （`identity-and-ids.md`）在物品层的落点：`def` 是指向类型的引用，
/// `count`/`durability` 是这一份实例独有的状态——同一个 `def` 可以
/// 同时存在很多个 `ItemStack`，互不影响，这正是「一千支箭共享一份
/// 定义，运行时只需要一个 `count: u32`」（`item-system.md` 一节「为
/// 什么必须分离」）里"共享定义、独立实例"这句话的直接体现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    /// 指向 `ItemDef` 的内容索引——这一堆是"哪种"物品。
    pub def: ContentIndex,
    /// 这一堆当前的数量，恒 ≥ 1（数量归零的堆没有存在的意义，调用方
    /// 应当在数量降到零时把整个 `ItemStack` 从容器里移除，本类型自身
    /// 不禁止构造出 `count == 0` 的值——这类不变式属于未来背包批次的
    /// 容器职责，不是这个纯数据类型自己的校验范围）。
    pub count: u32,
    /// 当前耐久——`None` 表示这件物品没有耐久概念（材料、消耗品），
    /// `Some` 表示有（武器、装备）。上限（`ItemDef.max_durability`）
    /// 与扣减耐久的具体规则是耐久系统落地批次（第五批）的工作，本
    /// 批次只落地这个字段的形状,并用它证明"同一个 `def` 的两个
    /// `ItemStack` 各自独立"这条区分是实的（见 `crate::item` 模块的
    /// 测试）。
    pub durability: Option<i32>,
}

impl ItemStack {
    /// 造一个没有耐久概念的堆（材料、消耗品……）。
    pub const fn new(def: ContentIndex, count: u32) -> Self {
        ItemStack {
            def,
            count,
            durability: None,
        }
    }

    /// 造一个带耐久的堆（武器、装备……）。
    pub const fn with_durability(def: ContentIndex, count: u32, durability: i32) -> Self {
        ItemStack {
            def,
            count,
            durability: Some(durability),
        }
    }
}

/// 物品堆操作可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStackError {
    /// [`merge_stacks`] 的两个堆不满足 [`can_merge`]——`def` 不同,或者
    /// `def` 相同但实例状态不同（当前只有 `durability`,见
    /// [`can_merge`] 文档）。
    CannotMerge,
    /// [`split_stack`] 请求的拆分数量非法：`0`（拆出一个空堆没有意义）
    /// 或 `>= available`（大于等于原堆当前数量——拆分必须留下两个都
    /// 非空的堆，"拆出全部数量"应当直接搬移整个 `ItemStack`,不是本
    /// 函数要处理的场景）。
    InvalidSplitAmount {
        /// 请求拆出的数量。
        requested: u32,
        /// 原堆当前的数量。
        available: u32,
    },
}

impl fmt::Display for ItemStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemStackError::CannotMerge => write!(f, "两个物品堆的定义或实例状态不同,无法合并"),
            ItemStackError::InvalidSplitAmount {
                requested,
                available,
            } => write!(f, "拆分数量 {requested} 非法（原堆当前数量 {available}）"),
        }
    }
}

impl std::error::Error for ItemStackError {}

/// 两个堆是否可以合并——`item-system.md` 二节原文：「当且仅当 `def`
/// 相同且全部实例状态相同」。
///
/// # 为什么只比较 `def`/`durability` 两个字段
///
/// 设计文档的完整判据还比较 `owner`/`quality`/`modifiers`——本批次
/// [`ItemStack`] 还没有这三个字段（见模块文档「本批次范围」一节），
/// 这里只能比较已经存在的字段。这不是对设计文档判据的简化：文档原文
/// 特别强调「新增任何实例字段都自动被覆盖：以后给 `ItemStack` 加了
/// 『绑定角色』字段，只要补进这个比较，堆叠逻辑就自动正确」——本函数
/// 现在按同一条纪律实现，未来补 `owner`/`quality`/`modifiers` 字段时
/// 只需要在这里追加对应的比较项，不需要改 `merge_stacks`/`split_stack`
/// 的算法本身。
pub fn can_merge(a: &ItemStack, b: &ItemStack) -> bool {
    a.def == b.def && a.durability == b.durability
}

/// 合并两个堆，`stack_limit` 是这个 `def` 声明的堆叠上限
/// （`ItemDef.stack_limit`，本模块不能直接读——依赖方向，见模块
/// 文档——因此由调用方传入这一个数值,不是整个 `ItemDef`）。
///
/// 返回 `(合并后的主堆, 若有溢出则是溢出堆)`——`a` 优先吸收 `b`，直到
/// 触及 `stack_limit`，多出的部分留在第二个堆里,数量保持
/// `a.count + b.count` 不变（守恒，不凭空产生或消灭物品）。
///
/// # 为什么不用三条特判分支
///
/// `item-system.md` 二节要求覆盖三种场景：合并后不超上限、合并后超
/// 上限（一满一余）、`stack_limit == 1` 的物品不能堆叠。若把
/// `stack_limit == 1` 单独写成一条"直接拒绝"分支，是在重复
/// [`SlotMask::intersects`](../../../knowledge/design/equipment-slots.md)
/// 一节论证过的错误——那份文档指出"看起来是特例，实际是同一个机制"，
/// 这里同理：当 `stack_limit == 1` 时，`a.count + b.count`（通常是
/// `1 + 1 = 2`）必然大于上限 `1`，一般分支自动产出「主堆截到 1、溢出
/// 堆拿走剩下的 1」——溢出堆恰好等于原来的 `b`、主堆恰好等于原来的
/// `a`，从调用方视角看就是"两堆各自数量原样不变，什么都没发生"，
/// 这正是"不能堆叠"这句话的行为含义，不需要另开一条特判分支去获得
/// 同样的结果。
///
/// # 错误
///
/// 两个堆不满足 [`can_merge`] 时返回 [`ItemStackError::CannotMerge`]
/// ——`def` 不同的两堆合并没有意义（数量该算进哪个物品的库存？），
/// 调用方应当在合并前自行判断是否要走这条路径,本函数不做静默丢弃或
/// 拒绝合并却假装成功这类模糊处理。
pub fn merge_stacks(
    a: ItemStack,
    b: ItemStack,
    stack_limit: u32,
) -> Result<(ItemStack, Option<ItemStack>), ItemStackError> {
    if !can_merge(&a, &b) {
        return Err(ItemStackError::CannotMerge);
    }

    let total = a.count.saturating_add(b.count);
    if total <= stack_limit {
        Ok((ItemStack { count: total, ..a }, None))
    } else {
        let merged = ItemStack {
            count: stack_limit,
            ..a
        };
        let overflow = ItemStack {
            count: total - stack_limit,
            ..b
        };
        Ok((merged, Some(overflow)))
    }
}

/// 从 `stack` 里拆出 `amount` 个，产出 `(拆出的新堆, 原堆剩余部分)`——
/// 两者的 `def`/`durability` 与原堆一致（拆分不改变"是什么物品"，只
/// 改变"有多少个"）。
///
/// # 错误
///
/// `amount == 0`（拆出一个空堆没有意义）或 `amount >= stack.count`
/// （拆分必须留下两个都非空的堆——拆出全部数量等于整堆搬移，调用方
/// 应当直接搬移原 `ItemStack`,不必经过本函数）时返回
/// [`ItemStackError::InvalidSplitAmount`]。
pub fn split_stack(
    stack: ItemStack,
    amount: u32,
) -> Result<(ItemStack, ItemStack), ItemStackError> {
    if amount == 0 || amount >= stack.count {
        return Err(ItemStackError::InvalidSplitAmount {
            requested: amount,
            available: stack.count,
        });
    }

    let remainder = stack.count - amount;
    Ok((
        ItemStack {
            count: amount,
            ..stack
        },
        ItemStack {
            count: remainder,
            ..stack
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出一个占位索引——
    /// 本模块的堆叠逻辑只关心索引之间"相不相等"，不关心它们具体指向
    /// 哪条命名空间标识符，与 `crate::traits` 测试同一条既有手法
    /// （见其 `tests::index` 文档）。
    fn index(raw: &str) -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    #[test]
    fn 同一个物品定义的两个堆各自携带独立的数量与耐久() {
        // 核心区分验收：ItemDef 是共享的类型身份,ItemStack 是各自独立
        // 的运行时实例——同一个 def 的两个堆修改其中一个不影响另一个。
        // Arrange
        let sword_def = index("lostland:iron_sword");
        let mut stack_a = ItemStack::with_durability(sword_def, 1, 80);
        let stack_b = ItemStack::with_durability(sword_def, 1, 100);

        // Act：只修改 stack_a 的耐久。
        stack_a.durability = Some(40);

        // Assert：一次断言同时验证两者——stack_a 的修改真的生效，
        // stack_b 完全未受影响,证明两个 ItemStack 是各自独立的实例,
        // 不是共享同一份存储的两个引用。
        assert_eq!(
            (stack_a.durability, stack_b.durability),
            (Some(40), Some(100))
        );
    }

    #[test]
    fn 合并两堆不超过上限时合成一堆() {
        // Arrange
        let arrow_def = index("lostland:arrow");
        let a = ItemStack::new(arrow_def, 30);
        let b = ItemStack::new(arrow_def, 20);

        // Act
        let result = merge_stacks(a, b, 99);

        // Assert
        assert_eq!(result, Ok((ItemStack::new(arrow_def, 50), None)));
    }

    #[test]
    fn 合并超过上限时产出一满一余且余数正确() {
        // Arrange
        let arrow_def = index("lostland:arrow");
        let a = ItemStack::new(arrow_def, 60);
        let b = ItemStack::new(arrow_def, 60);

        // Act
        let result = merge_stacks(a, b, 99);

        // Assert
        assert_eq!(
            result,
            Ok((
                ItemStack::new(arrow_def, 99),
                Some(ItemStack::new(arrow_def, 21))
            ))
        );
    }

    #[test]
    fn 堆叠上限为一的物品合并后两堆数量原样不变() {
        // stack_limit == 1 不需要特判分支——一般公式自动产出"什么都
        // 没发生"的结果,见 merge_stacks 文档「为什么不用三条特判
        // 分支」一节。
        // Arrange
        let sword_def = index("lostland:iron_sword");
        let a = ItemStack::new(sword_def, 1);
        let b = ItemStack::new(sword_def, 1);

        // Act
        let result = merge_stacks(a, b, 1);

        // Assert
        assert_eq!(
            result,
            Ok((
                ItemStack::new(sword_def, 1),
                Some(ItemStack::new(sword_def, 1))
            ))
        );
    }

    #[test]
    fn 不同物品定义的两堆无法合并() {
        // Arrange：两个索引必须出自同一个 Interner 才会真的不同——
        // `index` 帮手各自新开一个 Interner,同一个字符串在不同
        // Interner 里都会拿到号段起点的索引,不足以证明"不同 def"这条
        // 前提,这里改为显式共用一个 Interner intern 两个不同的标识符。
        let mut interner = Interner::new();
        let sword_def = interner.intern(NamespacedId::parse("lostland:iron_sword").unwrap());
        let shield_def = interner.intern(NamespacedId::parse("lostland:wooden_shield").unwrap());
        let a = ItemStack::new(sword_def, 1);
        let b = ItemStack::new(shield_def, 1);

        // Act
        let result = merge_stacks(a, b, 10);

        // Assert
        assert_eq!(result, Err(ItemStackError::CannotMerge));
    }

    #[test]
    fn 耐久不同的两堆无法合并() {
        // 同一个 def,但一把用了一半耐久、一把全新——item-system.md
        // 二节原文举的正是这个例子:「耐久 50/100 的剑不能和全新的剑
        // 堆在一起」。
        // Arrange
        let sword_def = index("lostland:iron_sword");
        let worn = ItemStack::with_durability(sword_def, 1, 50);
        let fresh = ItemStack::with_durability(sword_def, 1, 100);

        // Act
        let result = merge_stacks(worn, fresh, 10);

        // Assert
        assert_eq!(result, Err(ItemStackError::CannotMerge));
    }

    #[test]
    fn 拆分堆产出请求数量与剩余数量() {
        // Arrange
        let arrow_def = index("lostland:arrow");
        let stack = ItemStack::new(arrow_def, 30);

        // Act
        let result = split_stack(stack, 12);

        // Assert
        assert_eq!(
            result,
            Ok((ItemStack::new(arrow_def, 12), ItemStack::new(arrow_def, 18)))
        );
    }

    #[test]
    fn 拆分数量大于等于原堆数量时返回错误() {
        // Arrange
        let arrow_def = index("lostland:arrow");
        let stack = ItemStack::new(arrow_def, 5);

        // Act
        let result = split_stack(stack, 5);

        // Assert
        assert_eq!(
            result,
            Err(ItemStackError::InvalidSplitAmount {
                requested: 5,
                available: 5
            })
        );
    }

    #[test]
    fn 拆分数量为零时返回错误() {
        // Arrange
        let arrow_def = index("lostland:arrow");
        let stack = ItemStack::new(arrow_def, 5);

        // Act
        let result = split_stack(stack, 0);

        // Assert
        assert!(result.is_err());
    }
}
