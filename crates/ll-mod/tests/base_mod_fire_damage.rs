//! 端到端验证：本体第二个伤害类别 `lostland:fire` 真的被本体内容用上，
//! 且它与全局默认的 `lostland:physical` **确实不是一回事**。
//!
//! 在此之前本体只有 `lostland:physical` 一个伤害类别，而它正好就是
//! 全局默认类别——后果是物品的 `damage_category` 与 `rule_modifiers`
//! 两条字段在本体侧写不出任何有意义的值，`ll_mod::content_audit` 为此
//! 挂了两条字段豁免。`mods/lostland/damage_categories.json5` 落地之后
//! 两条豁免都摘掉了，本文件是那两条摘除的证据：
//!
//! 1. **武器这一路**：`lostland:forge_brand`（锻炉烙铁）声明
//!    `damage_category: "lostland:fire"`，真的存进了物品表；
//! 2. **抗性这一路**：`lostland:forge_apron`（锻炉围裙）声明对
//!    `lostland:fire` 的 6 点减伤，真的存进了物品表，且归在本体加值
//!    类型 `lostland:gear` 下（本体加值类型名册批次：此前是未分类
//!    共享桶，见 `mods/lostland/modifier_types.json5` 文件头）；
//! 3. **两者合起来真的改变结算**：穿着围裙挨烙铁打，伤害正好少 6 点；
//!    而同一件围裙挨**铁短剑**（不声明伤害类别，走全局默认那一类）打
//!    时那 6 点一点都不减——这一条是整个文件的重点：如果新类别挡起来
//!    跟物理一模一样，那它只是换了个名字的物理，不值得注册。
//!
//! 与 `example_mod_resistance.rs` 同一套「装载整个 `mods/` 目录」手法
//! （见其模块文档），区别是那份证明「第三方 mod 做得到」，本文件证明
//! 「**本体自己**的内容真的在用这条能力」，后者更强（ADR 0018）。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::RaceTable;
use ll_mod::trait_def::TraitTable;
use ll_sim::apply::apply;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories;
use ll_sim::rule_modifier::RuleModifier;
use ll_sim::skill::NoSkills;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引，理由同 `example_mod_resistance.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    /// 本体人类——攻防双方都用它，三族里唯一零属性修正的那个，免得
    /// 种族修正混进伤害数字里。
    human_id: ContentIndex,
    /// 锻炉烙铁：本体唯一一件显式声明 `damage_category` 的武器。
    forge_brand_id: ContentIndex,
    /// 铁短剑：对照组，**不**声明伤害类别，因此走全局默认那一类。
    iron_shortsword_id: ContentIndex,
    /// 锻炉围裙：本体唯一一件声明抗性的物品。
    forge_apron_id: ContentIndex,
    /// `lostland:fire`——本体第二个伤害类别。
    fire_id: ContentIndex,
    /// `lostland:gear`——本体加值类型名册里唯一的一条，围裙那条抗性
    /// 归的就是这一类。
    gear_type_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        race,
        trait_def,
        item,
        formula,
        ..
    } = session;
    let lostland_id = NamespacedId::parse("lostland:self").unwrap();
    let lostland_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        lostland_status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/lostland/ 的内容文件注册"))
    };

    RealModsHandle {
        human_id: resolve("lostland:human"),
        forge_brand_id: resolve("lostland:forge_brand"),
        iron_shortsword_id: resolve("lostland:iron_shortsword"),
        forge_apron_id: resolve("lostland:forge_apron"),
        gear_type_id: resolve("lostland:gear"),
        fire_id: resolve("lostland:fire"),
        race,
        trait_def,
        item,
        formula,
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

/// 造一个占位实体，站在 `(5, 5)`，理由同
/// `example_mod_weapon_reference.rs::spawn_agent`（本文件不需要验收
/// 击杀记录，因此不暴露 `remembered` 参数）。
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    health: i32,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
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
        inventory: Vec::new(),
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
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
    })
}

#[test]
fn 锻炉烙铁声明的伤害类别真的是本体第二个类别() {
    // 「这件武器打的是**别的**伤害类别」这条覆盖在本体侧的第一份证据
    // ——ItemAttrs::damage_category 那条字段豁免摘除的直接依据。
    // Arrange
    let handle = load_real_mods();

    // Act
    let brand = handle
        .item
        .get(handle.forge_brand_id)
        .expect("锻炉烙铁应已注册");
    let sword = handle
        .item
        .get(handle.iron_shortsword_id)
        .expect("铁短剑应已注册");

    // Assert：烙铁显式声明火，铁短剑整条不写（走全局默认那一类）。
    assert_eq!(brand.damage_category, Some(handle.fire_id));
    assert_eq!(sword.damage_category, None);
    // 两件武器正交的另一半：铁短剑声明公式不声明类别，烙铁声明类别
    // 不声明公式——「伤害公式」与「伤害类别」是互不相干的两条轴
    // （damage-formula-mod-api.md 十七节「正交，不合并」）。
    assert!(sword.damage_formula.is_some());
    assert_eq!(brand.damage_formula, None);
}

#[test]
fn 锻炉围裙的抗性声明真的写进了物品表且归在本体加值类型下() {
    // 「对某一类特别抗」这条覆盖在本体侧的第一份证据——
    // ItemAttrs::rule_modifiers 那条字段豁免摘除的直接依据。
    // Arrange
    let handle = load_real_mods();

    // Act
    let apron = handle
        .item
        .get(handle.forge_apron_id)
        .expect("锻炉围裙应已注册");

    // Assert
    assert_eq!(apron.rule_modifiers.len(), 1);
    let typed = &apron.rule_modifiers[0];
    // 加值类型这一条此前是 `None`（未分类共享桶），随本体加值类型名册
    // （mods/lostland/modifier_types.json5）落地改成 lostland:gear，
    // ll_mod::content_audit 里 ItemAttrs::rule_modifiers::modifier_type
    // 那条字段豁免同批摘除。断言的是**具体等于 lostland:gear**，不是
    // 「非 None」——「未分类」与「归错类」在结算里是两种不同的错。
    assert_eq!(typed.modifier_type, Some(handle.gear_type_id));
    let RuleModifier::Resistance {
        damage_category,
        damage_reduction,
    } = &typed.modifier
    else {
        panic!("围裙声明的应当是一条抗性");
    };
    assert_eq!(*damage_category, handle.fire_id);
    assert_eq!(*damage_reduction, 6);
    // 围裙同时有 2 点护甲——两条通道并存且各走各的（护甲无条件求和,
    // 抗性按类别分桶取最强再跨桶相加），这正是 stat_bonuses 与
    // rule_modifiers 两个字段并列而不合并的理由。
    assert_eq!(apron.stat_bonuses.len(), 1);
}

#[test]
fn 锻炉围裙挡得住烙铁的火伤挡不住铁短剑的物理伤害() {
    // 本文件的重点：一个新伤害类别值不值得注册，判据是「它的结算后果
    // 与既有类别真的不同」。这里用**同一件**围裙、**同一个**种族的
    // 防御方，只换攻击者手里的武器：
    //
    //   * 锻炉烙铁（声明 lostland:fire）→ 围裙那 6 点减伤命中；
    //   * 铁短剑（不声明类别，走全局默认那一类）→ 那 6 点一点不减，
    //     围裙只剩它那 2 点护甲照常起作用。
    //
    // 把 items.json5 里 forge_brand 的 damage_category 整条删掉，第一组
    // 的差值立刻从 6 掉到 2，本测试变红。
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 铁短剑显式引用 lostland:blade_damage_formula；烙铁整条不写,
        // 沿三级下探落到这个默认值——本夹具传 ContentIndex::default()
        // （查不到定义时 formula_for 退化成把攻击力原样交回），与
        // example_mod_resistance.rs 同一条既有做法。
        default_formula: ContentIndex::default(),
    };

    // 同一把武器各跑一遍「裸防御方 vs 穿围裙的防御方」，返回两个受伤值。
    let damage_pair = |weapon: ContentIndex| {
        let mut world = test_world();
        let attacker = spawn_agent(
            &mut world,
            handle.human_id,
            Agent::STARTING_HEALTH,
            BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(weapon, 1))]),
        );
        let bare = spawn_agent(&mut world, handle.human_id, 1_000, BTreeMap::new());
        let aproned = spawn_agent(
            &mut world,
            handle.human_id,
            1_000,
            BTreeMap::from([(EquipSlot::OUTER, ItemStack::new(handle.forge_apron_id, 1))]),
        );
        let attack = |world: &mut WorldState, defender: EntityId| {
            let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                world,
                &Intent::Attack {
                    actor: attacker,
                    target: defender,
                },
                &NoSkills,
                &handle.race,
                &handle.trait_def,
                &ll_sim::resource_pool::NoResourcePools,
                &handle.item,
                &formulas,
                &NoDamageCategories,
            );
            for effect in &effects {
                apply(world, effect);
            }
        };
        attack(&mut world, bare);
        attack(&mut world, aproned);
        let taken = |world: &WorldState, who: EntityId| {
            1_000 - world.actors.get(who).expect("未死亡").health
        };
        (taken(&world, bare), taken(&world, aproned))
    };

    // Act
    let (fire_bare, fire_aproned) = damage_pair(handle.forge_brand_id);
    let (physical_bare, physical_aproned) = damage_pair(handle.iron_shortsword_id);

    // Assert
    let fire_saved = fire_bare - fire_aproned;
    let physical_saved = physical_bare - physical_aproned;
    // **主断言**：两条路径省下来的伤害之差，恰好是围裙声明的那 6 点
    // 火抗。护甲那一路（围裙的 armor 2）对两把武器一视同仁，因此在
    // 相减时整条抵消——这就是「lostland:fire 与全局默认那一类确实不是
    // 一回事」这句话的可执行版本，也是这个新类别值得注册的全部理由。
    assert_eq!(
        fire_saved - physical_saved,
        6,
        "火伤省下 {fire_saved}（裸 {fire_bare} → 穿围裙 {fire_aproned}），         物理省下 {physical_saved}（裸 {physical_bare} → 穿围裙 {physical_aproned}），         两者之差应当正好是围裙那 6 点火抗"
    );
    // 顺带钉住两个绝对值。它们里面有一部分（护甲 2 点在本夹具下实际
    // 省下 3 点伤害）来自既有的护甲/防御模型，不是本批次的内容——那条
    // 模型改动时本行会红，属于预期：届时该核对的是护甲那一路，主断言
    // 的 6 点差值不受影响。
    assert_eq!((fire_bare, fire_aproned), (10, 1));
    assert_eq!((physical_bare, physical_aproned), (10, 7));
}
