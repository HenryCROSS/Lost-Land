//! 导航收敛批次（规格 N8 / N7 / N2 / N1 / N10）与鼠标接线的断言。
//!
//! # 为什么住在独立文件里
//!
//! 与 `app_save_tests.rs` 逐字同一条理由：`app.rs` 已经 4500 行开外
//! （既有违规，交接文档第四节第 8 条记着这笔账），本批次给它加的产品
//! 代码只有百来行，断言却有五百多行。用 `#[path]` 挂成
//! `crate::app::tests` 的子模块而不是搬进 `tests/`：这些断言要摸
//! `Demo` 的私有字段与私有方法（`modal`/`pointer`/`viewport`/
//! `screen_row_texts`），集成测试摸不到，而为了测试把它们摆成 `pub`
//! 是本末倒置。
//!
//! **共同点：Act 一律只有输入。** 全程走 `frame`/`走一帧` 构造
//! `InputState` 驱动真实的 `on_frame`/`advance`，鼠标走
//! `InputState` 那几个既有公开 setter——ADR 0025 要求的「程序化驱动
//! 同一条调用路径」，不合成任何操作系统级事件。

use super::*;

// ───────────────────── 导航收敛批次（规格 N8 / N2 / N7 / N10）─────────
//
// 共同点：**Act 一律只有按键**，全程走 `frame`/`走一帧` 构造
// `InputState` 驱动真实 `on_frame`/`advance`，不合成任何操作系统级
// 事件（ADR 0025）。

#[test]
fn 地图开着按取消键关地图而不是弹出暂停菜单() {
    // 规格 N8 判据 1、D3 的直接验收。**此前这条会红**：地图不在
    // 模态栈里，于是 `on_frame` 那条顶层取消判据成立，Esc 会在地图
    // 上面盖一层暂停菜单。
    //
    // 反例验证（已实跑）：删掉 `Demo::handle_world_map` 开头那条取消
    // 分支，本条立刻变红（screen 变成 Some(Menu)）。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Map]);
    assert!(demo.modal.world_map_open(), "Arrange：地图应当开着");

    // Act
    走一帧(&mut demo, 1, &[GameKey::Cancel]);

    // Assert
    assert!(!demo.modal.world_map_open(), "取消键应当把地图关掉");
    assert_eq!(demo.modal.screen(), None, "**不该**弹出暂停菜单");
    assert!(demo.modal.is_empty(), "关掉之后一层都不该剩");
}

#[test]
fn 模态屏盖着时地图键与方向键一概不作用于地图() {
    // 规格 N8 判据 2、D3 后半：`advance` 里地图的早退判据此前排在
    // 模态屏判据**前面**，于是暂停菜单盖在地图上的那一帧，方向键
    // 被 `update_screen`（菜单光标）与 `pan_and_zoom_world_map`
    // （地图平移）**同时**消费——玩家按一下「下」，菜单光标动一格，
    // 地图也跟着滚一格。
    //
    // 两道闸门换了次序之后，「屏盖着」压倒一切：地图键在那一帧连
    // 翻转都不该翻。这条断言正是**次序**本身。
    //
    // 反例验证（已实跑）：把 `advance` 里 `handle_world_map` 那一段
    // 挪回 `modal.screen().is_some()` 判据之前，本条立刻变红。
    // Arrange：开着暂停菜单。
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Menu]);
    assert_eq!(demo.modal.screen(), Some(ScreenState::Menu));

    // Act：在菜单里按地图键与方向键。
    走一帧(&mut demo, 1, &[GameKey::Map]);
    走一帧(&mut demo, 2, &[GameKey::Down]);

    // Assert
    assert!(
        !demo.modal.world_map_open(),
        "屏盖着的时候地图键不该翻转任何东西"
    );
    assert_eq!(demo.modal.depth(), 1, "也不该多盖一层");
}

#[test]
fn 背包开着时模态栈深度为一关掉之后栈必空() {
    // 规格 N8 判据 3 与 4。**判据 3 此前会红**（背包不进栈，深度
    // 恒为 0）。判据 4 盯的是 push/pop 配对。
    //
    // 反例验证（已实跑）：把 `run_turn` 里 `with_player_menu` 换成
    // 直接对一个局部 `PlayerMenu` 调 `player_command`，两条都红。
    // Arrange
    let mut demo = test_demo();

    // Act：开背包
    走一帧(&mut demo, 0, &[GameKey::Inventory]);

    // Assert
    assert!(demo.modal.player_menu().is_open(), "Arrange：背包应当开着");
    assert_eq!(demo.modal.depth(), 1, "背包也是一层模态 UI");
    assert!(!demo.modal.is_empty());
    assert_eq!(
        demo.input_context(),
        InputContext::Gameplay,
        "进栈但**不换键位表**——否则 I/C/空格在背包里全部解析不出来"
    );

    // Act：再按一次关掉
    走一帧(&mut demo, 1, &[GameKey::Inventory]);

    // Assert
    assert!(demo.modal.is_empty(), "关掉之后栈必空");
}

#[test]
fn 背包开着时按取消键关背包而不是开暂停菜单() {
    // 规格 N2：取消键只退最上面那一层。这条此前靠 `on_frame` 里那
    // 条手工合取里的 `&& !menu.is_open()` 成立，现在靠
    // `modal.is_empty()`——同一个行为，判据从「每加一套就多一项」
    // 变成一条。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Inventory]);

    // Act
    走一帧(&mut demo, 1, &[GameKey::Cancel]);

    // Assert
    assert!(!demo.modal.player_menu().is_open());
    assert_eq!(demo.modal.screen(), None, "**不该**开出暂停菜单");
    assert!(demo.modal.is_empty());
}

// ───────────────────── 鼠标接线：漏斗真的接上了 ─────────────────────

/// 首页第 `row` 行这一刻画在屏幕的哪一块——**用的是渲染侧与输入侧
/// 共用的那同一个 `screen_row_texts`**，测试里不另抄一份几何。
fn 首页行矩形(demo: &Demo, viewport: (f32, f32)) -> Vec<ll_ui::widget::geometry::Rect> {
    let (rows, cursor) = screen_row_texts(
        ScreenState::Title,
        &demo.config,
        &demo.catalog,
        &demo.screen_focus,
        !demo.save_slots.is_empty(),
        demo.can_save_manually(),
        &demo.save_slots,
        &demo.content,
        demo.new_game_draft.as_ref(),
    )
    .expect("首页画的就是那块居中面板");
    let data = screen_data(ScreenState::Title, &rows, cursor, None);
    ll_ui::screen::screen_row_rects(
        &data,
        &demo.catalog,
        &demo.config.language,
        viewport.0,
        viewport.1,
    )
}

#[test]
fn 在首页第三行上点一下真的进设置屏() {
    // **这一条盯的是漏斗**：`update_screen` 里那句
    // `self.resolve_screen_pointer(state, input)`，以及它算出来的
    // `RowPointer` 真的被传给了 `update_title`。`title_screen.rs`
    // 那几条只证明「`update_title` 收到 Activate(2) 会进设置屏」，
    // 少了本条，把漏斗里那一行改成恒传 `RowPointer::Idle` 不会有
    // 任何东西变红——而那正是本仓库最贵的失败模式。
    //
    // 反例验证（已实跑）：把 `update_screen` 里那句 `let pointer =
    // self.resolve_screen_pointer(state, input);` 改成
    // `RowPointer::Idle`，本条立刻变红。
    //
    // ADR 0025：不合成任何操作系统级事件。窗口尺寸经
    // `AppHandler::on_resize`（winit 真实事件的落点）交给 `Demo`，
    // 鼠标经 `InputState` 那几个既有公开 setter——两者都是真实鼠标
    // 最终也要走的同一条路径。
    // Arrange
    let mut demo = test_demo_at_title();
    demo.on_resize(PhysicalSize::new(1280, 720));
    let rects = 首页行矩形(&demo, (1280.0, 720.0));
    let 设置行 = rects[2];
    let mut input = InputState::new();
    input.set_cursor_position((
        设置行.x + 设置行.width / 2.0,
        设置行.y + 设置行.height / 2.0,
    ));

    // Act：在那一行上按下再松开。
    input.mouse_press(ll_platform::input::MouseButton::Left);
    input.mouse_release(ll_platform::input::MouseButton::Left);
    demo.on_frame(FrameId(0), &mut input);

    // Assert
    assert!(
        matches!(demo.modal.screen(), Some(ScreenState::Settings { .. })),
        "点第 3 行「设置」应当进设置屏，实际是 {:?}",
        demo.modal.screen()
    );
}

#[test]
fn 在首页面板外的空白上点一下什么都不发生() {
    // 约定二：点空白不改焦点、不触发、**不关屏**。
    // Arrange
    let mut demo = test_demo_at_title();
    demo.on_resize(PhysicalSize::new(1280, 720));
    let 焦点前 = crate::title_screen::title_focus_index(&demo.screen_focus);
    let mut input = InputState::new();
    input.set_cursor_position((5.0, 5.0));

    // Act
    input.mouse_press(ll_platform::input::MouseButton::Left);
    input.mouse_release(ll_platform::input::MouseButton::Left);
    demo.on_frame(FrameId(0), &mut input);

    // Assert
    assert_eq!(
        demo.modal.screen(),
        Some(ScreenState::Title),
        "屏不该被关掉"
    );
    assert_eq!(
        crate::title_screen::title_focus_index(&demo.screen_focus),
        焦点前,
        "焦点也不该动"
    );
}

#[test]
fn 窗口尺寸还没到手时鼠标一律不生效() {
    // `viewport` 为 `None` 的降级：没有窗口就没有窗口坐标可言。少了
    // 这条闸门，行矩形会按一个瞎猜的尺寸算出来，点击落到别的行上。
    // Arrange：**不调** `on_resize`。
    let mut demo = test_demo_at_title();
    let mut input = InputState::new();
    input.set_cursor_position((640.0, 360.0));

    // Act
    input.mouse_press(ll_platform::input::MouseButton::Left);
    input.mouse_release(ll_platform::input::MouseButton::Left);
    demo.on_frame(FrameId(0), &mut input);

    // Assert
    assert_eq!(demo.modal.screen(), Some(ScreenState::Title));
}

#[test]
fn 指针悬停记进跨帧状态但不改焦点() {
    // 约定一在整条链路上的验收：`Demo` 把悬停行记下来（渲染侧画那块
    // 淡高亮要用），而焦点一动不动。
    // Arrange
    let mut demo = test_demo_at_title();
    demo.on_resize(PhysicalSize::new(1280, 720));
    走一帧(&mut demo, 0, &[GameKey::Down]);
    let 焦点前 = crate::title_screen::title_focus_index(&demo.screen_focus);
    let rects = 首页行矩形(&demo, (1280.0, 720.0));
    let 第三行 = rects[3];
    let mut input = InputState::new();
    input.set_cursor_position((
        第三行.x + 第三行.width / 2.0,
        第三行.y + 第三行.height / 2.0,
    ));

    // Act：只移动，不按键。
    demo.on_frame(FrameId(1), &mut input);

    // Assert
    assert_eq!(demo.pointer.hovered_row(), Some(3), "悬停行要记下来");
    assert_eq!(
        crate::title_screen::title_focus_index(&demo.screen_focus),
        焦点前,
        "但焦点不跟着指针走"
    );
}

#[test]
fn 交互列表按取消退回方向列表而不是一次关到底() {
    // 规格 N7 / D4：方向列表 → 物品列表是**两级**，此前取消键一律
    // `Closed`，玩家发现选错格子想退回去重选，整个菜单没了。所有者
    // 已经裁定的「Esc 逐层往回退」在这里没有被兑现。
    //
    // **此前这条会红**。反例验证（已实跑）：把
    // `crate::player_action` 里那句 `cancelled_menu(...)` 换回
    // 无条件 `PlayerMenu::Closed`，本条立刻变红。
    // Arrange：脚下与正东各一堆 → 方向列表 → 选正东那一行进物品列表。
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
    frame(&mut demo, 0, &[GameKey::Interact]);
    // 方向列表里往下挪一格，选中的不再是第 0 行——这样「光标停在
    // 刚才选的那一格」这条才真的被验到，而不是被默认值 0 蒙混过去。
    frame(&mut demo, 1, &[GameKey::Down]);
    let 方向光标 = match demo.modal.player_menu() {
        PlayerMenu::InteractDirection { cursor } => cursor,
        other => panic!("Arrange：应当停在方向列表，实际是 {other:?}"),
    };
    assert_ne!(方向光标, 0, "Arrange：光标应当已经离开第 0 行");
    frame(&mut demo, 2, &[GameKey::Confirm]);
    assert!(
        matches!(demo.modal.player_menu(), PlayerMenu::Interact { .. }),
        "Arrange：应当已经进了物品列表"
    );

    // Act
    frame(&mut demo, 3, &[GameKey::Cancel]);

    // Assert
    assert_eq!(
        demo.modal.player_menu(),
        PlayerMenu::InteractDirection {
            cursor: 方向光标
        },
        "取消应当退回方向列表，且光标停在刚才选的那一格"
    );
    assert_eq!(demo.modal.depth(), 1, "退了一层，但玩家菜单那一层还在");

    // 再按一次才关到底——「逐层往回退」。
    frame(&mut demo, 4, &[GameKey::Cancel]);
    assert_eq!(demo.modal.player_menu(), PlayerMenu::Closed);
    assert!(demo.modal.is_empty());
}

#[test]
fn 只有一格有东西时进的物品列表按取消直接关到底() {
    // N7 的另一半：那条路**跳过**了方向列表，物品列表的上一层就是
    // 世界，一次关到底才**是**退一层。少了这条，把
    // `from_direction` 恒设成 `true` 也不会有任何东西变红。
    // Arrange
    let mut demo = test_demo();
    let fixture = arrange_smith(&mut demo);
    put_on_ground(&mut demo, fixture.iron_ingot, false);
    frame(&mut demo, 0, &[GameKey::Interact]);
    assert!(
        matches!(demo.modal.player_menu(), PlayerMenu::Interact { .. }),
        "Arrange：只有一格有东西时应当直接进物品列表"
    );

    // Act
    frame(&mut demo, 1, &[GameKey::Cancel]);

    // Assert
    assert_eq!(demo.modal.player_menu(), PlayerMenu::Closed);
    assert!(demo.modal.is_empty());
}

#[test]
fn 设置屏取值行按确认一个字段都不改() {
    // 规格 N1：确认键永远只做一件事——激活当前焦点。设置屏此前把
    // 「按确认 = 把这个值往前拨一档」当作左右键的冗余路径，与角色
    // 创建 / 世界配置那两块屏（取值行按确认刻意是空操作）直接冲突。
    //
    // **此前这条会红**。反例验证（已实跑）：把
    // `crate::menu_screen::update_navigation` 里那三个取值行的
    // 分支改回 `adjust_value(other, ctx, true)`，本条立刻变红。
    //
    // 断言的是**整个 `config` 逐字段不变**，不是只盯语言那一项：
    // 只盯一项的话，把 `adjust_value` 改成只跳过语言就还是绿的。
    // Arrange：进设置屏，光标挪到**垂直同步**那一行。
    //
    // 刻意不用第 0 行「语言」：测试用的 `Catalog` 是空目录，
    // `cycle_language` 因此本来就是空操作——拿它当判据会得到一条
    // 无论实现怎么改都绿的假断言。垂直同步是一个无条件翻转的布尔，
    // 改没改一眼看得出。
    let mut demo = test_demo();
    let mut at = 开到设置屏(&mut demo);
    let vsync_row = crate::menu_screen::settings_rows()
        .iter()
        .position(|row| *row == crate::menu_screen::SettingsRow::Vsync)
        .expect("设置屏必有垂直同步这一行");
    for _ in 0..vsync_row {
        走一帧(&mut demo, at, &[GameKey::Down]);
        at += 1;
    }
    // `GameConfig` 没有 `PartialEq`（它是一棵配置树，加派生要牵动
    // `ll-platform` 里一串类型）。用 `Debug` 串比对是**逐字段**的
    // ——派生的 `Debug` 会把每一个字段都打出来，漏改一个字段都会
    // 让串不同，正是这条判据要的。
    let 改前 = format!("{:?}", demo.config);

    // Act：在取值行上按确认。
    走一帧(&mut demo, at, &[GameKey::Confirm]);

    // Assert
    assert_eq!(
        format!("{:?}", demo.config),
        改前,
        "确认键不该改动任何配置字段"
    );
    assert!(
        matches!(
            demo.modal.screen(),
            Some(ScreenState::Settings {
                capturing: false,
                ..
            })
        ),
        "也不该进捕获模式"
    );
}

#[test]
fn 设置屏取值行按左右键仍然改得动() {
    // N1 的反面：删掉确认那条路之后，左右键必须还在——否则「统一
    // 到左右键」就成了「这一行没法改了」。
    // Arrange
    let mut demo = test_demo();
    let mut at = 开到设置屏(&mut demo);
    let vsync_row = crate::menu_screen::settings_rows()
        .iter()
        .position(|row| *row == crate::menu_screen::SettingsRow::Vsync)
        .expect("设置屏必有垂直同步这一行");
    for _ in 0..vsync_row {
        走一帧(&mut demo, at, &[GameKey::Down]);
        at += 1;
    }
    let 改前 = format!("{:?}", demo.config);

    // Act
    走一帧(&mut demo, at, &[GameKey::Right]);

    // Assert
    assert_ne!(
        format!("{:?}", demo.config),
        改前,
        "左右键应当仍然把取值往前拨一档"
    );
}

#[test]
fn 首页不按任何方向键直接按确认就能进角色创建屏() {
    // 规格 N10 / 所有者 2026-08-29 裁定（交接文档第〇之二节第 1
    // 条）：**标题屏预选第一项**。此前首页刻意不预置焦点，新玩家
    // 进游戏第一眼是首页、第一反应按 Enter，**什么都不发生**。
    //
    // **此前这条会红**。反例验证（已实跑）：把 `Demo::new` 里
    // `screen_focus` 那一支换回 `WidgetStateTable::new()`，本条
    // 立刻变红（屏仍停在 Title）。
    // Arrange：一个字都不按。
    let mut demo = test_demo_at_title();

    // Act
    走一帧(&mut demo, 0, &[GameKey::Confirm]);

    // Assert
    assert_eq!(
        demo.modal.screen(),
        Some(ScreenState::CharacterCreation { cursor: 0 }),
        "首页第一项应当已经预先选中"
    );
}

#[test]
fn 暂停菜单一开就预选第一项按确认直接回到世界() {
    // N10 覆盖的是**所有**列表，不只首页。暂停菜单第一项是「继续」。
    //
    // 反例验证（已实跑）：把 `Demo::open_menu` 里
    // `preselected_focus(&ids)` 换回 `WidgetStateTable::new()`，
    // 本条变红（屏仍是 Some(Menu)）。
    // Arrange
    let mut demo = test_demo();
    走一帧(&mut demo, 0, &[GameKey::Menu]);
    assert_eq!(demo.modal.screen(), Some(ScreenState::Menu));

    // Act：不按方向键，直接确认。
    走一帧(&mut demo, 1, &[GameKey::Confirm]);

    // Assert
    assert_eq!(demo.modal.screen(), None, "第一项「继续」应当已经预先选中");
    assert!(demo.modal.is_empty());
}
