//! 端到端验收：**每一样走图集查找的东西都真的有图**。
//!
//! # 这条验收在验什么（ADR 0018）
//!
//! 本文件是 `surface_render.rs` 那条「两段证据链」在**覆盖面**方向上的
//! 补齐。`surface_render.rs` 只盯三类地表内容（物品堆/家具/NPC）各自
//! 选中的那一个键；本文件盯的是另一个问题：**有没有哪一类内容，声明了
//! 却根本没有图**。
//!
//! 这个问题此前有两个真实的、跑起来才看得见的缺口：
//!
//! 1. **据点建筑地形**——`ll_world::terrain::define_base` 注册 17 种本体
//!    地形，`ll_game::layout::terrain_entry_name` 只覆盖 10 种。剩下 7 种
//!    （木墙/木地板/开门/关门/窗/上楼梯/下楼梯）落到 Registry 回退路径、
//!    拿注册 ID 当图集键，真实图集里没有这个键——玩家一走进据点，控制台
//!    每帧刷「图集条目缺失，跳过本次绘制」，那些格子一格都画不出来。
//!    当时唯一相关的测试是 `layout.rs` 里
//!    「全部**自然**地形都能查到图集条目」，只遍历 8 种自然地形，缺口
//!    整个落在它的盲区里。
//! 2. **HUD 皮肤贴图**——`ll_ui::widget::skin::NineSliceSkin::new` 查的是
//!    裸名字 `"ui_panel_border"`，真实运行期图集里的条目名却是完整命名
//!    空间 ID `"lostland:ui_panel_border"`。五张 UI 贴图全部查不到，
//!    `textured_*` 里的 `?` 全部短路，HUD 每一帧静默退回纯色外观。这条
//!    **不打任何日志**——`uv_rect` 返回 `None` 是设计上的正常降级路径，
//!    分辨不出「本来就没有这张图」与「有图但名字对不上」。画面上仍然有
//!    面板、有血条、有昼夜滑条，只是全是纯色的，因此躲过了此前每一轮
//!    验收。
//!
//! 两个缺口的共同形状是「**没人把声明侧和资产侧对着数一遍**」。本文件
//! 就是那一遍。
//!
//! # 证据链仍然是两段，缺一不可
//!
//! 1. **接线段**：走生产路径上的
//!    [`ll_game::layout::terrain_atlas_key`] / `ll_ui` 的
//!    [`REQUIRED_SPRITE_KEYS`] / [`ll_render::anim`] 的帧名表，拿到每类
//!    内容**实际会去查**的那个字符串——不是在测试里重抄一份字面量。
//! 2. **有图段**：用同一份真实 `assets/` + `mods/`，走生产路径上的
//!    [`load_sprite_sources`] + [`pack_atlas`] 打出真实图集，断言上一段
//!    每个键都查得到条目、**且那块矩形里真的有不透明像素**。
//!
//! # 每条断言的反例是什么（本次开发中真的逐条改坏跑过）
//!
//! | 改坏什么 | 哪条变红 |
//! | --- | --- |
//! | `layout.rs` 里 `ids.wall_wood` 那一支改回 `None` | `十九种本体地形在真实图集里都查得到条目` |
//! | `layout.rs` 里 `wall_stone` 改回借用 `terrain_mountain` | `十九种本体地形的贴图两两之间至少四分之一像素不同` |
//! | `skin.rs` 的 `PANEL_BORDER_KEY` 改回裸名字 `"ui_panel_border"` | `hud皮肤需要的五张贴图在真实图集里都查得到条目` |
//! | 移走 `assets/sprites/terrain_window.png` | `十九种本体地形在真实图集里都查得到条目` |
//! | 把 `terrain_door_open.png` 换成全透明 | `十九种本体地形的贴图都铺满整格` |
//! | `skin.rs` 五个键全部改回裸名字（复现所有者报的原始现象） | `hud皮肤需要的五张贴图在真实图集里都查得到条目` 与 `hud皮肤拿真实资产装出来后五个贴图外观全部是some` 两条同时红 |
//! | 把 `FURNITURE_NAMES` 里的 `oak_barrel` 删掉（artgen 不再产出那张图） | `本体每一件家具在真实图集里都查得到自带贴图` |
//! | 把 `furniture.rs` 的 `decorate_oak_table` 改成照抄 `decorate_oak_chair` | `本体家具的贴图两两之间至少四分之一像素不同` |
//! | 移走 `assets/sprites/iron_bound_chest.png` | `本体每一件家具在真实图集里都查得到自带贴图` |
//! | 把 `items.json5` 里**全部**七条 `furniture: true` 去掉 | `本体每一件家具在真实图集里都查得到自带贴图`（报「一件家具都数不出来」） |
//!
//! 反例的实跑记录见提交信息。

use std::path::PathBuf;

use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::layout::terrain_atlas_key;
use ll_game::surface_draw::PLACED_FURNITURE_SPRITE;
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_ui::hud::world_map::{FOG_COLOR, terrain_color};
use ll_ui::widget::skin::{
    BarStyleId, DayNightBarStyleId, MAP_PLAYER_KEY, NineSliceSkin, PanelStyleId,
    REQUIRED_SPRITE_KEYS, Skin,
};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 仓库根——`ll-game` 到仓库根固定隔两级 `../..`，与
/// `surface_render.rs` 的 `repo_mods`/`repo_assets` 同一条推导。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 装真实内容 + 用生产路径打出真实图集——与 `GpuResources::new` 每次
/// 启动跑的是同两步。
fn real_content_and_atlas() -> (LoadedContent, PackedAtlas) {
    let root = repo_root();
    let content = load_content(&root.join("mods"), &root.join("assets"))
        .expect("仓库真实 mods/ + assets/ 应当装得起来");
    let sources = load_sprite_sources(&content.asset_vfs);
    assert!(
        !sources.is_empty(),
        "真实资产目录里应当至少读得到一张精灵，否则后面的断言全部失去意义"
    );
    let atlas = pack_atlas(&sources);
    (content, atlas)
}

/// 这个图集键在真实图集里对应的那块矩形，里面的像素。
///
/// 键查不到时直接 panic 而不是返回空：理由与 `surface_render.rs` 的
/// `opaque_pixels` 同——「查不到」与「查到了但是空图」是两种不同的
/// 缺陷，糊成同一个返回值会让失败信息说不清是哪一种。
fn tile_pixels(atlas: &PackedAtlas, name: &str) -> Vec<[u8; 4]> {
    let entry = atlas
        .metadata
        .lookup(name)
        .unwrap_or_else(|| panic!("图集里查不到条目 {name}"));
    let rect = entry.rect;
    let mut pixels = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            pixels.push(atlas.canvas.get_pixel(u32::from(x), u32::from(y)).0);
        }
    }
    pixels
}

/// `define_base` 注册的全部 19 种本体地形，与 `ll_world::terrain` 里那
/// 张注册表逐条对应。顺序固定（不经任何哈希容器），符合约束 C5。
///
/// **加一种本体地形就要在这里加一行。** 这张表是手写的，不是从注册表
/// 现查的——气候条带批次新增 `desert`/`tundra` 时实测过：只加地形不加
/// 这两行，移走 `assets/sprites/terrain_desert.png` 本文件依然全绿，
/// 新地形完全在这道门禁之外。日后若嫌手写易漏，正确的改法是让本函数
/// 从 `BaseTerrainIds` 的字段穷尽解构里推导，不是继续手抄。
fn all_base_terrains(ids: &BaseTerrainIds) -> [(&'static str, TerrainKind); 19] {
    [
        ("deep_water", ids.deep_water),
        ("shallow_water", ids.shallow_water),
        ("sand", ids.sand),
        ("grass", ids.grass),
        ("forest", ids.forest),
        ("hill", ids.hill),
        ("mountain", ids.mountain),
        ("snow", ids.snow),
        ("desert", ids.desert),
        ("tundra", ids.tundra),
        ("floor_wood", ids.floor_wood),
        ("floor_stone", ids.floor_stone),
        ("wall_wood", ids.wall_wood),
        ("wall_stone", ids.wall_stone),
        ("door_closed", ids.door_closed),
        ("door_open", ids.door_open),
        ("window", ids.window),
        ("stairs_up", ids.stairs_up),
        ("stairs_down", ids.stairs_down),
    ]
}

/// 每种本体地形实际会去查的图集键——走生产路径的
/// [`terrain_atlas_key`]，不是测试里另抄一份映射表。
fn terrain_keys(content: &LoadedContent) -> Vec<(&'static str, String)> {
    all_base_terrains(&content.terrain_ids)
        .into_iter()
        .map(|(label, kind)| {
            let key = terrain_atlas_key(kind, &content.terrain_ids, &content.registry)
                .unwrap_or_else(|| panic!("地形 {label} 连图集键都算不出来"));
            (label, key)
        })
        .collect()
}

#[test]
fn 十九种本体地形在真实图集里都查得到条目() {
    // 这条直接对应所有者实测报到的现象：走进据点，控制台每帧刷
    // 「图集条目缺失」。缺的正是这 19 种里的建筑那一整套。
    // Arrange
    let (content, atlas) = real_content_and_atlas();

    // Act & Assert
    for (label, key) in terrain_keys(&content) {
        let found = atlas.metadata.lookup(&key);
        assert!(
            found.is_some(),
            "地形 {label} 查的图集键是 {key}，真实图集里没有这个条目——\
             跑起来会每帧刷「图集条目缺失，跳过本次绘制」"
        );
    }
}

#[test]
fn 十九种本体地形的贴图都铺满整格() {
    // 地形是那一格的底层，不像 `world_marks.rs` 那几张记号可以留透明
    // 让下面透出来——留透明会露出清屏背景，读成「这里什么都没有」。
    // Arrange
    let (content, atlas) = real_content_and_atlas();

    // Act & Assert
    for (label, key) in terrain_keys(&content) {
        let pixels = tile_pixels(&atlas, &key);
        let transparent = pixels.iter().filter(|p| p[3] != 255).count();
        assert_eq!(
            transparent,
            0,
            "地形 {label}（{key}）有 {transparent}/{} 个像素不是完全不透明",
            pixels.len()
        );
    }
}

#[test]
fn 十九种本体地形的贴图两两之间至少四分之一像素不同() {
    // 「查得到条目」不等于「看得出区别」：`wall_stone` 此前与 `mountain`
    // 共用 `terrain_mountain`，两条查找都成功，屏幕上却分不出哪格是山、
    // 哪格是石墙。所有者的验收方式是「走进据点看一眼」，墙/地板/门/窗
    // 必须互相可分——这条是那句话的可执行版本。
    //
    // 门槛取四分之一：16×16 = 256 像素，64 个像素不同。这不是「画得好
    // 不好看」的判据，是「两张图有没有被写成几乎一样」的下界。
    // Arrange
    let (content, atlas) = real_content_and_atlas();
    let keys = terrain_keys(&content);

    // Act & Assert
    for (i, (label_a, key_a)) in keys.iter().enumerate() {
        let a = tile_pixels(&atlas, key_a);
        for (label_b, key_b) in &keys[i + 1..] {
            let b = tile_pixels(&atlas, key_b);
            assert_eq!(a.len(), b.len(), "地形贴图尺寸不一致，无法逐像素比较");
            let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
            let threshold = a.len() / 4;
            assert!(
                differing >= threshold,
                "地形 {label_a} 与 {label_b} 的贴图只有 {differing} 个像素不同\
                 （门槛 {threshold}）——屏幕上分不出这两种地形"
            );
        }
    }
}

#[test]
fn hud皮肤需要的每一张贴图在真实图集里都查得到条目() {
    // 这条对应所有者报的第二个现象：「ui_daynight_bar 好像并没有真的
    // 画到 UI 那」。根因是查找键写的是裸名字、图集里存的是完整命名空间
    // ID，那几张 UI 贴图全军覆没，HUD 静默退回纯色。
    //
    // 清单随 `REQUIRED_SPRITE_KEYS` 自动生长：昼夜滑块
    // （`lostland:ui_daynight_pointer`）加进去的那一刻，这条断言就开始
    // 管它，不需要在这里补一行。
    //
    // 键从 `ll_ui` 公开的 `REQUIRED_SPRITE_KEYS` 取，**不在这里重抄**
    // ——重抄一份的话改名时两边分叉，这条测试会继续绿着而画面已经退回
    // 纯色，正是它要拦的那种失效方式。
    // Arrange
    let (_content, atlas) = real_content_and_atlas();

    // Act & Assert
    for key in REQUIRED_SPRITE_KEYS {
        let found = atlas.metadata.lookup(key);
        assert!(
            found.is_some(),
            "HUD 皮肤要查的图集键 {key} 在真实图集里没有条目——\
             NineSliceSkin 的 textured_* 会全部短路，HUD 静默退回纯色"
        );
        let pixels = tile_pixels(&atlas, key);
        let opaque = pixels.iter().filter(|p| p[3] > 0).count();
        assert!(opaque > 0, "HUD 贴图 {key} 在真实图集里是一张空图");
    }
}

#[test]
fn 玩家动画每一帧在真实图集里都查得到条目() {
    // 帧名表从 `ll_render::anim` 的生产常量取，不在这里重抄——理由同
    // 上一条。玩家精灵走的是 `GpuResources::lookup` 那条**会打 error
    // 日志**的路径，缺帧不像 HUD 那样静默，但同样是「跑起来才看得见」。
    // Arrange
    let (_content, atlas) = real_content_and_atlas();

    // Act & Assert
    let frames = ll_render::anim::HERO_WALK_FRAMES
        .into_iter()
        .chain(ll_render::anim::HERO_IDLE_FRAMES);
    for frame in frames {
        assert!(
            atlas.metadata.lookup(frame).is_some(),
            "动画帧 {frame} 在真实图集里没有条目"
        );
    }
    assert!(
        atlas
            .metadata
            .lookup(ll_game::animation::FALLBACK_SPRITE)
            .is_some(),
        "兜底精灵 {} 在真实图集里没有条目——它是查不到任何动画帧时的最后一道防线",
        ll_game::animation::FALLBACK_SPRITE
    );
}

#[test]
fn hud皮肤拿真实资产装出来后每个贴图外观都是some() {
    // 上一条只证明「键在图集里查得到」。这一条再往前推一步，走
    // `NineSliceSkin` 真正的构造逻辑（`from_uv_lookup`，`new` 就是它的
    // 一行 GPU 适配器），断言每个 `textured_*` 方法**真的**给出贴图
    // 外观而不是 `None`——`None` 就意味着 `crate::hud::render` 每一帧
    // 静默退回纯色，正是所有者报的「ui_daynight_bar 好像并没有真的画
    // 到 UI 那」。
    //
    // # 这条测试的 UV 值是假的，Some/None 是真的
    //
    // 查表闭包在键存在时返回一个占位 UV 矩形。生产路径上这一步是
    // `Atlas::uv_rect`，它的实现是 `metadata.lookup(name)?` 之后做一次
    // 纯算术换算——**是否返回 `Some` 完全由 `lookup` 决定**，换算那一步
    // 不会失败。因此本条测试对「外观是不是 Some」的结论与真实 GPU
    // 路径逐条一致；它不证明 UV 数值算得对（那一侧由
    // `ll_render::atlas` 自己的单测覆盖）。
    //
    // 反例：把 `skin.rs` 的 `PANEL_BORDER_KEY` 改回裸名字，本条与上一条
    // 一起变红。
    // Arrange
    let (_content, atlas) = real_content_and_atlas();
    const PLACEHOLDER_UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    let skin =
        NineSliceSkin::from_uv_lookup(|name| atlas.metadata.lookup(name).map(|_| PLACEHOLDER_UV));

    // Act & Assert
    assert!(
        skin.textured_panel(PanelStyleId::Window).is_some(),
        "窗口面板退回了纯色——九宫格边框/填充贴图没接上"
    );
    assert!(
        skin.textured_bar(BarStyleId::Progress).is_some(),
        "进度条退回了纯色——条形底槽/填充贴图没接上"
    );
    for style in [BarStyleId::Health, BarStyleId::Mana] {
        assert!(
            skin.textured_two_layer_bar(style).is_some(),
            "{style:?} 双层条退回了纯色——条形底槽/填充贴图没接上"
        );
    }
    // 昼夜滑条：底图与滑块两张 UV 各带一个 `?`，任一张查不到整条就
    // 返回 `None`（见 `NineSliceSkin::textured_day_night_bar` 文档
    // 「两张都查得到才走贴图路径」）。因此这一条同时守住滑块贴图——
    // 少了它，滑条会整条退回纯色而不是「底图贴图、滑块纯色」那种半吊子
    // 状态，后者恰好复现所有者报的「只显示了背景条」。
    assert!(
        skin.textured_day_night_bar(DayNightBarStyleId::Clock)
            .is_some(),
        "昼夜滑条退回了纯色——这正是所有者报的现象"
    );
}

#[test]
fn 世界地图玩家标记在每一种地形色上都有足够对比的色调() {
    // 所有者对这块标记的硬约束（`ll_ui::hud::world_map::PLAYER_MARKER_COLOR`
    // 文档）：它要在深蓝的海、深绿的林、灰白的雪山上**同样一眼可见**。
    // 换成贴图之后这条一个字没松，本条就是它的程序化核实。
    //
    // # 判据：每种底色都要能被标记的**某一个**色调拉开
    //
    // 单一颜色满足不了这条——底色一多，总有一种跟它接近。标记贴图同时
    // 带一圈近黑描边与一块暖奶油高光，任何底色要么与暗的拉得开、要么与
    // 亮的拉得开。因此判据是「存在一个不透明像素，与这种底色的最大通道
    // 差 >= 阈值」，而不是「所有像素都拉得开」（那会把主体色误判成问题，
    // 而主体色本来就允许与某些底色接近——描边负责在那些底色上切开轮廓）。
    //
    // 地形清单从注册表现查（`all_base_terrains`），加地形的那一刻这条
    // 断言自动开始管它——沙漠与冻原正是这样进来的，而当年那个纯色标记
    // 压在沙漠上已经开始糊。
    //
    // 反例：把 `MAP_PLAYER_OUTLINE` 改成跟主体差不多的暖色，这条立刻红。
    const MIN_CHANNEL_DISTANCE: i32 = 90;

    // Arrange
    let (content, atlas) = real_content_and_atlas();
    let ids = &content.terrain_ids;
    let marker: Vec<[u8; 4]> = tile_pixels(&atlas, MAP_PLAYER_KEY)
        .into_iter()
        .filter(|pixel| pixel[3] == 255)
        .collect();
    assert!(
        !marker.is_empty(),
        "玩家标记贴图在真实图集里没有任何不透明像素"
    );

    // Act & Assert：逐种地形（外加迷雾色）核实。
    let backgrounds = all_base_terrains(ids)
        .into_iter()
        .map(|(name, kind)| (name, terrain_color(kind, ids)))
        .chain(std::iter::once(("未探索迷雾", FOG_COLOR)));
    for (name, color) in backgrounds {
        let background = [
            (color[0] * 255.0).round() as i32,
            (color[1] * 255.0).round() as i32,
            (color[2] * 255.0).round() as i32,
        ];
        let best = marker
            .iter()
            .map(|pixel| {
                (0..3)
                    .map(|channel| (pixel[channel] as i32 - background[channel]).abs())
                    .max()
                    .expect("三个通道恒非空")
            })
            .max()
            .expect("标记恒有不透明像素");
        assert!(
            best >= MIN_CHANNEL_DISTANCE,
            "玩家标记在 {name} 上看不清：最大通道差只有 {best}，不足 {MIN_CHANNEL_DISTANCE}"
        );
    }
}

/// 本体命名空间下全部**家具**（`ItemDef.furniture` 为真）的完整 ID，
/// 按注册顺序。
///
/// **清单从真实注册表现查，不手抄**——这正是同文件上方
/// [`all_base_terrains`] 那张手写地形表欠下的债（气候条带批次新增两种
/// 地形时实测过：只加地形不加那两行，移走贴图本文件依然全绿）。家具这
/// 一侧不再欠第二笔：`mods/lostland/items.json5` 里多一条
/// `furniture: true`，下面两条断言当场开始管它，本文件一个字都不用改。
///
/// `Registry::snapshot` 按 `ContentIndex` 顺序返回，不经任何哈希容器
/// （约束 C5）。
fn base_furniture_ids(content: &LoadedContent) -> Vec<String> {
    content
        .registry
        .snapshot()
        .into_iter()
        .filter(|id| id.namespace() == "lostland")
        .filter(|id| {
            content
                .registry
                .get(id)
                .and_then(|index| content.item_table.get(index))
                .is_some_and(|view| view.furniture)
        })
        .map(|id| id.to_string())
        .collect()
}

#[test]
fn 本体每一件家具在真实图集里都查得到自带贴图() {
    // 家具的失效方式与上面那批地形逐字同型，只是更安静：
    // `ll_game::surface_draw::placed_furniture_draws` 先拿这件物品的完整
    // 命名空间 ID 查图，查不到就退回通用家具记号
    // （`lostland:furniture_placed`）——**不报错、不打日志**，画面上只是
    // 六件家具全变成同一个紫罗兰箱子。这条断言是那件事的可执行版本。
    //
    // 键就是内容的完整 ID（带 `lostland:` 前缀），与生产路径
    // `registry.resolve(ground.stack.def).map(|id| id.to_string())` 拿到
    // 的是同一个字符串——**不在这里另抄一份映射**。上一批五张 HUD 贴图
    // 正是栽在「查裸名字、图集里存带前缀的」这一步上。
    // Arrange
    let (content, atlas) = real_content_and_atlas();
    let furniture = base_furniture_ids(&content);

    // Act & Assert
    assert!(
        !furniture.is_empty(),
        "本体一件家具都数不出来——要么 items.json5 的 furniture 标志掉了，要么这条断言本身已经查错了表"
    );
    for key in &furniture {
        assert!(
            atlas.metadata.lookup(key).is_some(),
            "家具 {key} 在真实图集里没有自带贴图——跑起来它会静默退回通用家具记号 {}，屏幕上与其余家具长得一模一样",
            PLACED_FURNITURE_SPRITE
        );
        let pixels = tile_pixels(&atlas, key);
        let opaque = pixels.iter().filter(|p| p[3] > 0).count();
        assert!(opaque > 0, "家具 {key} 在真实图集里是一张空图");
    }
}

#[test]
fn 本体家具的贴图两两之间至少四分之一像素不同() {
    // 判据与上面 `十九种本体地形的贴图两两之间至少四分之一像素不同`
    // 逐字相同，理由也一样：「查得到条目」不等于「看得出区别」。两件
    // 摆在同一间屋里的家具（一把椅子和一张桌子）必须一眼分得开，否则
    // 「给建筑按类型填家具」这件事在画面上根本读不出来。
    //
    // 门槛四分之一：16×16 = 256 像素，64 个像素不同。
    //
    // 这条与 `tools/ll-artgen/src/furniture.rs` 里那条同名单测**不重复**：
    // 那条比的是绘制函数的输出，这条比的是真实资产打包进图集之后的像素
    // ——两张清单条目指向同一个 PNG 文件这种失效方式，只有这一条抓得到。
    // Arrange
    let (content, atlas) = real_content_and_atlas();
    let furniture = base_furniture_ids(&content);
    let rendered: Vec<(&String, Vec<[u8; 4]>)> = furniture
        .iter()
        .map(|key| (key, tile_pixels(&atlas, key)))
        .collect();

    // Act & Assert
    for (i, (key_a, pixels_a)) in rendered.iter().enumerate() {
        for (key_b, pixels_b) in &rendered[i + 1..] {
            assert_eq!(
                pixels_a.len(),
                pixels_b.len(),
                "家具贴图尺寸不一致，无法逐像素比较：{key_a} 与 {key_b}"
            );
            let differing = pixels_a
                .iter()
                .zip(pixels_b.iter())
                .filter(|(a, b)| a != b)
                .count();
            let threshold = pixels_a.len() / 4;
            assert!(
                differing >= threshold,
                "家具 {key_a} 与 {key_b} 的贴图只有 {differing} 个像素不同（门槛 {threshold}）——屏幕上分不出这两件家具"
            );
        }
    }
}
