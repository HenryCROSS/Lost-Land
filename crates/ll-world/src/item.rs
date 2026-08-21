//! 物品运行时实例与地面物品堆——落地 `knowledge/design/item-system.md`
//! 「定义与实例分离」的运行时一半，以及「四、位置」一节里
//! `ItemLocation::Ground` 对应的存储形状。
//!
//! # 为什么从 `ll-sim` 挪到本模块（P6 第二批）
//!
//! [`ItemStack`]（连同 [`can_merge`]/[`merge_stacks`]/[`split_stack`]）
//! 此前（P6 第一批）声明在 `ll_sim::item`——当时它只是一个供堆叠/合并
//! 算法测试用的纯数据类型，没有任何容器持有它，放在 `ll-sim` 是当时
//! 唯一能落地的选择（`ItemDef` 定义在下游的 `ll-mod`，`ll-sim` 依赖
//! `ll-world`，见该模块彼时的文档）。
//!
//! 本批次要让**背包**（[`crate::entity::Agent::inventory`]）与**地面
//! 物品**（[`crate::state::WorldState::ground_items`]）真正持有它——
//! 而背包/地面物品是世界状态的一部分（ADR 0022：新增世界状态必须进
//! `WorldState::hash()`），`Agent`/`WorldState` 都定义在 `ll-world`，
//! `ll-world` 不能依赖 `ll-sim`（依赖方向，规格 §5：`ll-world` ←
//! `ll-sim`，反过来会成环）。`ItemStack` 因此必须挪到本模块——与
//! `crate::resource_pool` 模块文档「为什么这些类型定义在 `ll-sim`，
//! 不是 `ll_mod::trait_def`」一节完全对称的判断，只是这次挪动方向
//! 相反（从更上游的 `ll-sim` 挪到更下游的 `ll-world`）。`ll_sim::item`
//! 改为 `pub use` 本模块的定义（与 `ll_mod::trait_def` 现在 `pub use`
//! `ll_sim::resource_pool` 是同一条先例，见其文档），不再维护一份会
//! 漂移的副本；`can_merge`/`merge_stacks`/`split_stack` 三个纯函数不
//! 依赖任何 `ll-sim` 专属类型（只用得到 `ItemStack` 自身），跟着一起
//! 挪动没有额外代价。
//!
//! # `Owner` 本批次仍然不落地——没有真正的消费者
//!
//! `item-system.md` 三节的 `Owner`（`Unowned`/`Player`/`Npc`/`Faction`/
//! `Shop`）按文档所写驱动三件事：偷窃判定、随从装备归属、商店库存。
//! 核实过当前代码库：没有偷窃系统（无治安反应/目击判定）、没有商店
//! 系统（`Shop` 相关字段/注册表不存在）、没有 NPC 私产系统——三个消费
//! 场景一个都不存在。本批次的四条端到端（拾取/丢弃/合并/老化）也不
//! 需要判断"这件物品归谁"：任何实体都能捡起地面上的任何物品，没有
//! 权限检查。给 [`ItemStack`] 加一个没有任何读者的 `owner` 字段，正是
//! 项目已经栽过十四次的那类死字段（与 P6 第一批 `owner`/`quality`/
//! `modifiers` 排除在外同一条 YAGNI 判断，见 `ll_sim::item` 模块此前
//! 文档「本批次范围」一节）。
//!
//! 最小形状记录在此，供未来真正需要它的批次（偷窃系统/商店系统）参考：
//!
//! ```text
//! pub enum Owner {
//!     Unowned,
//!     Player,
//!     Npc(EntityId),
//!     Faction(ContentIndex),
//!     Shop(EntityId),
//! }
//! ```
//!
//! 落地时机：`Owner` 一旦加进 [`ItemStack`]，[`can_merge`] 也必须同步
//! 追加这一条比较（`item-system.md` 二节原文：「新增任何实例字段都
//! 自动被覆盖……只要补进这个比较，堆叠逻辑就自动正确」）——两者是同一
//! 个改动的两半，不能只加字段不改比较逻辑。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};
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
///
/// # 参与序列化（P6 第二批）
///
/// 现在真正被 [`crate::entity::Agent::inventory`]/
/// [`GroundItemStack::stack`] 持有，两者都是世界状态的一部分，必须
/// 完整序列化——`def`/`count`/`durability` 全是纯整数或已可序列化的
/// [`ContentIndex`]，直接派生即可，不需要自定义编解码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    /// 指向 `ItemDef` 的内容索引——这一堆是"哪种"物品。
    pub def: ContentIndex,
    /// 这一堆当前的数量，恒 ≥ 1（数量归零的堆没有存在的意义，调用方
    /// 应当在数量降到零时把整个 `ItemStack` 从容器里移除，本类型自身
    /// 不禁止构造出 `count == 0` 的值——这类不变式属于容器自身
    /// （背包/地面）的职责，不是这个纯数据类型自己的校验范围）。
    pub count: u32,
    /// 当前耐久——`None` 表示这件物品没有耐久概念（材料、消耗品），
    /// `Some` 表示有（武器、装备）。上限（`ItemDef.max_durability`）
    /// 与扣减耐久的具体规则是耐久系统落地批次（第五批）的工作，本
    /// 批次只落地这个字段的形状,并用它证明"同一个 `def` 的两个
    /// `ItemStack` 各自独立"这条区分是实的（见本模块的测试）。
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

/// 世界某个位置上的一堆地面物品（`item-system.md` 四节
/// `ItemLocation::Ground { pos, dropped_at }`）——[`crate::state::WorldState::ground_items`]
/// 的元素类型。
///
/// # 为什么不是完整的 `ItemLocation` 枚举
///
/// 设计文档的 `ItemLocation` 还有 `Inventory`/`Equipped`/`Container`
/// 三个变体——本批次只落地背包（[`crate::entity::Agent::inventory`]，
/// 直接是 `Vec<ItemStack>`，不经 `ItemLocation` 包装）与地面两种位置
/// （见项目任务书「本批次范围」一节：装备栏位是第三批、箱子/尸体容器
/// 不在本批次范围内）。把还没有任何消费者的 `Equipped`/`Container`
/// 提前塞进一个统一枚举，只会制造两个用不上的死变体——与本模块文档
/// 「`Owner` 本批次仍然不落地」同一条 YAGNI 判断。真正需要统一表示
/// 「物品此刻在哪」时（例如脚本要查询一件物品当前位置），再引入这个
/// 枚举把 `Inventory`/`Ground`/未来的 `Equipped`/`Container` 收拢，不
/// 在本批次提前做。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundItemStack {
    /// 这堆物品所在的世界坐标。
    pub pos: TorusPos,
    /// 具体是哪一堆物品。
    pub stack: ItemStack,
    /// 丢弃/生成时刻——老化清理（[`crate::state::WorldState::cleanup_aged_ground_items`]）
    /// 的判定依据，见该方法文档。
    pub dropped_at: Tick,
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
/// 设计文档的完整判据还比较 `owner`/`quality`/`modifiers`——本模块
/// [`ItemStack`] 还没有这三个字段（见模块文档「`Owner` 本批次仍然不
/// 落地」一节），这里只能比较已经存在的字段。这不是对设计文档判据的
/// 简化：文档原文特别强调「新增任何实例字段都自动被覆盖：以后给
/// `ItemStack` 加了『绑定角色』字段，只要补进这个比较，堆叠逻辑就自动
/// 正确」——本函数现在按同一条纪律实现，未来补 `owner`/`quality`/
/// `modifiers` 字段时只需要在这里追加对应的比较项，不需要改
/// `merge_stacks`/`split_stack` 的算法本身。
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
    use ll_core::torus::TorusSize;

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出一个占位索引——
    /// 本模块的堆叠逻辑只关心索引之间"相不相等"，不关心它们具体指向
    /// 哪条命名空间标识符，与 `crate::terrain` 测试同一条既有手法。
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

    #[test]
    fn 地面物品堆序列化往返后与原值相等() {
        // GroundItemStack 现在参与 WorldState 序列化（P6 第二批）——
        // 单独验证一次结构本身的往返,不依赖整个 WorldState。
        // Arrange
        let arrow_def = index("lostland:arrow");
        let size = TorusSize::new(64, 64).expect("64x64 是合法尺寸");
        let original = GroundItemStack {
            pos: size.wrap(10, 20),
            stack: ItemStack::new(arrow_def, 5),
            dropped_at: Tick(123),
        };

        // Act
        let encoded = serde_json::to_string(&original).expect("全部字段均已可派生序列化");
        let decoded: GroundItemStack =
            serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }
}
