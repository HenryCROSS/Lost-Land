//! `crate::app` 里存档相关行为的断言：自动存档节拍、玩家死亡接线、
//! 手动存档、回主菜单。
//!
//! # 为什么住在独立文件里
//!
//! `app.rs` 已经 4000 行开外（既有违规）。这一批断言四百多行，塞回去
//! 只会让那笔账更难还。用 `#[path]` 挂成 `crate::app` 的子模块而不是
//! 搬进 `tests/`：这些断言要摸 `Demo` 的私有字段与私有方法
//! （`handle_player_death`/`maybe_autosave`/`save_now`/`back_to_title`），
//! 集成测试摸不到，而摆成 `pub` 只为了测试是本末倒置。
//!
//! ADR 0025 禁止用合成按键做验收：这里每一条都是程序化驱动公开/私有
//! 路径，不模拟任何键盘事件。

use ll_i18n::Catalog;
use ll_platform::config::GameConfig;
use ll_platform::input::InputState;

use super::{AUTOSAVE_INTERVAL_TICKS, Demo};
use crate::menu_screen::{ScreenNotice, ScreenState};

use super::tests::{test_content, test_demo};

#[test]
fn 自动存档按世界时间触发而不是按墙钟() {
    // C6。**这一条钉住的是「隐藏输入」这条约束**（C4）：墙钟会让存档
    // 时机取决于玩家盯着屏幕想了多久——同一串输入在两次运行里会在
    // 不同的世界状态上触发存档。世界时钟只由回合推进驱动，是玩家输入
    // 的纯函数。
    //
    // 反例验证（已实跑）：把判据里的 `session.game_world.world.clock`
    // 换成 `std::time::Instant::now()` 那一类墙钟量，本条的后半段
    // （世界时钟不动就绝不存档）当场变红。
    // Arrange
    let mut demo = test_demo();
    let saves_dir = demo.saves_dir.clone();

    // Act 1：世界时钟一动不动，连问一百次。
    for _ in 0..100 {
        demo.maybe_autosave();
    }

    // Assert 1：真实时间在流逝，世界时间没有 ⇒ 一次都不该存。
    assert!(
        crate::save_slot::list_slots(&saves_dir).is_empty(),
        "世界时钟不动时绝不该自动存档——那正是「按墙钟」会犯的错"
    );

    // Act 2：把世界时钟往前拨满一个周期。
    let 起点 = demo.test_world().world.clock;
    demo.test_world_mut().world.clock = ll_core::time::Tick(起点.0 + AUTOSAVE_INTERVAL_TICKS);
    demo.maybe_autosave();

    // Assert 2
    assert_eq!(
        crate::save_slot::list_slots(&saves_dir).len(),
        1,
        "世界时钟走满一个周期就该存一次"
    );

    // Act 3：紧接着再问一次，世界时钟没再动。
    demo.maybe_autosave();

    // Assert 3：节拍已经往前推了，不该连着存第二次。
    assert_eq!(
        crate::save_slot::list_slots(&saves_dir).len(),
        1,
        "刚存过就不该立刻再存一次"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 不足一个周期不触发自动存档() {
    // Arrange
    let mut demo = test_demo();
    let saves_dir = demo.saves_dir.clone();
    let 起点 = demo.test_world().world.clock;

    // Act：差一个 tick 就满一个周期。
    demo.test_world_mut().world.clock = ll_core::time::Tick(起点.0 + AUTOSAVE_INTERVAL_TICKS - 1);
    demo.maybe_autosave();

    // Assert
    assert!(crate::save_slot::list_slots(&saves_dir).is_empty());

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 玩家死亡后存档保留模式转普通并回到角色创建() {
    // C7 + C9。所有者的修正原话：「死亡后变成一般模式，可以再创建
    // 角色然后选择在某个地方出生。」
    //
    // 反例验证（已实跑）：把 `handle_player_death` 里那句
    // `downgrade_mode()` 去掉，「模式转普通」那条断言当场变红；把
    // 那句 `write_save` 去掉，「存档保留」当场变红。
    // Arrange：一局**肉鸽**档，先存一次（模拟已经玩过一阵）。
    let content = test_content();
    let game_world = crate::world::build_new_world_with_mode(
        &content,
        ll_world::generate::GenParams {
            seed: 7,
            ..ll_world::generate::GenParams::default()
        },
        ll_content::mode::SaveMode::Permadeath,
    )
    .expect("测试用布局满足全部构造前置条件");
    let saves_dir = crate::test_support::unique_temp_path("ll-game-death-saves");
    let mut demo = Demo::new(
        content,
        game_world,
        saves_dir.clone(),
        "测试旅人".to_string(),
        GameConfig::default(),
        crate::test_support::unique_temp_path("ll-game-death-config").join("config.json5"),
        Catalog::load_dir(&std::env::temp_dir().join("ll-game-death-empty-locales")),
    );
    assert!(!demo.can_save_manually(), "Arrange：这是一局肉鸽档");
    demo.save_now();
    assert_eq!(crate::save_slot::list_slots(&saves_dir).len(), 1);
    let mut input = InputState::new();

    // Act：玩家死了——实体从 arena 里消失，正是 `ll_sim::apply` 的
    // `Despawn` 在生产路径上做的事。
    let player = demo.test_world().player;
    demo.test_world_mut().world.actors.despawn(player);
    demo.handle_player_death(&mut input);

    // Assert：**存档还在**，世界比角色活得长。
    let slots = crate::save_slot::list_slots(&saves_dir);
    assert_eq!(slots.len(), 1, "死亡不删档");
    assert!(
        slots[0].allows_manual_save(),
        "模式应当已经从肉鸽单向转成普通"
    );
    assert!(
        slots[0].mode.was_downgraded_from_permadeath(),
        "「曾经是肉鸽」这条记录永久留下"
    );

    // Assert：回到角色创建，且走的是批次 8 留的那条接缝。
    assert_eq!(
        demo.screen,
        Some(ScreenState::CharacterCreation { cursor: 0 })
    );
    assert!(demo.session.is_none(), "玩家已经不在世界里了");
    let draft = demo.new_game_draft.as_ref().expect("死亡后应当有一份草稿");
    assert!(
        draft.world_already_exists,
        "世界本来就存在——状态机因此会跳过世界配置屏（批次 8 第七节接缝 1）"
    );
    assert!(draft.world.is_some(), "草稿里带着那一局原样的世界");
    assert!(
        draft.existing_target.is_some(),
        "沿用原来的槽位，不再起一个名字——否则同一个世界会在列表里出现两份"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 玩家还活着时死亡处理什么都不做() {
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    let saves_dir = demo.saves_dir.clone();

    // Act
    demo.handle_player_death(&mut input);

    // Assert
    assert!(demo.session.is_some());
    assert!(demo.new_game_draft.is_none());
    assert!(crate::save_slot::list_slots(&saves_dir).is_empty());
}

#[test]
fn 回主菜单之前先把进度存下来() {
    // B2。**这一条防的是静默丢弃玩家进度**：所有者只说要有「返回
    // 主菜单」这一项，没说未保存的进度怎么办；本批次选定「先存一次
    // 再回去」，理由见 `Demo::back_to_title` 文档。
    //
    // 反例验证（已实跑）：把 `back_to_title` 里那句 `write_save`
    // 删掉（直接置空 `session`），本条当场变红——存档目录里一份都
    // 没有。
    // Arrange
    let mut demo = test_demo();
    let mut input = InputState::new();
    let saves_dir = demo.saves_dir.clone();
    assert!(
        crate::save_slot::list_slots(&saves_dir).is_empty(),
        "Arrange：这一局还没存过"
    );
    assert!(demo.session.is_some(), "Arrange：玩家在世界里");

    // Act
    demo.back_to_title(&mut input);

    // Assert
    let slots = crate::save_slot::list_slots(&saves_dir);
    assert_eq!(slots.len(), 1, "回主菜单必须把进度写下来，不能静默丢弃");
    assert!(demo.session.is_none(), "回主菜单之后世界不该还在");
    assert_eq!(demo.screen, Some(ScreenState::Title));

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 回主菜单时写盘失败就留在暂停菜单不丢弃世界() {
    // B3。「回去了但没存上」是这条路径上最坏的结果——玩家已经离开
    // 世界，那份进度再也拿不回来。写盘失败时必须留在原地。
    //
    // 反例验证（已实跑）：把 `back_to_title` 里那句 `return` 去掉
    // （失败也照样置空 `session`），本条当场变红。
    // Arrange：让存档目录这条路径被一个**文件**占住，`create_dir_all`
    // 因此必然失败——不是假造一个错误类型，是真的写不进去。
    let mut demo = test_demo();
    let mut input = InputState::new();
    let blocker = demo.saves_dir.clone();
    if let Some(parent) = blocker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&blocker, b"not a directory").expect("占位文件应当写得出来");
    demo.session
        .as_mut()
        .expect("Arrange：玩家在世界里")
        .save_target
        .path = blocker.join("whatever.llsave");

    // Act
    demo.back_to_title(&mut input);

    // Assert
    assert!(
        demo.session.is_some(),
        "写盘失败时不该把玩家从世界里踢出去——那份进度会就此消失"
    );
    assert_eq!(
        demo.screen_notice,
        Some(ScreenNotice::GameSaveFailed),
        "必须明说存档失败了，不能静默"
    );

    // Cleanup
    let _ = std::fs::remove_file(&blocker);
}

#[test]
fn 手动存档写出一份档并留在菜单里() {
    // Arrange
    let mut demo = test_demo();
    let saves_dir = demo.saves_dir.clone();
    demo.screen = Some(ScreenState::Menu);

    // Act
    demo.save_now();

    // Assert
    assert_eq!(crate::save_slot::list_slots(&saves_dir).len(), 1);
    assert_eq!(demo.screen_notice, Some(ScreenNotice::GameSaved));
    assert_eq!(demo.screen, Some(ScreenState::Menu), "存完仍留在菜单里");

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 同一局连存两次是覆盖不是新建第二份() {
    // C2（本提交先把它钉住）：槽位标识在建档那一刻定死，此后永远
    // 写同一个文件。每次存档按当前名字重算文件名的话，列表里会出现
    // 两个同一个世界的条目。
    // Arrange
    let mut demo = test_demo();
    let saves_dir = demo.saves_dir.clone();

    // Act
    demo.save_now();
    demo.save_now();

    // Assert
    assert_eq!(
        crate::save_slot::list_slots(&saves_dir).len(),
        1,
        "同一局存两次应当覆盖同一份存档"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&saves_dir);
}

#[test]
fn 新建的世界默认是普通模式因此暂停菜单里有保存那一行() {
    // 迁移前 `save_game` 的 7 处调用点全部硬编码 `Permadeath`——
    // 也就是每一局都被记成肉鸽档，而玩家从来没做过这个选择。
    // Arrange
    let demo = test_demo();

    // Act & Assert
    assert!(
        demo.can_save_manually(),
        "没明说模式时建出来的应当是普通档（限制更少的那一档）"
    );
}

#[test]
fn 死亡重生走的是选出生地而不是重新生成世界() {
    // C9。**这一条防的是把玩家玩过的世界整个抹掉**：死亡重生若照开局那
    // 条路走进世界配置屏，玩家按下「生成世界」就会得到一个全新的世界，
    // 而他只是想换一个角色。
    //
    // 反例验证（已实跑）：把 `update_character_creation` 里那条
    // `CharacterRow::Next if world_already_exists` 分支去掉（恢复成
    // 无条件去 `WorldSetup`），本条当场变红。
    // Arrange：造一份「世界已经存在」的草稿——就是死亡重生那条路的形状。
    let content = test_content();
    let world = crate::world::build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: 9,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("测试用布局满足全部构造前置条件");
    let 种子 = world.world.seed;
    let target = crate::save_slot::SaveTarget::create_in(
        &crate::test_support::unique_temp_path("ll-game-reincarnate-saves"),
        "reborn",
    );
    let mut draft = crate::chargen::NewGameDraft::for_reincarnation(&content, world, target);
    let mut cursor = 0usize;
    let roster = draft.roster.clone();

    // Arrange：光标走到「下一步」那一行。
    let rows = crate::chargen::character_rows();
    let next_index = rows
        .iter()
        .position(|row| *row == crate::chargen::CharacterRow::Next)
        .expect("「下一步」必然是其中一行");
    let mut down = InputState::new();
    down.press(ll_platform::input::GameKey::Down);
    // `move_cursor` 从 0 起算（与 `navigate_focus` 不同，不需要先按一下
    // 把焦点「点亮」），所以到第 `next_index` 行按 `next_index` 次。
    for _ in 0..next_index {
        crate::chargen::update_character_creation(
            &mut cursor,
            &mut draft.choice,
            &roster,
            &down,
            draft.world_already_exists,
        );
    }

    // Act
    let mut confirm = InputState::new();
    confirm.press(ll_platform::input::GameKey::Confirm);
    let update = crate::chargen::update_character_creation(
        &mut cursor,
        &mut draft.choice,
        &roster,
        &confirm,
        draft.world_already_exists,
    );

    // Assert：直接去选出生地，**不经过世界配置屏**。
    assert_eq!(
        update.next,
        Some(ScreenState::SpawnPick),
        "世界已经存在时必须跳过世界配置屏"
    );
    // 而且草稿里那个世界原样还在——没有被重新生成过。
    assert_eq!(
        draft.world.as_ref().expect("世界还在").world.seed,
        种子,
        "重生不该动这个世界一个字节"
    );
}

#[test]
fn 开局那条路仍然经过世界配置屏() {
    // 守住既有行为不被上一条的分支改坏。
    // Arrange
    let content = test_content();
    let mut draft =
        crate::chargen::NewGameDraft::new(&content, &ll_platform::config::NewGameConfig::default());
    assert!(
        !draft.world_already_exists,
        "Arrange：开局那条路世界还不存在"
    );
    let mut cursor = 0usize;
    let roster = draft.roster.clone();
    let rows = crate::chargen::character_rows();
    let next_index = rows
        .iter()
        .position(|row| *row == crate::chargen::CharacterRow::Next)
        .expect("「下一步」必然是其中一行");
    let mut down = InputState::new();
    down.press(ll_platform::input::GameKey::Down);
    // `move_cursor` 从 0 起算（与 `navigate_focus` 不同，不需要先按一下
    // 把焦点「点亮」），所以到第 `next_index` 行按 `next_index` 次。
    for _ in 0..next_index {
        crate::chargen::update_character_creation(
            &mut cursor,
            &mut draft.choice,
            &roster,
            &down,
            draft.world_already_exists,
        );
    }

    // Act
    let mut confirm = InputState::new();
    confirm.press(ll_platform::input::GameKey::Confirm);
    let update = crate::chargen::update_character_creation(
        &mut cursor,
        &mut draft.choice,
        &roster,
        &confirm,
        draft.world_already_exists,
    );

    // Assert
    assert_eq!(update.next, Some(ScreenState::WorldSetup { cursor: 0 }));
}
