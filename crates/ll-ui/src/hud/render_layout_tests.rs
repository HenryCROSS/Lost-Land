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
