//! 端到端验收：地面物品堆、放置家具、NPC **真的会被画出来**。
//!
//! # 这条验收在验什么（ADR 0018）
//!
//! 「画出来了」是最容易自欺的一类断言——只测一个纯函数返回了某个字符串，
//! 完全可能那个字符串在真实图集里根本查不到任何东西，屏幕上照样一片空白。
//! 本文件因此把证据链钉成**两段，缺一不可**：
//!
//! 1. **接线段**：用真实 `mods/` 装出来的内容与真实生成的世界，调用生产
//!    路径上的 [`ll_game::surface_draw::surface_draws`]（`render_surface`
//!    每帧调的就是它），断言三类内容各自产出了指向哪个图集键的指令。
//! 2. **有图段**：用**同一份**真实 `assets/` + `mods/`，走生产路径上的
//!    [`ll_game::app::load_sprite_sources`] + [`ll_render::atlas_pack::pack_atlas`]
//!    （`GpuResources::new` 每次启动跑的就是这两步）打出真实图集，断言
//!    上一段选中的每个键都能在图集里查到条目，**且那块矩形里真的有
//!    不透明像素**——不是一张空图，也不是一个查不到的名字。
//!
//! 两段合起来才等于「跑起来能看见」。只有第一段，等于只证明了代码里写了
//! 个名字；只有第二段，等于只证明了美术资源存在但没人用它。
//!
//! # 每条断言的反例是什么（本次开发中真的逐条改坏跑过）
//!
//! ADR 0018 要求每条断言都用**故意改坏**的反例验证它真的会红。下面五条
//! 全部实跑过一遍：
//!
//! | 改坏什么 | 哪条变红 |
//! | --- | --- |
//! | 移走 `assets/sprites/forge.png` | `锻炉用的是内容自己那张图不是通用记号` |
//! | 把 `ground_pile.png` 换成全透明 | `地面物品堆那一个团在真实图集里真的有画` 与 `三张通用记号在真实图集里都有画` |
//! | `ground_pile_draws` 的 `BTreeSet` 改成 `Vec`（不去重） | `同一格上无论躺多少东西都只画一个团` |
//! | 删掉 `npc_draws` 里 `.filter(\|(id, _)\| *id != player)` | `玩家不会被当成npc再画一遍` |
//! | 删掉 `render_surface` 里那个 `for draw in surface_draws(..)` 循环 | 不是测试红，是**编译红**：`surface_draws` 的 import、`GpuResources::lookup_first`、`push_surface_draw` 三处同时变成死代码，而 `scripts/ci/run_tests.sh` 的 `RUSTFLAGS=-D warnings` 把它们变成三条 `error` |
//!
//! 最后一条值得单独说：本文件验的是 `surface_draws` 这个函数，**验不到
//! 「`render_surface` 有没有调用它」**——那一段在 GPU glue 里，没有窗口
//! 就跑不到。守住它的不是断言而是 `-D warnings`。这条替代手段是本批次
//! 的判断，如果日后有了无头渲染路径，应当换成真正的断言。
//!
//! 另有一条第一版写错、被反例抓出来的：`玩家不会被当成npc再画一遍` 起初
//! 断言的是「没有指令既落在玩家那一格、绘制序号又等于 `PLAYER_ENTITY`」
//! ——那是句废话（号段决定了任何 NPC 指令的序号都不可能是 0），删掉过滤
//! 之后它依然是绿的。现在钉的是玩家自己那个 Arena 槽位对应的序号，以及
//! 「指令数恰好比存活角色数少一条」。

use std::path::{Path, PathBuf};

use ll_core::ident::NamespacedId;
use ll_core::time::Tick;
use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::surface_draw::{
    GROUND_PILE_SPRITE, NPC_ENTITY_BASE, NPC_SPRITE, PLACED_FURNITURE_SPRITE, ground_pile_draws,
    npc_draws, placed_furniture_draws, surface_draws,
};
use ll_game::world::{GameWorld, build_new_world};
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_render::sprite::Layer;
use ll_sim::item::ItemStack;
use ll_world::item::GroundItemStack;

/// 仓库真实的 `mods/` 目录——`ll-game` 到仓库根固定隔两级 `../..`，与
/// `crate::test_support::repo_mods_dir` 同一条推导。
fn repo_mods() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods")
}

/// 仓库真实的 `assets/` 目录，理由同 [`repo_mods`]。
fn repo_assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// 装真实内容 + 建一个真实世界。世界种子固定，让本文件全部用例看到的
/// 是同一个世界（NPC 是否物化、物化在哪，取决于种子）。
fn real_world() -> (LoadedContent, GameWorld) {
    let content = load_content(&repo_mods(), &repo_assets()).expect("真实 mods/ 应当装得起来");
    let world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: 20260826,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认参数应当建得出世界");
    (content, world)
}

/// 用生产路径把真实资产打成图集——与 `GpuResources::new` 跑的是同两步。
fn real_atlas(content: &LoadedContent) -> PackedAtlas {
    let sources = load_sprite_sources(&content.asset_vfs);
    assert!(
        !sources.is_empty(),
        "真实资产目录里应当至少读得到一张精灵，否则后面的断言全部失去意义"
    );
    pack_atlas(&sources)
}

/// 这个图集键对应的矩形里，有多少个不透明像素。
///
/// 键查不到时直接 panic 而不是返回 0：「查不到」与「查到了但是空图」是
/// 两种不同的缺陷，糊成同一个返回值会让失败信息说不清是哪一种。
fn opaque_pixels(atlas: &PackedAtlas, name: &str) -> usize {
    let entry = atlas
        .metadata
        .lookup(name)
        .unwrap_or_else(|| panic!("图集里查不到条目 {name}"));
    let rect = entry.rect;
    let mut count = 0;
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            if atlas.canvas.get_pixel(u32::from(x), u32::from(y)).0[3] > 0 {
                count += 1;
            }
        }
    }
    count
}

/// 往世界里塞一堆躺着的/立着的东西（Arrange）。
///
/// 直接写 `ground_items` 而不是走 `Intent::Drop`/`Intent::Place`：那两条
/// 路径本身已经有自己的端到端验收
/// （`crates/ll-mod/tests/furniture_placement.rs`），在这里重跑一遍只会
/// 把「渲染接没接上」这件事的失败原因混进「放置前置判对没判对」里。
/// 本文件验的是**从 `WorldState` 到图集键**这一段。
fn put(world: &mut GameWorld, x: i32, y: i32, id: &str, content: &LoadedContent, placed: bool) {
    let def = content
        .registry
        .get(&NamespacedId::parse(id).expect("字面量合法"))
        .unwrap_or_else(|| panic!("真实 mods/ 里应当注册了 {id}"));
    let pos = world.world.size.wrap(x, y);
    world.world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(def, 1),
        dropped_at: Tick(0),
        contents: Vec::new(),
        placed,
    });
}

/// 玩家此刻站在哪一格——本文件把东西都摆在玩家脚边，这样它们必然落在
/// 视野内（`render_surface` 只画视野内的地表内容）。
fn player_pos(world: &GameWorld) -> (i32, i32) {
    let agent = world
        .world
        .actors
        .get(world.player)
        .expect("刚建好的世界里玩家必然存在");
    (agent.pos.x(), agent.pos.y())
}

#[test]
fn 同一格上无论躺多少东西都只画一个团() {
    // Arrange：玩家脚下那一格丢三样真实物品。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    for id in [
        "lostland:iron_ingot",
        "lostland:iron_rivet",
        "lostland:smith_hammer",
    ] {
        put(&mut world, px, py, id, &content, false);
    }

    // Act
    let draws = ground_pile_draws(&world.world);

    // Assert：项目所有者裁定「无论是一个还是N个……统一用一个团表示
    // 哪一个地方有东西」。三件东西 → 恰好一条指令、恒定那一个图集键。
    assert_eq!(draws.len(), 1, "三件东西应当只画出一个团");
    assert_eq!(
        draws[0].keys().collect::<Vec<_>>(),
        vec![GROUND_PILE_SPRITE],
        "地面物品堆不接受内容自带贴图，只有那一个团"
    );
}

#[test]
fn 地面物品堆那一个团在真实图集里真的有画() {
    // Arrange
    let (content, _) = real_world();
    let atlas = real_atlas(&content);

    // Act
    let opaque = opaque_pixels(&atlas, GROUND_PILE_SPRITE);

    // Assert：查得到条目、且那块矩形里真的有不透明像素——「接上了但是
    // 一张空图」与「压根没接上」在屏幕上是同一种表现，两者都必须被拦下。
    assert!(
        opaque > 0,
        "{GROUND_PILE_SPRITE} 在图集里是一张全透明的空图"
    );
}

#[test]
fn 三张通用记号在真实图集里都有画() {
    // Arrange：三张兜底记号缺任何一张，对应那类内容就会整类看不见。
    let (content, _) = real_world();
    let atlas = real_atlas(&content);

    // Act & Assert
    for name in [GROUND_PILE_SPRITE, PLACED_FURNITURE_SPRITE, NPC_SPRITE] {
        assert!(opaque_pixels(&atlas, name) > 0, "{name} 是一张空图");
    }
}

#[test]
fn 锻炉用的是内容自己那张图不是通用记号() {
    // Arrange：立一座真实 `mods/lostland/items.json5` 里的锻炉。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    put(&mut world, px, py, "lostland:forge", &content, true);
    let atlas = real_atlas(&content);

    // Act：取生产路径上「第一个在图集里查得到」的那个键——与
    // `GpuResources::lookup_first` 同一条次序。
    let draws = placed_furniture_draws(&world.world, &content.registry);
    let chosen = draws[0]
        .keys()
        .find(|name| atlas.metadata.lookup(name).is_some())
        .expect("至少兜底记号必须查得到");

    // Assert：引擎没有任何一处按 `lostland:forge` 分支——它只是拿内容
    // 的完整 ID 去查图，恰好查到了 `assets/sprites/forge.png` 生成的
    // 那条。把那张图删掉，这里会退回通用记号，本条随即变红。
    assert_eq!(chosen, "lostland:forge");
    assert!(opaque_pixels(&atlas, chosen) > 0, "锻炉那张图是空的");
}

#[test]
fn 没有自带贴图的家具退回通用记号() {
    // Arrange：铁锭不是家具，但把它硬立起来就构成「一件没有自带贴图的
    // 放置物」——这正是绝大多数家具将来的处境，兜底路径必须成立。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    put(&mut world, px, py, "lostland:iron_ingot", &content, true);
    let atlas = real_atlas(&content);

    // Act
    let draws = placed_furniture_draws(&world.world, &content.registry);
    let chosen = draws[0]
        .keys()
        .find(|name| atlas.metadata.lookup(name).is_some())
        .expect("至少兜底记号必须查得到");

    // Assert：内容自带键查不到 → 退回通用记号，而不是什么都不画。
    assert_eq!(
        draws[0].preferred_key.as_deref(),
        Some("lostland:iron_ingot")
    );
    assert_eq!(chosen, PLACED_FURNITURE_SPRITE);
}

#[test]
fn 玩家不会被当成npc再画一遍() {
    // Arrange：世界里除玩家外再放一个邻居，让「少一条」这个差值有意义
    // （只有玩家一个角色时，0 条和 1 条的区别太容易被别的原因凑巧满足）。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    let mut neighbour = world
        .world
        .actors
        .get(world.player)
        .expect("玩家存在")
        .clone();
    neighbour.pos = world.world.size.wrap(px + 1, py);
    world.world.actors.spawn(neighbour);

    // Act
    let draws = npc_draws(&world.world, &content.registry, world.player);

    // Assert：玩家由 `render_surface` 用当前动画帧单独画，绝不能同时又
    // 被 NPC 那一路画一个通用记号——那会在玩家身上叠一个紫红色的影子。
    //
    // 断言钉的是**玩家自己那个绘制序号**，不是「有没有指令落在玩家那一
    // 格」：后者是句废话（NPC 本来就可能站在玩家旁边，而序号号段决定了
    // 任何 NPC 指令的序号都不可能等于 `PLAYER_ENTITY`）。本次开发中，
    // 这条断言的第一版正是那句废话——把 `npc_draws` 的过滤删掉之后它
    // 依然是绿的，反例跑出来才发现，见本文件头「每条断言的反例是什么」。
    let player_slot = NPC_ENTITY_BASE + u64::from(world.player.index());
    assert!(
        draws.iter().all(|draw| draw.entity != player_slot),
        "玩家所在的 Arena 槽位不该产出 NPC 绘制指令"
    );
    assert_eq!(
        draws.len(),
        world.world.actors.len() - 1,
        "NPC 指令数应当恰好比存活角色数少一条（少的那条是玩家）"
    );
}

#[test]
fn npc的种族查不到自带贴图时退回通用记号且那张记号真的有画() {
    // Arrange：本体三个种族目前都没有自带贴图，因此全部走兜底。
    let (content, mut world) = real_world();
    let atlas = real_atlas(&content);
    let (px, py) = player_pos(&world);
    // 复制一份玩家当邻居——真实世界里 NPC 是否物化在视野内取决于种子，
    // 用一个确定存在的邻居把这条断言从「这个种子恰好生成了 NPC」里解耦。
    let mut neighbour = world
        .world
        .actors
        .get(world.player)
        .expect("玩家存在")
        .clone();
    neighbour.pos = world.world.size.wrap(px + 1, py);
    let neighbour_id = world.world.actors.spawn(neighbour);

    // Act
    let draws = npc_draws(&world.world, &content.registry, world.player);
    let neighbour_draw = draws
        .iter()
        .find(|draw| draw.pos == world.world.size.wrap(px + 1, py))
        .expect("刚 spawn 的邻居应当有一条绘制指令");
    let chosen = neighbour_draw
        .keys()
        .find(|name| atlas.metadata.lookup(name).is_some())
        .expect("至少兜底记号必须查得到");

    // Assert
    assert_eq!(neighbour_draw.layer, Layer::ENTITY);
    assert_eq!(chosen, NPC_SPRITE);
    assert!(opaque_pixels(&atlas, chosen) > 0);
    let _ = neighbour_id;
}

#[test]
fn 三类内容的层序是地形之上角色之下再到角色() {
    // Arrange：一格躺着东西、一格立着家具、外加一个邻居 NPC。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    put(&mut world, px, py, "lostland:iron_ingot", &content, false);
    put(&mut world, px + 1, py, "lostland:forge", &content, true);
    let mut neighbour = world
        .world
        .actors
        .get(world.player)
        .expect("玩家存在")
        .clone();
    neighbour.pos = world.world.size.wrap(px, py + 1);
    world.world.actors.spawn(neighbour);

    // Act
    let draws = surface_draws(&world.world, &content.registry, world.player);

    // Assert：地面物品与家具在 DECOR（地形之上、角色之下），NPC 在
    // ENTITY——玩家标记也在 ENTITY，两者按脚底屏幕纵坐标互相遮挡，
    // 这正是「层序：地形 → 地面物品/家具 → NPC → 玩家」那条要求。
    let decor = draws
        .iter()
        .filter(|draw| draw.layer == Layer::DECOR)
        .count();
    let entity = draws
        .iter()
        .filter(|draw| draw.layer == Layer::ENTITY)
        .count();
    assert!(decor >= 2, "至少一堆躺着的 + 一件立着的");
    assert!(entity >= 1, "至少那个邻居 NPC");
    assert!(Layer::TERRAIN < Layer::DECOR && Layer::DECOR < Layer::ENTITY);
}

#[test]
fn 同一份世界连算两次产出逐条相同() {
    // Arrange：约束 C5——绘制顺序不得依赖任何哈希容器的迭代顺序。
    let (content, mut world) = real_world();
    let (px, py) = player_pos(&world);
    for dx in 0..4 {
        put(
            &mut world,
            px + dx,
            py,
            "lostland:iron_ingot",
            &content,
            false,
        );
        put(
            &mut world,
            px - dx - 1,
            py,
            "lostland:iron_ingot",
            &content,
            false,
        );
    }
    put(&mut world, px, py + 1, "lostland:forge", &content, true);

    // Act
    let first = surface_draws(&world.world, &content.registry, world.player);
    let second = surface_draws(&world.world, &content.registry, world.player);

    // Assert
    assert_eq!(first, second);
    assert!(first.len() >= 9);
}

#[test]
fn 仓库里真实存在这四张新贴图文件() {
    // 守卫：`ll-artgen` 生成的 PNG 必须与源码一起提交。文件被漏提交时
    // 先在这里红，而不是等到某个跑图集打包的用例报出一条语焉不详的
    // 「条目缺失」。
    // Arrange
    let sprites = repo_assets().join("sprites");

    // Act & Assert
    for file in [
        "ground_pile.png",
        "furniture_placed.png",
        "npc_idle_0.png",
        "forge.png",
    ] {
        let path: &Path = &sprites.join(file);
        assert!(path.is_file(), "缺少 {}", path.display());
    }
}
