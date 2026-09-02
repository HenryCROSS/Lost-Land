//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 温度系统这一批新增的两条脚本注册能力真的能被已发货的 mod 脚本调用，
//! 且真的改变了**真实游戏路径**（`TurnEngine` → `resolve_with_catalogs`
//! → `resolve_attack` → `derive_stats_at`）上的结算结果——ADR 0018
//! 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本
//! 文件是那份证据，不能靠单元测试自证。
//!
//! 两条新增能力：
//!
//! 1. `register-weather` 的第九个参数 `temperature-offset`
//!    （`mods/example_mod/weather.scm` 的 `examplemod:ashfall`，取 +150
//!    ——仓库里唯一一条**正**偏移的天气内容）。
//! 2. `register-item-stat-bonus` 多认识的目标名 `"insulation"`
//!    （`mods/example_mod/items.json5` 的 `examplemod:wool_liner`
//!    与 `examplemod:fur_cloak`，各占一个不同的槽位）。
//!
//! # 本文件最关键的一条断言
//!
//! `两件保暖装备的绝缘值求和而不是取其一`：绝缘值走的是
//! `derive_stats` 的**求和**通道（`StatTarget`），不是
//! `ItemDef.rule_modifiers` 那条 **tie-break** 通道。这个判断若做反了，
//! 「两层衣服比一层暖」这条最基本的直觉会失效，而单件装备的测试**完全
//! 看不出区别**——只有同时穿两件才暴露。
//!
//! 与 `example_mod_weather.rs` 同一套「装载整个 `mods/` 目录，不是只挑
//! `example_mod`」的手法与理由。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR, Tick};
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::RaceTable;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::trait_def::TraitTable;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::NoRecipes;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::resolve::derive_stats_at;
use ll_sim::skill::NoSkills;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, AttributeKind, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::state::WorldState;
use ll_world::temperature::Temperature;
use ll_world::terrain::base_terrain_fixture;
use ll_world::weather::WeatherTable;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_weather.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 防御方的起始生命——远高于一次攻击可能造成的伤害，两条场景都不会
/// 触发击杀，理由同 `turn_engine_catalogs.rs` 里同名的常量。
const DEFENDER_HEALTH: i32 = 1_000;

/// 冬季第一天的午夜——本体内容下唯一会跌破冰点的那一段（见
/// `ll_world::temperature::SEASON_TEMPERATURE_OFFSETS` 的取值表）。
const WINTER_MIDNIGHT: Tick = Tick(90 * TICKS_PER_DAY);

/// 夏季第一天的正午——对照组，本体内容下最热的一刻。
const SUMMER_NOON: Tick = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与索引。
struct RealModsHandle {
    race: RaceTable,
    class: ClassTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    skill: SkillTable,
    resource_pool: ResourcePoolTable,
    space_profile: SpaceProfileTable,
    weather: WeatherTable,
    surface_profile_id: ContentIndex,
    ashfall_id: ContentIndex,
    wool_liner_id: ContentIndex,
    fur_cloak_id: ContentIndex,
    half_elf_id: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——与
    /// `ll_game::content::RuntimeCatalogs::as_resolve_catalogs` 同一个
    /// 形状、同一批表，**包含真实的环境来源**（这正是本文件要验的那一
    /// 条接线）。
    fn catalogs<'a>(&'a self, formulas: &'a RegistryFormulas<'a>) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &self.skill,
            quests: &NoQuests,
            race_traits: &self.race,
            class_traits: &self.class,
            // 副职天赋那一路接空实现：本文件的实体
            // `subclasses` 恒为空，接真实副职表与接空实现逐位等价
            // （`agent_trait_sources` 对空 `Vec` 一路来源都不展开）。
            // 那一路真的接进生产路径的证据在
            // `example_mod_subclass_traits.rs`。
            subclass_traits: &ll_sim::traits::NoTraitGrants,
            trait_defs: &self.trait_def,
            pools: &self.resource_pool,
            items: &self.item,
            formulas,
            damage_categories: &NoDamageCategories,
            recipes: &NoRecipes,
            ambient: AmbientSource::new(&self.space_profile, &self.weather),
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
            // 对话这两路（对话批次 2 新增）：本条测试与对话无关，
            // 接空实现即可。
            dialogues: &ll_sim::dialogue::NoDialogues,
            content_ids: &ll_sim::dialogue::NoContentIds,
            // 树木这一路（树木批次新增）：本条测试不砍树，接空实现。
            trees: &ll_sim::tree::NoTrees,
        }
    }
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        space_ids: base_space_ids,
        class,
        skill,
        race,
        trait_def,
        resource_pool,
        item,
        formula,
        space_profile,
        weather,
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
            .unwrap_or_else(|| panic!("{id} 应当已经注册进 Registry"))
    };

    RealModsHandle {
        surface_profile_id: base_space_ids.surface,
        ashfall_id: resolve("examplemod:ashfall"),
        wool_liner_id: resolve("examplemod:wool_liner"),
        fur_cloak_id: resolve("examplemod:fur_cloak"),
        half_elf_id: resolve("examplemod:half_elf"),
        race,
        class,
        trait_def,
        item,
        formula,
        skill,
        resource_pool,
        space_profile,
        weather,
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

/// 一个不属于任何真实注册职业的占位职业索引，理由同
/// `turn_engine_catalogs.rs` 里同名的帮手。
fn placeholder_profession() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"))
}

/// 造一个站在**真实本体地表层属性**上的占位实体——`current_space` 的
/// `profile` 必须是真的注册过的索引，否则
/// `AmbientSource::temperature_in` 会走 `is_defined` 那条降级分支退回
/// 中性温度，整个温度链路在本文件里就测不到任何东西。
fn spawn_agent(
    world: &mut WorldState,
    handle: &RealModsHandle,
    pos: (i32, i32),
    health: i32,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos: agent_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession: placeholder_profession(),
        goals: Vec::new(),
        race: handle.half_elf_id,
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
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, handle.surface_profile_id),
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

/// 喂给 [`TurnEngine::advance_ai`] 的 AI 策略，理由同
/// `turn_engine_catalogs.rs` 里同名的函数指针。
fn attack_controlled(_world: &WorldState, actor: EntityId, controlled: EntityId) -> Intent {
    Intent::Attack {
        actor,
        target: controlled,
    }
}

/// 跑一场「攻击方在 `at` 这一刻经由 `TurnEngine` 攻击防御方恰好一次」，
/// 返回防御方掉了多少血。
///
/// 攻击方穿 `attacker_clothes` 里列出的保暖装备，防御方不穿任何东西
/// （护甲恒为 0，两组之间唯一的差别因此只有攻击方的力量）。
///
/// # 每次调用都从零建一个新世界
///
/// 理由同 `turn_engine_catalogs.rs::damage_dealt_via_turn_engine`：让
/// 两次调用的实体生成顺序、时间轴排期、世界时钟推进序列逐位相同，
/// `DetRng::for_entity`（约束 C3）拿到的输入也就相同，暴击/骰子这类
/// 随机分支不会在场景之间引入无关噪声——衣服与时刻不进任何一条随机流
/// 的三元组。
fn damage_dealt_at(
    handle: &RealModsHandle,
    at: Tick,
    attacker_clothes: &[(EquipSlot, ContentIndex)],
) -> i32 {
    let equipment: BTreeMap<EquipSlot, ItemStack> = attacker_clothes
        .iter()
        .map(|(slot, def)| (*slot, ItemStack::new(*def, 1)))
        .collect();
    damage_dealt_wearing(handle, at, equipment, BTreeMap::new()).0
}

/// [`damage_dealt_at`] 的完整版本：攻防两方的装备都由调用方逐堆给定
/// （因此可以带上真实的 `durability`），除伤害之外**还返回防御方结算
/// 之后的装备栏**，供「挨打掉耐久」那一路断言使用。
///
/// 两个入口而不是给 `damage_dealt_at` 加参数，理由同
/// `ll_sim::resolve::derive_stats`/`derive_stats_at` 那一对：本文件既有
/// 的五条温度断言一件带耐久的装备都不需要，强迫它们每处都构造一个
/// `BTreeMap<EquipSlot, ItemStack>` 只是噪音。
fn damage_dealt_wearing(
    handle: &RealModsHandle,
    at: Tick,
    attacker_equipment: BTreeMap<EquipSlot, ItemStack>,
    defender_equipment: BTreeMap<EquipSlot, ItemStack>,
) -> (i32, BTreeMap<EquipSlot, ItemStack>) {
    let mut world = test_world();
    let equipment = attacker_equipment;
    let attacker = spawn_agent(
        &mut world,
        handle,
        (5, 5),
        Agent::STARTING_HEALTH,
        equipment,
    );
    let defender = spawn_agent(
        &mut world,
        handle,
        (6, 5),
        DEFENDER_HEALTH,
        defender_equipment,
    );

    let mut timeline = Timeline::new();
    timeline.schedule(attacker, at);
    timeline.schedule(defender, Tick(at.0 + 1));
    let mut engine = TurnEngine::new(timeline);

    // 本文件的攻击方赤手空拳（保暖装备不是武器，不携带公式引用），
    // 因此走的是默认公式那一路；这里给 `ContentIndex::default()`，与
    // `turn_engine_catalogs.rs` 同一处写法。
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);
    let acted = engine.advance_ai(
        &mut world,
        defender,
        &mut attack_controlled,
        &catalogs,
        &mut |_, _| {},
    );
    assert_eq!(
        acted,
        vec![attacker],
        "本场景应当恰好结算攻击方一次行动，实际结算序列不符"
    );

    let defender_after = world
        .actors
        .get(defender)
        .expect("防御方生命远高于单次伤害，不应死亡");
    (
        DEFENDER_HEALTH - defender_after.health,
        defender_after.equipment.clone(),
    )
}

#[test]
fn 示例mod脚本声明的天气温度偏移进了天气表() {
    // ADR 0018 第一半：`register-weather` 的第九个参数真的能被 mod
    // 脚本填，且原样进表。
    // Arrange
    let handle = load_real_mods();

    // Act
    let offset = handle.weather.temperature_offset(handle.ashfall_id);

    // Assert
    assert_eq!(offset, 150);
    assert!(
        offset > 0,
        "灰烬雨是仓库里唯一一条正偏移的天气内容——它验收的是\
         WEATHER_TEMPERATURE_OFFSET_LIMIT 那条「上下界对称」的设计声明"
    );
}

#[test]
fn 示例mod脚本声明的保暖装备进了物品表() {
    // ADR 0018 第一半：`register-item-stat-bonus` 多认识的
    // `"insulation"` 目标名真的能被 mod 脚本用。
    // Arrange
    let handle = load_real_mods();
    let now = Tick(0);
    let equip_one = |def: ContentIndex, slot: EquipSlot| {
        derive_stats_at(
            BaseStats::BASELINE,
            &BTreeMap::new(),
            &BTreeMap::from([(slot, ItemStack::new(def, 1))]),
            &handle.item,
            now,
            Temperature::TEMPERATE_BASELINE,
        )
        .insulation()
    };

    // Act & Assert
    assert_eq!(equip_one(handle.wool_liner_id, EquipSlot::BODY), 50);
    assert_eq!(equip_one(handle.fur_cloak_id, EquipSlot::OUTER), 90);
}

#[test]
fn 两件保暖装备的绝缘值求和而不是取其一() {
    // **本文件最关键的一条**：绝缘值走 `derive_stats` 的求和通道
    // （`StatTarget`），不是 `ItemDef.rule_modifiers` 那条 tie-break
    // 通道。判断做反了的话，单件装备的测试完全看不出区别——只有同时
    // 穿两件才暴露。
    // Arrange
    let handle = load_real_mods();
    let both = BTreeMap::from([
        (EquipSlot::BODY, ItemStack::new(handle.wool_liner_id, 1)),
        (EquipSlot::OUTER, ItemStack::new(handle.fur_cloak_id, 1)),
    ]);

    // Act
    let insulation = derive_stats_at(
        BaseStats::BASELINE,
        &BTreeMap::new(),
        &both,
        &handle.item,
        Tick(0),
        Temperature::TEMPERATE_BASELINE,
    )
    .insulation();

    // Assert
    assert_eq!(
        insulation, 140,
        "两件保暖装备必须求和（50 + 90），tie-break 会得到 90"
    );
}

#[test]
fn 冬夜露天不穿衣服时力量被削弱而夏日正午不会() {
    // 温度→惩罚这条链路在**真实装载出来的层属性表**上的验证，且两侧
    // 都钉住：极端条件下真的有后果，平时真的完全没有。
    // Arrange
    let handle = load_real_mods();
    let bare = BTreeMap::new();
    let derive_at = |ambient: Temperature| {
        derive_stats_at(
            BaseStats::BASELINE,
            &BTreeMap::new(),
            &bare,
            &handle.item,
            Tick(0),
            ambient,
        )
        .attribute(AttributeKind::Strength)
    };
    let clear = ll_world::weather::Weather::CLEAR;
    let winter = handle.space_profile.effective_temperature(
        handle.surface_profile_id,
        WINTER_MIDNIGHT,
        clear,
    );
    let summer =
        handle
            .space_profile
            .effective_temperature(handle.surface_profile_id, SUMMER_NOON, clear);

    // Act
    let winter_strength = derive_at(winter);
    let summer_strength = derive_at(summer);

    // Assert
    assert_eq!(
        summer_strength,
        BaseStats::BASELINE.strength,
        "夏日正午（{}）不该有任何暴露惩罚",
        summer.0
    );
    assert!(
        winter_strength < BaseStats::BASELINE.strength,
        "冬夜露天（{}）应当削弱力量",
        winter.0
    );
}

#[test]
fn 冬夜经真实回合引擎打出的伤害低于夏日正午() {
    // ADR 0018 第二半，也是「温度必须有真实消费者」那条要求的落点：
    // 走的是 `ll-game` 唯一使用的那条生产链路（`TurnEngine::advance_ai`
    // → `resolve_with_catalogs` → `resolve_attack` → `derive_stats_at`），
    // 不是直接调 `derive_stats_*` 自证。
    // Arrange
    let handle = load_real_mods();

    // Act
    let winter = damage_dealt_at(&handle, WINTER_MIDNIGHT, &[]);
    let summer = damage_dealt_at(&handle, SUMMER_NOON, &[]);

    // Assert
    assert!(
        winter < summer,
        "冬夜赤身打出的伤害 {winter} 应当低于夏日正午的 {summer}"
    );
}

#[test]
fn 穿上两件保暖装备后冬夜的伤害回到夏日水平() {
    // 「玩家怎么规避」那三条路径里的第一条（穿够衣服），同样走真实
    // 生产链路。两件衣服共提供 140（14℃），而冬夜露天是 -4℃——足够
    // 把体感抬回冰点以上，惩罚完全消失。
    // Arrange
    let handle = load_real_mods();
    let clothes = [
        (EquipSlot::BODY, handle.wool_liner_id),
        (EquipSlot::OUTER, handle.fur_cloak_id),
    ];

    // Act
    let bare_winter = damage_dealt_at(&handle, WINTER_MIDNIGHT, &[]);
    let clothed_winter = damage_dealt_at(&handle, WINTER_MIDNIGHT, &clothes);
    let summer = damage_dealt_at(&handle, SUMMER_NOON, &[]);

    // Assert
    assert!(
        clothed_winter > bare_winter,
        "穿衣服必须真的减轻惩罚：{clothed_winter} 应当高于 {bare_winter}"
    );
    assert_eq!(
        clothed_winter, summer,
        "两件衣服足以把冬夜的惩罚完全抵消，伤害应当回到夏日正午的水平"
    );
}

#[test]
fn 一件保暖装备的效果严格弱于两件() {
    // 求和语义在**真实结算结果**上的第二道验证：三档必须是三个不同
    // 的值（或至少单调），tie-break 会让第二件毫无作用。
    // Arrange
    let handle = load_real_mods();

    // Act
    let bare = damage_dealt_at(&handle, WINTER_MIDNIGHT, &[]);
    let one = damage_dealt_at(
        &handle,
        WINTER_MIDNIGHT,
        &[(EquipSlot::BODY, handle.wool_liner_id)],
    );
    let two = damage_dealt_at(
        &handle,
        WINTER_MIDNIGHT,
        &[
            (EquipSlot::BODY, handle.wool_liner_id),
            (EquipSlot::OUTER, handle.fur_cloak_id),
        ],
    );

    // Assert
    assert!(
        bare <= one && one <= two,
        "伤害应当随保暖层数单调不减：赤身 {bare}、一层 {one}、两层 {two}"
    );
    assert!(
        bare < two,
        "两层与赤身之间必须有真实差别，否则本测试测不到任何东西"
    );
}

#[test]
fn 真实注册的两件保暖装备现在带耐久上限() {
    // 耐久扩面批次（所有者裁定「衣服要耐久，受到攻击就会减少耐久」）：
    // `mods/example_mod/items.json5` 里这两件此前只能填 -1,注册期有
    // 一条「只允许占武器槽位的物品携带耐久」的校验拦着。本条把放宽
    // 之后的真实注册结果钉住——若那条校验被恢复，装载会直接失败,
    // `load_real_mods` 里的 `LoadStatus::Success` 断言先红。
    // Arrange
    let handle = load_real_mods();

    // Act
    let liner = handle.item.get(handle.wool_liner_id).expect("已注册");
    let cloak = handle.item.get(handle.fur_cloak_id).expect("已注册");

    // Assert
    assert_eq!(liner.max_durability, Some(60));
    assert_eq!(cloak.max_durability, Some(90));
}

#[test]
fn 穿着保暖装备的防御方经真实回合引擎挨一下之后两件衣服都掉了一点耐久() {
    // ADR 0018 端到端证据：走的是 `ll-game` 唯一使用的那条生产链路
    // （`TurnEngine::advance_ai` → `resolve_with_catalogs` →
    // `resolve_attack`），不是直接调 `resolve_attack`。
    //
    // 反例（手工验证过会红）：把 `resolve_attack` 里「挨打」通道那段
    // `effects.extend(defender.equipment.iter()...)` 删掉,本条立即从
    // `Some(59)`/`Some(89)` 变回 `Some(60)`/`Some(90)` 而失败；把过滤
    // 条件里的 `!WEAPON_GROUP_SLOTS.contains_slot(..)` 去掉本条照样绿
    // ——那一半由下一条测试负责。
    // Arrange
    let handle = load_real_mods();
    let defender_equipment = BTreeMap::from([
        (
            EquipSlot::BODY,
            ItemStack::with_durability(handle.wool_liner_id, 1, 60),
        ),
        (
            EquipSlot::OUTER,
            ItemStack::with_durability(handle.fur_cloak_id, 1, 90),
        ),
    ]);

    // Act
    let (_, equipment_after) =
        damage_dealt_wearing(&handle, SUMMER_NOON, BTreeMap::new(), defender_equipment);

    // Assert
    assert_eq!(
        equipment_after
            .get(&EquipSlot::BODY)
            .expect("内衬仍在装备栏里——耐久归零都不自动卸下，何况只掉一点")
            .durability,
        Some(59),
    );
    assert_eq!(
        equipment_after
            .get(&EquipSlot::OUTER)
            .expect("外袍仍在装备栏里")
            .durability,
        Some(89),
    );
}

#[test]
fn 耐久归零的保暖装备经真实回合引擎不再提供任何保暖() {
    // 本批次最关键的一条：玩家最容易被坑的情形是「穿着一件破衣服，
    // 以为还在保暖」。`derive_stats_at` 的「`durability == Some(0)` 即
    // `continue`」跳在读取 `stat_bonuses` **之前**,因此对
    // `StatTarget::Armor` 与 `StatTarget::Insulation` 一视同仁——这条
    // 测试把「一视同仁」钉在真实结算结果上,而不是靠读代码相信。
    //
    // 手法与 `穿上两件保暖装备后冬夜的伤害回到夏日水平` 完全相同,只把
    // 两件衣服的耐久从「没有耐久概念」换成「归零」：冬夜的力量惩罚必须
    // 原样回来,伤害回落到赤身水平。
    // Arrange
    let handle = load_real_mods();
    let broken = BTreeMap::from([
        (
            EquipSlot::BODY,
            ItemStack::with_durability(handle.wool_liner_id, 1, 0),
        ),
        (
            EquipSlot::OUTER,
            ItemStack::with_durability(handle.fur_cloak_id, 1, 0),
        ),
    ]);
    let intact = BTreeMap::from([
        (
            EquipSlot::BODY,
            ItemStack::with_durability(handle.wool_liner_id, 1, 1),
        ),
        (
            EquipSlot::OUTER,
            ItemStack::with_durability(handle.fur_cloak_id, 1, 1),
        ),
    ]);

    // Act
    let bare_winter = damage_dealt_at(&handle, WINTER_MIDNIGHT, &[]);
    let broken_winter = damage_dealt_wearing(&handle, WINTER_MIDNIGHT, broken, BTreeMap::new()).0;
    // 反例：耐久只剩 1 点（未归零）的同两件衣服必须照常保暖——证明上面
    // 那条断言拒绝的是「归零」这个状态本身，不是「带耐久字段的衣服一律
    // 不保暖」。
    let barely_alive_winter =
        damage_dealt_wearing(&handle, WINTER_MIDNIGHT, intact, BTreeMap::new()).0;

    // Assert
    assert_eq!(
        broken_winter, bare_winter,
        "耐久归零的衣服必须与没穿完全等价：破衣 {broken_winter} 应当等于赤身 {bare_winter}"
    );
    assert!(
        barely_alive_winter > bare_winter,
        "只剩 1 点耐久的衣服仍然照常保暖：{barely_alive_winter} 应当高于赤身 {bare_winter}"
    );
}

/// 空技能目录常量——同时充当空技能树目录（`NoSkills` 实现了
/// `SkillTreeCatalog`，见 `ll_sim::skill_overview` 里那条 impl 的
/// 文档：不为对称再造第二个语义相同的空对象）。
const NO_SKILLS: NoSkills = NoSkills;
