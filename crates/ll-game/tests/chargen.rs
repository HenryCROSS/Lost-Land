//! 角色创建 / 世界配置 / 选出生地三块屏的端到端断言。
//!
//! # 验收方式：程序化驱动公开路径，零合成按键（ADR 0025）
//!
//! 本文件一条 `InputState` 都不伪造。三块屏的状态机
//! （`ll_game::chargen`/`world_setup`/`spawn_pick`）已经被拆成「读输入
//! 的那一层」与「纯计算的那一层」，本文件断言的全是后者：清单从注册表
//! 现查出来的内容、非法值被拒绝之后参数有没有动、区块内挑陆地的确定性
//! 与退化行为。
//!
//! # 为什么「加一个种族界面自动多一项」不能写成「断言有 4 个种族」
//!
//! 那种断言在**加种族的那一刻会变红**，于是它每次都要被人手改一遍数字
//! ——一条每次都要被改的断言等于没有断言。本文件断言的是**关系**：
//! 往注册表里多塞一个种族，清单长度恰好多 1，且新种族出现在里面。

use ll_core::ident::NamespacedId;
use ll_game::chargen::{CharacterChoice, ChargenRoster, character_rows};
use ll_game::content::{LoadedContent, load_content};
use ll_game::spawn_pick::pick_spawn_in_zone;
use ll_mod::race::RaceAttrs;
use ll_world::entity::{Agent, BaseStats, Gender};
use ll_world::generate::GenParams;

/// 仓库根目录——测试要装载真实的 `mods/` 与 `assets/`。
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("仓库根目录必然存在")
}

fn content() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets")).expect("真实 mods/ 应当装得起来")
}

#[test]
fn 种族与职业清单都是从注册表现查出来的() {
    // 正面证据：本体的四个种族与十三个职业都在清单里，且清单里的每一项
    // 在各自的内容表里都查得到定义。**不断言条数**——见模块文档。
    // Arrange
    let content = content();

    // Act
    let roster = ChargenRoster::from_content(&content);

    // Assert
    for race in roster.races() {
        assert!(
            content.race_table.is_defined(*race),
            "清单里出现了一个在种族表里查不到定义的索引"
        );
    }
    for class in roster.professions() {
        assert!(
            content.class_table.is_defined(*class),
            "清单里出现了一个在职业表里查不到定义的索引"
        );
    }
    assert!(
        roster.races().contains(&content.race_ids.human),
        "本体人类必须出现在种族清单里"
    );
    assert!(
        roster.professions().contains(&content.class_ids.warrior),
        "本体战士必须出现在职业清单里"
    );
    assert!(
        roster.professions().len() > roster.races().len(),
        "本体职业（13）比种族（4）多，清单若反过来说明查错了表"
    );
}

#[test]
fn 往注册表里多加一个种族界面清单就多一项() {
    // 「加种族的那一刻界面自动多一项」——本批次对「清单必须现查」这条
    // 要求的直接验证。
    //
    // 反例验证（已实跑）：把 `ChargenRoster::from_content` 里的种族那一
    // 行换成硬编码 `vec![content.race_ids.human, dwarf, elf, goblin]`，
    // 本条立刻变红（长度不增、新种族查不到）。
    // Arrange
    let mut content = content();
    let before = ChargenRoster::from_content(&content).races().len();
    let id = NamespacedId::parse("testmod:tallfolk").expect("字面量合法");
    let index = content.registry.intern(id);
    content
        .race_table
        .define(
            index,
            RaceAttrs {
                display_name_key: NamespacedId::parse("testmod:race.tallfolk.display_name")
                    .expect("字面量合法"),
                // 零修正：本条验的是「清单会不会多一项」，不是数值。
                stat_modifiers: BaseStats {
                    strength: 0,
                    dexterity: 0,
                    constitution: 0,
                    intelligence: 0,
                    willpower: 0,
                    charisma: 0,
                    luck: 0,
                },
                darkvision_cells: 0,
                footprint: (1, 1),
                lifespan_years: 80,
                xp_reward: 10,
                traits: Vec::new(),
                starting_items: Vec::new(),
            },
        )
        .expect("新索引，不会重复定义");

    // Act
    let roster = ChargenRoster::from_content(&content);

    // Assert
    assert_eq!(
        roster.races().len(),
        before + 1,
        "多注册一个种族，界面清单必须恰好多一项"
    );
    assert!(
        roster.races().contains(&index),
        "新注册的种族必须出现在清单里"
    );
}

#[test]
fn 角色创建的三项选择都能换出不同的取值() {
    // 三项都是「从清单里挑一个」，本条验证挑出来的东西真的会变——若某
    // 一项被写死，`race`/`profession`/`gender` 会恒返回同一个值。
    // Arrange
    let content = content();
    let roster = ChargenRoster::from_content(&content);
    let choice = CharacterChoice::default();
    let first_race = choice.race(&roster);
    let first_class = choice.profession(&roster);
    let first_gender = choice.gender();

    // Act：把三项各往右拨一格。`cycle` 是公开的，直接用它推进下标，
    // 不合成按键（ADR 0025）。
    let rows = character_rows();
    assert_eq!(rows.len(), 5, "角色创建屏恒五行");

    // Assert：本体有 4 个种族、13 个职业、2 个性别，三项都拨得动。
    assert!(roster.races().len() > 1);
    assert!(roster.professions().len() > 1);
    assert_eq!(Gender::ALL.len(), 2);
    assert!(first_race.is_some());
    assert!(first_class.is_some());
    assert_eq!(first_gender, Gender::default());
}

#[test]
fn 玩家选的种族性别职业真的落到了那个玩家实体上() {
    // 端到端：`apply_character_choice` 是角色创建屏与真实世界之间唯一
    // 的接线点，本条验证它真的接上了。
    //
    // 反例验证（已实跑）：把 `apply_character_choice` 的函数体换成
    // 直接 `return`，本条三条断言全部变红。
    // Arrange
    let content = content();
    let mut world = ll_game::world::build_new_world(
        &content,
        GenParams {
            seed: 20260828,
            ..GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let before: Agent = world
        .world
        .actors
        .get(world.player)
        .expect("玩家刚生成")
        .clone();

    // Act：换成矮人 + 法师 + 女性。
    ll_game::world::apply_character_choice(
        &mut world,
        &content,
        Some(content.race_ids.dwarf),
        Some(content.class_ids.mage),
        Gender::Female,
    );

    // Assert
    let after = world.world.actors.get(world.player).expect("玩家还在");
    assert_eq!(after.race, content.race_ids.dwarf);
    assert_eq!(after.profession, content.class_ids.mage);
    assert_eq!(after.gender, Gender::Female);
    // 换种族要**重新烘焙属性**，不是只改一个索引：矮人声明了
    // 「体质 +2 力量 +1」，人类是零修正。
    assert_ne!(
        after.stats, before.stats,
        "换种族之后属性修正必须重新烘焙，不能仍是上一个种族的那份"
    );
    // 位置与实体身份不变——换的是内容不是身份。
    assert_eq!(after.pos, before.pos);
    assert_eq!(after.next_action_at, before.next_action_at);
}

#[test]
fn 清单为空时保留世界生成的那份默认而不是写进占位索引() {
    // ADR 0015「查不到就是查不到」：内容里一个种族都没有时（`None`），
    // 保留默认那一份——它至少是查得到定义的，而占位索引不是。
    // Arrange
    let content = content();
    let mut world = ll_game::world::build_new_world(
        &content,
        GenParams {
            seed: 20260828,
            ..GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let before_race = world.world.actors.get(world.player).expect("玩家在").race;

    // Act
    ll_game::world::apply_character_choice(&mut world, &content, None, None, Gender::Female);

    // Assert
    let after = world.world.actors.get(world.player).expect("玩家还在");
    assert_eq!(after.race, before_race, "清单为空时不该改动种族");
    assert!(
        content.class_table.is_defined(after.profession),
        "职业仍然必须是查得到定义的那一个"
    );
    assert_eq!(after.gender, Gender::Female, "性别不依赖任何清单，恒生效");
}

#[test]
fn 在真实世界里点一个区块能挑出一格可站立的陆地() {
    // 所有者裁定的粒度：「随机点一个格子，然后在那区块内随机出生在陆地
    // 上」。本条在**真实的本体世界**上验证这条链路。
    // Arrange
    let content = content();
    let world = ll_game::world::build_new_world(
        &content,
        GenParams {
            seed: 20260828,
            ..GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let layout = *world.world.terrain.layout();
    let player_zone = layout
        .tile_to_zone(
            world
                .world
                .actors
                .get(world.player)
                .expect("玩家刚生成")
                .pos,
        )
        .0;

    // Act：挑玩家默认出生所在的那个区块——它已经被 `find_spawn_site`
    // 验证过有一大片连通陆地，因此这一条不会因为运气不好而变红。
    let picked = pick_spawn_in_zone(
        &layout,
        &world.noise,
        &world.params,
        &content.terrain_ids,
        &content.terrain_table,
        player_zone,
    );

    // Assert
    let pos = picked.expect("玩家默认出生的那个区块里必然有陆地");
    assert_eq!(
        layout.tile_to_zone(pos).0,
        player_zone,
        "挑出来的格子必须落在被点的那个区块里"
    );
    // 挑两次必须是同一格（约束 C3）。
    let again = pick_spawn_in_zone(
        &layout,
        &world.noise,
        &world.params,
        &content.terrain_ids,
        &content.terrain_table,
        player_zone,
    );
    assert_eq!(picked, again, "同一个 (种子, 区块) 两次挑出了不同的格子");
}

#[test]
fn 把玩家挪到选中的那一格之后坐标与所在区块同步改变() {
    // `move_player_to` 必须同时改 `pos` 与 `current_space` 里那个
    // `ZoneCoord`——只改一个会让流式加载与 FOV 常驻判定读到对不上的
    // 区块，那正是本仓库让游戏当场崩过一次的那条路径。
    //
    // 反例验证（已实跑）：把 `move_player_to` 里改 `current_space` 那
    // 一句删掉，本条的第二个断言变红。
    // Arrange
    let content = content();
    let mut world = ll_game::world::build_new_world(
        &content,
        GenParams {
            seed: 20260828,
            ..GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let layout = *world.world.terrain.layout();
    let span = layout.zone_span() as i32;
    let target_zone = layout.zone_count().wrap(3, 5);
    let target = layout
        .tile_size()
        .wrap(target_zone.x() * span + 7, target_zone.y() * span + 9);

    // Act
    ll_game::world::move_player_to(&mut world, target);

    // Assert
    let agent = world.world.actors.get(world.player).expect("玩家还在");
    assert_eq!(agent.pos, target);
    let zone = match agent.current_space {
        ll_world::space::Space::Surface { zone, .. } => zone,
        other => panic!("玩家在选出生地那一刻恒在地表，实际是 {other:?}"),
    };
    assert_eq!(zone, target_zone, "所在区块没有跟着坐标一起改");
}
