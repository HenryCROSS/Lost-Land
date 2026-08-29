//! 世界生成参数从「玩家的选择」到「存档里的地图」这条完整链路的端到
//! 端证据（ADR 0018：新能力要有经真实 `mods/` 内容的端到端证据）。
//!
//! 全部用仓库真实的 `mods/lostland/` 内容装载、真实的
//! [`ll_game::world::build_new_world`]、真实的存档写出与读入管线——不
//! 造任何简化夹具。
//!
//! # 这条链路上此前真实存在的缺口
//!
//! 世界的绝大部分**在读档那一刻还不存在**：地形是流式生成的，玩家走到
//! 哪生成到哪。生成参数若不随存档往返，读档后重新拼出来的那一份就会
//! 与建档时的不同——已常驻的区块（存档里带着）仍是旧地形，玩家往前
//! 走一步现算出来的却是另一套阈值。**同一张地图会在玩家脚下裂成两种
//! 地形**，而且完全静默：不报错、不变红。本文件的第二条测试正是对着
//! 这个缺口写的。

use ll_content::world_identity::terrain_preset;
use ll_core::time::Tick;
use ll_game::content::{LoadedContent, load_content};
use ll_game::save::{LoadedGame, load_game, save_game};
use ll_game::world::build_new_world;
use ll_world::generate::{GenParams, TerrainShape, build_zone_noise, terrain_at_tile};
use ll_world::state::WorldState;

/// 装载仓库真实的本体内容——与 `ll_game` 内部测试同一条路径。
fn real_content() -> LoadedContent {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crates/ll-game 上溯两级就是仓库根")
        .to_path_buf();
    let scratch = std::env::temp_dir().join(format!(
        "ll-game-worldgen-e2e-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&scratch).expect("创建测试目录应当成功");
    let content = load_content(&repo_root.join("mods"), &scratch.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&scratch);
    content
}

fn temp_save_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ll-game-worldgen-e2e-{name}-{}.llsave",
        std::process::id()
    ))
}

/// 群岛预设的真实参数——不在测试里另抄一份数字，直接问预设表。
fn archipelago_params(seed: u64) -> GenParams {
    GenParams {
        seed,
        shape: terrain_preset("archipelago")
            .expect("预设表里有群岛这一档")
            .shape,
    }
}

/// 一批**远离出生点、建档时绝不可能已经常驻**的采样坐标。
///
/// 取质数步长在整张世界上撒点，而不是取连续的一片：连续一片可能整片
/// 落在同一个噪声格子里，两套不同参数在那一片上恰好给出相同结果的
/// 概率远高于撒开的点。
fn sample_positions(size: ll_core::torus::TorusSize) -> Vec<ll_core::torus::TorusPos> {
    (0..64i32)
        .map(|i| size.wrap(i * 401 + 7, i * 283 + 11))
        .collect()
}

/// 按给定参数**纯生成**这批坐标上的地形（不经任何据点铺设），返回结果
/// 序列。
///
/// # 为什么参照系取「纯生成」而不是另一个 `GameWorld`
///
/// `build_new_world` 建出的世界带着编年史，据点会把地面改写成木地板/
/// 木墙；而 `ll_content::save_file::load_game` 读回的世界**不带**编年史
/// （编年史是读档后由 `ll_game` 自己 `attach_chronicle` 重新派生的运行
/// 期数据，见 `ll_world::chronicle` 模块文档）。拿一个带据点的世界去比
/// 一个不带据点的世界，比出来的差异是「据点有没有铺」，不是本测试要问
/// 的「生成参数对不对」。这里把参照系降到两侧共有的那一层——地形生成
/// 本身。
fn generated_terrain(
    params: &GenParams,
    positions: &[ll_core::torus::TorusPos],
    layout: &ll_world::zone::ZoneLayout,
    content: &LoadedContent,
) -> Vec<u32> {
    let noise = build_zone_noise(layout, params).expect("布局恒能构造噪声源");
    positions
        .iter()
        .map(|pos| {
            terrain_at_tile(&noise, params, *pos, &content.terrain_ids)
                .index()
                .get()
        })
        .collect()
}

/// 在读回来的那个世界上，走**真实的流式加载入口**求出这批坐标的地形。
///
/// 与 [`generated_terrain`] 走的是同一套阈值逻辑（`SurfaceStore` 的
/// 流式生成最终调的就是 `terrain_at_coord`），因此两者可以逐位比较；
/// 这里坚持走 `terrain_at_streaming` 而不是直接调生成函数，是为了让
/// 断言覆盖玩家真正会走到的那条路径。
fn streamed_terrain(
    world: &mut WorldState,
    params: &GenParams,
    positions: &[ll_core::torus::TorusPos],
    content: &LoadedContent,
) -> Vec<u32> {
    let noise = build_zone_noise(world.terrain.layout(), params).expect("布局恒能构造噪声源");
    positions
        .iter()
        .map(|pos| {
            world
                .terrain_at_streaming(&noise, params, &content.terrain_ids, *pos, Tick(0))
                .index()
                .get()
        })
        .collect()
}

#[test]
fn 用群岛预设建的世界其形态参数原样写进世界状态() {
    // 最基础的一环：玩家选的那组参数真的落在 WorldState 上，而不是
    // 在 build_new_world 里被 GenParams::default() 顶掉——那正是本批次
    // 之前的实际行为（build_new_world 只收一个 seed）。
    // Arrange
    let content = real_content();
    let params = archipelago_params(31337);

    // Act
    let game_world = build_new_world(&content, params).expect("默认布局满足全部前置条件");

    // Assert
    assert_eq!(game_world.world.terrain_shape, params.shape);
    assert_ne!(
        game_world.world.terrain_shape,
        TerrainShape::default(),
        "群岛预设与默认形态必须真的不同，否则这条测试什么也没验证"
    );
    assert_eq!(game_world.world.gen_params(), params);
}

#[test]
fn 群岛存档重开之后玩家还没走到的地方生成出来的仍然是群岛() {
    // 本文件模块文档「这条链路上此前真实存在的缺口」那一段说的就是这
    // 条：流式地形意味着存档里只有玩家去过的那一小块，其余全靠读档后
    // 重新拼出来的参数现算。
    //
    // 反例（已实跑验证会红）：把读档侧那一行 `loaded.gen_params()` 换成
    // 「默认种子 + 默认形态」（本批次之前 `ll_game::rebuild_noise` 真正
    // 的行为），倒数第二条断言当场红——读回来的那串地形会与
    // `default_reference` 一致而与 `expected` 不一致。
    // Arrange
    let content = real_content();
    let params = archipelago_params(20_260_826);
    let game_world = build_new_world(&content, params).expect("默认布局满足全部前置条件");
    let layout = *game_world.world.terrain.layout();
    let positions = sample_positions(game_world.world.size);
    let path = temp_save_path("archipelago-roundtrip");
    save_game(
        &path,
        &content,
        &game_world,
        "测试旅人",
        "出生地",
        "测试存档",
    )
    .expect("写出应当成功");
    drop(game_world);

    // 建档时那套参数在这批远点上该生成什么。
    let expected = generated_terrain(&params, &positions, &layout, &content);
    // 同一个种子、默认形态在同一批点上该生成什么——用来证明这两串真的
    // 不一样，断言才有区分力。
    let default_params = GenParams {
        seed: params.seed,
        shape: TerrainShape::default(),
    };
    let default_reference = generated_terrain(&default_params, &positions, &layout, &content);
    assert_ne!(
        expected, default_reference,
        "群岛参数与默认参数在这批采样点上给出了相同的地形，本测试失去区分力"
    );

    // Act：读档，然后**只用存档自己记着的参数**继续流式生成。
    let LoadedGame::Playable {
        world: mut loaded, ..
    } = load_game(&path, &content)
    else {
        panic!("期望 Playable");
    };
    let loaded_params = loaded.gen_params();
    let actual = streamed_terrain(&mut loaded, &loaded_params, &positions, &content);

    // Assert
    assert_eq!(loaded_params, params, "读回的生成参数与建档时的不一致");
    assert_eq!(
        actual, expected,
        "读档后流式生成出来的地形与建档时的那张图不一致"
    );
    assert_ne!(actual, default_reference);

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 形态参数不同的两个世界摘要不同即使种子相同() {
    // WorldState::hash 必须看得见形态参数——否则「形态参数没有正确随
    // 存档往返」这类缺陷不会被任何确定性回归测出来（见
    // `ll_world::state::WorldState::hash` 里 write_terrain_shape 那段
    // 注释）。
    //
    // 反例（已实跑验证会红）：注释掉 `WorldState::hash` 里那一行
    // `write_terrain_shape(..)`，本条断言不一定立刻红（两个世界常驻的
    // 地形本来就不同），但 `crates/ll-world/tests/determinism.rs` 的
    // 黄金基准会精确回到旧值——那次实验正是本批次重冻那条常量的第二
    // 步证据。
    // Arrange
    let content = real_content();
    let seed = 4242;

    // Act
    let continent = build_new_world(
        &content,
        GenParams {
            seed,
            shape: TerrainShape::default(),
        },
    )
    .expect("默认布局满足全部前置条件");
    let archipelago =
        build_new_world(&content, archipelago_params(seed)).expect("默认布局满足全部前置条件");

    // Assert
    assert_eq!(continent.world.seed, archipelago.world.seed);
    assert_ne!(continent.world.hash(), archipelago.world.hash());
}

#[test]
fn 同一组参数与同一个种子反复建世界产出逐位相同的世界() {
    // 确定性：形态参数进来之后，「同一份构建反复运行产出同一个世界」
    // 这条纪律必须仍然成立。
    // Arrange
    let content = real_content();
    let params = archipelago_params(90210);

    // Act
    let first = build_new_world(&content, params).expect("默认布局满足全部前置条件");
    let second = build_new_world(&content, params).expect("默认布局满足全部前置条件");

    // Assert
    assert_eq!(first.world.hash(), second.world.hash());
}

#[test]
fn 四档预设都能建出带玩家实体且出生点连得开的世界() {
    // 预设不能只是数字对——它得真的能开局。出生点搜索
    // （`find_spawn_site`）要求一片至少 MIN_SPAWN_LAND_AREA 大的连通
    // 陆地；群岛预设把陆地切得很碎，这条正是它最可能踩的雷。搜索失败
    // 时 build_new_world 会退回硬编码坐标 + 强铺兜底并记一条警告，
    // 世界仍然建得出来但玩家会站在一块凭空铺出来的地上——那正是项目
    // 所有者实测抱怨过的「小得可怜的岛」。这里断言四档预设都不落进
    // 那条兜底路径：出生点周围的连通可行走面积必须真的够大。
    // Arrange
    let content = real_content();

    for preset in ll_content::world_identity::TERRAIN_PRESETS {
        let params = GenParams {
            seed: 20_260_826,
            shape: preset.shape,
        };

        // Act
        let game_world = build_new_world(&content, params).expect("默认布局满足全部前置条件");
        let player = game_world
            .world
            .actors
            .get(game_world.player)
            .unwrap_or_else(|| panic!("预设 {} 建出的世界没有玩家实体", preset.id));
        let area = connected_walkable_area(&game_world.world, player.pos, &content);

        // Assert
        assert!(
            area >= 500,
            "预设 {} 的出生点周围只有 {} 格连通可行走地面，落进了强铺兜底",
            preset.id,
            area
        );
    }
}

/// 从 `origin` 出发做洪水填充，数出连通可行走区域的大小（上限 2000 格
/// 后提前收手——本文件只需要知道「够不够 500」，不需要精确值）。
///
/// 只读已常驻的区块（出生点邻域在建世界时已经预热），用 `Vec` 而不是
/// `HashSet` 记访问过的格子（约束 C5）。
fn connected_walkable_area(
    world: &WorldState,
    origin: ll_core::torus::TorusPos,
    content: &LoadedContent,
) -> usize {
    let mut seen: Vec<ll_core::torus::TorusPos> = vec![origin];
    let mut queue = vec![origin];
    while let Some(pos) = queue.pop() {
        if seen.len() >= 2000 {
            break;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = world.size.wrap(pos.x() + dx, pos.y() + dy);
            if seen.contains(&next) {
                continue;
            }
            let Some(kind) = world.terrain.terrain_at_resident(next) else {
                continue;
            };
            if content.terrain_table.blocks_move(kind) {
                continue;
            }
            seen.push(next);
            queue.push(next);
        }
    }
    seen.len()
}
