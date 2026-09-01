//! 交互列表：脚下与相邻八格上**有什么可以交互**，以及每一行显示成
//! 什么字。
//!
//! 从 [`crate::player_action`] 拆出来的（对话批次 2）。分界线是一句话：
//! **本模块回答「有什么」，那边回答「按了什么键、于是提交什么」。**
//! 拆的直接触发是 §13 的 800 行上限——对话那一行让 `player_action.rs`
//! 越了线；但这条分界本身在拆之前就成立：本模块里没有一个函数读
//! [`crate::player_action::PlayerMenu`] 或 `InputState`。
//!
//! # 范围与顺序（约束 C5）
//!
//! 范围是**脚下加相邻八格**（八向，与移动一致，所有者裁定）。
//! 两处顺序都是真陷阱而不是形式要求——**玩家按的是「第几行」**：
//!
//! - 方向的顺序写死在 [`SCAN_ORDER`] 这个常量数组里；
//! - 一格上各行的顺序由 `WorldState::ground_items`（`Vec`，保序）的
//!   线性扫描决定，对话那一行由
//!   [`ll_sim::resolve::occupant_at`]（`Arena`，由 `Vec` 支撑）取。
//!
//! 全程不碰任何哈希容器的迭代顺序。同一个存档同一格两次打开列表的顺序
//! 若不一致，按同一串按键就会作用到不同的东西上，回放与存档一致性一起碎。

use ll_core::ident::ContentIndex;
use ll_core::torus::TorusPos;
use ll_i18n::Catalog;
use ll_mod::class::ClassTable;
use ll_mod::dialogue::DialogueTable;
use ll_mod::item::ItemTable;
use ll_sim::ai_query::declared_hostile;
use ll_sim::intent::Direction;
use ll_sim::resolve::occupant_at;
use ll_ui::hud::item_display_name;
use ll_world::culture::CultureTable;
use ll_world::entity::{AffiliationKind, Agent, EntityId, OrgRef};
use ll_world::state::WorldState;

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
    /// 一扇门——主交互是开或关（`Intent::OpenDoor` / `Intent::CloseDoor`）。
    ///
    /// # 这个变体怎么容纳「目标不是一件物品」
    ///
    /// 另外三个变体指着的都是一件**物品**（`ground_items` 里的一条），
    /// 门是**地形**：它没有 `ItemDef`、不在 `ground_items` 里、捡不起来。
    /// 因此本变体**不带 `ContentIndex`**，而那个「三个变体都携带同一个
    /// 字段」的收敛方法（旧名 `def`，返回裸 `ContentIndex`）已经改成
    /// [`InteractTarget::item_def`]，返回 `Option<ContentIndex>`——门那一
    /// 支是 `None`。
    ///
    /// 换句话说：**类型层面第一次表达了「这一行未必指着一件物品」**，
    /// 而不是随便找个索引塞进去冒充（那正是尸体 `def` 那次类型混淆的
    /// 形状，见 `ll_mod::corpse_item` 模块文档）。全部把 `item_def` 当
    /// 物品索引用的地方——`interact_row_text` 的名字与数量、按拾取键的
    /// 那条捷径——因此在门这一行上自然地什么都不做，编译器逼着每一处
    /// 都表态。
    ///
    /// 门当前是开是关不存在这个值里，只存一个「按下去要做什么」：地形
    /// 本身是世界状态，重新查一次比在这里缓存一份更不容易过期。
    Door {
        /// 按下去是开门还是关门。
        action: DoorAction,
    },
    /// 站在这一格上的一个人——主交互是**开口说话**（打开会话屏）。
    ///
    /// # 它与门那一支是同一类：目标不是一件物品
    ///
    /// [`InteractTarget::item_def`] 返回 `None`，于是
    /// [`interact_row_text`] 的数量、按拾取键那条捷径在它身上自然什么
    /// 都不做——门那一支当初把签名从裸 [`ContentIndex`] 改成
    /// `Option<ContentIndex>` 白送的好处，见 [`InteractTarget::Door`]
    /// 文档「这个变体怎么容纳『目标不是一件物品』」一节。
    ///
    /// # 为什么**带** `EntityId`（批次 21 的第 1 条临时裁定就此反转）
    ///
    /// 原文是「『这一格上站着谁』是世界状态，重新查一次比在这里缓存一份
    /// 更不容易过期」，并写明**反转条件**：「批次 4/5 的 `give-item`/
    /// `open-trade` 真的需要『给谁』时把它加回来，那时它从第一天起就有
    /// 消费者」。**加入据点这一批就是那一刻**（比预告的批次 4 早一批）：
    /// `ll_sim::dialogue::DialogueOutcome::JoinSettlement` 要读说话人的
    /// `ll_world::entity::Agent::home` 才知道加入哪座据点。
    ///
    /// **「重新查一次」在结算侧行不通**：`ll_sim::resolve` 手上只有一条
    /// `ll_sim::intent::Intent`，它没有、也不该有「玩家当初按空格时朝的
    /// 哪一格」这份输入层上下文。这个 `EntityId` 因此从这一行开始一路
    /// 带到 `Intent::DialogueChoose`，每一站都有真实消费者。
    ///
    /// 「过期」这条原来的顾虑仍然被兜住，只是兜在别处：会话屏是模态屏，
    /// `Demo::advance` 在它开着时整个早退（世界一个字节不动），而
    /// `resolve` 侧 `world.actors.get(speaker)` 查不到就整条产出空效果。
    Talk {
        /// 说话人是谁——见本变体文档「为什么带 `EntityId`」一节。
        speaker: EntityId,
        /// 说话人的职业——**排版取它的显示名**（`Agent` 今天没有名字，
        /// 走设计文档三节 3.4 的乙案：用职业显示名代替）。
        profession: ContentIndex,
        /// 跟他说哪一段话——`DialogueTable::match_speaker` 已经裁决完
        /// 的那一段（culture 优先、平局取最小 [`ContentIndex`]）。
        dialogue: ContentIndex,
    },
}

/// 「这一格上站着的人能不能说话、说哪一段」这个查询要的两份只读内容。
///
/// # 为什么不是 `Option<TalkLookup>`
///
/// 那会让「同一格、两个调用方、两份行列表」在类型上成为可能，而**玩家
/// 按的是第几行**：渲染侧列出的第 2 行与输入侧结算的第 2 行必须是同一
/// 行。收一个必填参数，调用方想「不接对话」就传一张空表——行为一样，
/// 但那是一个显式的选择，不是一条可以忘记传的可选路径。
#[derive(Clone, Copy)]
pub struct TalkLookup<'a> {
    /// 会话入口表——`match_speaker` 的宿主。
    pub dialogues: &'a DialogueTable,
    /// 文化表，敌对判定要它；`None` = 这个世界没有文化这一层，此时
    /// `ll_sim::ai_query::declared_hostile` 的文化那一半恒假（它自己
    /// 的既有降级，本模块不另作判断）。
    pub cultures: Option<&'a CultureTable>,
}

/// [`TalkLookup::none`] 借出的那张空对话表。
///
/// 写成 `static` 而不是在 `const fn` 里借用 `DialogueTable::EMPTY`：后者
/// 会造出一个需要析构的常量临时值（`Vec` 有 `Drop`），编译期拒绝。
/// `static` 永不析构，正合此处语义——它是一张全局唯一、永远为空的表。
static NO_DIALOGUES: DialogueTable = DialogueTable::EMPTY;

impl TalkLookup<'_> {
    /// **显式地不接对话内容**：一张常量空表 + 没有文化表，于是
    /// [`InteractTarget::Talk`] 那一行永远不出现，交互列表与本批次之前
    /// 逐条相同。
    ///
    /// 给「这条测试的标的不是对话」的调用点用（地面物品去重、开关门
    /// ……）。**它是一个选择，不是一条可以忘记传的可选路径**——这正是
    /// [`TalkLookup`] 收必填参数而不是 `Option` 的理由，见该类型文档
    /// 「为什么不是 `Option<TalkLookup>`」一节。
    pub const fn none() -> TalkLookup<'static> {
        TalkLookup {
            dialogues: &NO_DIALOGUES,
            cultures: None,
        }
    }
}

/// 一扇门这一行按下去做什么，见 [`InteractTarget::Door`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorAction {
    /// 这一格是关着的门（地形声明了 `opens_into`）→ 开。
    Open,
    /// 这一格是开着的门（是某种地形 `opens_into` 的目标）→ 关。
    Close,
}

impl InteractTarget {
    /// 这一行指着的**物品**是什么；门那一支没有物品，是 `None`。
    ///
    /// 本方法此前叫 `def` 且返回裸 [`ContentIndex`]（三个变体都携带同
    /// 一个字段）。门进交互列表之后那个签名不再诚实——一扇门没有物品
    /// 索引，见 [`InteractTarget::Door`] 文档「这个变体怎么容纳『目标
    /// 不是一件物品』」一节。
    pub fn item_def(self) -> Option<ContentIndex> {
        match self {
            InteractTarget::Facility { def }
            | InteractTarget::Container { def }
            | InteractTarget::Loose { def } => Some(def),
            InteractTarget::Door { .. } | InteractTarget::Talk { .. } => None,
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
pub fn interact_tiles(
    world: &WorldState,
    origin: TorusPos,
    actor: EntityId,
    talk: TalkLookup<'_>,
) -> Vec<InteractTile> {
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
        .filter(|tile| !interact_entries(world, tile.pos, actor, talk).is_empty())
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
/// - 容器（`contents` 非空）→ [`InteractTarget::Container`]。
///   **只留第一个**：`Intent::Loot` 不带参数，恒搜刮脚下第一个容器
///   （见 `ll_sim::resolve` 的 `resolve_loot`），列出第二行会是一行按了
///   跟第一行效果一样的假选项。
///
///   **今天这一支恒不命中**：尸体平铺批次之后没有任何生产路径会造出
///   `contents` 非空的地面物品（尸体不再是容器，见
///   `ll_world::item::GroundItemStack::contents` 字段文档），箱子那批
///   才会把它用起来。分支保留不删，理由同该字段文档。
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
pub fn interact_entries(
    world: &WorldState,
    pos: TorusPos,
    actor: EntityId,
    talk: TalkLookup<'_>,
) -> Vec<InteractTarget> {
    let mut rows: Vec<InteractTarget> = Vec::new();
    // 对话这一行**排在最前**（规格六节第 2 条）：一格上同时有人和东西
    // 时，「跟他说话」几乎总是玩家的意图。
    if let Some(target) = talk_target(world, pos, actor, talk) {
        rows.push(target);
    }
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
    if let Some(action) = door_action_at(world, pos) {
        rows.push(InteractTarget::Door { action });
    }
    rows
}

/// `pos` 这一格上站着的那个人能不能说话；不能就返回 `None`。
///
/// 四道闸门，任何一道不过就没有这一行：
///
/// 1. **这一格上站着别人**——`ll_sim::resolve::occupant_at`（`actor`
///    自己不算）。**不另写一份查找**：那条平局打破规则（同一格站着多于
///    一个单位时取谁）分叉之后，「列表里列的是 A、按下去跟 B 说话」这种
///    玩家可见却极难归因的不一致就会出现。今天「每格至多站一人」是强制
///    不变式，但依赖「今天恰好只有一个」正是本仓库反复付过代价的形状。
/// 2. **他没死**——`Agent::health > 0`。尸体不说话。
/// 3. **不敌对**——`ll_sim::ai_query::declared_hostile`（规格六节第 3
///    条），**不在输入层抄第二份判据**，那正是 ADR 0021 点名要拦的形状。
/// 4. **有一段对话认他**——`DialogueTable::match_speaker`，裁决顺序
///    （culture 优先、平局取最小 `ContentIndex`）在批次 1 就实现并有
///    测试，本函数一个字都不重写。
///
/// # 顺序确定（约束 C5）
///
/// `occupant_at` 走 `Arena::iter_with_id`（由 `Vec` 支撑），
/// `match_speaker` 走 `defined_indices` 再 `min_by_key`——两条链都不碰
/// 任何哈希容器的迭代顺序。
fn talk_target(
    world: &WorldState,
    pos: TorusPos,
    actor: EntityId,
    talk: TalkLookup<'_>,
) -> Option<InteractTarget> {
    let viewer = world.actors.get(actor)?;
    let (speaker, other) = occupant_at(world, pos, actor)?;
    if other.health <= 0 {
        return None;
    }
    if declared_hostile(viewer, other, talk.cultures) {
        return None;
    }
    let dialogue = talk
        .dialogues
        .match_speaker(other.profession, culture_of(other))?;
    Some(InteractTarget::Talk {
        speaker,
        profession: other.profession,
        dialogue,
    })
}

/// 一个实体的文化归属（没有就是 `None`）——[`talk_target`] 喂给
/// `DialogueTable::match_speaker` 的第二个参数。
///
/// `ll_sim::ai_query` 里那个同名私有帮手返回的是 `CultureKind`（敌意
/// 查表要的形状），本处要的是裸 [`ContentIndex`]（内容表匹配要的形状），
/// 两者是同一份数据的两种包装。**不为此把那个私有帮手开放出来**：它的
/// 返回类型对本处不合用，硬套要么在这里再拆一次包、要么让那边多一个
/// 只有本处用的重载，都比这三行贵。
fn culture_of(agent: &Agent) -> Option<ContentIndex> {
    agent.affiliations.iter().find_map(|affiliation| {
        match (affiliation.kind, affiliation.org) {
            (AffiliationKind::Culture, OrgRef::Def(index)) => Some(index),
            // `Culture` 恒走 `OrgRef::Def`（见
            // `ll_world::entity::affiliation::OrgRef` 文档），这里的
            // `_` 只是让 `match` 穷尽，不是一条真实分支。
            _ => None,
        }
    })
}

/// 这一格是不是一扇能开或能关的门；不是就返回 `None`。
///
/// # 判据完全由内容声明推出，没有任何硬编码地形 id
///
/// - 地形声明了 `opens_into`（[`ll_world::terrain::TerrainKind::opens_into`]）
///   → 它是一格「撞入即开」的地形，也就是**关着的门** → [`DoorAction::Open`]。
/// - 地形是某种地形 `opens_into` 的**目标**
///   （[`ll_world::terrain::TerrainTable::closes_into`] 有值）
///   → 它是一格**开着的门** → [`DoorAction::Close`]。
///
/// 因此 mod 自己声明的门（只要写了 `opens_into`）自动进交互列表，
/// 引擎侧零改动——与 `resolve_move` 的撞门分支当初把硬编码特判收拢成
/// 声明式属性是同一条收益（见 `ll_world::terrain` 模块文档
/// 「`opens_into`」一节）。
///
/// # 两条判据不可能同时成立吗
///
/// 内容上可以写出「A 开成 B，B 又开成 C」这样的链。真出现时**开优先**
/// （先判 `opens_into`）：一格还能继续被推开的地形，玩家的第一意图是
/// 推开它。这是一条确定性的先后，不是设计裁定——本体内容里不存在这种
/// 链（`door_closed → door_open`，而 `door_open` 没有 `opens_into`）。
///
/// 区块未常驻时返回 `None`：查不到地形就是查不到（ADR 0015），与
/// `resolve_move`/`resolve_open_door` 在同一情形下静默作废一致。
fn door_action_at(world: &WorldState, pos: TorusPos) -> Option<DoorAction> {
    let terrain = world.terrain_at(pos)?;
    if terrain.opens_into(&world.terrain_table).is_some() {
        return Some(DoorAction::Open);
    }
    world
        .terrain_table
        .closes_into(terrain)
        .map(|_| DoorAction::Close)
}

/// 方向列表一行的显示文本：方向名 + 那一格上第一样东西的名字。
///
/// 带上「那儿有什么」是这块列表唯一有用的信息：光有「北」「东南」两行
/// 方向名，玩家还是不知道该往哪一格去。只列第一样（后面加省略号）是
/// 因为这一行的作用是**认出是哪一格**，不是复述那一格的完整清单——
/// 完整清单在选完之后的物品列表里。
#[allow(clippy::too_many_arguments)]
pub fn direction_row_text(
    tile: InteractTile,
    world: &WorldState,
    actor: EntityId,
    agent: &Agent,
    items: &ItemTable,
    classes: &ClassTable,
    catalog: &Catalog,
    language: &str,
    talk: TalkLookup<'_>,
) -> String {
    let direction = catalog.resolve(language, direction_key(tile.dir));
    let entries = interact_entries(world, tile.pos, actor, talk);
    let first = entries
        .first()
        .map(|target| interact_target_name(*target, items, classes, catalog, language, agent))
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
    classes: &ClassTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let name = interact_target_name(target, items, classes, catalog, language, agent);
    let action_key = match target {
        InteractTarget::Facility { .. } => "hud-interact-action-work",
        InteractTarget::Container { .. } => "hud-interact-action-loot",
        InteractTarget::Loose { .. } => "hud-interact-action-take",
        InteractTarget::Door {
            action: DoorAction::Open,
        } => "hud-interact-action-open_door",
        InteractTarget::Door {
            action: DoorAction::Close,
        } => "hud-interact-action-close_door",
        InteractTarget::Talk { .. } => "hud-interact-action-talk",
    };
    let action = catalog.resolve(language, action_key);
    // **门这一行不写数量。** 「x1」对一件可以有好几堆的物品才有意义，
    // 一扇门是这一格的地形，不存在「两扇」。硬凑一个 `x1` 只会让玩家
    // 以为它是一件能捡的东西。
    let Some(def) = target.item_def() else {
        return format!("{name}（{action}）");
    };
    let count = world
        .ground_items
        .iter()
        .find(|ground| ground.pos == pos && ground.stack.def == def)
        .map_or(0, |ground| ground.stack.count);
    format!("{name} x{count}（{action}）")
}

/// 一行交互候选的**名字**——物品查物品表，门查一条专门的 Fluent 键。
///
/// # 门为什么不走 `item_display_name`
///
/// 地形没有 `display_name_key`：`ll_world::terrain::TerrainAttrs` 只有
/// `blocks_sight`/`blocks_move`/`move_cost`/`opens_into` 四个字段，本体
/// 十七种地形全部由引擎侧注册（`materialize_base_terrain`），从来没有
/// 过显示名这一层。给地形补一条显示名字段是一次真正的内容 schema 变更
/// （连带 `CONTENT_HASH_ALGORITHM_VERSION` 要递增），**不在本批次范围**。
///
/// 因此门这一行用两条专门的 HUD 文案键（`hud-interact-door-closed` /
/// `hud-interact-door-open`），与 `hud-item-unidentified` 同一档：一句
/// 属于呈现层的通用说法，不是某条内容自己的名字。
///
/// **代价要如实记一笔**：mod 声明的门也显示成这同一句「一扇门」，区分
/// 不出「橡木门」和「铁栅门」。要区分就得给地形补显示名字段——那是
/// 上面说的那次 schema 变更，留给需要它的那一批。
fn interact_target_name(
    target: InteractTarget,
    items: &ItemTable,
    classes: &ClassTable,
    catalog: &Catalog,
    language: &str,
    agent: &Agent,
) -> String {
    match target {
        InteractTarget::Door {
            action: DoorAction::Open,
        } => catalog.resolve(language, "hud-interact-door-closed"),
        InteractTarget::Door {
            action: DoorAction::Close,
        } => catalog.resolve(language, "hud-interact-door-open"),
        // `Agent` 今天没有 `name` 字段（设计文档三节 3.4：第一批走乙案
        // ——用职业显示名代替）。职业表查不到这一条时退回一句通用的
        // 「一个人」，与门那两条键同一档：不 panic，也不拿索引号冒充
        // 名字。NPC 姓名那一批（对话批次 6）会把这里换成真名。
        InteractTarget::Talk { profession, .. } => classes
            .get(profession)
            .map(|view| catalog.resolve(language, &view.display_name_key.to_string()))
            .unwrap_or_else(|| catalog.resolve(language, "hud-interact-someone")),
        other => {
            let def = other
                .item_def()
                .expect("除门之外的每一个变体都携带物品索引");
            item_display_name(def, items, catalog, language, &agent.identified_items)
        }
    }
}
