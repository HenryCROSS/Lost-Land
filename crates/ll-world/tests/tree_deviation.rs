//! 树木**偏差层**的回归：偏差真的覆盖派生，且**存读一轮之后仍然覆盖**。
//!
//! # 为什么「存读一轮之后」这半句不能省
//!
//! 「砍掉的树不再长回来」在内存里成立是容易的（`tree_at` 先查偏差就行）。
//! 真正会出事的是**读档之后**：偏差表若没有真的随存档主体往返，读回来的
//! 世界会用派生层的答案回答每一格——被砍光的林子原样长回来，而且**不报
//! 任何错**。本文件因此每一条都跑一趟真实的 `postcard` 往返。

use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_world::generate::GenParams;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::tree::{TreeDeviation, TreeSpecies, derived_tree_at, tree_at};
use ll_world::zone::ZoneLayout;

/// 与 `crates/ll-world/tests/determinism.rs` 同一份布局理由：边长 48
/// （噪声格点周期的整数倍、大于视口跨度、刻意不是 2 的幂）。
fn test_world() -> (WorldState, ll_world::terrain::BaseTerrainIds) {
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束");
    let spawn = layout.tile_size().wrap(0, 0);
    let world = WorldState::new(
        layout,
        &GenParams {
            seed: 5,
            ..GenParams::default()
        },
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");
    (world, terrain_ids)
}

/// 在这个世界里找一格**派生层说有树**的位置。
///
/// **先断言它找得到**：本文件下面每一条都建立在「真的有一棵派生出来的树
/// 可以砍」之上。找不到就意味着整组断言在空跑——ADR 0022 点名的「断言恒
/// 绿因为被断言的对象根本不存在」。
fn 一棵派生出来的树(
    world: &WorldState,
    forest: ll_world::terrain::TerrainKind,
) -> ll_core::torus::TorusPos {
    for y in 0..world.size.height() as i32 {
        for x in 0..world.size.width() as i32 {
            let pos = world.size.wrap(x, y);
            if tree_at(world, pos, forest).is_some() {
                return pos;
            }
        }
    }
    panic!(
        "这个测试世界里一棵派生出来的树都没有——本文件全部断言都会空跑。\
         多半是种子 5 的 48×48 世界里没有 forest 地形了，换一个种子并把\
         理由写在这里"
    );
}

fn 往返(world: &WorldState) -> WorldState {
    let bytes = postcard::to_allocvec(world).expect("WorldState 可序列化");
    postcard::from_bytes(&bytes).expect("刚序列化的数据必然合法")
}

#[test]
fn 砍掉的树存读一轮之后仍然不在() {
    // **本批点名要验的第二条。** 反例验证（已实跑，见计划文档十节）：
    // 把 `tree_at` 的查询次序反过来（先查派生、查不到才查偏差），本条
    // 当场红。
    // Arrange
    let (mut world, ids) = test_world();
    let pos = 一棵派生出来的树(&world, ids.forest);
    assert!(
        tree_at(&world, pos, ids.forest).is_some(),
        "前提：动手之前这一格真的有树"
    );

    // Act：砍掉。
    world.trees.set(pos, TreeDeviation::felled());

    // Assert（内存里）
    assert!(
        tree_at(&world, pos, ids.forest).is_none(),
        "砍掉之后这一格不该还有树——偏差没有覆盖派生"
    );
    // 派生层本身**不变**（它不知道偏差的存在）——这一条钉住「覆盖」是
    // 发生在解析层的，不是有人偷偷改了派生公式。
    assert!(
        derived_tree_at(
            world.seed,
            pos,
            ids.forest,
            ids.forest,
            world.size.height(),
            world.terrain_shape.climate_band_width
        )
        .is_some(),
        "派生层不该知道偏差的存在"
    );

    // Assert（存读一轮之后）——**这一半才是真正会出事的那一半。**
    let 读回来 = 往返(&world);
    assert!(
        读回来.trees.get(pos).is_some(),
        "偏差记录没有随存档主体往返"
    );
    assert!(
        tree_at(&读回来, pos, ids.forest).is_none(),
        "存读一轮之后被砍掉的树又长回来了"
    );
}

#[test]
fn 种下的树存读一轮之后还在() {
    // Arrange：找一格派生层说**没有**树的位置。
    let (mut world, ids) = test_world();
    let mut 空地 = None;
    for y in 0..world.size.height() as i32 {
        for x in 0..world.size.width() as i32 {
            let pos = world.size.wrap(x, y);
            if world.terrain_at(pos) == Some(ids.forest)
                && tree_at(&world, pos, ids.forest).is_none()
            {
                空地 = Some(pos);
                break;
            }
        }
        if 空地.is_some() {
            break;
        }
    }
    let pos = 空地.expect("这个测试世界里找不到一格没有树的森林——断言会空跑");

    // Act
    world
        .trees
        .set(pos, TreeDeviation::planted(TreeSpecies::Pine, Tick(0)));

    // Assert
    let 读回来 = 往返(&world);
    let tree = tree_at(&读回来, pos, ids.forest).expect("种下的树存读一轮之后必须还在");
    assert_eq!(tree.species, TreeSpecies::Pine, "树种没有随存档往返");
    assert!(
        !tree.fruit_ready,
        "刚种下的树不该立刻就能采果——那是一条零成本刷种子的路径"
    );
}

#[test]
fn 采过果的树要等满一个周期才重新有果() {
    // Arrange
    let (mut world, ids) = test_world();
    let pos = 一棵派生出来的树(&world, ids.forest);
    let species = tree_at(&world, pos, ids.forest)
        .expect("前提：有树")
        .species;

    // Act：在 t=100 采一次。
    world.clock = Tick(100);
    world.trees.set(
        pos,
        TreeDeviation {
            species: Some(species),
            harvested_at: Some(world.clock),
        },
    );

    // Assert：当场没果、快到了还是没果、满一个周期才有。
    assert!(
        !tree_at(&world, pos, ids.forest)
            .expect("树还在")
            .fruit_ready
    );
    world.clock = Tick(100 + ll_world::tree::FRUIT_REGROW_TICKS - 1);
    assert!(
        !tree_at(&world, pos, ids.forest)
            .expect("树还在")
            .fruit_ready
    );
    world.clock = Tick(100 + ll_world::tree::FRUIT_REGROW_TICKS);
    assert!(
        tree_at(&world, pos, ids.forest)
            .expect("树还在")
            .fruit_ready,
        "满一个周期之后果子该长回来了"
    );

    // 采果**不该**把树砍掉。
    assert_eq!(
        tree_at(&world, pos, ids.forest).expect("树还在").species,
        species
    );
}

#[test]
fn 没动过的格子在偏差表里一条记录都没有() {
    // **这就是「默认派生，只存偏差」那句话的可执行版本。**
    //
    // 一个刚生成的 48×48 世界里有几百棵派生出来的树（下面断言了这一点），
    // 而偏差表是**空的**——这正是这套架构存在的全部理由。缺了这一条，
    // 「只存偏差」就退化成一句没人验过的口号。
    let (world, ids) = test_world();
    let 树 = (0..world.size.height() as i32)
        .flat_map(|y| (0..world.size.width() as i32).map(move |x| (x, y)))
        .filter(|(x, y)| tree_at(&world, world.size.wrap(*x, *y), ids.forest).is_some())
        .count();
    assert!(树 > 0, "前提：这个世界里真的有树可数");
    assert!(
        world.trees.is_empty(),
        "刚生成的世界里有 {树} 棵树，偏差表却有 {} 条记录——「默认派生」没有生效",
        world.trees.len()
    );
}

#[test]
fn 砍掉末尾树木字段的旧字节流用postcard解不回新形状() {
    // **`CURRENT_SCHEMA_VERSION` 6 → 7 那条不兼容声明的字节级证据。**
    //
    // 存档主体走 postcard（non-self-describing，按声明顺序定位、不带
    // 字段名）。`WorldState` 末尾新增的 `trees` 在空表时恰好编码成
    // **一个字节**（`BTreeMap` 的长度 varint `0`）——砍掉它就是加 `trees`
    // 之前的那份字节流。
    //
    // 与 `ll_world::entity::Agent` 的
    // `少一个末尾字段的旧形状用postcard解不回新形状` 同一条写法：断言的是
    // **具体错误**（`DeserializeUnexpectedEnd`），不是「大概会失败」——
    // 否则这条断言可能在某天因为一个完全无关的解码失败而继续绿着。
    //
    // 读档管线**根本走不到这一步**（它在版本比较那里就明确拒绝了），
    // 那一半的证据在 `crates/ll-content/src/save_file.rs` 的
    // `树木批次之前的老存档被明确拒绝而不是静默按新布局误解析`。本条回答的
    // 是「万一绕过了版本检查会怎样」：**报错，不是静默误解析**。
    // Arrange
    let (world, _) = test_world();
    assert!(
        world.trees.is_empty(),
        "本条依赖「空偏差表恰好编码成一个字节」，非空表要改这条断言的算法"
    );
    let full = postcard::to_allocvec(&world).expect("WorldState 可序列化");

    // 对照组：完整字节流解得回来，且世界摘要相同。**没有这一半，
    // 「截断之后失败」什么都证不了**（随便截一刀都会失败）。
    let round: WorldState = postcard::from_bytes(&full).expect("完整字节流必须解得回来");
    assert_eq!(round.hash(), world.hash(), "对照组：完整往返必须无损");

    // Act：砍掉末尾那一个字节 = 加 `trees` 之前的那份字节流。
    let old_shape = &full[..full.len() - 1];

    // Assert：解不回来，**且红的理由必须是「字节不够了」**。
    let decoded: Result<WorldState, _> = postcard::from_bytes(old_shape);
    let Err(err) = decoded else {
        panic!("旧形状不该解得回来");
    };
    assert_eq!(
        err,
        postcard::Error::DeserializeUnexpectedEnd,
        "旧形状必须因为『缓冲区提前结束』而失败，实际 {err:?}"
    );
}
