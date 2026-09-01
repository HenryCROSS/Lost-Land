//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-race-starting-item` 这个新脚本 API 真的能被
//! `mods/example_mod/races.json5` 调用，且真实注册的出生物品：
//!
//! 1. 能被 [`ll_mod::race::starting_inventory`] 转换成背包物品；
//! 2. 一旦发给一个真实存在的实体（哥布林），死亡结算真的会把它们
//!    （连同已装备物品）与尸体一起**平铺**到死者倒下的那一格
//!    （[`Intent::Attack`] → `resolve_with_skills_traits_pools_and_items`
//!    → `crate::resolve` 内部的 `append_corpse_drop`，本文件看不到那个
//!    私有函数，只能通过端到端的公开入口验证它的效果）；
//! 3. 尸体是一件**普通可拾取、可堆叠**的物品——[`Intent::PickUp`] 捡得
//!    走，两具同物种的尸体在背包里堆成一堆。
//!
//! # 第 2、3 条被尸体平铺批次翻转过，如实记录
//!
//! 此前尸体是**容器**（`GroundItemStack::contents` 装着死者遗物），第
//! 3 条写的是「尸体不会被普通 `Intent::PickUp` 吞掉，只能通过
//! `Intent::Loot` 搜刮」。那条形状撞上一个死结：`resolve_pick_up` 把
//! `contents` 非空的地面物品整体排除，于是尸体**根本捡不起来**。
//! 项目所有者的裁定「尸体会变成物品，然后原本的物品和尸体都会放在一
//! 格子内的掉落物列表里」解开了它。
//!
//! 容器这条路径本身**没有被删**（箱子是它将来的消费者），本文件另有
//! 两条手工摆容器的测试守着它——见
//! `普通拾取仍然跳过真容器不吞掉其内容物` 与
//! `搜刮真容器后内容物进入背包且容器从地面消失`。
//!
//! ——NPC 生命周期批次（NPC 带物品 → 死亡掉落 → 尸体 → 老化回收）
//! 端到端的那份证据,与 `crates/ll-mod/tests/example_mod_equipment.rs`
//! 同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法（见其
//! 模块文档），ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实
//! mod 脚本为证」。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::{RaceTable, starting_inventory};
use ll_sim::apply::apply;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{NoTraitGrants, NoTraits};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_equipment.rs::RealModsHandle`。
struct RealModsHandle {
    item: ItemTable,
    race: RaceTable,
    goblin_id: ContentIndex,
    crude_dagger_id: ContentIndex,
    arrow_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        race,
        item,
        ..
    } = session;
    let examplemod_id = NamespacedId::parse("examplemod:self").unwrap();
    let examplemod_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == examplemod_id)
        .map(|(_, status)| status);
    assert_eq!(
        examplemod_status,
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/races.json5 注册"))
    };

    RealModsHandle {
        goblin_id: resolve("examplemod:goblin"),
        crude_dagger_id: resolve("examplemod:crude_dagger"),
        arrow_id: resolve("examplemod:arrow"),
        item,
        race,
    }
}

fn test_world() -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件")
}

/// 造一个占位实体——理由同 `example_mod_equipment.rs::spawn_agent`，
/// 额外携带 `race`/`health`/`stats`（死亡掉落测试需要真的能被杀死、
/// 真的能在攻击公式里算出非零伤害）。
#[allow(clippy::too_many_arguments)]
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    stats: BaseStats,
    health: i32,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    pos_offset: (i32, i32),
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5 + pos_offset.0, 5 + pos_offset.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        home: None,
    })
}

/// 把 `intent` 结算并应用到 `world`——本文件全部测试共用的一步。
fn resolve_and_apply(world: &mut WorldState, intent: &Intent, items: &ItemTable) {
    let effects = resolve_with_skills_traits_pools_and_items(
        world,
        intent,
        &NoSkills,
        &NoTraitGrants,
        &NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        items,
    );
    for effect in &effects {
        apply(world, effect);
    }
}

#[test]
fn 真实注册的哥布林出生物品是粗制匕首一把与箭两支() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");

    // Assert
    assert_eq!(
        view.starting_items,
        &[(handle.crude_dagger_id, 1), (handle.arrow_id, 2)]
    );
}

#[test]
fn 真实哥布林出生物品转换成对应的两条物品堆() {
    // Arrange
    let handle = load_real_mods();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");

    // Act
    let inventory = starting_inventory(&view, &handle.item);

    // Assert：粗劣匕首出生就是**满耐久 20**（mods/example_mod/items.json5
    // 声明的 max_durability），不是「没有耐久概念」——「新造出来的物品
    // 带多少耐久」这条共同规则在出生装备这一路的端到端证据，见
    // `ll_world::item::ItemStack::freshly_made`。箭矢可堆叠、没有耐久
    // 上限，因此仍然是 None（可堆叠物品不能带耐久，注册期硬校验）。
    assert_eq!(
        inventory,
        vec![
            ItemStack::with_durability(handle.crude_dagger_id, 1, 20),
            ItemStack::new(handle.arrow_id, 2),
        ]
    );
}

#[test]
fn 携带出生物品的哥布林被杀死后尸体与遗物平铺进同一格() {
    // 死亡掉落端到端验收（尸体平铺批次改写）：给一个真实存在的哥布林
    // 发真实 mod 注册的出生物品 → 用真实的 Intent::Attack 结算杀死它
    // → 断言地上出现的是**尸体一条 + 每堆遗物各一条**，全在同一格，
    // 且尸体的 contents 是空的（它不再是容器）。数量守恒：不多不少。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");
    let loadout = starting_inventory(&view, &handle.item);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1, // 一击必杀,不依赖具体伤害公式的精确取值。
        loadout.clone(),
        BTreeMap::new(),
        (0, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50, // 调整值 (50-10)/2 = 20,远高于 1 点血量。
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (0, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：受害者已死；地上恰好 1 + loadout.len() 条（尸体一条、
    // 每堆遗物各一条），全在死者倒下的那一格；尸体的 contents 是空的。
    assert!(world.actors.get(victim).is_none());
    assert_eq!(
        world.ground_items.len(),
        1 + loadout.len(),
        "尸体一条 + 每堆遗物各一条"
    );
    let pos = world.ground_items[0].pos;
    assert!(
        world.ground_items.iter().all(|item| item.pos == pos),
        "所有者原话「都会放在一格子内」——全部落在同一格"
    );
    assert!(
        world
            .ground_items
            .iter()
            .all(|item| item.contents.is_empty()),
        "尸体不再是容器，遗物本来就不是——一条 contents 非空的都不该有"
    );
    // 第一条是尸体本身（append_corpse_drop 先推尸体再推遗物）。
    let corpse = &world.ground_items[0];
    assert!(
        !loadout.iter().any(|stack| stack.def == corpse.stack.def),
        "夹具前提：尸体的 def 与任何一件遗物都不同，下面按 def 比对才有意义"
    );
    // 其余各条逐条等于死者结算前的背包，顺序不变。
    let dropped: Vec<_> = world.ground_items[1..]
        .iter()
        .map(|item| item.stack)
        .collect();
    assert_eq!(dropped, loadout, "遗物逐条守恒，既没消失也没多出");
}

#[test]
fn 携带已装备物品的哥布林被杀死后装备也掉在同一格() {
    // 装备也要掉：死者身上穿着的物品（Agent::equipment）同样要平铺到
    // 地上，不只是背包（Agent::inventory）。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let equipped = ItemStack::new(handle.crude_dagger_id, 1);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        Vec::new(),
        BTreeMap::from([(EquipSlot::MAIN_HAND, equipped)]),
        (1, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (1, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：地上两条——尸体 + 那把匕首，同一格。
    assert_eq!(world.ground_items.len(), 2, "尸体一条 + 匕首一条");
    assert_eq!(world.ground_items[0].pos, world.ground_items[1].pos);
    assert_eq!(world.ground_items[1].stack, equipped);
    assert!(world.ground_items[0].contents.is_empty());
}

#[test]
fn 空手死者也产出一具可拾取的尸体() {
    // **这条测试的期望被尸体平铺批次翻转了。** 旧期望是「背包与装备栏
    // 都空的死者不占地面物品条目」，理由是当时尸体是容器、一具空容器
    // 谁也搜刮不了。平铺之后尸体是一件普通可拾取物品，那条理由作废
    // ——见 append_corpse_drop 文档「空手死者**也**产出尸体」一节。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        Vec::new(),
        BTreeMap::new(),
        (2, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (2, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：恰好一条，就是尸体本身，contents 空。
    assert_eq!(world.ground_items.len(), 1, "空手死者也留下一具尸体");
    assert!(world.ground_items[0].contents.is_empty());
    assert_eq!(world.ground_items[0].stack.count, 1);
}

#[test]
fn 普通拾取现在能把尸体捡进背包() {
    // **这条测试的期望被尸体平铺批次翻转了。** 旧期望是「Intent::PickUp
    // 不是尸体的合法目标」，理由是普通拾取只会搬走 GroundItemStack.stack
    // 这个壳、把 contents 里的战利品丢在地上永久不可达。那个死结正是
    // 所有者要解开的：「尸体会变成物品」。平铺之后尸体的 contents 恒空，
    // 容器排除对它不再生效。
    //
    // 容器排除本身**没有被删掉**，仍然咬得住真容器——见下面
    // `普通拾取仍然跳过真容器不吞掉其内容物` 那条。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");
    let loadout = starting_inventory(&view, &handle.item);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        loadout.clone(),
        BTreeMap::new(),
        (3, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (3, 0),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );
    let before = world.ground_items.len();
    assert_eq!(before, 1 + loadout.len(), "前置条件：尸体与遗物已经平铺");

    // Act：攻击者站在尸体所在格，点名要捡那具尸体本身。
    let corpse_def = world.ground_items[0].stack.def;
    let corpse_pos = world.ground_items[0].pos;
    resolve_and_apply(
        &mut world,
        &Intent::PickUp {
            actor: attacker,
            pos: (corpse_pos.x(), corpse_pos.y()),
            def: corpse_def,
        },
        &handle.item,
    );

    // Assert：尸体离开地面进了背包，遗物原样留在地上（这一次只捡了
    // 尸体这一堆）。
    assert_eq!(world.ground_items.len(), before - 1);
    assert!(
        !world
            .ground_items
            .iter()
            .any(|item| item.stack.def == corpse_def),
        "尸体应当已经不在地上了"
    );
    let inventory = &world.actors.get(attacker).unwrap().inventory;
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].def, corpse_def);
    assert_eq!(inventory[0].count, 1);
}

#[test]
fn 两具同物种的尸体捡进背包后堆成一堆() {
    // `ll_mod::corpse_item::CORPSE_STACK_LIMIT` 此前是一条**观察不到**的
    // 诚实声明——尸体是容器、捡不起来，那条判定路径没有任何调用点能走
    // 到（见 append_corpse_drop 旧文档「两具尸体今天仍然不会被合并」）。
    // 平铺之后它第一次真的在跑：两具空手哥布林的尸体 def 相同、
    // durability 同为 None、owner 同为 Unowned，can_merge 三项全等。
    // Arrange：两个空手哥布林先后死在同一格。
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (6, 0),
    );
    for _ in 0..2 {
        let victim = spawn_agent(
            &mut world,
            handle.goblin_id,
            BaseStats::BASELINE,
            1,
            Vec::new(),
            BTreeMap::new(),
            (6, 0),
        );
        resolve_and_apply(
            &mut world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &handle.item,
        );
    }
    assert_eq!(world.ground_items.len(), 2, "前置条件：两具尸体各占一条");
    let corpse_def = world.ground_items[0].stack.def;
    let corpse_pos = world.ground_items[0].pos;

    // Act：连捡两次。
    for _ in 0..2 {
        resolve_and_apply(
            &mut world,
            &Intent::PickUp {
                actor: attacker,
                pos: (corpse_pos.x(), corpse_pos.y()),
                def: corpse_def,
            },
            &handle.item,
        );
    }

    // Assert：地上空了，背包里是**一堆 x2**，不是两堆 x1。
    assert!(world.ground_items.is_empty());
    let inventory = &world.actors.get(attacker).unwrap().inventory;
    assert_eq!(inventory.len(), 1, "两具尸体应当堆成一堆");
    assert_eq!(inventory[0].count, 2);
}

#[test]
fn 普通拾取仍然跳过真容器不吞掉其内容物() {
    // 容器排除这道闸门**没有被尸体平铺批次删掉**，只是今天没有任何
    // 生产路径会造出它的目标（尸体不再是容器，箱子还没开工）。这里
    // 手工摆一个 contents 非空的地面物品，证明闸门本身还咬得住——
    // 否则箱子那批开工时会发现保护早就没了、而且没人知道是哪一批
    // 弄丢的。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (7, 0),
    );
    let pos = world.actors.get(actor).unwrap().pos;
    let chest_def = handle.crude_dagger_id; // 借一个真实注册的 def 当箱子的壳
    let inside = ItemStack::new(handle.arrow_id, 5);
    world.ground_items.push(ll_world::item::GroundItemStack {
        pos,
        stack: ItemStack::new(chest_def, 1),
        dropped_at: Tick(0),
        contents: vec![inside],
        placed: false,
    });

    // Act：点名要捡那个容器的壳。
    resolve_and_apply(
        &mut world,
        &Intent::PickUp {
            actor,
            pos: (pos.x(), pos.y()),
            def: chest_def,
        },
        &handle.item,
    );

    // Assert：容器原封不动，背包空——内容物没有被丢在地上永久不可达。
    assert_eq!(world.ground_items.len(), 1);
    assert_eq!(world.ground_items[0].contents, vec![inside]);
    assert!(world.actors.get(actor).unwrap().inventory.is_empty());
}

#[test]
fn 搜刮真容器后内容物进入背包且容器从地面消失() {
    // resolve_loot 同样保留、同样今天没有生产者（尸体已经不是容器）。
    // 手工摆一个容器验证这条路径本身还在工作，理由同上一条：箱子那批
    // 要直接用它。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (8, 0),
    );
    let pos = world.actors.get(actor).unwrap().pos;
    let inside = vec![
        ItemStack::with_durability(handle.crude_dagger_id, 1, 20),
        ItemStack::new(handle.arrow_id, 2),
    ];
    world.ground_items.push(ll_world::item::GroundItemStack {
        pos,
        stack: ItemStack::new(handle.crude_dagger_id, 1),
        dropped_at: Tick(0),
        contents: inside.clone(),
        placed: false,
    });

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Loot {
            actor,
            pos: (pos.x(), pos.y()),
        },
        &handle.item,
    );

    // Assert：容器从地面消失，内容物原样进了背包。
    assert!(world.ground_items.is_empty());
    assert_eq!(world.actors.get(actor).unwrap().inventory, inside);
}
