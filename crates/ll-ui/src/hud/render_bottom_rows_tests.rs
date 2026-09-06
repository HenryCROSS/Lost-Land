//! 屏幕底部那三行（反馈行 / 按键提示行 / 自动存档痕迹）在
//! `build_hud_frame` 产出里的断言——规格 F6 与 F3。
//!
//! 用 `#[path]` 挂成 `super::render::tests` 的子模块，理由见挂载点的
//! 注释：本条要用那一套夹具，而 `render.rs` 在行数棘轮的快照里。

use super::*;

#[test]
fn 按键提示行落在常驻hud层且给这一层多加一块面板一行字() {
    // 规格 F6 判据：`build_hud_frame` 产出的 `Hud` 层里有这一行。
    // 用「有 / 没有」两帧相减，而不是断言一个绝对数字——绝对数字会
    // 在任何一块面板加一行时无关地变红。
    //
    // 反例验证（已实跑）：把 `build_hud_frame` 里那段
    // `if let Some(text) = key_hint` 删掉，本条当场红——两帧的
    // 面板数与行数完全相同。
    // Arrange
    let dir = temp_dir("key-hint-row");
    write_fixture_catalog(&dir);
    let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
    let status = StatusBarData {
        clock: Tick(0),
        health: 100,
        mana: 50,
        fps: 0.0,
        weather_display_name_key: None,
    };
    let modifiers = BTreeMap::new();
    let equipment = BTreeMap::new();
    let character = sample_character_data(&modifiers, &equipment);
    let item_table = ItemTable::new();
    let mut anim = WidgetStateTable::new();

    // Act
    let 建 = |anim: &mut WidgetStateTable, now, key_hint| {
        build_hud_frame(
            &status,
            &character,
            &[],
            &equipment,
            &[],
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            anim,
            now,
            1280.0,
            720.0,
            None,
            None,
            None,
            key_hint,
            None,
        )
    };
    let 没有提示 = 建(&mut anim, 0, None);
    let 有提示 = 建(&mut anim, 1, Some("I 背包　C 制作"));

    // Assert：多出来的恰好是一块面板背景（九宫格 4 块矩形）与一行字，
    // 而且全部落在 `Hud` 那一层——不是 `Notice`（那是反馈行的层）。
    let hud_labels = |frame: &LayeredFrame| frame.layer(UiLayer::Hud).labels.len();
    let hud_quads = |frame: &LayeredFrame| frame.layer(UiLayer::Hud).quads.len();
    assert_eq!(
        hud_labels(&有提示) - hud_labels(&没有提示),
        1,
        "常驻 HUD 层应当多出提示那一行字"
    );
    assert_eq!(
        hud_quads(&有提示) - hud_quads(&没有提示),
        9,
        "以及它那一块面板背景（九宫格 = 9 块矩形，见 `widget::panel::panel_quads`）"
    );
    assert!(
        有提示.layer(UiLayer::Notice).labels.is_empty(),
        "提示行不该跑到反馈行那一层去——两者分层的理由见 `super::bottom_rows`"
    );
    assert!(
        有提示
            .layer(UiLayer::Hud)
            .labels
            .iter()
            .any(|label| label.text == "I 背包　C 制作"),
        "那一行字必须真的是传进去的那一句"
    );
}

#[test]
fn 自动存档痕迹落在通告层且叠在反馈行上面一格() {
    // **规格 F3 在渲染侧的判据**：那条痕迹真的进了 `build_hud_frame`
    // 的产出，落在 `Notice` 层（不是常驻 `Hud` 层——它是一次性通告，
    // 说完自己消失），而且叠在反馈行**上面**一格，不把反馈行挤走。
    //
    // 同样用「有 / 没有」两帧相减，不断言绝对数字。
    //
    // 反例验证（已实跑）：把 `build_hud_frame` 里那段
    // `if let Some(text) = autosave` 删掉，本条红在「通告层应当多出
    // 痕迹那一行字」。另一条（已实跑）：把 `AUTOSAVE_BOTTOM_MARGIN`
    // 改成与 `FEEDBACK_BOTTOM_MARGIN` 相同，红在「痕迹应当叠在反馈行
    // 上面」。
    // Arrange
    let dir = temp_dir("autosave-row");
    write_fixture_catalog(&dir);
    let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
    let status = StatusBarData {
        clock: Tick(0),
        health: 100,
        mana: 50,
        fps: 0.0,
        weather_display_name_key: None,
    };
    let modifiers = BTreeMap::new();
    let equipment = BTreeMap::new();
    let character = sample_character_data(&modifiers, &equipment);
    let item_table = ItemTable::new();
    let mut anim = WidgetStateTable::new();

    // Act
    let 建 = |anim: &mut WidgetStateTable, now, autosave| {
        build_hud_frame(
            &status,
            &character,
            &[],
            &equipment,
            &[],
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            anim,
            now,
            1280.0,
            720.0,
            None,
            None,
            // 反馈行同时开着：这两行必须能共存，不是互相顶掉。
            Some("这一下没起作用"),
            None,
            autosave,
        )
    };
    let 没有痕迹 = 建(&mut anim, 0, None);
    let 有痕迹 = 建(&mut anim, 1, Some("已自动保存"));

    // Assert：先证明两行都在（否则下面比 y 是在比空气）。
    let notice_labels = |frame: &LayeredFrame| frame.layer(UiLayer::Notice).labels.len();
    assert_eq!(
        notice_labels(&有痕迹) - notice_labels(&没有痕迹),
        1,
        "通告层应当多出痕迹那一行字"
    );
    assert!(
        有痕迹
            .layer(UiLayer::Hud)
            .labels
            .iter()
            .all(|label| label.text != "已自动保存"),
        "痕迹不该跑到常驻 HUD 层去——它是一次性通告，见 `super::bottom_rows`"
    );
    let 找 = |frame: &LayeredFrame, text: &str| {
        frame
            .layer(UiLayer::Notice)
            .labels
            .iter()
            .find(|label| label.text == text)
            .unwrap_or_else(|| panic!("通告层里应当有「{text}」这一行"))
            .y
    };
    let 痕迹y = 找(&有痕迹, "已自动保存");
    let 反馈y = 找(&有痕迹, "这一下没起作用");
    assert!(
        痕迹y < 反馈y,
        "痕迹应当叠在反馈行上面（y 更小），实际 痕迹={痕迹y} 反馈={反馈y}"
    );
    // 反馈行自己一格没挪：新加的那一行不许把玩家正等着看的那句话推走。
    assert_eq!(
        找(&没有痕迹, "这一下没起作用"),
        反馈y,
        "加了痕迹之后反馈行不该跟着挪位置"
    );
}
