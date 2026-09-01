//! 端到端实测：**尸体与遗物平铺之后，一格的交互列表会变多长。**
//!
//! 尸体平铺批次（所有者原话「尸体会变成物品，然后原本的物品和尸体都会
//! 放在一格子内的掉落物列表里」）把一次死亡从产出 **1 条容器**改成产出
//! **1 + N 条**独立地面物品。任务书要求勘查「一格可能有很多堆，列表会
//! 不会太长」，并且**不许顺手改交互列表的规则**（所有者已裁定：脚下 +
//! 相邻八格、无条件弹）。
//!
//! 本文件是那次勘查的**可复现证据**，不是一句口头结论。
//!
//! # 勘查结论（三条，逐条由下面的测试钉住）
//!
//! 1. **列表长度不等于地面堆数，等于这一格上不同 `def` 的个数**——
//!    `interact_entries` 对 `InteractTarget::Loose` **按 `def` 去重**
//!    （同 `def` 的第二堆按下去仍然会捡到第一堆，列出来是骗玩家）。
//!    这条去重是平铺**之前就已经存在**的规则，本批次一个字没改。
//! 2. 因此一具尸体从「1 行容器」变成「1 行尸体 + 每**种**遗物各 1 行」
//!    ——上界是死者身上物品的**种类数**，不是堆数。本体哥布林的出生装备
//!    是两种（粗劣匕首 + 箭），实测 3 行。
//! 3. **同一格堆着两具同物种的尸体只占 1 行**（`def` 相同，被同一条
//!    去重收走）——战场堆尸不会让列表线性膨胀。
//!
//! 全程走真实 `mods/` 内容与 `build_new_world`，不启动窗口、不模拟
//! 键盘（ADR 0025）。

use ll_core::ident::NamespacedId;
use ll_core::torus::TorusPos;
use ll_game::content::LoadedContent;
use ll_game::player_action::{InteractTarget, TalkLookup, interact_entries};
use ll_game::world::{GameWorld, build_new_world};
use ll_world::item::{GroundItemStack, ItemStack};

/// 固定种子，理由同 `door_interaction.rs` 的同名常量。
const SEED: u64 = 20260829;

fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-corpse-flat-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

fn test_world(content: &LoadedContent) -> (GameWorld, TorusPos) {
    let game_world = build_new_world(
        content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家实体必然存在")
        .pos;
    (game_world, pos)
}

fn index(content: &LoadedContent, id: &str) -> ll_core::ident::ContentIndex {
    content
        .registry
        .get(&NamespacedId::parse(id).expect("合法标识符"))
        .unwrap_or_else(|| panic!("本体应当注册过 {id}"))
}

/// 往 `pos` 上摆一堆躺着的普通地面物品——平铺之后尸体与遗物在数据上
/// 与它完全同形（`contents` 空、`placed` 假），因此本帮手足以复现平铺
/// 之后那一格的形状，不必真的杀一个 NPC（那需要 `ll-mod` 的攻击链，
/// 已在 `crates/ll-mod/tests/example_mod_starting_items.rs` 端到端验过）。
fn drop_at(game_world: &mut GameWorld, pos: TorusPos, def: ll_core::ident::ContentIndex) {
    let clock = game_world.world.clock;
    game_world.world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(def, 1),
        dropped_at: clock,
        contents: Vec::new(),
        placed: false,
    });
}

#[test]
fn 一具哥布林尸体平铺后脚下交互列表是三行() {
    // 实测（任务书要求的那次勘查）：本体哥布林的出生装备是粗劣匕首 ×1
    // 与箭 ×2 两**种**，平铺之后这一格有 3 条地面物品（尸体 + 两堆
    // 遗物），交互列表因此是 3 行——尸体一行、匕首一行、箭一行。
    //
    // 反例（真实执行过）：把 interact_entries 里 Loose 那一支整个去掉
    // （只列 Facility/Container/Door），本条的 3 变成 0，当场红。
    // Arrange
    let content = test_content();
    let (mut game_world, pos) = test_world(&content);
    let before = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    )
    .len();
    let corpse = index(&content, "lostland:goblin.corpse");
    let dagger = index(&content, "examplemod:crude_dagger");
    let arrow = index(&content, "examplemod:arrow");
    drop_at(&mut game_world, pos, corpse);
    drop_at(&mut game_world, pos, dagger);
    drop_at(&mut game_world, pos, arrow);

    // Act
    let rows = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    );

    // Assert
    assert_eq!(
        rows.len(),
        before + 3,
        "尸体一行 + 每种遗物各一行；实测 rows = {rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| !matches!(row, InteractTarget::Container { .. })),
        "尸体不再是容器，这一格不该出现任何 Container 行"
    );
}

#[test]
fn 同一格两具同物种的尸体只占交互列表一行() {
    // 战场堆尸不会让列表线性膨胀：interact_entries 对 Loose 按 def
    // 去重（这条规则**平铺之前就存在**，本批次一个字没改），两具哥布林
    // 尸体 def 相同，只列一行。
    //
    // 反例（人工验证过）：把 interact_entries 里那条
    // `!rows.iter().any(|row| matches!(row, InteractTarget::Loose { def: seen }
    // if *seen == def))` 去掉，本条的 1 变成 2，当场红。
    // Arrange
    let content = test_content();
    let (mut game_world, pos) = test_world(&content);
    let before = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    )
    .len();
    let corpse = index(&content, "lostland:goblin.corpse");
    drop_at(&mut game_world, pos, corpse);
    drop_at(&mut game_world, pos, corpse);

    // Act
    let rows = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    );

    // Assert
    assert_eq!(
        rows.len(),
        before + 1,
        "同 def 的第二具尸体不该再占一行；实测 rows = {rows:?}"
    );
    assert_eq!(
        game_world
            .world
            .ground_items
            .iter()
            .filter(|item| item.pos == pos)
            .count(),
        2,
        "地上确实是两条——列表短是去重的结果，不是东西少了"
    );
}

#[test]
fn 列表长度随物品种类数增长而不是随堆数增长() {
    // 把上面两条合成一句可度量的话：往同一格上摆 6 堆、但只有 3 种，
    // 列表仍然是 3 行。这就是「列表会不会太长」这个问题的确切答案——
    // 上界是这一格的**物品种类数**。
    // Arrange
    let content = test_content();
    let (mut game_world, pos) = test_world(&content);
    let before = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    )
    .len();
    let defs = [
        index(&content, "lostland:goblin.corpse"),
        index(&content, "examplemod:crude_dagger"),
        index(&content, "examplemod:arrow"),
    ];
    for def in defs {
        drop_at(&mut game_world, pos, def);
        drop_at(&mut game_world, pos, def);
    }

    // Act
    let rows = interact_entries(
        &game_world.world,
        pos,
        game_world.player,
        TalkLookup::none(),
    );

    // Assert
    assert_eq!(
        game_world
            .world
            .ground_items
            .iter()
            .filter(|item| item.pos == pos)
            .count(),
        6,
        "前置条件：地上真的是六堆"
    );
    assert_eq!(rows.len(), before + defs.len(), "列表只有三种物品那么长");
}
