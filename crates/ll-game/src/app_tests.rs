//! `crate::app` 的断言。
//!
//! 用 `#[path]` 挂成 `crate::app` 的子模块而不是 `tests/` 下的集成测试：
//! 这些断言要摸 `Demo` 的私有字段与私有方法，集成测试摸不到。
//! 同一手法在 `app_save_tests.rs`、`app_navigation_tests.rs` 已有两处先例。
//!
//! 批次 16 把它整体搬出 `app.rs`，**断言一个字都没改**。搬出来还顺手消掉了一处
//! rustc 与 rustfmt 的分歧：`mod navigation_tests` 原先是**内联** `mod tests` 里的
//! `#[path]` 模块，路径基准要算成「文件名 + 内联模块名」两层目录（`src/app/tests/`），
//! 于是写成 `../../app_navigation_tests.rs`。批次 16 新建 `src/app/` 目录之后 rustfmt
//! 不再按这条规则算基准，`cargo fmt` 当场解析不到那个文件（rustc 仍然找得到）。
//! 现在它是本文件里的**非内联**模块，基准就是本文件所在目录，与 `mod save_tests`
//! 完全同一种写法，两个工具都认。

//! 世界时钟推进批次的组合断言——这是本任务真正要修的缺陷：
//! `Demo::advance`（真实生产入口，不是手搭的测试世界）此前完全不碰
//! `world.clock`，昼夜循环、buff 到期、技能冷却、地面物品老化全部
//! 因此失效。下面两条测试都跑在 [`test_demo`] 建出的真实
//! `Demo`（真实内容装载、真实 `build_new_world`、真实
//! `TurnEngine`）上，不是直接摆弄 `WorldState`/`resolve`/`apply`——
//! 那种写法只能证明「结算管线本身正确」，证明不了「真的接到了
//! 玩家输入这条生产路径上」。
//!
//! 手工验证过这两条测试确实会红：临时把 `Demo::advance` 里
//! `self.engine.advance_ai(...)`/`try_player_turn(...)` 两行注释掉、
//! 换回改动前那种直接 `intent_from_input` → `resolve` → `apply`
//! （不途经 `TurnEngine`，因此不写 `world.clock`）的写法，两条测试
//! 都会失败：第一条因为 `clock` 全程不变，第二条因为 buff 从不到期
//! （`derive_stats` 用的 `now` 全程等于建局时刻，恒早于 `expires_at`）。
//! 恢复后两条都转绿。

use super::*;
use crate::player_action::{InventoryEntry, inventory_entries};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_platform::input::GameKey;
use ll_sim::item::NoItems;
use ll_sim::resolve::derive_stats;
use ll_world::entity::{ActiveStatModifier, AttributeKind};
use ll_world::item::ItemStack;

pub(super) fn test_content() -> LoadedContent {
    // 判据只有一处：`crate::test_support::test_content`。此前这段拷贝
    // 住在这里，而 `crate::draft_world` 的断言也要同一份内容集——
    // 兄弟模块看不见彼此的 `mod tests`，抽到 `test_support` 才是唯一
    // 不会分叉的落点。
    crate::test_support::test_content()
}

/// 建一个真实可用的 `Demo`——`Demo::new` 本身不触碰 GPU/窗口（那些
/// 在 `on_resume` 才建，见 [`GpuResources`] 字段文档），因此可以
/// 脱离真实窗口直接在单元测试里构造并调用私有的 `advance`。
pub(super) fn test_demo() -> Demo {
    let content = test_content();
    let game_world = crate::world::build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: 1,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("测试用布局满足全部构造前置条件");
    let saves_dir = crate::test_support::unique_temp_path("ll-game-app-test-saves");
    Demo::new(
        content,
        game_world,
        saves_dir,
        "测试旅人".to_string(),
        GameConfig::default(),
        crate::test_support::unique_temp_path("ll-game-app-test-config").join("config.json5"),
        Catalog::load_dir(&std::env::temp_dir().join("ll-game-app-test-empty-locales")),
    )
}

/// 建一个**停在首页**的 `Demo`——与 [`test_demo`] 的区别只有一个：
/// 世界尚未存在。`saves_dir` 指向一个必然不存在的临时目录，因此
/// 槽位列表为空、「读取存档」那一行不可按。
fn test_demo_at_title() -> Demo {
    let content = test_content();
    let saves_dir = crate::test_support::unique_temp_path("ll-game-title-saves");
    Demo::at_title(
        content,
        saves_dir,
        "测试旅人".to_string(),
        GameConfig::default(),
        crate::test_support::unique_temp_path("ll-game-title-config").join("config.json5"),
        Catalog::load_dir(&std::env::temp_dir().join("ll-game-app-test-empty-locales")),
    )
}

#[test]
fn 启动后停在首页且世界尚未存在() {
    // 所有者原话：「我需要一个游戏的主菜单，而不是开始直接进入
    // 存档」。此前 `run_game` 在建窗口之前就 `load_or_new_game`，
    // 玩家没有任何机会在进世界之前做选择。
    //
    // 反例验证（已实跑）：把 `Demo::assemble` 里 `at_title` 那两个
    // 三元判断的取值互换（屏恒为 `None`、栈恒为空），本条立刻变红。
    // Arrange & Act
    let demo = test_demo_at_title();

    // Assert：三件事必须同时成立，见 `Demo::session` 字段文档。
    assert!(demo.session.is_none(), "首页那一刻世界不该存在");
    assert_eq!(demo.modal.screen(), Some(ScreenState::Title));
    assert_eq!(
        demo.input_context(),
        InputContext::Menu,
        "首页必须按菜单那张表解析物理键"
    );
}

#[test]
fn 首页开着时推进一帧世界不动() {
    // `Demo::advance` 有两道闸门（屏开着、世界不存在）。这一条走的
    // 是真实的 `on_frame`：没有世界的那些帧一路跑到底也不该 panic，
    // 更不该凭空造出一个世界来。
    // Arrange
    let mut demo = test_demo_at_title();

    // Act：连按方向键与等待键——在世界里这些都会推进时钟。
    for at in 0..3 {
        走一帧(&mut demo, at, &[GameKey::Right, GameKey::Wait]);
    }

    // Assert
    assert!(demo.session.is_none(), "首页按方向键不该把世界造出来");
    assert_eq!(demo.modal.screen(), Some(ScreenState::Title));
}

#[test]
fn 首页选开始游戏之后进的是角色创建屏而不是世界() {
    // **这条断言被角色创建批次改写过一次，如实记录。**
    //
    // 批次 6（首页）落地时它断言的是「开始游戏 → 世界建出来 → 屏关
    // 掉」，因为那一批的「开始游戏」就是直接建新档进世界，且那一批的
    // 计划文档第七节写明：`ScreenOutcome::StartNewGame` 那个 match 臂
    // 就是下一批的衔接点。本批次正是接在那里——「开始游戏」现在先进
    // 一串屏（角色创建 → 世界配置 → 选出生地），终点仍然是
    // `Session::begin`。
    //
    // 断言因此跟着改：改的是**行为**，不是把一条碍事的断言删掉。
    // 「走完三块屏真的能进世界」由 `crates/ll-game/tests/chargen.rs`
    // 端到端钉住。
    // Arrange：首页一开焦点就在第一项「开始游戏」上（规格 N10），
    // **不再需要先按一次方向键**。
    let mut demo = test_demo_at_title();

    // Act
    走一帧(&mut demo, 0, &[GameKey::Confirm]);

    // Assert
    assert!(
        demo.session.is_none(),
        "玩家还没选完角色，这一刻不该有世界在跑"
    );
    assert_eq!(
        demo.modal.screen(),
        Some(ScreenState::CharacterCreation { cursor: 0 }),
        "「开始游戏」该进角色创建屏"
    );
    assert!(demo.new_game_draft.is_some(), "草稿必须建出来");
    assert_eq!(
        demo.input_context(),
        InputContext::Menu,
        "还在模态屏里，键位表不该切回游戏内那张"
    );
}

#[test]
fn 没有存档时首页按读取存档不进世界() {
    // 玩家点的是「读取存档」，没有存档就该被告知，而不是悄悄开一局
    // 新游戏——那局新游戏退出时会把（本来就该被保护的）存档位置
    // 写掉。
    // Arrange：焦点从第一项往下挪一格，落在第二项「读取存档」。
    let mut demo = test_demo_at_title();
    走一帧(&mut demo, 0, &[GameKey::Down]);

    // Act
    走一帧(&mut demo, 1, &[GameKey::Confirm]);

    // Assert
    assert!(demo.session.is_none(), "没有存档就不该进世界");
    assert_eq!(demo.modal.screen(), Some(ScreenState::Title));
    assert_eq!(demo.screen_notice, Some(ScreenNotice::NoSave));
}

#[test]
fn 从首页直接离开时一个字节都不写存档() {
    // **这一条防的是数据丢失，不只是 panic**。`on_exit` 此前
    // 无条件 `save_on_exit()`，从首页直接离开会把一个「玩家从未
    // 玩过」的空世界写到玩家真正那份存档上。
    //
    // 反例验证（已实跑）：把 `save_on_exit` 开头那道闸门的
    // `return` 换成「照旧无条件存档」（现建一局新世界再
    // `save_game`，也就是改动前那条路径在新结构下的等价物），本条
    // 当场变红，且是因为**磁盘上真的多出了一份存档文件**——不是
    // 因为 panic。
    // Arrange
    let mut demo = test_demo_at_title();
    let saves_dir = demo.saves_dir.clone();
    assert!(!saves_dir.exists(), "Arrange：这个目录本来就不该存在");

    // Act
    demo.on_exit();

    // Assert
    assert!(
        crate::save_slot::list_slots(&saves_dir).is_empty(),
        "从首页退出不该写出任何存档：{}",
        saves_dir.display()
    );
}

/// 读出玩家当前结算出的力量值——途经与真实战斗结算
/// （`ll_sim::resolve::resolve_attack`）完全相同的
/// `ll_sim::resolve::derive_stats` 聚合入口,不是另写一套判断逻辑。
fn player_derived_strength(demo: &Demo) -> i32 {
    let player = demo.test_world().player;
    let agent = demo
        .test_world()
        .world
        .actors
        .get(player)
        .expect("玩家仍应存在");
    let now = demo.test_world().world.clock;
    derive_stats(
        agent.stats,
        &agent.active_stat_modifiers,
        &agent.equipment,
        &NoItems,
        now,
    )
    .attribute(AttributeKind::Strength)
}

#[test]
fn 连续多次玩家等待后世界时钟真的前进() {
    // Arrange
    let mut demo = test_demo();
    let clock_before = demo.test_world().world.clock;
    let mut input = InputState::new();
    input.press(GameKey::Wait);

    // Act：推进三帧，每帧都带着等待键——`was_activated` 只依赖
    // `just_pressed`/`repeated` 标志位（见 `ll_platform::input`
    // 文档），本测试不调用 `begin_frame`/`end_frame`，`just_pressed`
    // 因此在整个循环里保持置位，每一帧都会被 `try_player_turn`
    // 判定为「等待键激活」并真正消费一次回合——与
    // `ll_sim::turn::tests` 里驱动 `TurnEngine` 的现成测试同一个
    // 手法（不模拟按键事件，只构造 `InputState` 的值）。
    for frame in 0..3u64 {
        demo.advance(&mut input, FrameId(frame));
    }

    // Assert：不是「变了」，是「前进了」——严格大于，不允许倒退。
    assert!(demo.test_world().world.clock > clock_before);
}

#[test]
fn 临时属性修正过期后其加成不再计入结算() {
    // 比「时钟前进了」更强的一条：验证时钟推进与既有的惰性到期
    // 判定（`ll_sim::resolve::derive_stats`）真的咬合，不只是
    // `world.clock` 这个数字在动——单看时钟前进可能被一个「每帧
    // 直接 +1」之类的假实现骗过,这条测试还要求它与既有到期判定
    // 生效的那一刻精确对齐。
    // Arrange：跑两次等待，量出「一次行动」真实推进的 tick 数——
    // 不写死 `ll-sim` 内部私有的 `BASE_ACTION_COST`,只依赖公开可
    // 观察的时钟差值,避免测试与结算层的内部常量耦合。
    //
    // 第一次等待只是「热身」：玩家的初次可行动时刻就等于建局时的
    // `world.clock`（`crate::world::spawn_player` 把 `next_action_at`
    // 设成建局时的 `world.clock`,见其文档),`TurnEngine::perform`
    // 结算这次弹出的条目时 `world.clock = entry.at` 恰好是「设成
    // 它已经是的那个值」,不产生可观察的变化——真正能测出「一次
    // 行动的 tick 代价」要看第二次、第三次行动之间的差值。
    let mut demo = test_demo();
    let player = demo.test_world().player;
    let mut input = InputState::new();
    input.press(GameKey::Wait);
    demo.advance(&mut input, FrameId(0));
    let clock_after_warm_up = demo.test_world().world.clock;
    demo.advance(&mut input, FrameId(1));
    let clock_after_second_wait = demo.test_world().world.clock;
    let ticks_per_wait = clock_after_second_wait.0 - clock_after_warm_up.0;
    assert!(ticks_per_wait > 0, "第二次等待起，世界时钟应当真实推进");

    // 给玩家叠一条力量 +50 的临时修正，到期时刻卡在「刚才那次行动
    // 结束的时刻」与「下一次行动结束的时刻」正中间——按同一份
    // dexterity 结算出的行动代价恒定（本用例全程不改属性），下一次
    // 等待结算后世界时钟必然已经越过这个到期时刻。
    let source = ContentIndex::default();
    let expires_at = Tick(clock_after_second_wait.0 + ticks_per_wait / 2);
    {
        let agent = demo
            .test_world_mut()
            .world
            .actors
            .get_mut(player)
            .expect("玩家刚建局，必然存在");
        agent
            .active_stat_modifiers
            .entry(AttributeKind::Strength)
            .or_default()
            .insert(
                source,
                ActiveStatModifier {
                    delta: 50,
                    expires_at,
                },
            );
    }
    let base_strength = demo
        .test_world()
        .world
        .actors
        .get(player)
        .expect("玩家仍存在")
        .stats
        .strength;
    assert_eq!(
        player_derived_strength(&demo),
        base_strength + 50,
        "buff 生效期间应体现在结算出的力量值上——这一步只是确认
             Arrange 本身摆对了，不是本测试真正要验证的断言"
    );

    // Act：再跑一次等待（第三次）——时钟应当越过 expires_at。
    demo.advance(&mut input, FrameId(2));

    // Assert：buff 已过期，结算值应回落到裸属性值。
    assert_eq!(
        player_derived_strength(&demo),
        base_strength,
        "buff 过期后不应再计入结算——时钟真的推进到了 expires_at \
         之后，且推进结果真的被既有到期判定读到"
    );
}

#[test]
fn 世界地图新建时默认关闭() {
    // ADR 0025 相关的验收难题第一层——程序化断言开关的初始状态,
    // 不依赖任何合成按键。
    // Arrange & Act
    let demo = test_demo();

    // Assert
    assert!(!demo.modal.world_map_open());
}

#[test]
fn 按下地图键后开关状态翻转为打开() {
    // 程序化验证「M 键事件 → 开关状态真的翻转」——见任务书「验收
    // 难题」一节要求的第一层。
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    input.press(GameKey::Map);

    // Act
    demo.advance(&mut input, FrameId(0));

    // Assert
    assert!(demo.modal.world_map_open());
}

#[test]
fn 再次按下地图键后开关状态翻回关闭() {
    // 与 `ll_sim::turn` 等既有测试同一个手法（见 `test_demo` 上方
    // 「连续多次玩家等待后世界时钟真的前进」测试文档）：本测试全程
    // 不调用 `begin_frame`/`end_frame`，`just_pressed` 因此在两次
    // `advance` 调用之间保持置位，每次调用都会被判定为「地图键
    // 激活」并各自触发一次翻转——恰好用来验证「开 → 关」这条翻转
    // 本身，而不是「按住不会重复翻转」（那是 `was_just_pressed` 与
    // 真实按键事件循环之间的既有职责划分，见
    // `ll_platform::input::InputState` 模块文档，不是本测试要
    // 覆盖的范围）。
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    input.press(GameKey::Map);
    demo.advance(&mut input, FrameId(0));
    assert!(demo.modal.world_map_open(), "第一次按下后应先翻转为打开");

    // Act
    demo.advance(&mut input, FrameId(1));

    // Assert
    assert!(!demo.modal.world_map_open());
}

#[test]
fn 首页按下地图键不打开世界地图() {
    // 世界地图缩放批次与首页批次整合时新加的闸门（见 [`Demo::advance`]
    // 里地图开关那一段）。世界地图是画在世界之上的观测层，它的视野
    // （`Session::world_map_view`）与粗粒度地形场都住在 `Session` 上；
    // 首页那一刻 `Session` 是 `None`，开关翻上去只会得到一块对着空气
    // 的浮层，且下一帧就会走进 `pan_and_zoom_world_map`。
    //
    // 反例验证（已实跑）：把 `Demo::advance` 里那句翻转挪到
    // `if let Some(session)` 之外——也就是回到两批各自独立时的无条件
    // 翻转——本条立刻变红。
    // Arrange
    let mut demo = test_demo_at_title();
    let mut input = InputState::new();
    input.press(GameKey::Map);

    // Act
    demo.advance(&mut input, FrameId(0));

    // Assert
    assert!(
        !demo.modal.world_map_open(),
        "世界尚未存在的时候地图开关不该翻上去"
    );
}
/// 导航收敛（规格 N8/N7/N2/N1/N10）与鼠标接线的断言住在一个
/// **独立文件**里，理由与 `save_tests` 逐字相同，见那个模块的
/// 文档与 `app_navigation_tests.rs` 的模块文档。
#[path = "app_navigation_tests.rs"]
mod navigation_tests;

// ───────────────────── 输入接线批次：物品链六个意图 ─────────────────────
//
// 下面这一组的共同点：**Act 一律只有按键**。它们要证明的不是
// 「`resolve_craft` 会扣食材」（那条早就有测试，走 AI 策略直接返回
// 意图这条最小提交路径），而是「玩家在真实游戏里按得出来」——本
// 批次全部的价值就在这一句上，证据因此必须从输入侧出发。
//
// 全程跑在 `test_demo` 建出的真实 `Demo` 上：真实 `mods/` 内容
// （与 `crates/ll-mod/tests/furniture_placement.rs` 同一份 `mods/`）、
// 真实 `build_new_world`、真实 `TurnEngine`。Arrange 里只摆「玩家
// 身上有什么」（背包、已知配方、脚下地形），一次都不直接构造
// `Intent`、不直接调 `resolve`/`apply`。

/// 按内容 id 取索引——真实注册表，查不到直接 panic（说明 `mods/`
/// 里那条内容被改名或删了，应当立刻显形）。
fn content_index(demo: &Demo, id: &str) -> ContentIndex {
    demo.content
        .registry
        .get(&NamespacedId::parse(id).expect("测试里的字面量恒合法"))
        .unwrap_or_else(|| panic!("{id} 应当已被 mods/lostland/ 注册"))
}

/// 跑一帧，本帧按住 `keys` 里的每个键。
///
/// 每次都新建 `InputState`：`was_just_pressed` 因此在这一帧恰好置位
/// 一次，与真实事件循环里「按下 → 下一帧清标志」的时序等价，且不
/// 依赖 `begin_frame`/`end_frame`——与本模块既有的地图开关测试同一
/// 个手法。
fn frame(demo: &mut Demo, at: u64, keys: &[GameKey]) {
    let mut input = InputState::new();
    for key in keys {
        input.press(*key);
    }
    demo.advance(&mut input, FrameId(at));
}

/// 把光标从第 0 行按到第 `row` 行——每帧一次「下」。
fn move_cursor_to(demo: &mut Demo, at: &mut u64, row: usize) {
    for _ in 0..row {
        frame(demo, *at, &[GameKey::Down]);
        *at += 1;
    }
}

/// 背包里这一种东西一共有几个（一件都没有时是 0）。
///
/// **把全部同种堆加起来，不是只看第一条**：同一种东西在背包里完全
/// 可能占着不止一堆（出生装备里已经有一份、测试又塞了一份，两者不
/// 会自动合并——合并只发生在 `merge_into_inventory_effect` 那条路径
/// 上）。只看第一条会在第一堆被吃空、`apply` 把它移除之后突然"变
/// 多"，那是一次真实踩到的假失败。
fn carried(demo: &Demo, def: ContentIndex) -> u32 {
    demo.test_world()
        .world
        .actors
        .get(demo.test_world().player)
        .expect("玩家仍存在")
        .inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

/// 玩家脚下那一格摆着的全部地面物品定义。
fn ground_defs_underfoot(demo: &Demo) -> Vec<ContentIndex> {
    let pos = demo
        .test_world()
        .world
        .actors
        .get(demo.test_world().player)
        .expect("玩家仍存在")
        .pos;
    demo.test_world()
        .world
        .ground_items
        .iter()
        .filter(|ground| ground.pos == pos)
        .map(|ground| ground.stack.def)
        .collect()
}

/// 把玩家脚下那一格改成草地。
///
/// **不是可有可无的布景**：`build_new_world` 生成出来的出生格是什么
/// 地形取决于噪声，完全可能是深水（`blocks_move` 为真），那会让
/// `can_place_furniture` 的第 ② 道前置不成立，于是「放得下」与
/// 「放不下」两侧同时因为一个与被测逻辑无关的原因落到同一侧——
/// `crates/ll-mod/tests/furniture_placement.rs` 的
/// `Scene::terrain_underfoot` 字段文档记的是同一个坑。
fn clear_terrain_underfoot(demo: &mut Demo) {
    let pos = demo
        .test_world()
        .world
        .actors
        .get(demo.test_world().player)
        .expect("玩家仍存在")
        .pos;
    let grass = demo.content.terrain_ids.grass;
    demo.test_world_mut().world.terrain.set_terrain(pos, grass);
}

/// 玩家背包/装备菜单里，第一条指着 `def` 的行是第几行。
fn inventory_row_of(demo: &Demo, def: ContentIndex) -> usize {
    let agent = demo
        .test_world()
        .world
        .actors
        .get(demo.test_world().player)
        .expect("玩家仍存在");
    inventory_entries(agent)
        .iter()
        .position(|entry| match entry {
            InventoryEntry::Carried { def: candidate } => *candidate == def,
            InventoryEntry::Equipped { def: candidate, .. } => *candidate == def,
        })
        .expect("这一行应当在菜单里")
}

/// 制作菜单里这条配方是第几行。
fn craft_row_of(demo: &Demo, recipe: ContentIndex) -> usize {
    crate::player_action::craft_entries(&demo.content.recipe_table)
        .iter()
        .position(|candidate| *candidate == recipe)
        .expect("这条配方应当在菜单里")
}

/// [`arrange_smith`] 摆出来的那一组内容索引。
struct SmithFixture {
    iron_ingot: ContentIndex,
    leather_strip: ContentIndex,
    smith_hammer: ContentIndex,
    forge: ContentIndex,
    iron_shortsword: ContentIndex,
    forge_recipe: ContentIndex,
    iron_shortsword_recipe: ContentIndex,
}

/// 玩家此刻站在哪一格。
fn player_pos(demo: &Demo) -> TorusPos {
    demo.test_world()
        .world
        .actors
        .get(demo.test_world().player)
        .expect("玩家仍存在")
        .pos
}

/// 直接往玩家脚下这一格摆一堆东西（Arrange 用，不经按键）。
fn put_on_ground(demo: &mut Demo, def: ContentIndex, placed: bool) {
    let pos = player_pos(demo);
    let clock = demo.test_world().world.clock;
    demo.test_world_mut()
        .world
        .ground_items
        .push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: clock,
            contents: Vec::new(),
            placed,
        });
}

/// 按键把铁匠锤装上——多条用例共用的一段 Arrange，**仍然全程按键**
/// （不是直接写 `agent.equipment`）：它在别的用例里是 Arrange，在
/// 「按装备键…」那条里是被验证的 Act，两处走同一条路才谈得上一致。
fn equip_hammer_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
    frame(demo, *at, &[GameKey::Inventory]);
    *at += 1;
    let row = inventory_row_of(demo, fixture.smith_hammer);
    move_cursor_to(demo, at, row);
    frame(demo, *at, &[GameKey::Equip]);
    *at += 1;
    frame(demo, *at, &[GameKey::Inventory]);
    *at += 1;
}

/// 按键砌出一座锻炉（进背包，还没立起来）。
fn craft_forge_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
    frame(demo, *at, &[GameKey::Craft]);
    *at += 1;
    let row = craft_row_of(demo, fixture.forge_recipe);
    move_cursor_to(demo, at, row);
    frame(demo, *at, &[GameKey::Confirm]);
    *at += 1;
    frame(demo, *at, &[GameKey::Craft]);
    *at += 1;
}

/// 按键把背包里的锻炉立在脚下。
fn place_forge_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
    frame(demo, *at, &[GameKey::Inventory]);
    *at += 1;
    let row = inventory_row_of(demo, fixture.forge);
    move_cursor_to(demo, at, row);
    frame(demo, *at, &[GameKey::Place]);
    *at += 1;
    frame(demo, *at, &[GameKey::Inventory]);
    *at += 1;
}

/// 摆好「能打铁的玩家」：背包里 8 块铁锭 + 1 条皮革 + 1 把铁匠锤，
/// 已知打铁短剑那条配方（它 `requires_discovery: true`），脚下是草地。
///
/// 8 块铁锭 = 砌锻炉 6 块 + 打短剑 2 块，正好是整条链的用量。
fn arrange_smith(demo: &mut Demo) -> SmithFixture {
    let fixture = SmithFixture {
        iron_ingot: content_index(demo, "lostland:iron_ingot"),
        leather_strip: content_index(demo, "lostland:leather_strip"),
        smith_hammer: content_index(demo, "lostland:smith_hammer"),
        forge: content_index(demo, "lostland:forge"),
        iron_shortsword: content_index(demo, "lostland:iron_shortsword"),
        forge_recipe: content_index(demo, "lostland:forge_recipe"),
        iron_shortsword_recipe: content_index(demo, "lostland:iron_shortsword_recipe"),
    };
    let hammer_durability = demo
        .content
        .item_table
        .get(fixture.smith_hammer)
        .and_then(|view| view.max_durability);
    let player = demo.test_world().player;
    let agent = demo
        .test_world_mut()
        .world
        .actors
        .get_mut(player)
        .expect("玩家刚建局，必然存在");
    agent.inventory.push(ItemStack::new(fixture.iron_ingot, 8));
    agent
        .inventory
        .push(ItemStack::new(fixture.leather_strip, 1));
    agent.inventory.push(ItemStack::freshly_made(
        fixture.smith_hammer,
        1,
        hammer_durability,
    ));
    agent.known_recipes.push(fixture.iron_shortsword_recipe);
    clear_terrain_underfoot(demo);
    fixture
}

#[test]
fn 全程只按键就能砌炉子立起来再打出一把铁短剑且铁锭真的被扣掉() {
    // 本批次的验收线本身。Act 里没有任何一次手工构造的 `Intent`：
    // 从开背包、选锤子、按装备键，到砌炉子、按放置键立起来、走到
    // 它上面按空格交互、在制作菜单里选配方按确认——全部经由
    // `crate::player_action::player_command` →
    // `TurnEngine::try_player_intent`，与玩家真的坐在键盘前一模一样。
    // Arrange
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        8,
        "Arrange 应当摆了 8 块铁锭"
    );
    let mut at = 0u64;

    // Act ①：开背包 → 光标移到铁匠锤 → 按装备键。
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let hammer_row = inventory_row_of(&demo, fixture.smith_hammer);
    move_cursor_to(&mut demo, &mut at, hammer_row);
    frame(&mut demo, at, &[GameKey::Equip]);
    at += 1;
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    assert!(
        demo.test_world()
            .world
            .actors
            .get(demo.test_world().player)
            .expect("玩家仍存在")
            .equipment
            .values()
            .any(|stack| stack.def == fixture.smith_hammer),
        "按装备键之后铁匠锤应当真的穿在身上——打铁短剑的 required_tool"
    );

    // Act ②：开制作菜单 → 光标移到「砌锻炉」→ 确认。
    frame(&mut demo, at, &[GameKey::Craft]);
    at += 1;
    let forge_recipe_row = craft_row_of(&demo, fixture.forge_recipe);
    move_cursor_to(&mut demo, &mut at, forge_recipe_row);
    frame(&mut demo, at, &[GameKey::Confirm]);
    at += 1;
    assert_eq!(
        carried(&demo, fixture.forge),
        1,
        "按确认之后背包里应当多出一座锻炉"
    );
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        2,
        "砌锻炉吃掉 6 块铁锭，应当只剩 2 块"
    );

    // Act ③：关制作菜单 → 开背包 → 光标移到锻炉 → 按**放置**键。
    // 按丢弃键在这里是不够的——丢下去的炉子是躺着的，当不了场地，
    // 见 `ll_sim::intent::Intent::Place` 文档。
    frame(&mut demo, at, &[GameKey::Craft]);
    at += 1;
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let forge_row = inventory_row_of(&demo, fixture.forge);
    move_cursor_to(&mut demo, &mut at, forge_row);
    frame(&mut demo, at, &[GameKey::Place]);
    at += 1;
    let player_pos = player_pos(&demo);
    assert_eq!(
        demo.test_world()
            .world
            .placed_at(player_pos)
            .map(|ground| ground.stack.def),
        Some(fixture.forge),
        "按放置键之后锻炉应当**立**在脚下这一格上"
    );
    assert_eq!(carried(&demo, fixture.forge), 0, "锻炉应当已经离开背包");

    // Act ④：关背包 → 按**空格**与脚下的炉子交互 → 在交互列表里
    // 按确认（那一行的主交互是「在此开工」，会换开制作菜单）→
    // 光标移到「打铁短剑」→ 确认。
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    frame(&mut demo, at, &[GameKey::Interact]);
    at += 1;
    assert!(
        matches!(demo.modal.player_menu(), PlayerMenu::Interact { .. }),
        "脚下立着炉子，按空格应当开出交互列表，实际是 {:?}",
        demo.modal.player_menu()
    );
    frame(&mut demo, at, &[GameKey::Confirm]);
    at += 1;
    assert!(
        matches!(demo.modal.player_menu(), PlayerMenu::Craft { .. }),
        "对着立着的炉子按确认应当换开制作菜单，实际是 {:?}",
        demo.modal.player_menu()
    );
    let sword_row = craft_row_of(&demo, fixture.iron_shortsword_recipe);
    move_cursor_to(&mut demo, &mut at, sword_row);
    frame(&mut demo, at, &[GameKey::Confirm]);

    // Assert：剑真的造出来了，两味食材真的被扣掉。
    assert_eq!(
        carried(&demo, fixture.iron_shortsword),
        1,
        "按确认之后背包里应当多出一把铁短剑"
    );
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        0,
        "打短剑再吃掉 2 块铁锭，8 块应当一块不剩"
    );
    assert_eq!(
        carried(&demo, fixture.leather_strip),
        0,
        "打短剑吃掉那一条皮革"
    );
}

#[test]
fn 按丢弃键放下的炉子是躺着的当不了场地() {
    // 上一条的反例，也是「丢弃与放置是两个动作」这条裁定在输入侧的
    // 直接后果：同样的按键序列，只把放置键换成丢弃键，制作就做不出
    // 来。没有这一条，无法排除「其实丢弃也照样能立起来」。
    // Arrange：按键砌一座炉子出来，然后**丢**在脚下。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    let mut at = 0u64;
    equip_hammer_by_keys(&mut demo, &mut at, &fixture);
    craft_forge_by_keys(&mut demo, &mut at, &fixture);
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let forge_row = inventory_row_of(&demo, fixture.forge);
    move_cursor_to(&mut demo, &mut at, forge_row);
    frame(&mut demo, at, &[GameKey::Drop]);
    at += 1;
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let player_pos = player_pos(&demo);
    assert!(
        ground_defs_underfoot(&demo).contains(&fixture.forge),
        "Arrange：炉子确实落在了脚下"
    );
    assert!(
        demo.test_world().world.placed_at(player_pos).is_none(),
        "Arrange：但它是躺着的，不是立着的"
    );

    // Act：开制作菜单选打铁短剑，按确认。
    frame(&mut demo, at, &[GameKey::Craft]);
    at += 1;
    let sword_row = craft_row_of(&demo, fixture.iron_shortsword_recipe);
    move_cursor_to(&mut demo, &mut at, sword_row);
    frame(&mut demo, at, &[GameKey::Confirm]);

    // Assert：一把剑都没有，食材一块没少。
    assert_eq!(carried(&demo, fixture.iron_shortsword), 0);
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        2,
        "静默失败不消耗任何东西"
    );
}

#[test]
fn 按空格开出的交互列表里按捡起键能把立着的炉子收回背包() {
    // 「摆下去还能收回来」这条闭环的出口。
    // Arrange：按键把炉子立起来。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    let mut at = 0u64;
    craft_forge_by_keys(&mut demo, &mut at, &fixture);
    place_forge_by_keys(&mut demo, &mut at, &fixture);
    let player_pos = player_pos(&demo);
    assert!(
        demo.test_world().world.placed_at(player_pos).is_some(),
        "Arrange 应当已经把锻炉立在脚下"
    );

    // Act：空格开列表 → 按捡起键。
    frame(&mut demo, at, &[GameKey::Interact]);
    at += 1;
    frame(&mut demo, at, &[GameKey::PickUp]);

    // Assert
    assert_eq!(
        carried(&demo, fixture.forge),
        1,
        "按捡起键之后锻炉应当回到背包"
    );
    assert!(
        !ground_defs_underfoot(&demo).contains(&fixture.forge),
        "锻炉应当已经离开地面"
    );
}

#[test]
fn 脚下只有一样东西时按空格也照样弹列表() {
    // 所有者原话「无论是一个还是 N 个，交互的时候都统一以列表显示」
    // ——这一条守的正是那句话：不许有「只有一件就直接捡走」的捷径。
    // 没有它，实现随时可能为了「体贴」加回那条捷径，从而造出两条
    // 拾取路径。
    // Arrange：脚下恰好一堆铁锭。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    put_on_ground(&mut demo, fixture.iron_ingot, false);
    let carried_before = carried(&demo, fixture.iron_ingot);

    // Act：按一次空格。
    frame(&mut demo, 0, &[GameKey::Interact]);

    // Assert：列表开着，东西**还在地上**——没有被直接捡走。
    assert!(
        matches!(demo.modal.player_menu(), PlayerMenu::Interact { .. }),
        "只有一样东西时也必须弹列表，实际是 {:?}",
        demo.modal.player_menu()
    );
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        carried_before,
        "开列表这一步不该捡走任何东西"
    );
    assert!(ground_defs_underfoot(&demo).contains(&fixture.iron_ingot));
}

#[test]
fn 附近什么都没有时按空格给出提示而不是静默作废() {
    // 所有者那张表的第 0 行：范围内一格有东西的都没有 → 一句话。
    // Arrange：确认脚下与相邻八格全空。
    let mut demo = test_demo();
    assert!(
        crate::player_action::interact_tiles(&demo.test_world().world, player_pos(&demo))
            .is_empty(),
        "Arrange 假设出生点周围没有任何地面物品"
    );

    // Act
    frame(&mut demo, 0, &[GameKey::Interact]);

    // Assert
    assert_eq!(
        demo.modal.player_menu(),
        PlayerMenu::Closed,
        "什么都没有时不该开出菜单"
    );
    assert_eq!(demo.feedback, Some(Feedback::NothingNearby));
}

#[test]
fn 范围内两格有东西时按空格先弹方向列表() {
    // 所有者那张表的第「2 以上」行：先选和哪一格交互。
    // Arrange：脚下一堆铁锭，正东相邻格再一堆。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    put_on_ground(&mut demo, fixture.iron_ingot, false);
    let here = player_pos(&demo);
    let east = demo.test_world().world.size.wrap(here.x() + 1, here.y());
    let clock = demo.test_world().world.clock;
    demo.test_world_mut()
        .world
        .ground_items
        .push(ll_world::item::GroundItemStack {
            pos: east,
            stack: ItemStack::new(fixture.leather_strip, 1),
            dropped_at: clock,
            contents: Vec::new(),
            placed: false,
        });

    // Act
    frame(&mut demo, 0, &[GameKey::Interact]);

    // Assert：开的是方向列表，不是物品列表。
    assert!(
        matches!(
            demo.modal.player_menu(),
            PlayerMenu::InteractDirection { .. }
        ),
        "两格有东西时应当先弹方向列表，实际是 {:?}",
        demo.modal.player_menu()
    );

    // 再按确认：选中第一行（脚下），进那一格的物品列表——**不**捡走
    // 任何东西（所有者原话「不是捡走，只是打开交互列表」）。
    let carried_before = carried(&demo, fixture.iron_ingot);
    frame(&mut demo, 1, &[GameKey::Confirm]);
    assert_eq!(
        demo.modal.player_menu(),
        PlayerMenu::Interact {
            pos: here,
            cursor: 0,
            from_direction: true
        },
        "选完方向应当进那一格的物品列表"
    );
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        carried_before,
        "选方向这一步不该捡走任何东西"
    );
}

#[test]
fn 隔一格也能从方向列表里把东西捡过来() {
    // 「够得着的范围」那条规则的正向证据：范围是脚下加相邻八格
    // （切比雪夫 1），因为移动本身是八向的。这里取**斜后方**那一格
    // ——若范围只认正交四邻，本条会红。
    // Arrange：脚下空着，西北相邻格上一堆皮革。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    let here = player_pos(&demo);
    let north_west = demo
        .test_world()
        .world
        .size
        .wrap(here.x() - 1, here.y() - 1);
    let clock = demo.test_world().world.clock;
    demo.test_world_mut()
        .world
        .ground_items
        .push(ll_world::item::GroundItemStack {
            pos: north_west,
            stack: ItemStack::new(fixture.leather_strip, 2),
            dropped_at: clock,
            contents: Vec::new(),
            placed: false,
        });
    let carried_before = carried(&demo, fixture.leather_strip);

    // Act：只有一格有东西 → 直接进那一格的物品列表 → 按确认捡起。
    frame(&mut demo, 0, &[GameKey::Interact]);
    assert_eq!(
        demo.modal.player_menu(),
        PlayerMenu::Interact {
            pos: north_west,
            cursor: 0,
            from_direction: false
        },
        "只有一格有东西时应当跳过方向列表，直接开那一格的物品列表"
    );
    frame(&mut demo, 1, &[GameKey::Confirm]);

    // Assert：斜后方那一格上的东西真的到了背包里。
    assert_eq!(carried(&demo, fixture.leather_strip), carried_before + 2);
    assert!(
        demo.test_world()
            .world
            .ground_items
            .iter()
            .all(|ground| ground.pos != north_west)
    );
}

#[test]
fn 够不着的两格之外按拾取意图静默无效() {
    // 上一条的反例：把同一堆东西挪到切比雪夫距离 2，一样的意图就
    // 什么都不发生。没有这一条，无法排除「够得着判定其实没生效，
    // 隔多远都能捡」。
    // Arrange
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    let here = player_pos(&demo);
    let far = demo.test_world().world.size.wrap(here.x() + 2, here.y());
    let clock = demo.test_world().world.clock;
    demo.test_world_mut()
        .world
        .ground_items
        .push(ll_world::item::GroundItemStack {
            pos: far,
            stack: ItemStack::new(fixture.leather_strip, 2),
            dropped_at: clock,
            contents: Vec::new(),
            placed: false,
        });
    let carried_before = carried(&demo, fixture.leather_strip);

    // Act：按空格——那一格在范围外，因此扫不到任何候选格。
    frame(&mut demo, 0, &[GameKey::Interact]);

    // Assert
    assert_eq!(demo.modal.player_menu(), PlayerMenu::Closed);
    assert_eq!(demo.feedback, Some(Feedback::NothingNearby));
    assert_eq!(carried(&demo, fixture.leather_strip), carried_before);
}

#[test]
fn 这一格立着东西时按丢弃键丢不下去并给出反馈() {
    // 「静默作废对 AI 可以，对玩家不行」这条的落点，见
    // `ll_sim::turn::PlayerTurnOutcome` 文档；同时守所有者那条
    // 「家具如果是放置在那个地方，那物品就无法被丢在那」。
    // Arrange：脚下立着一座炉子，背包里有铁锭。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    put_on_ground(&mut demo, fixture.forge, true);
    let carried_before = carried(&demo, fixture.iron_ingot);
    let mut at = 0u64;

    // Act：开背包 → 移到铁锭 → 按丢弃键。
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let ingot_row = inventory_row_of(&demo, fixture.iron_ingot);
    move_cursor_to(&mut demo, &mut at, ingot_row);
    frame(&mut demo, at, &[GameKey::Drop]);

    // Assert：一块都没丢出去，而且玩家被告知了。
    assert_eq!(carried(&demo, fixture.iron_ingot), carried_before);
    assert_eq!(
        demo.feedback,
        Some(Feedback::NothingHappened),
        "按了键却什么都没发生时，必须有一句话告诉玩家"
    );
}

#[test]
fn 菜单开着时方向键只移动光标不移动角色() {
    // 守 `player_command` 里「菜单开着时不走回落分支」这条顺序——
    // 顺序反过来的话，玩家在菜单里挑东西的同时角色会在地图上一路
    // 走动。
    // Arrange
    let mut demo = test_demo();
    arrange_smith(&mut demo);
    frame(&mut demo, 0, &[GameKey::Inventory]);
    let pos_before = player_pos(&demo);
    let clock_before = demo.test_world().world.clock;

    // Act：菜单开着，连按三次「下」。
    for at in 1..4u64 {
        frame(&mut demo, at, &[GameKey::Down]);
    }

    // Assert：角色没动，时钟也没走（移动会消耗一次回合）。
    assert_eq!(player_pos(&demo), pos_before);
    assert_eq!(demo.test_world().world.clock, clock_before);
    assert!(demo.modal.player_menu().is_open(), "菜单应当仍然开着");
}

/// 走**真实生产入口** `on_frame` 跑一帧——不是直接调 `advance`：
/// 菜单键的消费点、模态屏路由、退出判定全部住在 `on_frame` 里，
/// `advance` 根本看不见它们。`resources` 为 `None` 时 `on_frame`
/// 会在渲染那一步提前返回 `Continue`，因此脱离 GPU 也能跑。
fn 走一帧(demo: &mut Demo, at: u64, keys: &[GameKey]) -> FrameOutcome {
    let mut input = InputState::new();
    for key in keys {
        input.press(*key);
    }
    demo.on_frame(FrameId(at), &mut input)
}

#[test]
fn 按下菜单键打开模态屏且输入上下文切到菜单() {
    // 交接文档第四节第 17 条那条死路径的端到端验收：按 Tab 之前
    // 上下文是 Gameplay，按下之后是 Menu，平台层从此按菜单那张表
    // 解析物理键。
    // Arrange
    let mut demo = test_demo();
    assert_eq!(demo.input_context(), InputContext::Gameplay);

    // Act
    走一帧(&mut demo, 0, &[GameKey::Menu]);

    // Assert
    assert_eq!(demo.modal.screen(), Some(ScreenState::Menu));
    assert_eq!(demo.input_context(), InputContext::Menu);
    assert_eq!(demo.modal.depth(), 1);
}

#[test]
fn 切到命名屏之后输入上下文是文本输入态离开后换回来() {
    // 「文本输入态下游戏按键不该触发游戏动作」这条要求的**接线**
    // 断言：`InputContext::TextEntry` 那张表里没有 WASD（
    // `ll_platform::keybind` 里有独立断言），但表对了不代表这块屏
    // 真的切到了那个上下文。少了这条，`sync_text_entry_mode` 不
    // 接线也不会有任何东西变红。
    // Arrange：先开一块普通菜单屏。
    let mut demo = test_demo();
    let mut input = InputState::new();
    demo.open_menu(&mut input);
    assert_eq!(demo.input_context(), InputContext::Menu);

    // Act：切到命名屏。
    demo.modal.set_screen(
        Some(ScreenState::SaveNaming {
            origin: crate::spawn_pick::SpawnOrigin::WorldSetup,
        }),
        &mut input,
    );

    // Assert
    assert_eq!(demo.input_context(), InputContext::TextEntry);

    // Act：切走。
    demo.modal.set_screen(Some(ScreenState::Menu), &mut input);

    // Assert：换回来，而且没有把底下那层菜单一起弹掉。
    assert_eq!(demo.input_context(), InputContext::Menu);
    assert_eq!(demo.modal.depth(), 1);
}

#[test]
fn 屏切换漏斗真的调了文本输入态同步() {
    // 上一条只证明 `sync_text_entry_mode` 这个方法本身对；这一条
    // 证明它**被接进了屏切换那个唯一漏斗**。少了这条，把漏斗里那
    // 一行删掉不会有任何东西变红——而那正是本仓库最贵的失败模式
    // （声明了但没接线）。
    //
    // **手法本批改过**：原来是「在栈上留一层与当前屏不相符的
    // `TextEntry`，看漏斗会不会把它弹掉」。那种状态现在**构造不
    // 出来**——`crate::modal::Modal` 把栈与屏封在一起，「屏是存档
    // 列表而栈顶是文本输入」在 `modal` 之外写不出来（那正是规格 N8
    // 要的结果）。
    //
    // 改成从**外面**看同一件事：驱动一帧 `update_screen`，让它算出
    // 一块新屏。漏斗里那行 `self.modal.set_screen(...)` 一旦删掉，
    // 屏根本不会换，这条当场变红；而 `set_screen` 自己就负责文本
    // 输入层的同步（有单元测试盯着），两件事因此不可能只做一件。
    // Arrange：首页，焦点落在「设置」那一行。
    let mut demo = test_demo_at_title();
    走一帧(&mut demo, 0, &[GameKey::Down]);
    走一帧(&mut demo, 1, &[GameKey::Down]);

    // Act
    走一帧(&mut demo, 2, &[GameKey::Confirm]);

    // Assert
    assert!(
        matches!(demo.modal.screen(), Some(ScreenState::Settings { .. })),
        "屏切换漏斗应当真的把新屏写回去，实际是 {:?}",
        demo.modal.screen()
    );
    assert_eq!(demo.modal.depth(), 1, "换屏不是又盖一层");
    assert_eq!(demo.input_context(), InputContext::Menu);
}

#[test]
fn 反复切到同一块屏不会把模态栈越堆越高() {
    // 屏切换漏斗每帧都会调一次 `set_screen`（`update_screen` 算出
    // 的 `next` 大多数帧等于当前屏），必须幂等——否则停在命名屏
    // 几秒钟就会堆出几百层栈。
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    demo.open_menu(&mut input);

    // Act
    for _ in 0..10 {
        demo.modal.set_screen(
            Some(ScreenState::SaveNaming {
                origin: crate::spawn_pick::SpawnOrigin::WorldSetup,
            }),
            &mut input,
        );
    }

    // Assert
    assert_eq!(demo.modal.depth(), 2, "菜单层 + 文本输入层，就两层");
    assert_eq!(demo.input_context(), InputContext::TextEntry);
}

#[test]
fn 模态屏开着时世界时钟与玩家坐标都不动() {
    // 「打开菜单时世界不应继续推进」的直接验收。回合制本来就是
    // 「玩家不提交意图时钟就不走」，但方向键仍然会被
    // `player_command` 读成移动——`advance` 的整段早退才是真正
    // 挡住它的那一步，见该方法里的注释。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Menu]);
    let 开屏后坐标 = player_pos(&demo);
    let 开屏后时钟 = demo.test_world().world.clock;

    // Act：菜单开着，连按十帧方向键与等待键。
    for at in 1..11u64 {
        走一帧(&mut demo, at, &[GameKey::Right]);
        走一帧(&mut demo, at + 100, &[GameKey::Wait]);
    }

    // Assert
    assert_eq!(player_pos(&demo), 开屏后坐标, "角色不该动");
    assert_eq!(demo.test_world().world.clock, 开屏后时钟, "时钟不该走");
    assert!(demo.modal.screen().is_some(), "菜单应当仍然开着");
}

#[test]
fn 模态屏关掉之后世界重新可以推进() {
    // 上一条的另一半：早退不能把游戏永久关死。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Menu]);
    走一帧(&mut demo, 1, &[GameKey::Cancel]);
    let 关屏后时钟 = demo.test_world().world.clock;

    // Act：连按几帧「等待」——第一帧多半还轮不到玩家
    // （`PlayerTurnOutcome::NotYet`，非受控实体先结算），与既有的
    // `连续多次玩家等待后世界时钟真的前进` 那条测试同一个理由。
    for at in 2..8u64 {
        走一帧(&mut demo, at, &[GameKey::Wait]);
    }

    // Assert
    assert!(demo.modal.screen().is_none());
    assert!(
        demo.test_world().world.clock > 关屏后时钟,
        "等待应当推进时钟"
    );
}

#[test]
fn 模态屏开着时取消键只关屏不退出游戏() {
    // 与背包菜单那条同型的陷阱：玩家想关个菜单不该直接退出整局。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Menu]);

    // Act
    let outcome = 走一帧(&mut demo, 1, &[GameKey::Cancel]);

    // Assert
    assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
    assert!(demo.modal.screen().is_none(), "取消键应当把模态屏关掉");
    assert_eq!(demo.input_context(), InputContext::Gameplay);
}

#[test]
fn 模态屏里选中退出项才真的退出() {
    // Arrange：开菜单，焦点连按三次「下」落到第三项（退出游戏）。
    let mut demo = test_demo();
    let at = 开到菜单行(&mut demo, crate::pause_menu::MenuRow::Quit);

    // Act
    let outcome = 走一帧(&mut demo, at, &[GameKey::Confirm]);

    // Assert
    assert_eq!(outcome, FrameOutcome::Exit);
}

#[test]
fn 模态屏里选中设置项进入设置界面() {
    // Arrange：焦点落到「设置」那一行。
    let mut demo = test_demo();
    let at = 开到菜单行(&mut demo, crate::pause_menu::MenuRow::Settings);

    // Act
    走一帧(&mut demo, at, &[GameKey::Confirm]);

    // Assert
    assert!(matches!(
        demo.modal.screen(),
        Some(ScreenState::Settings { .. })
    ));
}

#[test]
fn 设置界面里按取消退回菜单屏而不是退出整局() {
    // 这一条比「菜单屏按取消不退出」咬得更紧：菜单屏那一条会关屏，
    // 而关屏顺带 `InputState::clear()`（`UiModeStack::pop` 的语义），
    // 取消键因此被吃掉，即使少一道闸门也看不出问题。设置界面按
    // 取消**不关屏**（只退回菜单屏），取消键的「刚按下」标志原封
    // 不动地留到下面那条退出判定——`self.screen.is_none()` 那道
    // 闸门在这条路径上是真的在挡事。
    // Arrange：进设置界面。
    let mut demo = test_demo();
    let at = 开到设置屏(&mut demo);
    assert!(matches!(
        demo.modal.screen(),
        Some(ScreenState::Settings { .. })
    ));

    // Act
    let outcome = 走一帧(&mut demo, at, &[GameKey::Cancel]);

    // Assert
    assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
    assert_eq!(demo.modal.screen(), Some(ScreenState::Menu));
}

#[test]
fn 背包开着时按菜单键不叠第二块模态屏() {
    // 两块模态 UI 叠在一起会立刻引出「Esc 关哪一层」的新裁定，
    // 而没有任何人要求过这件事。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Inventory]);
    assert!(demo.modal.player_menu().is_open(), "Arrange 应当把背包打开");

    // Act
    走一帧(&mut demo, 1, &[GameKey::Menu]);

    // Assert
    assert!(demo.modal.screen().is_none());
    assert_eq!(demo.input_context(), InputContext::Gameplay);
}

#[test]
fn 打开模态屏时按住的键被视为全部松开() {
    // 设计文档 2.3 节的硬结论：上下文切换是第三种「隐式全键松开」
    // 边界。不清空的话，打开菜单那一刻按着的 W 会带着「已按住」
    // 进菜单，用移动场景的重复计时基准去滚菜单光标。
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    input.press(GameKey::Up);
    input.press(GameKey::Menu);

    // Act
    demo.on_frame(FrameId(0), &mut input);

    // Assert
    assert!(!input.is_held(GameKey::Up));
}

/// 把 `demo` 的暂停菜单光标开到 `target` 那一行，返回**下一帧**的
/// 时刻。
///
/// 按**行的语义**走，不按写死的按键次数：菜单行数随存档模式变化
/// （「保存」那一行在肉鸽档里整行不存在），写死次数的测试会在行数
/// 一变时静默地按到另一项上去——本批次加两行的那一刻，六条既有断言
/// 正是这么红的。
fn 开到菜单行(demo: &mut Demo, target: crate::pause_menu::MenuRow) -> u64 {
    let can_save = demo.can_save_manually();
    let index = crate::pause_menu::menu_rows(can_save)
        .iter()
        .position(|row| *row == target)
        .expect("这一行必然存在");
    走一帧(demo, 0, &[GameKey::Menu]);
    let mut at = 1;
    // **本批改过**：菜单一开焦点就在第 0 行（规格 N10），所以到第
    // `index` 行只要按 `index` 次。此前第一次「向下」是用来把焦点
    // 从「什么都没选」落到第 0 行的。
    for _ in 0..index {
        走一帧(demo, at, &[GameKey::Down]);
        at += 1;
    }
    at
}

/// 把 `demo` 开到设置界面，返回下一帧的时刻。
fn 开到设置屏(demo: &mut Demo) -> u64 {
    let at = 开到菜单行(demo, crate::pause_menu::MenuRow::Settings);
    走一帧(demo, at, &[GameKey::Confirm]);
    at + 1
}

/// 把 `demo` 开到设置界面，光标停在 `action` 那一行，并且已经进了
/// 捕获模式（下一帧按什么物理键就绑什么）。
///
/// 走的全是玩家真正走的公开路径（`on_frame`），不直接摆弄
/// `demo.modal.screen()`——那样测出来的是「我把状态摆成这样之后会怎样」，
/// 不是「玩家按出来会怎样」。
fn 开到捕获模式(demo: &mut Demo, action: GameKey) -> u64 {
    let mut at = 开到设置屏(demo);
    let target = crate::menu_screen::settings_rows()
        .iter()
        .position(|row| *row == crate::menu_screen::SettingsRow::Keybind(action))
        .expect("每个动作在设置界面都有一行");
    for _ in 0..target {
        走一帧(demo, at, &[GameKey::Down]);
        at += 1;
    }
    // 确认键把这一行推进捕获模式。
    走一帧(demo, at, &[GameKey::Confirm]);
    at + 1
}

#[test]
fn 设置屏开着但玩家什么都不按时不产生待送回的键位表() {
    // 项目所有者实机撞到的缺陷：这一支此前**每帧无条件**
    // `pending_bindings = Some(整表克隆)`，于是设置屏一开，终端每帧
    // 刷一行「键位绑定表已由上层替换」——一条为稀有事件准备的诊断
    // 日志被烧成了噪音，而它旁边的注释还写着「不是每帧」。
    //
    // 反例验证（已实跑）：把 `update_screen` 里那道
    // `if update.rebound` 判断去掉、恢复成无条件赋值，本条立刻变红。
    // Arrange：进设置界面，把开屏那几帧可能攒下的东西先取空。
    let mut demo = test_demo();
    let start = 开到设置屏(&mut demo);
    assert!(matches!(
        demo.modal.screen(),
        Some(ScreenState::Settings { .. })
    ));
    demo.take_rebound_keys();

    // Act：屏开着，一连三帧一个键都不按，也不移动光标。
    for at in start..start + 3 {
        走一帧(&mut demo, at, &[]);
    }

    // Assert
    assert!(
        demo.take_rebound_keys().is_none(),
        "没改键位的帧不该产生整表克隆"
    );
}

#[test]
fn 设置界面真的重绑之后新表会被平台层取走且内容正确() {
    // 真正被解析路径查的表住在平台层；不送回去，玩家会看到「设置
    // 界面里改好了，按下去还是旧的」。
    //
    // 本条此前的写法是「在设置界面里动一下（哪怕只是移动光标）」
    // 就断言取得到表——那恰好把上面那个缺陷当成了正确行为钉死。
    // 现在改成真的重绑一次，并检查取回来的表里那个键确实生效。
    //
    // 反例验证（已实跑）：把 `SettingsUpdate::rebound` 构造器里的
    // `rebound: true` 改成 `false`，本条立刻变红。
    // Arrange：进设置界面并把光标推到 Map 那一行的捕获模式。
    let mut demo = test_demo();
    let at = 开到捕获模式(&mut demo, GameKey::Map);
    demo.take_rebound_keys();

    // Act：按下一个默认表里谁都没占的物理键。
    let mut input = InputState::new();
    input.record_physical_key(ll_platform::keybind::KeyCode::KeyN);
    demo.on_frame(FrameId(at), &mut input);

    // Assert
    let taken = demo.take_rebound_keys().expect("真的改过键位，必须送回");
    assert_eq!(
        taken.resolve(
            ll_platform::keybind::KeyCode::KeyN,
            ll_platform::keybind::Modifiers::NONE,
            crate::menu_screen::EDITABLE_CONTEXT,
        ),
        Some(GameKey::Map),
        "送回平台层的表必须已经带上这次重绑"
    );
}

#[test]
fn 菜单开着时取消键只关菜单不退出游戏() {
    // 守 `on_frame` 里那道 `!self.menu.is_open()` 闸门：没有它，玩家
    // 想关个背包会直接退出整局。
    //
    // 本条**必须**走 `AppHandler::on_frame`（不是像其余几条那样直接
    // 调 `advance`）：退不退出这个决定就做在 `on_frame` 里，`advance`
    // 根本看不见它。`on_frame` 在 `resources` 为 `None` 时会在渲染
    // 那一步提前返回 `Continue`（见其实现），因此脱离 GPU 也能跑。
    // Arrange
    let mut demo = test_demo();
    let mut open_inventory = InputState::new();
    open_inventory.press(GameKey::Inventory);
    assert_eq!(
        demo.on_frame(FrameId(0), &mut open_inventory),
        FrameOutcome::Continue
    );
    assert!(
        demo.modal.player_menu().is_open(),
        "Arrange 应当把背包菜单打开"
    );

    // Act
    let mut cancel = InputState::new();
    cancel.press(GameKey::Cancel);
    let outcome = demo.on_frame(FrameId(1), &mut cancel);

    // Assert：没退出，菜单关了。
    assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
    assert!(!demo.modal.player_menu().is_open(), "取消键应当把菜单关掉");

    // 再按一次（子菜单已经关着、模态屏也没开）——**开主菜单，不退出**。
    //
    // 这一半在「顶层取消键改成开主菜单」那次改动里反转过：此前它断言
    // `FrameOutcome::Exit`（按一下 Esc 整局就没了，没有任何确认，
    // 项目所有者实机撞到并要求改掉）。现在 Esc 是逐层往回退，退出的
    // 唯一入口是主菜单里那一项。
    let mut cancel_again = InputState::new();
    cancel_again.press(GameKey::Cancel);
    assert_eq!(
        demo.on_frame(FrameId(2), &mut cancel_again),
        FrameOutcome::Continue,
        "什么都没开时按取消键不该退出整局"
    );
    assert!(
        demo.modal.screen().is_some(),
        "什么都没开时按取消键应当开出主菜单——否则这个键就成了死键，\
         玩家既退不出也进不去菜单"
    );
}

#[test]
fn 不开制作菜单时按确认键不会制作任何东西() {
    // 反例守卫：证明上面那条锻造测试真正依赖的是「开菜单 + 选中
    // 那一行」，不是「按了确认键就会造东西」。
    // Arrange
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);

    // Act：菜单全关着，连按三次确认。
    for at in 0..3u64 {
        frame(&mut demo, at, &[GameKey::Confirm]);
    }

    // Assert
    assert_eq!(carried(&demo, fixture.forge), 0);
    assert_eq!(
        carried(&demo, fixture.iron_ingot),
        8,
        "一块铁锭都不该被吃掉"
    );
}

#[test]
fn 按装备键再按一次就把它卸回背包() {
    // `Intent::Unequip` 的键位产出者——背包菜单里落在「已装备」那
    // 一段的行，见 `crate::player_action` 模块文档「为什么装备与
    // 卸下共用一个键」一节。
    // Arrange：先按键把锤子装上。
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    let mut at = 0u64;
    equip_hammer_by_keys(&mut demo, &mut at, &fixture);
    assert_eq!(
        carried(&demo, fixture.smith_hammer),
        0,
        "Arrange 之后锤子应当已经不在背包里"
    );

    // Act：开背包、光标归零后移到「已装备」那一段的锤子上，再按一
    // 次装备键。
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let equipped_row = inventory_row_of(&demo, fixture.smith_hammer);
    move_cursor_to(&mut demo, &mut at, equipped_row);
    frame(&mut demo, at, &[GameKey::Equip]);

    // Assert
    assert_eq!(
        carried(&demo, fixture.smith_hammer),
        1,
        "再按一次装备键应当把它卸回背包"
    );
    assert!(
        demo.test_world()
            .world
            .actors
            .get(demo.test_world().player)
            .expect("玩家仍存在")
            .equipment
            .values()
            .all(|stack| stack.def != fixture.smith_hammer),
        "装备栏里不该还留着这把锤子"
    );
}

#[test]
fn 按使用键能吃掉背包里的一份烤肉() {
    // `Intent::Use` 的键位产出者。
    // Arrange
    let mut demo = test_demo();
    let roast = content_index(&demo, "lostland:roast_meat");
    let player = demo.test_world().player;
    demo.test_world_mut()
        .world
        .actors
        .get_mut(player)
        .expect("玩家刚建局")
        .inventory
        .push(ItemStack::new(roast, 2));
    let before = carried(&demo, roast);
    let mut at = 0u64;

    // Act：开背包 → 移到烤肉 → 按使用键。
    frame(&mut demo, at, &[GameKey::Inventory]);
    at += 1;
    let row = inventory_row_of(&demo, roast);
    move_cursor_to(&mut demo, &mut at, row);
    frame(&mut demo, at, &[GameKey::Use]);

    // Assert：恰好少一份——不是「变少了」，是「少了一份」。
    assert_eq!(carried(&demo, roast), before - 1);
}
