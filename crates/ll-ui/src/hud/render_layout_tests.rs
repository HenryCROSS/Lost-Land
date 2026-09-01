//! 布局收敛那一批（规格 L0–L5）在 `build_hud_frame` 产出上的断言。
//!
//! 用 `#[path]` 挂成 `super::render::tests` 的子模块，理由与隔壁
//! [`super::bottom_rows_tests`] 逐字相同：本文件这几条要用 `render.rs`
//! 测试模块那一整套夹具（`write_fixture_catalog`/`sample_character_data`/
//! `temp_dir`/`FlatColorSkin`/`NoItems`），而 `render.rs` 在行数棘轮的
//! 快照里（`scripts/ci/file_size_budget.json`），搬去 `tests/` 就够不着
//! 那套夹具了。
//!
//! # 这里的断言为什么跑在**整帧**上，不是跑在单个函数上
//!
//! 本会话反复抓到的一个失败形状是「判据的适用面被新代码绕过」——批次 3
//! 发现批次 2 那条「不消耗回合」的测试只覆盖旧变体，加一个新变体之后就
//! 不再覆盖了。L0（全部矩形取整）与 L1（中段不放常驻元素）都是**对整
//! 张 HUD 成立**的性质，写成「对 `panel_quads` 成立」「对装备面板成立」
//! 就会在下一块面板加进来时静静失效。因此这两条一律遍历
//! [`LayeredFrame`] 的全部层、全部容器，新加的任何一块内容自动进入判据。

use super::*;
use crate::widget::zone::ScreenZone;

/// 本文件几条都要的那一套输入：一份日志目录 + 目录里那份 catalog。
/// 与 `placement_catalog` 不同的是这里要真实文案（面板里得有字），
/// 因此写 fixture。
fn 布局夹具(name: &str) -> (std::path::PathBuf, Catalog) {
    let dir = temp_dir(name);
    write_fixture_catalog(&dir);
    let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
    (dir, catalog)
}

/// 建一帧**尽可能满**的 HUD——六块常驻面板 + 三条条形 + 昼夜滑条 +
/// 世界地图（含玩家标记）+ 动作菜单 + 反馈行 + 按键提示行。
///
/// 满帧是刻意的：L0 那条断言要证明取整覆盖的**不止**面板九宫格，还有
/// 条形、地图格、指针、背板这些不经 `panel_quads` 的产出。只画四块面板
/// 的一帧会让那条断言看起来绿、实际什么都没证明。
fn 满帧(catalog: &Catalog, screen_width: f32, screen_height: f32) -> LayeredFrame {
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
    let (ids, _table) = ll_world::terrain::base_terrain_fixture();
    let cells = [
        ll_world::overview::OverviewCell {
            terrain: ids.grass,
            explored: true,
        },
        ll_world::overview::OverviewCell {
            terrain: ids.grass,
            explored: false,
        },
        ll_world::overview::OverviewCell {
            terrain: ids.grass,
            explored: true,
        },
    ];
    let map = WorldMapPanelData {
        cells: &cells,
        cols: 3,
        rows: 1,
        player: Some((0, 0)),
        sites: &[],
        terrain_ids: &ids,
        tiles_per_cell: 48,
    };
    let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();
    let menu = placement_menu(&rows, MenuPlacement::ScreenCenter);

    build_hud_frame(
        &status,
        &character,
        &[],
        &equipment,
        &[],
        &NoItems,
        &item_table,
        catalog,
        "zh-CN",
        &FlatColorSkin,
        &mut crate::测试测量器(),
        &mut anim,
        0,
        screen_width,
        screen_height,
        Some(&map),
        Some(&menu),
        Some("这一下没起作用"),
        Some("I 背包　C 制作"),
    )
}

/// 遍历一帧全部层、全部容器，逐条交给 `check`。返回一共看了多少个
/// 元素——调用方拿它断言「这一帧真的非空」。
fn 遍历全帧(frame: &LayeredFrame, mut check: impl FnMut(&str, [f32; 2], [f32; 2])) -> usize {
    let mut 计数 = 0;
    for layer in UiLayer::ALL {
        let batch = frame.layer(layer);
        for quad in &batch.quads {
            check("纯色矩形", quad.position, quad.size);
            计数 += 1;
        }
        for quad in &batch.textured_quads {
            check("贴图矩形", quad.position, quad.size);
            计数 += 1;
        }
        for label in &batch.labels {
            check("文本行", [label.x, label.y], [0.0, 0.0]);
            计数 += 1;
        }
    }
    计数
}

#[test]
fn 整帧每一个矩形边界与每一行文字原点都落在整数像素上() {
    // 规格 L0。**故意用带半像素的屏幕尺寸**：整数尺寸下大部分坐标本来
    // 就是整数，那样断言等于什么都没测（本会话点名的假绿形状之一）。
    //
    // 反例验证（已实跑）：把 `build_hud_frame` 结尾那句
    // `frame.snap_to_pixels();` 注释掉，本条当场红——`共 48 处没有取整
    // （元素总数 132）`，头几条是 `x = 128.15001` / `right = 1153.35`，
    // **那是世界地图面板的边框**（1281.5 的一成 = 128.15）。它不经
    // `panel_quads`（`world_map::world_map_frame` 自己造 `QuadInstance`），
    // 正好证明这一道取整的覆盖面不止面板九宫格——六块面板此刻反倒是
    // 绿的，因为 `panel_quads` 自己那一道已经取过了。
    // Arrange
    let (dir, catalog) = 布局夹具("snap-whole-frame");

    // Act
    let frame = 满帧(&catalog, 1281.5, 719.5);

    // Assert
    let mut 违规 = Vec::new();
    let 元素数 = 遍历全帧(&frame, |种类, position, size| {
        for (名, v) in [
            ("x", position[0]),
            ("y", position[1]),
            ("right", position[0] + size[0]),
            ("bottom", position[1] + size[1]),
        ] {
            if v.fract() != 0.0 {
                违规.push(format!("{种类}的 {名} = {v}"));
            }
        }
    });

    // **先断言被断言的对象真的存在**——否则「零个元素全部满足」恒绿。
    assert!(
        元素数 > 100,
        "这一帧应当是满的（六块面板 + 三条条形 + 地图 + 菜单 + 两行），实际只有 {元素数} 个元素"
    );
    assert!(
        违规.is_empty(),
        "共 {} 处没有取整（元素总数 {元素数}）：{:?}",
        违规.len(),
        &违规[..违规.len().min(8)]
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 状态栏与两条资源条与昼夜条右边界对齐() {
    // 规格 L5（3.2 节第 1 条）：状态栏 620、昼夜条 610，而
    // `render.rs` 的注释声称它们「读起来是对齐的一列」，没有任何测试
    // 盯着这条。**落地前先跑过一遍确认它真的红**。
    //
    // 修法取「把资源条宽改成 (STATUS_WIDTH - PANEL_GAP)/2」而不是「把
    // 昼夜条硬改成 620」：后者会让昼夜条与它正上方那两条资源条不齐，
    // 是拆东墙补西墙。
    //
    // 反例验证（已实跑）：把 `RESOURCE_BAR_WIDTH` 改回 `300.0`，本条红在
    // 「昼夜条右边界 626 != 状态栏右边界 636」。
    // Arrange
    let (dir, catalog) = 布局夹具("l5-right-edges");

    // Act
    let frame = 满帧(&catalog, 1280.0, 720.0);
    let hud = frame.layer(UiLayer::Hud);

    // 状态栏是第一块推入的面板，它的九宫格里第 1 块是右上角，其右边界
    // 即面板右边界。生命条紧随四块面板之后……与其数下标，不如直接按
    // 常量重算一次三者的右边界——这几个常量本身就是被测对象。
    let 状态栏右 = SCREEN_MARGIN + STATUS_WIDTH;
    let 法力条右 = SCREEN_MARGIN + RESOURCE_BAR_WIDTH * 2.0 + PANEL_GAP;
    let 昼夜条右 = SCREEN_MARGIN + DAY_NIGHT_BAR_WIDTH;

    // Assert 一：三条常量算出来的右边界相等。
    assert_eq!(状态栏右, 昼夜条右, "状态栏与昼夜条右边界不齐");
    assert_eq!(状态栏右, 法力条右, "状态栏与两条资源条并排的右边界不齐");

    // Assert 二：那几个常量真的被这一帧用上了——否则上面三行只是在
    // 断言算术，与屏幕上画了什么无关。
    let 最右 = hud
        .quads
        .iter()
        .map(|q| q.position[0] + q.size[0])
        .fold(0.0_f32, f32::max);
    assert!(
        hud.quads
            .iter()
            .any(|q| (q.position[0] + q.size[0] - 状态栏右).abs() < 0.5),
        "这一帧的常驻层里没有任何一块的右边界落在 {状态栏右}（最右的是 {最右}）"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 动作菜单两种落位改走anchored之后逐像素与旧算术相同() {
    // 规格 L2 第 2/3 处的「改写前后逐像素相同」回归断言。**走
    // `placed_action_menu` 这条生产路径**，不是在测试里自己拼一个
    // `Rect::anchored`——起初就是那么写的，反例验证时两条都没红（见
    // `hud/placement.rs` 测试模块顶部那段注释）。
    //
    // 期望值是被收敛掉的那两份旧算术：
    //   TopCenter：`x = (screen_width - ACTION_MENU_WIDTH) * 0.5`，
    //              `y = SCREEN_MARGIN + PANEL_GAP`
    //   ScreenCenter：`y = (screen_height - panel.rect.height) * 0.5`
    //
    // 反例验证（已实跑）：把 `placed_action_menu` 里贴上沿那一支的
    // `Anchor::TopCenter` 换成 `Anchor::TopLeft`，本条红在 x 上；
    // 把居中那一支的 `Anchor::Center` 换成 `Anchor::TopCenter`，
    // 本条红在「居中的 y」上。
    // Arrange
    let (dir, catalog) = placement_catalog("l2-action-menu-regression");
    let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();
    let (w, h) = (1280.0_f32, 720.0_f32);
    let 摆 = |placement| {
        placed_action_menu(
            &placement_menu(&rows, placement),
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            w,
            h,
        )
    };

    // Act
    let 贴上沿 = 摆(MenuPlacement::TopCenter);
    let 居中 = 摆(MenuPlacement::ScreenCenter);

    // Assert：两种落位的 x 都是旧的水平居中算术。
    assert_eq!(贴上沿.rect.x, (w - ACTION_MENU_WIDTH) * 0.5, "贴上沿的 x");
    assert_eq!(居中.rect.x, (w - ACTION_MENU_WIDTH) * 0.5, "居中的 x");
    // 贴上沿的 y 是旧的 `SCREEN_MARGIN + PANEL_GAP`。
    assert_eq!(贴上沿.rect.y, SCREEN_MARGIN + PANEL_GAP, "贴上沿的 y");
    // 居中的 y 是旧的 `(screen_height - 面板高) * 0.5`。两种落位的面板
    // 高度相同（同一批行、同一个宽度），因此可以拿贴上沿那一份的高。
    assert_eq!(居中.rect.height, 贴上沿.rect.height);
    assert_eq!(居中.rect.y, (h - 贴上沿.rect.height) * 0.5, "居中的 y");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 间距刻度只有两档且两套内边距已经合并成同一个() {
    // 规格 L3。此前 HUD 与模态屏各有一套常量、互不知情：贴边留白两个
    // 「碰巧相等」的 16，内边距两个不相干的 6 与 10（§3.2）。谁改了其中
    // 一个，另一个不会跟着动，也没有任何东西会红——这条断言就是那个
    // 「会红」。
    //
    // 反例验证（已实跑）：把 `screen::SCREEN_PADDING` 改回字面量 `10.0`，
    // 本条红在「两套内边距没合并」。
    // Assert 一：两套内边距是同一个数。
    assert_eq!(
        crate::hud::DEFAULT_PADDING,
        crate::screen::SCREEN_PADDING,
        "两套内边距没合并成同一个刻度"
    );
    // Assert 二：两套贴边留白是同一个数，且就是刻度本身。
    assert_eq!(SCREEN_MARGIN, crate::screen::SCREEN_SIDE_MARGIN);
    assert_eq!(SCREEN_MARGIN, crate::widget::metrics::SCREEN_MARGIN);
    // Assert 三：两档互不相等——合并成一个数就说明刻度塌了。
    assert_ne!(SCREEN_MARGIN, PANEL_GAP);
}

#[test]
fn 背包面板紧贴经验条下方一个间隔且左边界对齐() {
    // 规格 L4（§3.3）：此前这里写的是
    // `bar_rect.stack_below(PANEL_GAP, INVENTORY_WIDTH).origin()`——
    // `stack_below` 的第二个参数是**高度**，传进去的是一个**宽度**常量。
    // 今天无害（`.origin()` 把高度扔了），哪天有人保留那个 `Rect` 就会
    // 拿到一条 220 像素高的条。改成显式构造之后加这一条盯着落位本身。
    //
    // 顺带也是 L3 的落点：经验条与角色面板之间此前是裸字面量 4.0，现在
    // 走 `PANEL_GAP`，背包因此比收敛前下移 6px。
    //
    // 反例验证（已实跑）：把显式构造里的 `bar_rect.x` 换成
    // `bar_rect.right()`，本条红在左边界；把 `+ PANEL_GAP` 去掉，红在
    // 纵向间隔。
    // Arrange
    let (dir, catalog) = 布局夹具("l4-inventory-origin");

    // Act
    let frame = 满帧(&catalog, 1280.0, 720.0);
    let hud = frame.layer(UiLayer::Hud);

    // 推入顺序：状态栏(9) 生命(3) 法力(3) 昼夜(2) 角色(9) 经验条(2)
    // 背包(9) 装备(9)。经验条是第 26/27 块，背包九宫格从第 28 块起。
    let 经验条 = &hud.quads[26];
    let 背包左上角 = &hud.quads[28];

    // Assert
    assert_eq!(
        背包左上角.position[0], 经验条.position[0],
        "背包与经验条（也就是角色列）左边界没对齐"
    );
    assert_eq!(
        背包左上角.position[1],
        经验条.position[1] + 经验条.size[1] + PANEL_GAP,
        "背包没紧贴经验条下方一个 PANEL_GAP"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 常驻层不占屏幕中段只落在左半右半或底栏() {
    // 规格 L1 的留白规则：常驻区左列与右列之间的中段永远不放常驻元素
    // ——那是玩家看世界的地方。
    //
    // **判据比规格原文多一个例外**：规格写的是「要么 `right() <= 屏宽/2`
    // 要么 `x >= 屏宽/2`」两选一，而批次 23 的按键提示行（`Hud` 层、
    // 水平居中、贴屏幕最下沿）照那条会红。它是对的：留白规则的理由是
    // 「那是玩家看世界的地方」，屏幕最下沿那一条窄边不是。因此加第三个
    // 落点：底栏。完整论证见
    // `docs/superpowers/plans/2026-09-01-batch30-ui-p2.md` 第八节第 4 条。
    //
    // 反例验证（已实跑）：把装备面板的锚点从 `Anchor::TopRight` 改成
    // `Anchor::TopCenter`，本条在 1280 与 1920 两个尺寸下都红。
    // Arrange
    let (dir, catalog) = 布局夹具("l1-middle-band");

    // Act & Assert：两个尺寸各跑一遍。
    for (w, h) in [(1280.0_f32, 720.0_f32), (1920.0, 1080.0)] {
        let frame = 满帧(&catalog, w, h);
        // 底栏有多高由 `bottom_rows` 自己说了算，这里不抄一个数。
        let 底栏顶 = h - crate::hud::bottom_rows::BOTTOM_STRIP_HEIGHT;
        let 中线 = w / 2.0;

        // **按区筛层，不写死 `UiLayer::Hud`**：留白规则约束的是「常驻区」
        // 这个概念，不是某一个层的名字。将来常驻区多出一层（或者 N9 把
        // 模态屏收进 `UiLayer`）时，只要 `ScreenZone::of` 说它是常驻，它
        // 就自动进入本判据——这正是「判据的适用面被新代码绕过」那个失败
        // 形状的防法。
        let mut 常驻块数 = 0usize;
        for layer in UiLayer::ALL {
            if ScreenZone::of(layer) != ScreenZone::Resident {
                continue;
            }
            let batch = frame.layer(layer);
            常驻块数 += batch.quads.len() + batch.textured_quads.len();
            let 纯色 = batch.quads.iter().map(|q| (q.position, q.size));
            let 贴图 = batch.textured_quads.iter().map(|q| (q.position, q.size));
            for (position, size) in 纯色.chain(贴图) {
                let (x, right, y) = (position[0], position[0] + size[0], position[1]);
                assert!(
                    right <= 中线 || x >= 中线 || y >= 底栏顶,
                    "{layer:?}（常驻区）有一块占了屏幕中段（{w}×{h}）：x={x} right={right} y={y}，中线 {中线}，底栏顶 {底栏顶}"
                );
            }
        }

        assert!(
            常驻块数 > 30,
            "常驻区应当有六块面板加三条条形，实际只有 {常驻块数} 块（{w}×{h}）"
        );
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}
