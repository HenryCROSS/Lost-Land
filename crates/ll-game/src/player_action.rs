//! 玩家怎么按出物品链那六个意图。
//!
//! # 这个模块补的是哪条断线
//!
//! `ll_sim::intent::Intent` 有二十四个变体。本模块落地之前，**真实
//! 游戏里按得出来的只有三个**：`Move`、`Wait`，以及 `Move` 撞到人时由
//! `ll_sim::turn` 就地路由出的 `Attack`。物品链那六个——
//! `PickUp`/`Drop`/`Equip`/`Unequip`/`Use`/`Craft`——引擎侧的结算、
//! 效果、端到端测试全都齐全，玩家一个都按不出来：`intent_from_input`
//! 不映射它们，也没有任何别的键位产出者。于是物品、装备、锻造、家具
//! 这一整条链在真实游玩中完全不可达。（第七个 `Place` 是本批次连同
//! 家具放置状态一起新增的，从落地那一刻起就带着键位。）
//!
//! 这是本仓库反复出现的「声明了但从没接线」在**输入层**的一次复发，
//! 与 `ll_sim::turn::TurnEngine` 当初只接进 demo、内容目录没接进
//! `TurnEngine`（天赋在真实游戏里全是死的）是同一类缺陷，见
//! `ll_sim::turn::TurnEngine::perform` 文档记录的前两次。
//!
//! # 为什么不能全塞进 `intent_from_input`
//!
//! `ll_sim::intent::intent_from_input` 按设计**不读 `WorldState`**
//! （见其模块文档「本层只管『按了什么键』」一节）。这条纪律对
//! `Move`/`Wait` 完全够用，但物品链那六个意图里有五个要带参数：
//! `Craft { recipe }`、`Drop { def }`、`Equip { def }`、
//! `Unequip { slot }`、`Use { def }`。「选哪一条」不是可以由按键单独
//! 决定的事——它要求玩家先看见一张列表，而那张列表的内容来自背包与
//! 配方表。
//!
//! 因此分工是：
//!
//! - `ll-sim` 提供 `TurnEngine::try_player_intent`：把一个**已经选好
//!   的**意图当作玩家这一回合提交（见其文档）。
//! - 本模块持有菜单与光标，把「按了哪个键、光标停在第几行」翻译成
//!   那个意图。
//! - `ll-ui` 的 `hud::action_menu` 只负责把一列字符串加一个光标画出来。
//!
//! # 交互键、方向列表、物品列表
//!
//! 项目所有者定的最终形状：
//!
//! > 按空格的时候，如果范围内一格有东西就显示一个列表，显示交互的
//! > 方向。如果只有一个就直接和那个东西交互好了
//!
//! > 当物品丢在地上，无论是一个还是 N 个，交互的时候都统一以列表显示
//!
//! > 不需要什么空格+方向了
//!
//! > 不是捡走，只是打开交互列表
//!
//! 落地成**两块不同的列表**，别混成一块：
//!
//! | 范围内有东西的格数 | 按下交互键之后 |
//! |---|---|
//! | 0 | 一句「附近没有可交互的东西」（[`Feedback::NothingNearby`]） |
//! | 1 | **直接**开那一格的物品列表，跳过方向列表 |
//! | 2 以上 | 先开方向列表选一格，选完再开那一格的物品列表 |
//!
//! - **方向列表**（[`PlayerMenu::InteractDirection`]）：选「和哪一格
//!   交互」。只在两格以上有东西时出现。
//! - **物品列表**（[`PlayerMenu::Interact`]）：选「这一格上的哪一样」。
//!   **无条件**出现——哪怕那一格只有一件东西，也是一行的列表，不是
//!   直接捡走（所有者原话「不是捡走，只是打开交互列表」）。
//!
//! 「只有一个」指的是**只有一格有东西**，不是「总共只有一件物品」：
//! 脚下躺着一把剑、四周全空，跳过的是方向列表那一层，物品列表照弹。
//!
//! 「同按空格 + 方向」那条路径**不存在**（所有者原话「不需要什么
//! 空格+方向了」）：方向只在方向列表里用方向键选，与在背包/制作菜单里
//! 上下移光标是同一套导航。
//!
//! 物品列表里每一行按 `GameKey::Confirm` 执行它的**主交互**：立着的
//! 设施 → 在它这儿开工（打开制作菜单）；尸体 → 搜刮（`Intent::Loot`）；
//! 其余 → 捡起（`Intent::PickUp`）。按 `GameKey::PickUp` 则是「不管它
//! 是什么，把选中的这一样捡走」。
//!
//! 所有者另有一句「普通物品和第一点应该是一样的」，因此这两块列表都不
//! 是家具专属机制：立着的炉子和散落的铁锭在列表里是平等的两行。
//!
//! # 范围为什么是脚下加相邻八格
//!
//! 因为**移动是八向的**：[`ll_sim::intent::Direction`] 有八个变体，
//! `intent_from_input` 的方向键组合会产出四条对角线。交互范围若只取
//! 正交四邻，玩家会遇到「斜前方那堆东西看得见、走一步就到，却伸手够
//! 不着」这种毫无道理的不一致。够得着的判定本身在结算层
//! （`ll_sim::resolve` 的 `INTERACT_REACH`），本层只负责扫出候选格。
//!
//! # 三块菜单的键位全部走绑定表
//!
//! 交互键、方向键、确认键、捡起键都是 `ll_platform::input::GameKey` 的
//! 抽象动作，绑在哪个物理键上由 `ll_platform::keybind::KeyBindings`
//! 决定（`config.json5` 可改）。本模块从不比对键码——所有者那句「这些
//! 都可以修改键位的」由这条结构性事实满足，不需要额外做什么。
//!
//! # 菜单状态算不算「跨帧隐式状态」（约束 C1）
//!
//! 约束 C1 禁止在 `WorldState` 之外留跨帧隐式状态。[`PlayerMenu`] 是
//! 跨帧的，也确实不在 `WorldState` 里，所以这个问题必须正面回答。
//!
//! 结论是**不算**，与 `crate::app::Demo::world_map_open`（M 键开关）
//! 和 `hud_anim`（条形动画旁表）同一条既有先例，判据有三条：
//!
//! 1. **结算层读不到它。** 它不作为任何 `ll_sim::resolve` 输入出现，
//!    连引用都传不进去——`resolve` 的参数表里没有它的位置。
//! 2. **回放不需要它。** 决定一局怎么走的是 `Intent` 流加世界种子
//!    （见 `ll_sim::intent` 模块文档开篇）。菜单只影响「这一帧产出的是
//!    哪个 `Intent`」，产出之后它对世界再无影响；拿同一串 `Intent`
//!    重放，光标当时停在哪一行完全无关紧要。
//! 3. **它不进存档。** 退出时菜单关不关、光标在第几行，读档后重来一遍
//!    没有任何可观察差异。
//!
//! 反过来说，真正会违反 C1 的写法是把「玩家上一次选了什么」攒起来影响
//! 后续结算（例如「连续制作同一条配方有加成」这类）——那种状态属于世界，
//! 必须进 `WorldState`。本模块不做这种事。
//!
//! 「正在选方向」（[`PlayerMenu::InteractDirection`]）与「正在选这一格
//! 上的哪一样」（[`PlayerMenu::Interact`]）都是同一个枚举里的态，上面
//! 三条判据逐条同样成立，不需要第二套论证。
//!
//! **刻意没有用 `ll_platform::keybind::InputContext` 来表达它们**：那个
//! 机制解决的是「同一个物理键在不同场景绑不同动作」，而这里要的恰恰
//! 相反——方向键在这两个态下仍然要解析成 `GameKey::Up/Down/Left/Right`
//! （所以玩家改了移动键位，列表导航也跟着改），变的只是**这一层怎么
//! 解读**它。真去切上下文还要把「当前上下文」从这里一路穿回
//! `ll_platform::window` 的事件循环（那里目前恒传
//! `InputContext::Gameplay`），换来的是同一个结果加一条跨 crate 的
//! 回传通道。
//!
//! # 为什么装备与卸下共用一个键
//!
//! 背包菜单的列表是**两段拼起来的**：先是背在包里的堆
//! （[`InventoryEntry::Carried`]），再是已经穿在身上的
//! （[`InventoryEntry::Equipped`]）。`GameKey::Equip` 落在前一段就是
//! 装备（`Intent::Equip`），落在后一段就是卸下（`Intent::Unequip`）。
//!
//! 这不是「看起来该对称所以合并」（ADR 0021 反对的那种），恰恰相反：
//! 拆成两个键、两块面板，玩家要记的是「装备键只在背包面板管用、卸下键
//! 只在装备面板管用」这条纯粹由实现细节决定的规矩，而两块面板列的是
//! 同一类东西（一件件装备）、要做的是同一件事（把它挪到另一段去）。
//! 合成一个键之后规矩变成「对着它按 E，它就换一边」——这是玩家真正
//! 需要理解的那条规则，少一个键、少一块面板、少一条规矩。

use ll_core::ident::ContentIndex;
use ll_core::torus::TorusPos;
use ll_i18n::Catalog;
use ll_mod::item::ItemTable;
use ll_mod::recipe::RecipeTable;
use ll_platform::input::{GameKey, InputState};
use ll_sim::intent::{Direction, Intent, intent_from_input};
use ll_ui::hud::action_menu::{ActionMenuData, MenuPlacement};
use ll_ui::hud::item_display_name;
use ll_world::entity::{Agent, EntityId};
use ll_world::item::EquipSlot;
use ll_world::state::WorldState;

/// 玩家菜单当前是关着的，还是开着哪一块、光标停在第几行。
///
/// 见模块文档「菜单状态算不算跨帧隐式状态」一节——它是纯表现层状态，
/// 不进 `WorldState`、不进存档、不参与回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerMenu {
    /// 没有菜单打开——方向键正常移动角色。
    #[default]
    Closed,
    /// 背包菜单（`GameKey::Inventory` 切换）：背包堆 + 已装备堆两段。
    Inventory {
        /// 光标落在合并后列表的第几行。
        cursor: usize,
    },
    /// 制作菜单（`GameKey::Craft` 切换）：全部已注册配方。
    Craft {
        /// 光标落在配方列表的第几行。
        cursor: usize,
    },
    /// **方向列表**：范围内两格以上有东西时先选「和哪一格交互」。
    /// 见模块文档「交互键、方向列表、物品列表」一节那张表。
    InteractDirection {
        /// 光标落在候选格列表的第几行。
        cursor: usize,
    },
    /// **物品列表**：某一格上的东西，一样一行。无条件出现，一件也列
    /// 一行。
    Interact {
        /// 这块列表列的是哪一格——脚下或相邻八格之一，由方向列表选出
        /// （只有一格有东西时直接就是那一格）。够不够得着由结算层判
        /// （`ll_sim::resolve` 的 `INTERACT_REACH`）。
        pos: TorusPos,
        /// 光标落在候选列表的第几行。
        cursor: usize,
    },
}

impl PlayerMenu {
    /// 有没有菜单开着——开着时方向键归菜单用，不再移动角色，见
    /// [`player_command`] 第 ③ 步。
    pub fn is_open(self) -> bool {
        !matches!(self, PlayerMenu::Closed)
    }
}

/// 背包菜单的一行指向什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryEntry {
    /// 背在包里的一堆。
    Carried {
        /// 这一堆是哪一种东西。
        def: ContentIndex,
    },
    /// 已经穿在身上的一件。
    Equipped {
        /// 挂在哪个锚点槽位——`Intent::Unequip` 认的是槽位不是物品。
        slot: EquipSlot,
        /// 这件东西是什么，只用来排版显示。
        def: ContentIndex,
    },
}

/// 输入层自己就能判定的「这一下按空了」。
///
/// **只覆盖输入层看得见的情形**（列表是空的、光标没指着任何一行），
/// 不复制任何一条结算期前置判定——那些判据住在 `ll_sim::resolve`，在
/// 输入层再抄一遍正是 ADR 0021 点名要拦的「把同一个算法抄六遍」。
/// 结算期判定「什么都不发生」时的反馈走另一条路：
/// `ll_sim::turn::PlayerTurnOutcome::Nothing`，见
/// [`Feedback::NothingHappened`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    /// 菜单里没有任何可选条目，或者光标没指着任何一行。
    NoSelection,
    /// 意图已经提交给结算层，但结算判定这一步什么都不发生——放置家具
    /// 的三道前置任一不成立、脚下没东西可捡、食材不齐，都会落在这里。
    ///
    /// 只说「没起作用」，不说「为什么」：理由见
    /// `ll_sim::turn::PlayerTurnOutcome` 文档「本枚举不解释为什么」一节。
    NothingHappened,
    /// 按了交互键，但范围内（脚下加相邻八格）一格可交互的东西都没有。
    ///
    /// 与 [`Self::NothingHappened`] 分开，不是同一句话：这一条是输入层
    /// **自己**就能判定的（候选格是空的），而且它有一句确切的话可说，
    /// 比笼统的「这一下没有起作用」有用得多。凡是输入层能说清楚的就说
    /// 清楚，说不清楚的才退回那句笼统的——这条分界与
    /// [`Self::NoSelection`] 是同一条。
    NothingNearby,
}

impl Feedback {
    /// 这条反馈对应的 Fluent 键——用户可见文本一律走 i18n，与
    /// `ll_platform::input::GameKey::display_name_key` 同一条理由。
    pub fn i18n_key(self) -> &'static str {
        match self {
            Feedback::NoSelection => "hud-feedback-no-selection",
            Feedback::NothingHappened => "hud-feedback-nothing-happened",
            Feedback::NothingNearby => "hud-feedback-nothing-nearby",
        }
    }
}

/// [`player_command`] 这一帧的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommand {
    /// 这一帧没有要提交的东西——没按任何相关键，或者按的是翻菜单/移
    /// 光标这类不消耗回合的操作。
    Idle,
    /// 提交这个意图当作玩家这一回合。
    Submit(Intent),
    /// 按了，但输入层这一层就判定按空了，见 [`Feedback`]。
    Rejected(Feedback),
}

/// 背包菜单这一帧的行——背包堆在前，已装备的在后。
///
/// 顺序确定（约束 C5）：`Agent::inventory` 是 `Vec`（保序），
/// `Agent::equipment` 是 `BTreeMap`（按 [`EquipSlot`] 排序），两段都不
/// 涉及任何哈希容器的迭代顺序。这条对**显示顺序**尤其要紧——玩家按的
/// 是「第几行」，列表顺序在按下那一刻就是逻辑输入。
pub fn inventory_entries(agent: &Agent) -> Vec<InventoryEntry> {
    agent
        .inventory
        .iter()
        .map(|stack| InventoryEntry::Carried { def: stack.def })
        .chain(
            agent
                .equipment
                .iter()
                .map(|(slot, stack)| InventoryEntry::Equipped {
                    slot: *slot,
                    def: stack.def,
                }),
        )
        .collect()
}

/// 交互列表的一行指向脚下的什么东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractTarget {
    /// 立着的一件设施——主交互是「在它这儿开工」（打开制作菜单）。
    ///
    /// 判据是 [`ll_world::item::GroundItemStack::placed`]，**不是**
    /// 「它是不是某条配方的 `required_station`」：后者要查配方表，而
    /// 一件立在那里的东西无论有没有配方指着它，玩家想做的都是「用它」。
    /// 真的没有任何配方认它时，制作菜单照样打得开，只是选什么都做不出
    /// 来——那是 `resolve_craft` 的场地前置在说话，不是本层该抢答的。
    Facility {
        /// 这件设施是什么，只用来排版显示。
        def: ContentIndex,
    },
    /// 一具尸体/容器——主交互是搜刮（`Intent::Loot`）。
    Container {
        /// 容器本身这件「物品」的壳是什么，只用来排版显示。
        def: ContentIndex,
    },
    /// 散落的一堆——主交互是捡起（`Intent::PickUp`）。
    Loose {
        /// 这一堆是哪一种东西。
        def: ContentIndex,
    },
}

impl InteractTarget {
    /// 这一行指着的东西是什么（三个变体都携带同一个字段，本方法把那个
    /// 穷尽 `match` 收敛成一次调用）。
    pub fn def(self) -> ContentIndex {
        match self {
            InteractTarget::Facility { def }
            | InteractTarget::Container { def }
            | InteractTarget::Loose { def } => def,
        }
    }
}

/// 交互范围内的一格候选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractTile {
    /// 它相对行动者在哪个方向——`None` 表示就是脚下这一格。
    pub dir: Option<Direction>,
    /// 这一格的世界坐标（已归一化）。
    pub pos: TorusPos,
}

/// 交互范围内**有东西可交互**的那些格，按固定罗盘顺序。
///
/// 顺序是「脚下 → 北 → 东北 → 东 → 东南 → 南 → 西南 → 西 → 西北」：
/// 一个写死在 [`SCAN_ORDER`] 里的常量数组，不依赖任何容器的迭代顺序
/// （约束 C5）。这条在这里是**真陷阱**而不是形式要求：玩家按的是
/// 「第几行」，同一个存档同一处两次按空格若列出的方向顺序不一致，同一
/// 串按键就会作用到不同的格上，回放与存档一致性一起碎。
///
/// 范围是脚下加相邻**八**格，理由见模块文档「范围为什么是脚下加相邻
/// 八格」一节：移动是八向的。
pub fn interact_tiles(world: &WorldState, origin: TorusPos) -> Vec<InteractTile> {
    SCAN_ORDER
        .iter()
        .map(|dir| {
            let pos = match dir {
                Some(dir) => {
                    let (dx, dy) = dir.delta();
                    world.size.wrap(origin.x() + dx, origin.y() + dy)
                }
                None => origin,
            };
            InteractTile { dir: *dir, pos }
        })
        .filter(|tile| !interact_entries(world, tile.pos).is_empty())
        .collect()
}

/// [`interact_tiles`] 的扫描顺序：脚下优先，其余按罗盘顺时针。
///
/// 罗盘顺序而不是 [`Direction`] 的变体声明顺序（北/南/西/东/东北/…）：
/// 玩家看到的是一列方向名，顺时针排列读起来是一圈，声明顺序读起来是
/// 一堆。两者都确定，选好读的那个。
const SCAN_ORDER: [Option<Direction>; 9] = [
    None,
    Some(Direction::North),
    Some(Direction::NorthEast),
    Some(Direction::East),
    Some(Direction::SouthEast),
    Some(Direction::South),
    Some(Direction::SouthWest),
    Some(Direction::West),
    Some(Direction::NorthWest),
];

/// 这一格相对行动者的方向名的 Fluent 键。
fn direction_key(dir: Option<Direction>) -> &'static str {
    match dir {
        None => "hud-direction-here",
        Some(Direction::North) => "hud-direction-north",
        Some(Direction::NorthEast) => "hud-direction-north_east",
        Some(Direction::East) => "hud-direction-east",
        Some(Direction::SouthEast) => "hud-direction-south_east",
        Some(Direction::South) => "hud-direction-south",
        Some(Direction::SouthWest) => "hud-direction-south_west",
        Some(Direction::West) => "hud-direction-west",
        Some(Direction::NorthWest) => "hud-direction-north_west",
    }
}

/// 某一格上**可以交互**的东西，按 `ground_items` 的存储顺序。
///
/// # 分类与去重
///
/// - 立着的（`placed`）→ [`InteractTarget::Facility`]。一格至多一件
///   （`resolve_place` 的第 ④ 道前置保证），因此不需要去重。
/// - 容器（`contents` 非空，典型是尸体）→ [`InteractTarget::Container`]。
///   **只留第一具**：`Intent::Loot` 不带参数，恒搜刮脚下第一具容器
///   （见 `ll_sim::resolve` 的 `resolve_loot`），列出第二行会是一行按了
///   跟第一行效果一样的假选项。
/// - 其余 → [`InteractTarget::Loose`]，**同一个 `def` 只留第一次出现**：
///   `Intent::PickUp` 认的是 `def`，同 `def` 的第二堆按下去仍然会捡到
///   第一堆（见 `resolve_pick_up` 文档「同一格同一个 `def` 有两堆时取
///   哪一条」）。列出两行一模一样的东西、其中一行按了没有对应效果，
///   那是在骗玩家。
///
/// # 顺序确定（约束 C5）
///
/// `WorldState::ground_items` 是 `Vec`（保序），全程线性扫描，不涉及
/// 任何哈希容器。这条在这里是**真陷阱**而不是形式要求：玩家按的是
/// 「第几行」，同一个存档同一格两次打开列表的顺序若不一致，按同一串
/// 按键就会作用到不同的东西上，回放与存档一致性一起碎。
pub fn interact_entries(world: &WorldState, pos: TorusPos) -> Vec<InteractTarget> {
    let mut rows: Vec<InteractTarget> = Vec::new();
    let mut has_container = false;
    for ground in &world.ground_items {
        if ground.pos != pos {
            continue;
        }
        let def = ground.stack.def;
        if ground.placed {
            rows.push(InteractTarget::Facility { def });
        } else if !ground.contents.is_empty() {
            if !has_container {
                has_container = true;
                rows.push(InteractTarget::Container { def });
            }
        } else if !rows
            .iter()
            .any(|row| matches!(row, InteractTarget::Loose { def: seen } if *seen == def))
        {
            rows.push(InteractTarget::Loose { def });
        }
    }
    rows
}

/// 制作菜单这一帧的行——全部已注册配方，按索引升序。/// 制作菜单这一帧的行——全部已注册配方，按索引升序。
///
/// 不在这里做任何「玩家现在做不做得出来」的筛选：那些判据（副职闸门、
/// 已知闸门、场地、工具、食材）全都住在 `ll_sim::resolve` 的
/// `resolve_craft` 里，在这里复制一份就是 ADR 0021 点名要拦的重复实现，
/// 而且两份判据迟早会分叉——分叉的表现是「菜单里能选，按下去没反应」
/// 或者更糟的「明明做得出来，菜单里却没有」。
///
/// 玩家看得出「为什么做不出来」靠的是行文本本身：每一行都列出食材、
/// 场地与工具（见 [`craft_row_text`]），对着背包一看便知。
pub fn craft_entries(recipes: &RecipeTable) -> Vec<ContentIndex> {
    recipes.defined_indices()
}

/// 把这一帧的输入翻译成「玩家这一回合要提交什么」。
///
/// 判定顺序（先到先得，每一步都可能提前返回）：
///
/// ```text
/// ① 背包键/制作键：切换对应菜单（另一块开着就换成这块）→ Idle
/// ② 菜单关着：拾取键 → 看脚下有几堆（0 报空 / 1 直接捡 / 多开菜单）
/// ③ 菜单关着且没按拾取：回落到 intent_from_input（Move/Wait）
/// ④ 菜单开着且按了取消：关掉 → Idle
/// ⑤ 菜单开着：上/下移光标 → Idle；动作键 → Submit/Rejected
/// ```
///
/// 第 ⑤ 步**必须**在第 ③ 步管不到的地方（菜单开着时压根不走第 ③ 步）：
/// 否则玩家在菜单里按上下选东西的同时，角色会在地图上一路走动。
///
/// 收 `&WorldState` 而不是 `&Agent`：拾取候选来自 `world.ground_items`，
/// 背包与配方候选来自 `agent`/`recipes`，三者只有前者够不着——多传一个
/// `&Agent` 参数会让调用方有责任保证「这个 agent 就是 world 里那个
/// `actor`」，那是一条没人强制得了的隐式约定。
pub fn player_command(
    menu: &mut PlayerMenu,
    input: &InputState,
    world: &WorldState,
    actor: EntityId,
    recipes: &RecipeTable,
) -> PlayerCommand {
    let Some(agent) = world.actors.get(actor) else {
        return PlayerCommand::Idle;
    };
    // ① 两个菜单开关。用 `was_just_pressed` 而非 `was_activated`：与
    // `GameKey::Map` 同一类一次性动作键（`GameKey::is_repeatable` 没有
    // 收进这几个），长按不该反复开关。
    if input.was_just_pressed(GameKey::Inventory) {
        *menu = match menu {
            PlayerMenu::Inventory { .. } => PlayerMenu::Closed,
            _ => PlayerMenu::Inventory { cursor: 0 },
        };
        return PlayerCommand::Idle;
    }
    if input.was_just_pressed(GameKey::Craft) {
        *menu = match menu {
            PlayerMenu::Craft { .. } => PlayerMenu::Closed,
            _ => PlayerMenu::Craft { cursor: 0 },
        };
        return PlayerCommand::Idle;
    }
    // 交互键：扫一圈范围，按有东西的格数分三种（见模块文档那张表）。
    // 两块交互菜单开着时再按一次就关掉——与另外两个开关同形。
    if input.was_just_pressed(GameKey::Interact) {
        if matches!(
            menu,
            PlayerMenu::Interact { .. } | PlayerMenu::InteractDirection { .. }
        ) {
            *menu = PlayerMenu::Closed;
            return PlayerCommand::Idle;
        }
        return begin_interact(menu, world, agent.pos);
    }
    // 拾取键在**菜单外**按下时，开的是同一条交互流程——[`begin_interact`]
    // 是唯一的实现，两个键只是两个入口，不是两条拾取路径（所有者原话
    // 「统一以列表显示」，见模块文档）。在物品列表**里**按它是「把选中
    // 的这一样捡走」，那一支在下面的 `PlayerMenu::Interact` 分支里。
    if input.was_just_pressed(GameKey::PickUp) && !menu.is_open() {
        return begin_interact(menu, world, agent.pos);
    }

    if !menu.is_open() {
        return closed_menu_command(input, actor);
    }
    // ④ 取消键关菜单。**必须在这里拦下**：`crate::app` 的
    // `AppHandler::on_frame` 把 `GameKey::Cancel` 当作「退出游戏」，
    // 菜单开着时按取消如果穿透过去，玩家想关个背包会直接退出整局。
    if input.was_just_pressed(GameKey::Cancel) {
        *menu = PlayerMenu::Closed;
        return PlayerCommand::Idle;
    }

    match *menu {
        // `is_open` 已经在上面排除掉这一支，写成回落而不是
        // `unreachable!()`：一个恒不成立的分支不该用 panic 表达，那是
        // 把「将来有人改了 `is_open`」的代价从"多走一条无害的回落"
        // 抬成"整个游戏崩溃"。
        PlayerMenu::Closed => closed_menu_command(input, actor),
        PlayerMenu::Inventory { cursor } => {
            let entries = inventory_entries(agent);
            match moved_cursor(input, cursor, entries.len()) {
                Some(next) => {
                    *menu = PlayerMenu::Inventory { cursor: next };
                    PlayerCommand::Idle
                }
                None => {
                    inventory_command(input, actor, &entries, cursor_row(cursor, entries.len()))
                }
            }
        }
        PlayerMenu::Craft { cursor } => {
            let entries = craft_entries(recipes);
            match moved_cursor(input, cursor, entries.len()) {
                Some(next) => {
                    *menu = PlayerMenu::Craft { cursor: next };
                    PlayerCommand::Idle
                }
                None => craft_command(input, actor, &entries, cursor_row(cursor, entries.len())),
            }
        }
        PlayerMenu::InteractDirection { cursor } => {
            let tiles = interact_tiles(world, agent.pos);
            match moved_cursor(input, cursor, tiles.len()) {
                Some(next) => {
                    *menu = PlayerMenu::InteractDirection { cursor: next };
                    PlayerCommand::Idle
                }
                None => {
                    if !input.was_just_pressed(GameKey::Confirm) {
                        return PlayerCommand::Idle;
                    }
                    // 选完方向进那一格的物品列表——**不**在这里就提交
                    // 任何意图：所有者原话「不是捡走，只是打开交互列表」。
                    match cursor_row(cursor, tiles.len()).map(|row| tiles[row]) {
                        Some(tile) => {
                            *menu = PlayerMenu::Interact {
                                pos: tile.pos,
                                cursor: 0,
                            };
                            PlayerCommand::Idle
                        }
                        None => {
                            *menu = PlayerMenu::Closed;
                            PlayerCommand::Rejected(Feedback::NoSelection)
                        }
                    }
                }
            }
        }
        PlayerMenu::Interact { pos, cursor } => {
            let entries = interact_entries(world, pos);
            match moved_cursor(input, cursor, entries.len()) {
                Some(next) => {
                    *menu = PlayerMenu::Interact { pos, cursor: next };
                    PlayerCommand::Idle
                }
                None => interact_command(
                    input,
                    menu,
                    actor,
                    pos,
                    agent.pos,
                    &entries,
                    cursor_row(cursor, entries.len()),
                ),
            }
        }
    }
}

/// 物品列表里的两个动作键。
///
/// `pos` 是这块列表列的那一格，`actor_pos` 是行动者站的那一格——两者
/// 不一定相同（方向列表可以选中相邻格），主交互的分派要用到这条区别，
/// 见下面 `Facility` 那一支。
#[allow(clippy::too_many_arguments)]
fn interact_command(
    input: &InputState,
    menu: &mut PlayerMenu,
    actor: EntityId,
    pos: TorusPos,
    actor_pos: TorusPos,
    entries: &[InteractTarget],
    row: Option<usize>,
) -> PlayerCommand {
    let pressed_confirm = input.was_just_pressed(GameKey::Confirm);
    let pressed_pick_up = input.was_just_pressed(GameKey::PickUp);
    if !(pressed_confirm || pressed_pick_up) {
        return PlayerCommand::Idle;
    }
    let naked = (pos.x(), pos.y());
    let Some(target) = row.map(|row| entries[row]) else {
        *menu = PlayerMenu::Closed;
        return PlayerCommand::Rejected(Feedback::NoSelection);
    };
    // 「不管它是什么，把这一样捡走」——立着的炉子也能这样收回背包，
    // 这是「摆下去还能收回来」那条闭环的出口。
    if pressed_pick_up {
        *menu = PlayerMenu::Closed;
        return PlayerCommand::Submit(Intent::PickUp {
            actor,
            pos: naked,
            def: target.def(),
        });
    }
    // 主交互按东西的种类分派，见 [`InteractTarget`] 各变体文档。
    match target {
        // 在这件设施这儿开工：换开制作菜单，不消耗回合。这块菜单与
        // `GameKey::Craft` 直接打开的是**同一块**（同一个
        // `PlayerMenu::Craft`、同一份行、同一个 `Intent::Craft`），
        // 两个入口一份实现。
        //
        // **只有站在它上面才开工**：`resolve_craft` 第 ⑤ 步的场地判定
        // 是「站在这格上」（`crafting-system.md` 六节，相邻判定会引入
        // 「多个相邻工作台算哪个」这类问题），本层不去推翻那条既有裁定。
        // 隔一格对着炉子按确认时退化成「捡起它」——那是这一格上这件
        // 东西此刻真正做得到的事，比弹一块按了没反应的「开工」诚实。
        InteractTarget::Facility { def } => {
            *menu = if pos == actor_pos {
                PlayerMenu::Craft { cursor: 0 }
            } else {
                PlayerMenu::Closed
            };
            if pos == actor_pos {
                PlayerCommand::Idle
            } else {
                PlayerCommand::Submit(Intent::PickUp {
                    actor,
                    pos: naked,
                    def,
                })
            }
        }
        InteractTarget::Container { .. } => {
            *menu = PlayerMenu::Closed;
            PlayerCommand::Submit(Intent::Loot { actor, pos: naked })
        }
        InteractTarget::Loose { def } => {
            // 选完就把菜单关掉：捡走之后这一格的候选列表已经变了，让它
            // 停在原地只会让下一次按确认落在一个不同的东西上。
            *menu = PlayerMenu::Closed;
            PlayerCommand::Submit(Intent::PickUp {
                actor,
                pos: naked,
                def,
            })
        }
    }
}

/// 按下交互键的那一刻：扫一圈范围，按有东西的格数分三种。
///
/// 见模块文档「交互键、方向列表、物品列表」一节那张表：0 格报一句、
/// 1 格直接开那一格的物品列表、2 格以上先开方向列表。
///
/// **一格上只有一件东西时也开物品列表**（不走「直接捡走」的捷径）——
/// 所有者原话「不是捡走，只是打开交互列表」。跳过的只是方向列表那一层。
fn begin_interact(menu: &mut PlayerMenu, world: &WorldState, origin: TorusPos) -> PlayerCommand {
    let tiles = interact_tiles(world, origin);
    match tiles.len() {
        0 => {
            *menu = PlayerMenu::Closed;
            PlayerCommand::Rejected(Feedback::NothingNearby)
        }
        1 => {
            *menu = PlayerMenu::Interact {
                pos: tiles[0].pos,
                cursor: 0,
            };
            PlayerCommand::Idle
        }
        _ => {
            *menu = PlayerMenu::InteractDirection { cursor: 0 };
            PlayerCommand::Idle
        }
    }
}

/// 光标当前真正指着第几行——列表为空或光标越界时是 `None`。
///
/// 不就地钳制 `cursor`：钳制会让「列表在光标之下缩短了」与「玩家自己把
/// 光标移到了这里」变得不可区分，而 [`moved_cursor`] 的环绕已经保证正常
/// 操作下光标恒在范围内；真落到越界，说明列表在两帧之间变短了（刚做完
/// 一次消耗掉最后一味食材的制作之类），这一帧如实报告「没指着任何行」
/// 比默默指到别的东西上安全——后者会让玩家丢掉/用掉一件他没打算动的
/// 东西。
fn cursor_row(cursor: usize, len: usize) -> Option<usize> {
    (cursor < len).then_some(cursor)
}

/// 这一帧光标该移到第几行——没有移动时返回 `None`，调用方据此接着
/// 判动作键。
///
/// `was_activated` 而非 `was_just_pressed`：方向键参与自动重复
/// （`GameKey::is_repeatable`），长按连续滚动与它们在地图上长按连续
/// 移动是同一种手感，不该在菜单里变成一次一格。
///
/// 环绕而不是撞到头就停：列表短（本体一共十来条配方），从头绕到尾比
/// 一路按回去快，且不需要玩家记得「到顶了」。
fn moved_cursor(input: &InputState, cursor: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let up = input.was_activated(GameKey::Up);
    let down = input.was_activated(GameKey::Down);
    // 同时按住上下视为无输入——与 `intent_from_input` 里
    // `direction_from_input` 对相反方向同时按住的处理一致（两者抵消，
    // 不猜测玩家意图）。
    if up == down {
        return None;
    }
    Some(if down {
        (cursor + 1) % len
    } else {
        (cursor + len - 1) % len
    })
}

/// 菜单关着、交互/拾取键也没按时：回落到既有的 `Move`/`Wait` 映射。
fn closed_menu_command(input: &InputState, actor: EntityId) -> PlayerCommand {
    // ③ 回落：`Move`/`Wait` 仍然由 `ll-sim` 那份既有映射产出，本模块
    // 一行都不重写它。
    match intent_from_input(actor, input) {
        Some(intent) => PlayerCommand::Submit(intent),
        None => PlayerCommand::Idle,
    }
}

/// 背包菜单里的三个动作键。
fn inventory_command(
    input: &InputState,
    actor: EntityId,
    entries: &[InventoryEntry],
    row: Option<usize>,
) -> PlayerCommand {
    // 四个动作键各查一次。按同一帧多个键同时按下的极端情形取**先到
    // 先得**（下面 `match` 的分支顺序），不试图猜「玩家真正想按的是
    // 哪个」——真实键盘上同一帧按下两个动作键几乎不会发生，为它设计
    // 一套优先级只会多一条没人测得到的规则。
    let pressed_drop = input.was_just_pressed(GameKey::Drop);
    let pressed_place = input.was_just_pressed(GameKey::Place);
    let pressed_equip = input.was_just_pressed(GameKey::Equip);
    let pressed_use = input.was_just_pressed(GameKey::Use);
    if !(pressed_drop || pressed_place || pressed_equip || pressed_use) {
        return PlayerCommand::Idle;
    }
    let Some(entry) = row.map(|row| entries[row]) else {
        return PlayerCommand::Rejected(Feedback::NoSelection);
    };
    match entry {
        InventoryEntry::Carried { def } => {
            if pressed_drop {
                // 丢：东西从手里掉在脚下，躺着的一堆普通物品。
                PlayerCommand::Submit(Intent::Drop { actor, def })
            } else if pressed_place {
                // 立：占住这一格的设施。与丢是**两个动作**，见
                // `ll_sim::intent::Intent::Place` 文档。这东西能不能立
                // 由 `resolve_place` 判（`ItemDef.furniture`），本层不
                // 复制那条判据。
                PlayerCommand::Submit(Intent::Place { actor, def })
            } else if pressed_equip {
                PlayerCommand::Submit(Intent::Equip { actor, def })
            } else {
                PlayerCommand::Submit(Intent::Use { actor, def })
            }
        }
        // 已经穿在身上的：装备键改判成卸下（见模块文档「为什么装备与
        // 卸下共用一个键」）。丢弃/放置/使用键对着装备段按不产出任何
        // 意图——要丢要立要用先卸下来，这与
        // `resolve_drop`/`resolve_place`/`resolve_use_item` 只认背包里
        // 的堆（三者都从 `agent.inventory` 里找）完全一致，不是本模块
        // 另立的规矩。
        InventoryEntry::Equipped { slot, .. } => {
            if pressed_equip {
                PlayerCommand::Submit(Intent::Unequip { actor, slot })
            } else {
                PlayerCommand::Rejected(Feedback::NoSelection)
            }
        }
    }
}

/// 制作菜单里的确认键。
fn craft_command(
    input: &InputState,
    actor: EntityId,
    entries: &[ContentIndex],
    row: Option<usize>,
) -> PlayerCommand {
    if !input.was_just_pressed(GameKey::Confirm) {
        return PlayerCommand::Idle;
    }
    match row.map(|row| entries[row]) {
        Some(recipe) => PlayerCommand::Submit(Intent::Craft { actor, recipe }),
        None => PlayerCommand::Rejected(Feedback::NoSelection),
    }
}

/// 背包菜单一行的显示文本。
pub fn inventory_row_text(
    entry: InventoryEntry,
    agent: &Agent,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let identified = &agent.identified_items;
    match entry {
        InventoryEntry::Carried { def } => {
            let name = item_display_name(def, items, catalog, language, identified);
            let count = agent
                .inventory
                .iter()
                .find(|stack| stack.def == def)
                .map_or(0, |stack| stack.count);
            format!("{name} x{count}")
        }
        InventoryEntry::Equipped { def, .. } => {
            let name = item_display_name(def, items, catalog, language, identified);
            let label = catalog.resolve(language, "hud-inventory-menu-equipped-label");
            format!("{name}（{label}）")
        }
    }
}

/// 制作菜单一行的显示文本：成品名 + 食材清单 + 场地/工具前置。
///
/// 三样前置都列出来，玩家对着背包一看就知道差什么——这是本模块**不**
/// 在菜单里过滤「现在做不出来的配方」之后仍然可用的原因，见
/// [`craft_entries`] 文档。
pub fn craft_row_text(
    recipe: ContentIndex,
    recipes: &RecipeTable,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let Some(view) = recipes.get(recipe) else {
        // 查不到定义时退化显示原始索引，与
        // `ll_ui::hud::inventory_panel` 模块文档「查不到物品定义时怎么
        // 办」同一条纪律：不 panic、不悄悄跳过整行。
        return format!("#{}", recipe.get());
    };
    let name = catalog.resolve(language, &view.display_name_key.to_string());
    // 成品与食材的名字都走 `item_display_name` 的**已鉴定**分支：配方
    // 表上写着的东西是内容作者声明的，不是玩家在地上捡到的一件未知物，
    // 不该因为玩家还没鉴定过就显示成「未鉴定的物品」。
    let known: &[ContentIndex] = &[];
    let ingredients: Vec<String> = view
        .ingredients
        .iter()
        .map(|ingredient| {
            let item = item_display_name(ingredient.item, items, catalog, language, known);
            format!("{item} x{}", ingredient.count)
        })
        .collect();
    let mut text = format!("{name} <= {}", ingredients.join(", "));
    if let Some(station) = view.required_station {
        let label = catalog.resolve(language, "hud-craft-station-label");
        let station = item_display_name(station, items, catalog, language, known);
        text.push_str(&format!(" | {label}: {station}"));
    }
    if let Some(tool) = view.required_tool {
        let label = catalog.resolve(language, "hud-craft-tool-label");
        let tool = item_display_name(tool, items, catalog, language, known);
        text.push_str(&format!(" | {label}: {tool}"));
    }
    text
}

/// 这一帧要给 `ll_ui::hud::render::build_hud_frame` 的菜单行——菜单
/// 关着时返回空 `Vec`（调用方据此传 `None`）。
///
/// 与 [`player_command`] 各自独立地重建一次列表：两者跑在同一帧的同一
/// 份 `Agent`/`RecipeTable` 上（`advance` 与 `draw_hud` 之间没有别的
/// 写入点），列表必然逐条相同。把它攒成一个字段跨帧复用才是真正的
/// 风险——那份缓存要有人负责在背包变化时失效，而背包每一次结算都可能
/// 变，见 `crate::app::draw_hud` 里「派生而不缓存」同一条纪律。
pub fn menu_rows(
    menu: PlayerMenu,
    world: &WorldState,
    actor: EntityId,
    recipes: &RecipeTable,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> Vec<String> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    match menu {
        PlayerMenu::Closed => Vec::new(),
        PlayerMenu::Inventory { .. } => inventory_entries(agent)
            .into_iter()
            .map(|entry| inventory_row_text(entry, agent, items, catalog, language))
            .collect(),
        PlayerMenu::Craft { .. } => craft_entries(recipes)
            .into_iter()
            .map(|recipe| craft_row_text(recipe, recipes, items, catalog, language))
            .collect(),
        PlayerMenu::Interact { pos, .. } => interact_entries(world, pos)
            .into_iter()
            .map(|target| interact_row_text(target, pos, world, agent, items, catalog, language))
            .collect(),
        PlayerMenu::InteractDirection { .. } => interact_tiles(world, agent.pos)
            .into_iter()
            .map(|tile| direction_row_text(tile, world, agent, items, catalog, language))
            .collect(),
    }
}

/// 方向列表一行的显示文本：方向名 + 那一格上第一样东西的名字。
///
/// 带上「那儿有什么」是这块列表唯一有用的信息：光有「北」「东南」两行
/// 方向名，玩家还是不知道该往哪一格去。只列第一样（后面加省略号）是
/// 因为这一行的作用是**认出是哪一格**，不是复述那一格的完整清单——
/// 完整清单在选完之后的物品列表里。
pub fn direction_row_text(
    tile: InteractTile,
    world: &WorldState,
    agent: &Agent,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let direction = catalog.resolve(language, direction_key(tile.dir));
    let entries = interact_entries(world, tile.pos);
    let first = entries
        .first()
        .map(|target| {
            item_display_name(
                target.def(),
                items,
                catalog,
                language,
                &agent.identified_items,
            )
        })
        .unwrap_or_default();
    if entries.len() > 1 {
        let more = catalog.resolve(language, "hud-interact-direction-more");
        format!("{direction}：{first} {more}")
    } else {
        format!("{direction}：{first}")
    }
}

/// 交互列表一行的显示文本：名字 + 数量 + 这一行的主交互是什么。
///
/// 标出主交互是玩家唯一能在这块列表里分辨「这是我立着的炉子（按确认
/// 会开工）」和「这是掉在这儿的一座炉子（按确认会捡走）」的途径——
/// 两者名字一模一样，后果完全不同。
#[allow(clippy::too_many_arguments)]
pub fn interact_row_text(
    target: InteractTarget,
    pos: TorusPos,
    world: &WorldState,
    agent: &Agent,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let def = target.def();
    let name = item_display_name(def, items, catalog, language, &agent.identified_items);
    let count = world
        .ground_items
        .iter()
        .find(|ground| ground.pos == pos && ground.stack.def == def)
        .map_or(0, |ground| ground.stack.count);
    let action_key = match target {
        InteractTarget::Facility { .. } => "hud-interact-action-work",
        InteractTarget::Container { .. } => "hud-interact-action-loot",
        InteractTarget::Loose { .. } => "hud-interact-action-take",
    };
    let action = catalog.resolve(language, action_key);
    format!("{name} x{count}（{action}）")
}

/// 把菜单状态与已经排好版的行拼成 `ll-ui` 要的那份数据——菜单关着时
/// 是 `None`，整块面板不参与这一帧的产出。
///
/// # 位置逐个变体声明（所有者裁定：交互窗口居中）
///
/// > 「那个互动显示的 UI 窗口，我希望是出现在屏幕正中间」
///
/// 三块菜单共用 `ll_ui::hud::render::build_hud_frame` 的**同一条**渲染
/// 路径（那个参数是 `Option<&ActionMenuData>`，认不出打开的是哪一块），
/// 因此「画在哪」不能由渲染层拍——那会把背包与制作一并挪走。位置是
/// 这份数据的一个字段（[`MenuPlacement`]），在这里按 [`PlayerMenu`] 的
/// 变体逐个声明：
///
/// - **交互列表**与**方向列表**（同一次交互流程的两步）→
///   [`MenuPlacement::ScreenCenter`]，所有者要的那个位置。
/// - **背包**与**制作** → [`MenuPlacement::TopCenter`]，**原位不动**，
///   与本次改动之前逐像素相同。所有者只提了交互那一块。
pub fn menu_data(menu: PlayerMenu, rows: &[String]) -> Option<ActionMenuData<'_>> {
    match menu {
        PlayerMenu::Closed => None,
        PlayerMenu::Inventory { cursor } => Some(ActionMenuData {
            title_key: "hud-inventory-menu-title",
            rows,
            cursor,
            empty_key: "hud-inventory-menu-empty",
            hint_key: "hud-inventory-menu-hint",
            placement: MenuPlacement::TopCenter,
        }),
        PlayerMenu::Craft { cursor } => Some(ActionMenuData {
            title_key: "hud-craft-menu-title",
            rows,
            cursor,
            empty_key: "hud-craft-menu-empty",
            hint_key: "hud-craft-menu-hint",
            placement: MenuPlacement::TopCenter,
        }),
        PlayerMenu::Interact { cursor, .. } => Some(ActionMenuData {
            title_key: "hud-interact-menu-title",
            rows,
            cursor,
            empty_key: "hud-interact-menu-empty",
            hint_key: "hud-interact-menu-hint",
            placement: MenuPlacement::ScreenCenter,
        }),
        PlayerMenu::InteractDirection { cursor } => Some(ActionMenuData {
            title_key: "hud-interact-direction-title",
            rows,
            cursor,
            empty_key: "hud-interact-direction-prompt",
            hint_key: "hud-interact-direction-hint",
            placement: MenuPlacement::ScreenCenter,
        }),
    }
}
