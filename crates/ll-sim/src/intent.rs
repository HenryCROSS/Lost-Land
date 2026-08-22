//! `Intent`：玩家或 AI「想做什么」的纯数据描述，以及从玩家输入到
//! `Intent` 的映射。
//!
//! # 为什么 `Intent` 必须是纯数据且可序列化
//!
//! 记录下一局游戏里每一次 `Intent` 加上世界种子，就足以完整重放这
//! 一局——不需要额外录制随机数或帧时序，因为 `resolve`（批次 C）读到
//! 同一个 `Intent` 时，从 `DetRng::for_entity` 派生出的随机数序列必然
//! 相同（约束 C3）。这是排查玩家报告缺陷最强的手段：只要留住 Intent
//! 流，就能在开发机上原样复现玩家遇到的那一局，而不必祈祷缺陷恰好
//! 再次触发。
//!
//! `Intent` 本身不做任何校验或世界查询——它只是「玩家按了什么、想
//! 干什么」的记录，合法性判断（能不能这样移动、这个方向有没有可攻击
//! 的目标）全部留给 `resolve`。这也是为什么 `pos` 字段用裸
//! `(i32, i32)` 而不是 `ll_core::torus::TorusPos`：后者的唯一构造
//! 路径需要世界尺寸做取模归一化，而 `Intent` 在产生的这一刻未必已经
//! 拿到世界——`resolve` 读取 `Intent` 时自然持有 `WorldState`，届时
//! 用 `world.size.wrap(x, y)` 归一化一次即可，不需要 `Intent` 自己
//! 提前做这件事。

use ll_core::ident::ContentIndex;
use ll_platform::input::InputState;
use ll_world::entity::EntityId;
use ll_world::item::EquipSlot;
use ll_world::space::SpaceId;
use serde::{Deserialize, Serialize};

/// 八方向。
///
/// 只是「移动往哪个方向」的枚举，不含步长——移动恒为一格，见
/// `knowledge/design` 对回合制网格移动的约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// 北（世界坐标 y 减一）。
    North,
    /// 南（世界坐标 y 加一）。
    South,
    /// 西（世界坐标 x 减一）。
    West,
    /// 东（世界坐标 x 加一）。
    East,
    /// 东北。
    NorthEast,
    /// 东南。
    SouthEast,
    /// 西南。
    SouthWest,
    /// 西北。
    NorthWest,
}

impl Direction {
    /// 该方向对应的一格位移 `(dx, dy)`。
    ///
    /// `dy` 的符号沿用既有的相机/移动惯例（见
    /// `crates/ll-world/examples/p2_acceptance/main.rs` 的
    /// `move_player` 调用点）：北是 `y - 1`，南是 `y + 1`。
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::West => (-1, 0),
            Direction::East => (1, 0),
            Direction::NorthEast => (1, -1),
            Direction::SouthEast => (1, 1),
            Direction::SouthWest => (-1, 1),
            Direction::NorthWest => (-1, -1),
        }
    }
}

/// 玩家或 AI「想做什么」的纯数据描述。见模块文档「为什么必须是纯数据
/// 且可序列化」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    /// 朝某方向移动一格。
    Move {
        /// 发起者。
        actor: EntityId,
        /// 移动方向。
        dir: Direction,
    },
    /// 攻击某个目标。
    Attack {
        /// 发起者。
        actor: EntityId,
        /// 目标。
        target: EntityId,
    },
    /// 原地等待一回合。
    Wait {
        /// 等待者。
        actor: EntityId,
    },
    /// 开启某处的门。
    OpenDoor {
        /// 发起者。
        actor: EntityId,
        /// 门所在的世界坐标，未经归一化——见模块文档「为什么 `pos`
        /// 用裸元组」一节。
        pos: (i32, i32),
    },
    /// 尝试进入一个具体的 `Interior` 空间实例（任务 12：两级坐标系
    /// 重写）。
    ///
    /// `target` 必须由调用方给出具体是哪一个实例——与
    /// `Intent::OpenDoor` 的 `pos` 同一个理由：`resolve` 才持有
    /// `WorldState`，能查「玩家当前站的这一格有哪些入口」
    /// （`InteriorTable::entries_at`），但**选哪一个**（若同一格恰好有
    /// 多个入口）不该由 `resolve` 替玩家决定——那是一个真实的玩法
    /// 选择，不是可以静默取「排序后第一个」蒙混过去的实现细节。本批次
    /// 的输入映射（[`intent_from_input`]）不产出这个变体（同一格多入口
    /// 的选择 UI 不在本次重写范围内），它面向已经知道要进哪个具体
    /// 实例的调用方（demo/未来的交互层）。
    EnterSpace {
        /// 发起者。
        actor: EntityId,
        /// 目标 `Interior` 实例。
        target: SpaceId,
    },
    /// 退出当前所在的 `Interior`，返回地表。在地表触发这个意图是
    /// 「这一步无意义」，`resolve` 静默作废（与撞墙同一种处理，见
    /// `resolve.rs` 模块文档）。
    ExitSpace {
        /// 发起者。
        actor: EntityId,
    },
    /// 使用一个技能（P5-B 任务 5）。
    ///
    /// 与 [`Intent::OpenDoor`]/[`Intent::EnterSpace`] 同一条纪律：只携带
    /// 「想用哪个技能、对谁用」这条裸请求，不做任何合法性判断——技能是
    /// 否已解锁、是否在冷却、资源是否充足、具体产出什么效果，全部留给
    /// `resolve`（见 `resolve_use_skill` 文档）结合
    /// `Agent` 状态与调用方提供的技能定义现算，`Intent` 自身不查任何
    /// 表。`target` 是 `Option`：某些技能效果（例如自我增益）不需要目标，
    /// 由 `resolve_use_skill` 在缺省时回落到施法者自身。
    UseSkill {
        /// 发起者。
        actor: EntityId,
        /// 使用的技能，指向内容注册表。
        skill: ContentIndex,
        /// 目标（若这个技能需要一个）。
        target: Option<EntityId>,
    },
    /// 开始一段休息会话（`resource-pools-and-rest.md` 七节）——只用来
    /// **开始**这段会话,后续每回合不需要再提交本意图,照常提交
    /// [`Intent::Wait`] 即可,`resolve_wait` 会持续检查是否已到达
    /// `target_ticks`（见 `crate::resolve::resolve_wait` 文档）。
    ///
    /// 若发起者已经在休息中,`resolve` 按继续休息处理（与提交
    /// `Intent::Wait` 等价），不会重新开始一段会话——见
    /// `crate::resolve::resolve_rest` 文档。
    Rest {
        /// 发起者。
        actor: EntityId,
        /// 目标持续的 tick 数。
        target_ticks: u32,
    },
    /// 拾取脚下地面上的一堆物品（P6 第二批：背包与地面物品）。
    ///
    /// # 为什么不指定要捡哪一种（对比 `Intent::Drop` 的 `def`）
    ///
    /// 与 [`Intent::OpenDoor`]/[`Intent::EnterSpace`] 同一条纪律：只携带
    /// 「想干什么」这条裸请求,不做任何合法性判断——脚下有没有东西、
    /// 捡起来之后要不要跟背包已有的堆合并,全部留给 `resolve`
    /// （`crate::resolve::resolve_pick_up`）结合 `WorldState` 现算。
    /// 不要求调用方指定 `def` 是刻意的：捡东西的人事先并不知道地上
    /// 那堆到底是什么（不像 `Intent::Drop`——玩家丢东西时看的是自己
    /// 背包里已知的物品列表）。若同一格恰好有多堆不同种类的物品,
    /// `resolve_pick_up` 按 [`ll_world::state::WorldState::ground_items`]
    /// 的存储顺序取第一条——多堆选择 UI 不在本批次范围内,与
    /// `Intent::EnterSpace` 「同一格多入口的选择 UI 不在本次重写范围内」
    /// 同一条既有先例。
    PickUp {
        /// 发起者，捡到它自己的背包里。
        actor: EntityId,
    },
    /// 把背包里的某种物品整堆丢在脚下（P6 第二批：背包与地面物品）。
    ///
    /// # 为什么是整堆，不支持部分数量
    ///
    /// [`ll_world::item::split_stack`] 已经存在且已在 P6 第一批测试
    /// 过——但把"丢一部分"接进 `Intent` 需要再决定"数量哪里来"（新增
    /// 一个 `amount` 字段还是走两步式的"先拆堆再丢整堆"交互),这属于
    /// 背包 UI/槽位批次（第三批）该定的手感,不是本批次要解决的问题。
    /// 本批次的验收范围（拾取/丢弃/合并/老化,见项目任务书）不需要
    /// 部分丢弃,提前引入只会制造一个当前没有测试覆盖的分支。
    Drop {
        /// 发起者。
        actor: EntityId,
        /// 要丢弃的物品定义——玩家从自己背包的已知列表里选,不像
        /// `Intent::PickUp` 那样不知道地上有什么,因此这里要求显式
        /// 指定。
        def: ContentIndex,
    },
    /// 把背包里的某种物品装备起来（装备栏位批次，P6 第三批）——落地
    /// `knowledge/design/equipment-slots.md`「装备流程」一节。
    ///
    /// # 为什么携带 `def`，不携带目标槽位
    ///
    /// 与 [`Intent::Drop`] 同一条纪律：玩家从自己背包的已知列表里选
    /// 「装备哪一种物品」，不需要（也不应该）自己算出这件物品该落在
    /// 哪个槽位——槽位由物品自身的 `equip_mask` 决定
    /// （[`crate::item::SlotMask::anchor_slot`]），这是内容数据决定的
    /// 事实，不是玩家的选择，因此不做成 `Intent` 的字段，交给
    /// `resolve_equip`（`crate::resolve`）结合物品目录现算。
    Equip {
        /// 发起者。
        actor: EntityId,
        /// 要装备的物品定义——玩家从自己背包的已知列表里选,与
        /// `Intent::Drop` 的 `def` 同一条理由。
        def: ContentIndex,
    },
    /// 卸下某个槽位当前装备的物品（装备栏位批次，P6 第三批）。
    ///
    /// # 为什么携带槽位而不是物品定义
    ///
    /// 与 `Intent::Equip` 反过来：玩家看到的是"装备栏里某个槽位现在
    /// 穿着什么"，不一定记得住那件物品的确切内容 ID——从槽位出发更
    /// 符合装备栏 UI 的自然交互（点开一个槽位、选择卸下）。`slot` 不
    /// 要求精确落在物品的锚点槽位上——横跨多槽的物品（双手武器）任选
    /// 其占用的一个槽位都能成功卸下,`resolve_unequip`
    /// （`crate::resolve`）会把请求槽位翻译成真实的存储键（锚点槽位），
    /// 见其文档。
    Unequip {
        /// 发起者。
        actor: EntityId,
        /// 玩家请求卸下的槽位。
        slot: EquipSlot,
    },
    /// 使用背包里的一件物品（耐久与 `Intent::Use` 落地批次，P6 第五批）
    /// ——落地 `knowledge/design/item-system.md` 八节「物品作用」。
    ///
    /// # 为什么携带 `def`，不携带目标
    ///
    /// 与 [`Intent::Drop`]/[`Intent::Equip`] 同一条纪律：玩家从自己背包
    /// 的已知列表里选「用哪一种物品」。不携带目标实体——本批次的物品
    /// 使用效果恒施于发起者自身（药水喝给自己），没有「对着别人用一件
    /// 消耗品」这个真实场景需要表达（不像 `Intent::UseSkill::target`
    /// 那样确实存在指向他人的技能），提前加一个恒为 `None`/恒被忽略的
    /// 字段是死字段，见 `ll_mod::item` 模块文档同一条 YAGNI 判断。真要
    /// 支持「扔药水砸别人」，是该场景真正落地时再给这个变体加字段，不
    /// 在本批次预留。
    Use {
        /// 发起者，同时是效果的承受者。
        actor: EntityId,
        /// 要使用的物品定义——玩家从自己背包的已知列表里选。
        def: ContentIndex,
    },
    /// 搜刮脚下的一具容器（尸体，NPC 死亡掉落批次）——把容器
    /// [`ll_world::item::GroundItemStack::contents`] 里的全部战利品
    /// 移进背包,容器本身随后从地面消失。
    ///
    /// # 为什么不是 `Intent::PickUp` 多分支一条判断
    ///
    /// 与 [`Intent::PickUp`] 是两个不同的玩法动作，不是同一个意图的两
    /// 种结果：`PickUp` 面向"地上有什么就捡什么"的无差别拾取，`Loot`
    /// 面向"这是一具尸体，我要搜刮它"这个明确的、需要与普通拾取区分
    /// 交互提示的动作（项目所有者裁定「尸体变成放在地上可交互的物品，
    /// 其他物品在地上也一样能被交互，只是有不同选项而已」）——两者的
    /// 结算规则也确实不同（`resolve_pick_up`/`resolve_loot`，见各自
    /// 文档），合并成一个意图内部分支判断反而会让"这次到底捡到了什么"
    /// 这个问题在 `Intent` 层就变得含糊。
    ///
    /// # 为什么不指定要搜刮哪一具（对比 `Intent::Drop` 的 `def`）
    ///
    /// 与 [`Intent::PickUp`] 同一条纪律：玩家事先不知道脚下这具容器
    /// 里到底装着什么，只知道"我要搜刮它"——具体内容物由 `resolve`
    /// （`crate::resolve::resolve_loot`）结合 `WorldState` 现算。
    Loot {
        /// 发起者，搜刮到它自己的背包里。
        actor: EntityId,
    },
    /// 盘查：`actor` 检查 `target` 此刻背包与已装备的物品（卫兵职业
    /// 接线批次，见 `crate::resolve::resolve_inspect` 文档）。
    ///
    /// # 为什么没有携带任何"判定结果"字段
    ///
    /// 与 [`Intent::Attack`]/[`Intent::UseSkill`] 同一条纪律：`Intent`
    /// 只记录"想做什么"，不预判"结果如何"——这次盘查会看到什么、算不
    /// 算违法，全部留给 `resolve` 结合当时的 `WorldState` 现算（本批次
    /// 只读出"看到了什么"，"算不算违法"需要 `Owner`，尚未落地，见
    /// `knowledge/design/ownership-and-crime-detection.md`）。
    ///
    /// # 谁会产出这个变体
    ///
    /// 目前唯一的产出路径是卫兵职业的行为树脚本（`(list 'inspect
    /// target)`，`ll_script::api::intent::parse_intent`
    /// 识别，见其文档），不是玩家输入映射（`intent_from_input`
    /// 不产出本变体）——盘查是卫兵 AI 的主动行为，不是玩家操作。
    Inspect {
        /// 发起盘查的一方（卫兵）。
        actor: EntityId,
        /// 被盘查的一方。
        target: EntityId,
    },
    /// 切换潜行状态（潜行与盗贼被动批次）——项目所有者裁定「潜行需要
    /// 时可切换状态的」，本变体就是那个「切换」。
    ///
    /// # 为什么是「切换」而不是 `Enter`/`Exit` 两个变体
    ///
    /// 载荷会是一个恒等于「当前状态取反」的 `bool`——调用方要么先读一次
    /// [`ll_world::entity::Agent::stealthed`] 才能填对（那它就不是「纯
    /// 请求」了，是把结算的一半搬进了 `Intent`），要么填错时
    /// `resolve` 还得决定「请求进入潜行但已经在潜行」算什么。与
    /// [`Intent::Rest`]「已经在休息时按继续休息处理」那条既有先例
    /// 对照：那里的载荷（`target_ticks`）是真实的玩家选择，这里没有
    /// 任何对应的选择存在。切换语义把这两个问题一起消掉。
    ///
    /// # 为什么消耗一个回合
    ///
    /// `resolve_toggle_stealth` 按 [`Intent::Wait`] 同一条基础代价
    /// 计费（见 `crate::resolve::resolve_toggle_stealth` 文档）。不计费
    /// 的话，玩家可以在每走一格之前开、走完立刻关，白嫖潜行的收益
    /// （偷袭直通、盘查率下降）而完全不付潜行的代价（移动开销上升）
    /// ——那会让本批次唯一的代价形同虚设。
    ///
    /// # 谁会产出这个变体
    ///
    /// 与 [`Intent::PickUp`]/[`Intent::Rest`]/[`Intent::Equip`] 等既有
    /// 玩法意图完全一致：[`intent_from_input`] 目前只映射
    /// `Move`/`Wait` 两种，本变体（和上面那六种）同样还没有绑定按键，
    /// 面向已经知道自己要做什么的调用方（AI 策略、未来的交互层）。
    /// 这不是本变体特有的缺口，是输入映射层整体尚未展开的既有状态。
    ToggleStealth {
        /// 发起者，同时是状态的承受者。
        actor: EntityId,
    },
    /// 按一条配方制作一次（制作系统批次，`knowledge/design/crafting-system.md`
    /// 五节）——烹饪/锻造/裁缝/炼金四类共用这一个变体，四类的差别全部
    /// 落在配方数据上（类别/食材/场地/工具），不在意图上，见该设计文档
    /// 二节用 ADR 0021 做的统一论证。
    ///
    /// # 为什么不复用 [`Intent::Use`]
    ///
    /// 输入不同（这里是配方索引，不是物品索引）、输出也不同（多条
    /// 消耗加一条产出，不是单条对单条）——见
    /// `knowledge/design/food-and-cooking-system.md` 四节。
    ///
    /// # 为什么只携带配方索引，不携带数量/食材来源
    ///
    /// 与既有 15 个变体同一条纪律：`Intent` 只记录「想做什么」，一切
    /// 合法性判断（副职闸门/场地/工具/食材是否齐全）留给
    /// `crate::resolve::resolve_craft` 结合当时的 `WorldState` 现算。
    /// 「一次做几个」是将来 UI 真的提供这个选择时再加的字段，不在
    /// 本批次预留（同 [`Intent::Use`] 不预留 `target` 的既有判断）。
    ///
    /// # 谁会产出这个变体——本批次的已知缺口，如实标注
    ///
    /// **目前没有任何产出者。** [`intent_from_input`] 不映射本变体
    /// （它至今只映射 `Move`/`Wait`/`ToggleStealth` 三种），
    /// `ll_script::api::intent::parse_intent` 也不识别它——制作界面
    /// （`action-capability-and-input-context.md` 的 `UiMode` 模式栈）
    /// 是纯设计零实现。这与 `PickUp`/`Drop`/`Equip`/`Rest`/`Loot`/`Use`
    /// 六个既有玩法意图的处境完全相同：输入映射层整体尚未展开，不是
    /// 本变体特有的缺口。本批次落地的是「配方注册 → 结算 → 效果」这
    /// 一整条链路，验收证据走测试里直接构造本变体经
    /// [`crate::turn::TurnEngine`] 提交（见
    /// `crates/ll-mod/tests/example_mod_crafting.rs`）。
    Craft {
        /// 发起者，同时是食材的出处与成品的去处。
        actor: EntityId,
        /// 要制作的配方——指向配方表。
        recipe: ContentIndex,
    },
}

impl Intent {
    /// 这条意图的发起者——全部变体都恰好有一个 `actor: EntityId` 字段
    /// （被谁发起、代表谁的这一次行动），本方法只是把这条穷尽 `match`
    /// 收敛成一次调用，供 `crate::resolve` 判断"这是谁的回合"（例如
    /// 资源池 `RegenRule::OnTurnStart` 的触发点，见
    /// `crate::resolve::resolve_dispatch` 文档），不需要在每个调用点
    /// 各自重复一遍这个 `match`。
    pub fn actor(&self) -> EntityId {
        match *self {
            Intent::Move { actor, .. }
            | Intent::Attack { actor, .. }
            | Intent::Wait { actor }
            | Intent::OpenDoor { actor, .. }
            | Intent::EnterSpace { actor, .. }
            | Intent::ExitSpace { actor }
            | Intent::UseSkill { actor, .. }
            | Intent::Rest { actor, .. }
            | Intent::PickUp { actor }
            | Intent::Drop { actor, .. }
            | Intent::Equip { actor, .. }
            | Intent::Unequip { actor, .. }
            | Intent::Use { actor, .. }
            | Intent::Loot { actor }
            | Intent::Inspect { actor, .. }
            | Intent::ToggleStealth { actor }
            | Intent::Craft { actor, .. } => actor,
        }
    }
}

/// 把一帧的玩家输入映射成 `Intent`。
///
/// 只产出 [`Intent::Move`] 与 [`Intent::Wait`]：`Attack`/`OpenDoor`
/// 需要知道「那个方向上到底有什么」（目标实体、门的确切位置），这
/// 属于读世界之后才能判断的事，是 `resolve`（批次 C）从一次
/// `Intent::Move` 结合世界状态推导出来的，不是输入层能单独决定的——
/// 见批次 B 的分工：本层只管「按了什么键」，不读 `WorldState`。
///
/// 四个方向键按住的组合决定八向：例如同时按住上与右得到东北。若上下
/// 或左右两个相反方向同时被按住，视为无方向输入（两者抵消，不猜测
/// 玩家意图）。方向输入与等待键同时激活时，移动优先——方向输入的
/// 信号更具体，等待通常只是长按等待键连续触发的巡航状态，不应盖过
/// 一次明确的转向。
///
/// 无任何相关按键激活时返回 [`None`]。
pub fn intent_from_input(actor: EntityId, input: &InputState) -> Option<Intent> {
    if let Some(dir) = direction_from_input(input) {
        return Some(Intent::Move { actor, dir });
    }
    if input.was_activated(ll_platform::input::GameKey::Wait) {
        return Some(Intent::Wait { actor });
    }
    None
}

/// 单轴上按下的是负方向还是正方向。
///
/// 用具名的两变体枚举而非 `-1`/`1` 整数：整数版本的 `match` 需要对
/// `i32` 的全部取值做穷尽性检查，编译器无法从字面量模式推断出实际
/// 只会出现 `-1`/`1`/`None` 三种情况，被迫要求一个兜底分支——而兜底
/// 分支恰恰是这里最想避免的：它会悄悄吞掉「本不该出现的第三种输入」
/// 而不是让编译器帮忙钉死只有这两种可能。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// 该轴上的负方向（西、北）。
    Negative,
    /// 该轴上的正方向（东、南）。
    Positive,
}

/// 从方向键的按住组合推出八向；两个相反方向键同时按住或都未按住时
/// 返回 [`None`]。
fn direction_from_input(input: &InputState) -> Option<Direction> {
    use ll_platform::input::GameKey;

    let vertical = match (
        input.was_activated(GameKey::Up),
        input.was_activated(GameKey::Down),
    ) {
        (true, false) => Some(Axis::Negative),
        (false, true) => Some(Axis::Positive),
        _ => None,
    };
    let horizontal = match (
        input.was_activated(GameKey::Left),
        input.was_activated(GameKey::Right),
    ) {
        (true, false) => Some(Axis::Negative),
        (false, true) => Some(Axis::Positive),
        _ => None,
    };

    match (horizontal, vertical) {
        (Some(Axis::Negative), Some(Axis::Negative)) => Some(Direction::NorthWest),
        (Some(Axis::Positive), Some(Axis::Negative)) => Some(Direction::NorthEast),
        (Some(Axis::Negative), Some(Axis::Positive)) => Some(Direction::SouthWest),
        (Some(Axis::Positive), Some(Axis::Positive)) => Some(Direction::SouthEast),
        (Some(Axis::Negative), None) => Some(Direction::West),
        (Some(Axis::Positive), None) => Some(Direction::East),
        (None, Some(Axis::Negative)) => Some(Direction::North),
        (None, Some(Axis::Positive)) => Some(Direction::South),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_platform::input::{GameKey, InputState};
    use ll_world::entity::Arena;

    fn entity() -> EntityId {
        let mut arena: Arena<()> = Arena::new();
        arena.spawn(())
    }

    /// 按下给定按键组合后立即读一次意图——`press` 本身就会置起
    /// `just_pressed`，`was_activated` 因此为真，不需要先走一帧
    /// `begin_frame`。
    fn intent_after_pressing(keys: &[GameKey]) -> Option<Intent> {
        let mut input = InputState::new();
        for &key in keys {
            input.press(key);
        }
        intent_from_input(entity(), &input)
    }

    #[test]
    fn 单按上键映射为向北移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Up]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::North,
                ..
            })
        ));
    }

    #[test]
    fn 单按下键映射为向南移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Down]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::South,
                ..
            })
        ));
    }

    #[test]
    fn 单按左键映射为向西移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Left]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::West,
                ..
            })
        ));
    }

    #[test]
    fn 单按右键映射为向东移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Right]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::East,
                ..
            })
        ));
    }

    #[test]
    fn 同按上与右映射为向东北移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Up, GameKey::Right]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::NorthEast,
                ..
            })
        ));
    }

    #[test]
    fn 同按上与左映射为向西北移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Up, GameKey::Left]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::NorthWest,
                ..
            })
        ));
    }

    #[test]
    fn 同按下与右映射为向东南移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Down, GameKey::Right]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::SouthEast,
                ..
            })
        ));
    }

    #[test]
    fn 同按下与左映射为向西南移动() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Down, GameKey::Left]);

        // Assert
        assert!(matches!(
            intent,
            Some(Intent::Move {
                dir: Direction::SouthWest,
                ..
            })
        ));
    }

    #[test]
    fn 相反方向键同时按住时没有方向输入() {
        // 上下抵消，不应猜测玩家意图；由于没有等待键，最终应无意图。
        // Act
        let intent = intent_after_pressing(&[GameKey::Up, GameKey::Down]);

        // Assert
        assert!(intent.is_none());
    }

    #[test]
    fn 无任何按键时返回空值() {
        // Arrange
        let input = InputState::new();

        // Act
        let intent = intent_from_input(entity(), &input);

        // Assert
        assert!(intent.is_none());
    }

    #[test]
    fn 按下等待键映射为等待意图() {
        // Act
        let intent = intent_after_pressing(&[GameKey::Wait]);

        // Assert
        assert!(matches!(intent, Some(Intent::Wait { .. })));
    }

    #[test]
    fn 意图序列化往返后与原值相等() {
        // Arrange
        let actor = entity();
        let original = Intent::Attack {
            actor,
            target: entity(),
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn useskill意图序列化往返后与原值相等() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let skill = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:strike").expect("合法标识符"));
        let original = Intent::UseSkill {
            actor: entity(),
            skill,
            target: Some(entity()),
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn pickup意图序列化往返后与原值相等() {
        // Arrange
        let original = Intent::PickUp { actor: entity() };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn drop意图序列化往返后与原值相等() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let def = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        let original = Intent::Drop {
            actor: entity(),
            def,
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn actor方法对pickup意图返回发起者字段() {
        // Arrange
        let actor = entity();
        let intent = Intent::PickUp { actor };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }

    #[test]
    fn equip意图序列化往返后与原值相等() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let def = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:great_axe").expect("合法标识符"));
        let original = Intent::Equip {
            actor: entity(),
            def,
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn unequip意图序列化往返后与原值相等() {
        // Arrange
        let original = Intent::Unequip {
            actor: entity(),
            slot: EquipSlot::MAIN_HAND,
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn inspect意图序列化往返后与原值相等() {
        // Arrange
        let original = Intent::Inspect {
            actor: entity(),
            target: entity(),
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn actor方法对inspect意图返回发起者字段() {
        // Arrange
        let actor = entity();
        let intent = Intent::Inspect {
            actor,
            target: entity(),
        };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }

    #[test]
    fn actor方法对equip意图返回发起者字段() {
        // Arrange
        let actor = entity();
        let mut interner = ll_core::ident::Interner::new();
        let def = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:great_axe").expect("合法标识符"));
        let intent = Intent::Equip { actor, def };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }

    #[test]
    fn actor方法对unequip意图返回发起者字段() {
        // Arrange
        let actor = entity();
        let intent = Intent::Unequip {
            actor,
            slot: EquipSlot::HEAD,
        };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }

    #[test]
    fn use意图序列化往返后与原值相等() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let def = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:healing_potion").expect("合法标识符"),
        );
        let original = Intent::Use {
            actor: entity(),
            def,
        };

        // Act
        let json = serde_json::to_string(&original).expect("Intent 全字段均可序列化");
        let decoded: Intent = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn actor方法对use意图返回发起者字段() {
        // Arrange
        let actor = entity();
        let mut interner = ll_core::ident::Interner::new();
        let def = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:healing_potion").expect("合法标识符"),
        );
        let intent = Intent::Use { actor, def };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }

    #[test]
    fn actor方法对useskill意图返回发起者字段() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let skill = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:strike").expect("合法标识符"));
        let actor = entity();
        let intent = Intent::UseSkill {
            actor,
            skill,
            target: None,
        };

        // Act & Assert
        assert_eq!(intent.actor(), actor);
    }
}
