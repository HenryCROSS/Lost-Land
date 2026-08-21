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

use crate::entity::AttributeKind;

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

/// 单个装备槽位——[`SlotMask`] 的一个具体位，也是
/// [`crate::entity::Agent::equipment`] 的键类型（装备栏位批次，P6 第
/// 三批，落地 `knowledge/design/equipment-slots.md`）。
///
/// 与 `SlotMask`（多槽位集合）是两个不同类型——该设计文档「一条规则
/// 覆盖所有特例」一节原文点名了这条区分但未给出 `EquipSlot` 的正式
/// 形状（「本文档尚未给出 `EquipSlot` 的正式定义，只给出了槽位表」），
/// 本类型是这条区分在代码里的落地。
///
/// # 为什么是「位下标的新类型」，不是一个 22 变体的 `enum`
///
/// 一个 `enum EquipSlot { MainHand, OffHand, .. }` 只能穷尽引擎已知的
/// 22 个变体——`SlotMask` 给 mod 预留的 10 个高位（见 [`SlotMask`] 模块
/// 文档「mod 扩展位」一节）就无法用同一个类型表示：mod 想引用自己的
/// 保留位时，Rust 的 `enum` 不支持运行期新增变体。`EquipSlot(u8)` 把
/// 「一个槽位」表示成「位下标」本身，22 个引擎槽位只是 22 个具名关联
/// 常量（`u8` 取值 0..=21），mod 保留位（22..=31）用同一个类型、只是
/// 没有具名常量——两者在类型层面完全统一，不需要为"引擎槽位"和"mod
/// 槽位"分裂成两个类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EquipSlot(u8);

impl EquipSlot {
    /// 主手——武器、法杖。
    pub const MAIN_HAND: EquipSlot = EquipSlot(0);
    /// 副手——盾、副武器、法器。
    pub const OFF_HAND: EquipSlot = EquipSlot(1);
    /// 头顶——头盔、帽。
    pub const HEAD: EquipSlot = EquipSlot(2);
    /// 脸部——面具、口罩。
    pub const FACE: EquipSlot = EquipSlot(3);
    /// 眼——眼罩、护目镜。
    pub const EYES: EquipSlot = EquipSlot(4);
    /// 颈——项链、护符。
    pub const NECK: EquipSlot = EquipSlot(5);
    /// 上身——衬衣、内甲。
    pub const BODY: EquipSlot = EquipSlot(6);
    /// 外套——大衣、罩袍。
    pub const OUTER: EquipSlot = EquipSlot(7);
    /// 背部——披风、背包、翅膀。
    pub const BACK: EquipSlot = EquipSlot(8);
    /// 左肩甲。
    pub const SHOULDER_L: EquipSlot = EquipSlot(9);
    /// 右肩甲。
    pub const SHOULDER_R: EquipSlot = EquipSlot(10);
    /// 左臂——护腕、臂甲。
    pub const ARM_L: EquipSlot = EquipSlot(11);
    /// 右臂。
    pub const ARM_R: EquipSlot = EquipSlot(12);
    /// 左手甲——手套。
    pub const HAND_L: EquipSlot = EquipSlot(13);
    /// 右手甲。
    pub const HAND_R: EquipSlot = EquipSlot(14);
    /// 腰带。
    pub const BELT: EquipSlot = EquipSlot(15);
    /// 裙甲。
    pub const TASSET: EquipSlot = EquipSlot(16);
    /// 裤子。
    pub const LEGS: EquipSlot = EquipSlot(17);
    /// 左靴。
    pub const BOOT_L: EquipSlot = EquipSlot(18);
    /// 右靴。
    pub const BOOT_R: EquipSlot = EquipSlot(19);
    /// 左戒。
    pub const RING_L: EquipSlot = EquipSlot(20);
    /// 右戒。
    pub const RING_R: EquipSlot = EquipSlot(21);

    /// 引擎具名槽位的数量——`0..ENGINE_SLOT_COUNT` 是本体分配的位，
    /// `ENGINE_SLOT_COUNT..32` 是 mod 保留位，见 [`SlotMask`] 模块文档
    /// 「mod 扩展位」一节。
    pub const ENGINE_SLOT_COUNT: u8 = 22;

    /// 全部 22 个引擎具名槽位，按位下标升序——供脚本层名称解析
    /// （[`Self::from_name`]）与测试穷尽遍历使用。
    const ENGINE_SLOTS: [(EquipSlot, &'static str); 22] = [
        (EquipSlot::MAIN_HAND, "main-hand"),
        (EquipSlot::OFF_HAND, "off-hand"),
        (EquipSlot::HEAD, "head"),
        (EquipSlot::FACE, "face"),
        (EquipSlot::EYES, "eyes"),
        (EquipSlot::NECK, "neck"),
        (EquipSlot::BODY, "body"),
        (EquipSlot::OUTER, "outer"),
        (EquipSlot::BACK, "back"),
        (EquipSlot::SHOULDER_L, "shoulder-l"),
        (EquipSlot::SHOULDER_R, "shoulder-r"),
        (EquipSlot::ARM_L, "arm-l"),
        (EquipSlot::ARM_R, "arm-r"),
        (EquipSlot::HAND_L, "hand-l"),
        (EquipSlot::HAND_R, "hand-r"),
        (EquipSlot::BELT, "belt"),
        (EquipSlot::TASSET, "tasset"),
        (EquipSlot::LEGS, "legs"),
        (EquipSlot::BOOT_L, "boot-l"),
        (EquipSlot::BOOT_R, "boot-r"),
        (EquipSlot::RING_L, "ring-l"),
        (EquipSlot::RING_R, "ring-r"),
    ];

    /// 按 `knowledge/design/equipment-slots.md` 槽位表的 kebab-case 名称
    /// 解析出一个引擎槽位——`register-item-equip-mask`
    /// （`ll_mod::script_item_api`）用它把 mod 脚本传入的字符串列表
    /// 转成 [`SlotMask`]。只认识 22 个引擎具名槽位,未知名称返回
    /// `None`——mod 保留位（22..=31）目前没有名称可供脚本引用,见
    /// [`SlotMask`] 模块文档「mod 扩展位」一节「本批次不做」小节。
    pub fn from_name(name: &str) -> Option<EquipSlot> {
        Self::ENGINE_SLOTS
            .iter()
            .find(|(_, slot_name)| *slot_name == name)
            .map(|(slot, _)| *slot)
    }

    /// 取出底层位下标（0..=31）。
    pub const fn get(self) -> u8 {
        self.0
    }

    /// 这个槽位对应的单一位掩码——`1 << 位下标`。
    pub const fn mask(self) -> SlotMask {
        SlotMask(1 << self.0)
    }
}

/// 装备占用的槽位集合，按位表示（装备栏位批次，P6 第三批）——落地
/// `knowledge/design/equipment-slots.md`「一条规则覆盖所有特例」一节：
/// 双手剑、全身板甲、连体服、独眼罩看起来是四种特殊情况，实际是同一个
/// 机制——每件装备声明自己占用哪些槽位，装备时凡与之相交的已装备物品
/// 全部自动卸下（[`crate::entity::Agent::equipment`] 文档「占位冲突」
/// 一节是这条规则在 `resolve` 侧的落地）。
///
/// # 为什么是定宽位标志（`u32`），不是像 `SurfaceKind` 那样的稠密位集
///
/// 项目里对"要不要用定宽位标志"已经有过一次正面的判断分歧，值得在此
/// 复核而不是想当然地照抄设计文档的既有选择：
///
/// `knowledge/design/vehicle-and-mounting.md` 三节讨论地表分类
/// （`SurfaceKind`）时，明确**否决**了定宽位标志方案，改用「内容索引 +
/// 装载期定长位集」（`Vec<u64>`，无上限）。该文档给出的判据是**「可
/// 扩展项数量有没有自然上限」**：地表分类是开放集合（熔岩、云层、
/// 流沙、酸液、蛛网、沼泽……一个整合包里五个 mod 各加三种就能把预留位
/// 吃光，且位号依赖装载顺序，参与哈希时装载顺序一变就产生不必要的
/// 失效）。同一份文档同时明确保留 `SlotMask`/`ActionCapability` 在**
/// 各自领域**继续使用定宽位标志——原文：「这两者的可扩展项确实天然
/// 有限」，并把「装备槽位统共 22 个」作为具体理由点出。
///
/// 本类型独立复核这条判据，不是因为文档这么说就照抄：装备槽位由**人形
/// 解剖结构**天然约束——躯干、四肢、头部各只有固定的几个部位可以穿戴
/// 东西，这是生物学事实，不是内容作者可以无限细分的开放集合。即使
/// mod 想加"尾巴槽""第三只手"这类奇幻扩展，量级也是个位数（不会有
/// 五个 mod 各自发明三种新装备部位——装备槽位不像地表分类那样是纯粹
/// 描述性的环境标签，每加一个新槽位都要求配套的美术资产/UI 布局/护甲
/// 计算全部跟进，这个制作成本本身就是一道天然的数量闸门，地表分类没有
/// 同等的闸门）。22 个引擎槽位 + 10 个 mod 保留位（本类型选择的具体
/// 数字，见下方「mod 扩展位」一节）稳稳落在 `u32` 内，不需要
/// `SurfaceKind` 那种无上限的动态位集。
///
/// 换言之：两个场景用的是**同一条判据**（可扩展项数量有没有自然
/// 上限），只是代入的答案不同——这与 `vehicle-and-mounting.md` 八节
/// 「更正」一节的结论完全一致，本文档的独立复核只是把同一条判据在
/// 装备槽位这个具体场景里重新论证了一遍，不是简单地援引"文档说过"。
///
/// # mod 扩展位
///
/// 低 22 位（`EquipSlot::ENGINE_SLOT_COUNT`）是引擎具名槽位（见
/// [`EquipSlot`] 关联常量），高 10 位（22..=31）保留给 mod：
/// [`Self::mod_bit`] 是**唯一**合法的构造方式——直接拿数字位移
/// （例如 `SlotMask(1 << 25)`）绕过它，即使凑巧落在保留区间内也不算
/// 「按规矩申请」，因为它跳过了`offset < 10` 的范围校验,一次笔误就可能
/// 悄悄踩进引擎的 22 个位或另一个 mod 已经占用的位。
///
/// **本批次不做**：mod 保留位目前没有命名空间隔离的动态分配表（对比
/// `SurfaceKindTable::define` 那样的"字符串 ID → 位下标"注册期分配）——
/// 10 个位由多个 mod 各自协调认领哪个偏移量，运行期不做冲突检测。这不
/// 是偷懒：本批次没有任何真实消费者需要"两个 mod 同时声明自定义槽位"
/// 这个场景（`mods/example_mod` 的两个装备示例只用引擎槽位，见
/// `crates/ll-mod/tests/example_mod_equipment.rs`），在没有真实场景
/// 验证需求形状之前建一张`SurfaceKindTable` 式的分配表是投机性设计
/// （YAGNI，同一条纪律见 `ll_mod::item` 模块文档「本批次范围」一节对
/// `equip_mask`/`stat_bonuses` 的处理）。`Self::mod_bit` 只是把"如何
/// 安全地构造一个落在保留区间内的位"这个最小职责先做对，真正的多 mod
/// 协调分配表留给出现真实冲突场景的那个批次。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SlotMask(u32);

impl SlotMask {
    /// 不占用任何槽位——不可装备的物品（材料、消耗品……）的默认值。
    pub const EMPTY: SlotMask = SlotMask(0);

    /// mod 保留位的数量——`EquipSlot::ENGINE_SLOT_COUNT` 之后还剩
    /// `32 - 22 = 10` 位，见类型文档「mod 扩展位」一节。
    pub const MOD_RESERVED_BITS: u8 = 32 - EquipSlot::ENGINE_SLOT_COUNT;

    /// 按 mod 保留区间内的偏移量（`0..MOD_RESERVED_BITS`）构造一个单
    /// 槽位掩码——`offset` 越界（`>= 10`）返回 `None`，不静默钳位或
    /// 环绕，理由同 `register-item` 拒绝 `stack_limit == 0` 而不是静默
    /// 钳位成 1（同一条"非法输入即拒绝,不猜测意图"纪律）。
    pub const fn mod_bit(offset: u8) -> Option<SlotMask> {
        if offset >= Self::MOD_RESERVED_BITS {
            None
        } else {
            Some(SlotMask(1 << (EquipSlot::ENGINE_SLOT_COUNT + offset)))
        }
    }

    /// 两个掩码的并集——`register-item-equip-mask` 把多个槽位名称各自
    /// 解析成单槽位掩码后,用它们逐个并起来。
    pub const fn union(self, other: SlotMask) -> SlotMask {
        SlotMask(self.0 | other.0)
    }

    /// 取出底层位表示——`ll_mod::content_hash` 值哈希需要把这个掩码的
    /// 具体取值混入摘要（内容哈希覆盖面扩展批次新增）：装备占位掩码是
    /// `ItemDef` 的一个真实字段，改变一件物品占用的槽位是一次真实的
    /// 内容变化，理应被值哈希感知到，与其余整数字段没有本质区别——
    /// 见 [`crate::item::EquipSlot::get`] 同一条「暴露底层位表示供调用方
    /// 按需使用」的先例。
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// 两个掩码是否有交集——装备时用它找出需要卸下的物品
    /// （`knowledge/design/equipment-slots.md`「一条规则覆盖所有特例」
    /// 一节：双手武器/全身甲/连体装的占位冲突判定全部走这一个方法）。
    pub const fn intersects(self, other: SlotMask) -> bool {
        self.0 & other.0 != 0
    }

    /// 这个掩码是否包含某个具体槽位——`resolve_unequip`
    /// （`ll_sim::resolve`）用它判断"玩家请求卸下的槽位，是否恰好落在
    /// 某件已装备物品（可能是横跨多槽的双手武器）的占位范围内"。
    pub const fn contains_slot(self, slot: EquipSlot) -> bool {
        self.intersects(slot.mask())
    }

    /// 这个掩码里最低位对应的槽位——多槽物品在
    /// [`crate::entity::Agent::equipment`] 里的存储键（"锚点槽位"），
    /// 见该字段文档「为什么以锚点槽位为键」一节。空掩码返回 `None`。
    pub const fn anchor_slot(self) -> Option<EquipSlot> {
        if self.0 == 0 {
            None
        } else {
            Some(EquipSlot(self.0.trailing_zeros() as u8))
        }
    }
}

/// [`StatBonus`] 加成落在哪个量上——`ItemDef.stat_bonuses`（P6 第四批：
/// `derive_stats` 与装备属性接进战斗）的每一条都要回答"这份加成具体
/// 加在什么上"，本类型是这个问题的答案域。
///
/// # 为什么不是只有 `AttributeKind` 一种取值
///
/// `AttributeKind` 只有六个变体（力量/敏捷/体质/智力/意志/魅力），且
/// **没有对应"护甲/防御"的变体**——`knowledge/design/vehicle-and-mounting.md`
/// 一节已经核实这一点，`attribute-system.md` 二节把护甲描述成三系
/// 攻防里"防御"一侧的独立数值,不是某个主属性经调整值公式推出的派生
/// 量。若把护甲强行映射成"加某个 `AttributeKind`"（例如"体质每点转 2
/// 点护甲"），会在没有任何设计依据的情况下发明一条换算公式,并让
/// "穿上这件护甲，体质却跟着变了"这种不该发生的副作用悄悄混进结算
/// （体质还驱动生命上限/抗性，两者不该被装备的护甲加成污染）。因此
/// `StatTarget` 需要能表达"直接加护甲"这个独立于六项主属性之外的
/// 目标,与"加某一项主属性"并列,而不是复用 `AttributeKind`。
///
/// # 为什么不现在就加魔抗/意志抗性两个变体
///
/// `combat-three-axis.md` 四节点名的完整 `DerivedStats` 还应该有
/// `magic_resist`/`will_resist`,但那两项服务的是尚未落地的魔法/精神
/// 伤害系别（`DamageSchool`）——`resolve_attack` 本批次仍是纯物理近战
/// 占位实现（见其文档「防御与穿透」一节，三轴战斗结算本身是后续批次
/// 的工作)，`Armor` 是当前唯一有真实读者（`resolve_attack` 的防御端）
/// 的目标，另外两项现在加进来只是两个没有任何消费者的死变体，与
/// `ll_mod::item` 模块文档「本批次范围」一节同一条 YAGNI 判断。三轴
/// 战斗结算批次落地魔法/精神伤害时,照本变体的先例各加一个即可，不需要
/// 改动 `StatBonus` 自身的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatTarget {
    /// 加在六项主属性其中一项上——与状态效果（`ActiveStatModifier`）
    /// 共用同一个 [`AttributeKind`] 取值域,最终在 [`crate::state::WorldState`]
    /// 之外的 `derive_stats`（`ll-sim`，装备批次新增）里与状态效果的
    /// 修正求和到同一个"最终生效值"上。
    Attribute(AttributeKind),
    /// 直接加护甲——`resolve_attack` 防御端的来源，见本类型文档「为什么
    /// 不是只有 `AttributeKind` 一种取值」一节。
    Armor,
}

/// 装备/物品定义里的一条静态属性加成——`ItemDef.stat_bonuses`
/// （`ll_mod::item`，本模块不能直接引用它，依赖方向不允许，见
/// [`ItemStack`] 文档同一条约束）的元素类型，落地
/// `knowledge/design/item-system.md`「定义与实例分离」表格右列
/// `stat_bonuses: Vec<StatBonus>` 一行、`knowledge/design/attribute-system.md`
/// 七节 `derive_stats` 签名里"装备"这一个输入。
///
/// # 为什么是"目标 + 增量"两个字段，不是六七个布尔/可选字段
///
/// 一件装备可能同时加力量与护甲（例如"猛虎护腕"）——`ItemDef.stat_bonuses`
/// 因此是 `Vec<StatBonus>`，一件装备可以携带任意多条加成，每条各自声明
/// 自己的目标与增量。与写成 `struct StatBonus { strength: i32, armor:
/// i32, .. }`（七个字段各自可能是零）相比，"目标 + 增量"的列表形状不
/// 需要为"这条加成到底加了几项"另外发明一套"哪些字段非零"的隐式约定，
/// 也不会在新增第七个可加成的量时逼着已经写好的每一件装备都补一个新
/// 字段的默认值——这与 `RaceDef.traits: Vec<TraitGrant>`「先定形状后续
/// 接消费者」是同一条既有纪律（见 `crate::item::ItemDef::equip_mask`
/// 文档同一类"新增能力用新函数/新条目，不改已有形状"的先例）。
///
/// # 静态数据，不是 `ActiveStatModifier`
///
/// 与状态效果（[`crate::entity::ActiveStatModifier`]）形状故意不同：
/// 装备加成没有 `expires_at`——一件装备"生效"与否是二元的（穿没穿在
/// 身上），不是一个会随世界时钟到期的临时效果，见 `derive_stats`
/// （`ll-sim::resolve`）模块文档「装备加成与状态效果如何合」一节完整
/// 论证：两条加成来源分处两条不同的数据通道，`derive_stats` 是把它们
/// 汇总到同一个最终值的唯一入口，不是要把装备也塞进
/// `active_stat_modifiers`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatBonus {
    /// 这份加成落在哪个量上。
    pub target: StatTarget,
    /// 增减量，可为负（诅咒装备："这把剑很锋利，但拿着它的手会发抖"
    /// 一类设计需要负值,与 [`crate::entity::ActiveStatModifier::delta`]
    /// 同一条"允许为负"的既有纪律）。
    pub amount: i32,
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

    #[test]
    fn 双手武器占用的主手与副手掩码互相相交() {
        // 双手武器占位规则的最小单元验证:MAIN_HAND | OFF_HAND 与单独
        // 一个 MAIN_HAND 掩码相交——这是"装备双手武器要卸下主手已有
        // 物品"这条规则成立的前提。
        // Arrange
        let two_handed = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        let existing_main_hand_only = EquipSlot::MAIN_HAND.mask();

        // Act & Assert
        assert!(two_handed.intersects(existing_main_hand_only));
    }

    #[test]
    fn 不相交的两个槽位掩码没有交集() {
        // Arrange
        let head = EquipSlot::HEAD.mask();
        let legs = EquipSlot::LEGS.mask();

        // Act & Assert
        assert!(!head.intersects(legs));
    }

    #[test]
    fn 空掩码不与任何掩码相交() {
        // Arrange
        let empty = SlotMask::EMPTY;
        let head = EquipSlot::HEAD.mask();

        // Act & Assert
        assert!(!empty.intersects(head));
    }

    #[test]
    fn 双手武器掩码的锚点槽位是位下标较低的主手() {
        // 双手武器只在背包/装备栏存一份,存储键取掩码最低位——见
        // Agent::equipment 文档「为什么以锚点槽位为键」一节。
        // Arrange
        let two_handed = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());

        // Act
        let anchor = two_handed.anchor_slot();

        // Assert
        assert_eq!(anchor, Some(EquipSlot::MAIN_HAND));
    }

    #[test]
    fn 空掩码没有锚点槽位() {
        // Arrange
        let empty = SlotMask::EMPTY;

        // Act & Assert
        assert_eq!(empty.anchor_slot(), None);
    }

    #[test]
    fn 单槽位掩码包含自身对应的槽位() {
        // Arrange
        let mask = EquipSlot::RING_L.mask();

        // Act & Assert
        assert!(mask.contains_slot(EquipSlot::RING_L));
    }

    #[test]
    fn 单槽位掩码不包含其它槽位() {
        // Arrange
        let mask = EquipSlot::RING_L.mask();

        // Act & Assert
        assert!(!mask.contains_slot(EquipSlot::RING_R));
    }

    #[test]
    fn 双手武器掩码包含副手槽位() {
        // 验证"玩家请求卸下副手,resolve_unequip 需要能识别出这个请求
        // 命中的其实是横跨两槽的双手武器"这条查询的最小单元前提。
        // Arrange
        let two_handed = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());

        // Act & Assert
        assert!(two_handed.contains_slot(EquipSlot::OFF_HAND));
    }

    #[test]
    fn 按引擎槽位名称解析出对应的具名常量() {
        // Arrange & Act & Assert
        assert_eq!(
            EquipSlot::from_name("main-hand"),
            Some(EquipSlot::MAIN_HAND)
        );
        assert_eq!(EquipSlot::from_name("off-hand"), Some(EquipSlot::OFF_HAND));
        assert_eq!(EquipSlot::from_name("ring-r"), Some(EquipSlot::RING_R));
    }

    #[test]
    fn 未知的槽位名称解析返回空值() {
        // Arrange & Act & Assert
        assert_eq!(EquipSlot::from_name("tail"), None);
    }

    #[test]
    fn mod保留位偏移量在合法范围内时构造成功() {
        // Arrange & Act
        let mask = SlotMask::mod_bit(0);

        // Assert：mod 第一个保留位紧邻 22 个引擎槽位之后,即位下标 22。
        assert_eq!(mask, Some(SlotMask(1 << 22)));
    }

    #[test]
    fn mod保留位偏移量越界时构造返回空值() {
        // 保留区间只有 10 位（偏移量 0..=9）,10 已经越界。
        // Arrange & Act & Assert
        assert_eq!(SlotMask::mod_bit(10), None);
    }

    #[test]
    fn 引擎二十二个具名槽位互不相同() {
        // 穷尽性验证:22 个具名常量对应 22 个互不相同的位下标,不存在
        // 两个常量意外撞到同一个位——这是整张槽位表的基础不变式。
        // Arrange
        let slots: Vec<u8> = EquipSlot::ENGINE_SLOTS
            .iter()
            .map(|(slot, _)| slot.get())
            .collect();
        let mut unique = slots.clone();
        unique.sort_unstable();
        unique.dedup();

        // Act & Assert
        assert_eq!(slots.len(), unique.len());
    }

    #[test]
    fn 装备槽位序列化往返后与原值相等() {
        // Arrange
        let original = EquipSlot::BODY;

        // Act
        let encoded = serde_json::to_string(&original).expect("EquipSlot 可派生序列化");
        let decoded: EquipSlot = serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn 装备掩码序列化往返后与原值相等() {
        // Arrange
        let original = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());

        // Act
        let encoded = serde_json::to_string(&original).expect("SlotMask 可派生序列化");
        let decoded: SlotMask = serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn 加成目标为不同主属性时不相等() {
        // StatTarget::Attribute 包裹的 AttributeKind 不同,整个 StatTarget
        // 也应视为不同——这是 derive_stats 按目标分派求和的前提。
        // Arrange
        let strength_target = StatTarget::Attribute(AttributeKind::Strength);
        let dexterity_target = StatTarget::Attribute(AttributeKind::Dexterity);

        // Act & Assert
        assert_ne!(strength_target, dexterity_target);
    }

    #[test]
    fn 加成目标护甲与加成目标同名主属性不相等() {
        // Armor 是独立于 AttributeKind 之外的目标——不存在任何一个
        // AttributeKind 变体的 StatTarget::Attribute 会与 StatTarget::Armor
        // 相等,即使两者字面上都在讨论"防御相关"的属性。
        // Arrange
        let armor_target = StatTarget::Armor;
        let constitution_target = StatTarget::Attribute(AttributeKind::Constitution);

        // Act & Assert
        assert_ne!(armor_target, constitution_target);
    }

    #[test]
    fn 属性加成结构体保留目标与增量两个字段() {
        // Arrange & Act
        let bonus = StatBonus {
            target: StatTarget::Attribute(AttributeKind::Strength),
            amount: 5,
        };

        // Assert
        assert_eq!(
            (bonus.target, bonus.amount),
            (StatTarget::Attribute(AttributeKind::Strength), 5)
        );
    }

    #[test]
    fn 属性加成允许负增量表达诅咒装备() {
        // Arrange & Act
        let cursed = StatBonus {
            target: StatTarget::Attribute(AttributeKind::Dexterity),
            amount: -3,
        };

        // Assert
        assert_eq!(cursed.amount, -3);
    }
}
