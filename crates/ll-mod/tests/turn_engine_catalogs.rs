//! 端到端验证：真实装载仓库里的 `mods/` 目录，证明真实注册的天赋
//! **经由 [`ll_sim::turn::TurnEngine`]**（本体二进制 `ll-game` 驱动
//! 世界的唯一路径）真的改变了结算结果——不是靠测试直接调
//! `resolve_with_*` 系列自证。
//!
//! # 这个文件为什么必须存在
//!
//! `TurnEngine::perform` 此前调的是不带任何内容目录的
//! `ll_sim::resolve::resolve`，而 `ll-game` 全程只通过 `TurnEngine`
//! 驱动世界、从不直接调用任何 `resolve_with_*` 变体。两件事合起来的
//! 后果是：种族天赋、职业天赋、抗性、偷袭规则、资源池容量——所有走
//! `effective_traits` 的东西——**在真正能跑的游戏里从未生效过**，而
//! 天赋系统当时全部的「真实证据」（`example_mod_resistance.rs`/
//! `example_mod_sneak_attack.rs`/`example_mod_class_traits.rs`）都停在
//! 「直接调 `resolve_with_*`」这一层，没有一条穿过生产路径。这与
//! `ll_sim::turn` 模块文档开头记的「`TurnEngine` 本身当初只在 demo 里
//! 接了线、本体二进制没接」是同一类缺陷的第二次复发。
//!
//! 因此本文件的验收标准比 ADR 0018「要有真实 mod 脚本为证」再高一层：
//! 内容来自真实 `mods/`（ADR 0018），**且**结算必须经由 `TurnEngine`
//! 的公开入口发生（本批次追加）。
//!
//! # 接线本身怎么守住
//!
//! [`目录从回合引擎摘掉后同一场景里抗性不再生效`] 是这份守卫：同一
//! 段场景、同一个 `TurnEngine`，只把目录束换成
//! [`ResolveCatalogs::empty`]，抗性立刻消失、两个防御方受到完全相同的
//! 伤害。两条测试合起来钉死的是「差异只可能由那一束目录引起」——谁把
//! 目录从 `TurnEngine` 摘掉（比如把 `perform` 改回调用裸 `resolve`），
//! 上面那条正向测试就会拿到与本条完全一样的结果而变红。
//!
//! 装载真实 `mods/` 的手法与 `example_mod_resistance.rs` 完全一致，
//! 见其模块文档。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::modifier_type::ModifierTypeTable;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::NoRecipes;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::skill::NoSkills;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_resistance.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 防御方的起始生命——远高于一次攻击可能造成的伤害，两条场景都不会
/// 触发击杀（`TurnEngine::perform` 对 `Effect::Kill` 会把死者移出时间
/// 轴，那条分支不属于本文件的验收范围）。
const DEFENDER_HEALTH: i32 = 1_000;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与索引，理由同
/// `example_mod_resistance.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    class: ClassTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    skill: SkillTable,
    resource_pool: ResourcePoolTable,
    ooze_id: ContentIndex,
    half_elf_id: ContentIndex,
    acid_dagger_id: ContentIndex,
    rogue_id: ContentIndex,
    /// 战锤——主手 + 副手双持武器，`max-durability` 150。
    war_hammer_id: ContentIndex,
    /// 木盾——只占副手（武器组），`max-durability` 80。
    wooden_shield_id: ContentIndex,
    /// 酸抗护符——占脖子，`max-durability` 60，带 `lostland:armor` 标签。
    acid_ward_amulet_id: ContentIndex,
    /// 粗劣匕首——**可双持**（主手 + 副手），`max-durability` 20，
    /// 只带 `lostland:weapon` 标签。本批次的关键夹具：它与木盾占同一个
    /// 副手槽位，挨打时结果却相反。
    crude_dagger_id: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——本体二进制
    /// （`ll_game::content::RuntimeCatalogs::as_resolve_catalogs`）交给
    /// `TurnEngine` 的是同一个形状、同一批表。
    ///
    /// `formulas` 必须由调用方在外面先构造好并保持存活：
    /// [`RegistryFormulas`] 是借着 `FormulaTable` 现造的值，`&dyn` 不能
    /// 指向临时值——本体二进制那侧的 `RuntimeCatalogs` 存在的理由完全
    /// 相同，见其文档。
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
            // 本文件的两条场景都在一个 `ContentIndex::default()` 占位
            // 层属性的地表上打架，温度这一路没有可查的表；接一个空的
            // 环境来源与「温度还没接线」逐位等价（见
            // `ll_sim::exposure::AmbientSource::NONE`）。温度真的接进
            // 生产路径的证据在 `example_mod_temperature.rs`。
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }
}

fn load_real_mods() -> RealModsHandle {
    let mut registry = Registry::new();
    let mut terrain = ll_world::terrain::TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let mut modifier_type_table = ModifierTypeTable::new();

    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
            race: &mut race,
            clip: &mut clip,
            xp_curve: &mut xp_curve,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_def,
            resource_pool: &mut resource_pool,
            item: &mut item,
            formula: &mut formula,
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            modifier_type: &mut modifier_type_table,
            tag: &mut tag_table,
        },
    );
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/items.json5 注册"))
    };

    // 背刺技能索引存进进程级 `OnceLock`：`advance_ai` 的 `ai_intent`
    // 是**函数指针**（见其文档：当前调用方都不需要捕获环境），捕获不了
    // 装载出来的索引。全部用例装载的是同一份真实 `mods/`，同一个字符串
    // 解析出的索引恒相同，因此并行跑的多个用例写同一个值不会互相干扰。
    // 重复写入（多个用例各自装载一次）按同值忽略——`set` 返回的 `Err`
    // 只说明「已经有人先写过了」，而那个值与本次要写的必然相同。
    let _ = BACKSTAB_SKILL.set(resolve("examplemod:backstab"));

    RealModsHandle {
        ooze_id: resolve("examplemod:ooze"),
        half_elf_id: resolve("examplemod:half_elf"),
        acid_dagger_id: resolve("examplemod:acid_dagger"),
        rogue_id: resolve("examplemod:rogue"),
        war_hammer_id: resolve("examplemod:war_hammer"),
        wooden_shield_id: resolve("examplemod:wooden_shield"),
        acid_ward_amulet_id: resolve("examplemod:acid_ward_amulet"),
        crude_dagger_id: resolve("examplemod:crude_dagger"),
        race,
        class,
        trait_def,
        item,
        formula,
        skill,
        resource_pool,
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

/// 一个不属于任何真实注册职业的占位职业索引——本文件里「不该吃职业
/// 天赋」的实体用它，理由同 `example_mod_resistance.rs::spawn_agent`
/// （那里同样临时 intern 一个 `lostland:tester`）。
fn placeholder_profession() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"))
}

/// 造一个占位实体，理由同 `example_mod_resistance.rs::spawn_agent`；
/// 本文件额外暴露 `profession`/`level` 两个参数——职业天赋那条链路的
/// 所有者取自 `Agent::profession`，解锁门槛比的是 `Agent::level`。
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    profession: ContentIndex,
    pos: (i32, i32),
    health: i32,
    level: i32,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        pos: agent_pos,
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
        level,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 喂给 [`TurnEngine::advance_ai`] 的 AI 策略：非受控实体一律攻击受控
/// 实体。函数指针（不是闭包），与 `advance_ai` 的既有签名一致。
fn attack_controlled(_world: &WorldState, actor: EntityId, controlled: EntityId) -> Intent {
    Intent::Attack {
        actor,
        target: controlled,
    }
}

/// 跑一场「攻击方经由 `TurnEngine` 攻击防御方恰好一次」，返回防御方
/// 掉了多少血。
///
/// # 为什么恰好只结算一次
///
/// 攻击方排在 `Tick(0)`、防御方排在 `Tick(1)`：`advance_ai` 先弹出
/// 攻击方（非受控 → 结算一次攻击，随后按行动耗时重排到 `Tick(100)`
/// 附近），下一次弹出的是防御方——它就是 `controlled`，`advance_ai`
/// 于是立即返回（见其文档）。防御方必须真的排进时间轴：受控实体若
/// 不在队列里，「弹出的条目属于受控实体」这条退出条件永远不成立，
/// 攻击方会一直打到 `MAX_STEPS_PER_ADVANCE` 才罢休。
///
/// 每次调用都从零建一个新世界：两场景的实体生成顺序、时间轴排期、
/// 世界时钟推进序列因此逐位相同，`DetRng::for_entity`（约束 C3）拿到
/// 的输入也就相同——两场景之间唯一的差别只有防御方的种族（以及本文件
/// 另一条测试里的目录束），暴击/偷袭这类随机分支不会在两场景之间
/// 引入无关的噪声。
fn damage_dealt_via_turn_engine(
    handle: &RealModsHandle,
    defender_race: ContentIndex,
    catalogs: &ResolveCatalogs<'_>,
) -> i32 {
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.half_elf_id,
        placeholder_profession(),
        (5, 5),
        Agent::STARTING_HEALTH,
        Agent::STARTING_LEVEL,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.acid_dagger_id, 1),
        )]),
    );
    let defender = spawn_agent(
        &mut world,
        defender_race,
        placeholder_profession(),
        (6, 5),
        DEFENDER_HEALTH,
        Agent::STARTING_LEVEL,
        BTreeMap::new(),
    );

    let mut timeline = Timeline::new();
    timeline.schedule(attacker, Tick(0));
    timeline.schedule(defender, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let acted = engine.advance_ai(
        &mut world,
        defender,
        &mut attack_controlled,
        catalogs,
        &mut |_, _| {},
    );
    assert_eq!(
        acted,
        vec![attacker],
        "本场景应当恰好结算攻击方一次行动，实际结算序列不符"
    );

    DEFENDER_HEALTH
        - world
            .actors
            .get(defender)
            .expect("防御方生命远高于单次伤害，不应死亡")
            .health
}

/// 跑一场「攻击方经由 `TurnEngine` 攻击防御方恰好一次」，返回
/// **攻防双方结算之后各自的装备栏**——排期手法与
/// [`damage_dealt_via_turn_engine`] 逐字相同（同一条「恰好只结算一次」
/// 的理由，见其文档），只是关心的产物从伤害换成了耐久。
fn equipment_after_one_attack(
    handle: &RealModsHandle,
    attacker_equipment: BTreeMap<EquipSlot, ItemStack>,
    defender_equipment: BTreeMap<EquipSlot, ItemStack>,
    catalogs: &ResolveCatalogs<'_>,
) -> (
    BTreeMap<EquipSlot, ItemStack>,
    BTreeMap<EquipSlot, ItemStack>,
) {
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.half_elf_id,
        placeholder_profession(),
        (5, 5),
        Agent::STARTING_HEALTH,
        Agent::STARTING_LEVEL,
        attacker_equipment,
    );
    let defender = spawn_agent(
        &mut world,
        handle.half_elf_id,
        placeholder_profession(),
        (6, 5),
        DEFENDER_HEALTH,
        Agent::STARTING_LEVEL,
        defender_equipment,
    );

    let mut timeline = Timeline::new();
    timeline.schedule(attacker, Tick(0));
    timeline.schedule(defender, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let acted = engine.advance_ai(
        &mut world,
        defender,
        &mut attack_controlled,
        catalogs,
        &mut |_, _| {},
    );
    assert_eq!(
        acted,
        vec![attacker],
        "本场景应当恰好结算攻击方一次行动，实际结算序列不符"
    );

    (
        world
            .actors
            .get(attacker)
            .expect("攻击方不会在自己的攻击里死掉")
            .equipment
            .clone(),
        world
            .actors
            .get(defender)
            .expect("防御方生命远高于单次伤害，不应死亡")
            .equipment
            .clone(),
    )
}

/// `mods/example_mod/items.json5` 里 `examplemod:rogue` 授予
/// `examplemod:cutpurse_training` 的解锁等级——与
/// `example_mod_class_traits.rs` 的同名常量同一个来源，故意不是 1
/// （种族天赋恒填 1，职业这一路才真正用得上按等级解锁）。
const ROGUE_TRAIT_UNLOCK_LEVEL: i32 = 3;

/// 背刺技能索引——见 `load_real_mods` 里写入处的注释。
static BACKSTAB_SKILL: OnceLock<ContentIndex> = OnceLock::new();

/// 喂给 [`TurnEngine::advance_ai`] 的 AI 策略：非受控实体一律尝试放
/// 背刺（`target: None`，与 `example_mod_class_traits.rs` 同一形状）。
fn use_backstab(_world: &WorldState, actor: EntityId, _controlled: EntityId) -> Intent {
    Intent::UseSkill {
        actor,
        skill: *BACKSTAB_SKILL
            .get()
            .expect("load_real_mods 必然已经写入过背刺索引"),
        target: None,
    }
}

/// 让一个 `level` 级的真实盗贼经由 [`TurnEngine`] 放一次背刺，返回这次
/// 结算产出的全部效果（经 `on_effect` 回调收集——那是本引擎与调用方
/// 之间唯一的接缝，见其文档）。
///
/// 「恰好只结算一次」的排期手法与
/// [`damage_dealt_via_turn_engine`] 完全相同，见其文档。
fn backstab_effects_via_turn_engine(
    handle: &RealModsHandle,
    level: i32,
    catalogs: &ResolveCatalogs<'_>,
) -> Vec<Effect> {
    let mut world = test_world();
    let rogue = spawn_agent(
        &mut world,
        handle.half_elf_id,
        handle.rogue_id,
        (5, 5),
        Agent::STARTING_HEALTH,
        level,
        BTreeMap::new(),
    );
    let bystander = spawn_agent(
        &mut world,
        handle.half_elf_id,
        placeholder_profession(),
        (20, 20),
        DEFENDER_HEALTH,
        Agent::STARTING_LEVEL,
        BTreeMap::new(),
    );

    let mut timeline = Timeline::new();
    timeline.schedule(rogue, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let mut seen = Vec::new();
    engine.advance_ai(
        &mut world,
        bystander,
        &mut use_backstab,
        catalogs,
        &mut |_, effect| {
            seen.push(effect.clone());
        },
    );
    seen
}

/// 三件真实注册物品的耐久上限——与 `mods/example_mod/items.json5` 的
/// `register-item` 调用逐字对应，见 `RealModsHandle` 各字段文档。
const WAR_HAMMER_MAX_DURABILITY: i32 = 150;
const WOODEN_SHIELD_MAX_DURABILITY: i32 = 80;
const ACID_WARD_AMULET_MAX_DURABILITY: i32 = 60;
const CRUDE_DAGGER_MAX_DURABILITY: i32 = 20;

#[test]
fn 副手拿刀与副手拿盾在同一次挨打里结果相反() {
    // **本批次最核心的一条断言**，直接钉住项目所有者推翻按槽位分类的
    // 那条裁定：
    //
    // > 「副手也可能拿着武器,例如双刀,双盾」
    //
    // 两个场景**唯一**的差别是副手那一件东西是什么：
    //   - 粗劣匕首（`lostland:weapon`，只走 on-use）→ 挨打**不掉**耐久
    //   - 木盾（`lostland:armor` + `lostland:weapon`，含 on-hit）→ 掉一点
    //
    // 同一个 `EquipSlot::OFF_HAND`、同样带耐久、同样挨一下,结果相反。
    // 上一版按存储键分类的判据**表达不了这个差别**：它对整个武器组
    // （主手 + 副手）一视同仁地跳过,两个场景都会是"不掉"。
    //
    // 反例（手工验证过会红）：把 `resolve_attack`「挨打」通道里那句
    // `rule.wear_channels.contains(WearChannels::ON_HIT)` 换回按槽位判断
    // （`!WEAPON_GROUP_SLOTS.contains_slot(slot)`），木盾那一半立即从
    // `Some(79)` 变回 `Some(80)` 而失败。
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // Act
    let (_, with_dagger) = equipment_after_one_attack(
        &handle,
        BTreeMap::new(),
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(handle.crude_dagger_id, 1, CRUDE_DAGGER_MAX_DURABILITY),
        )]),
        &catalogs,
    );
    let (_, with_shield) = equipment_after_one_attack(
        &handle,
        BTreeMap::new(),
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(handle.wooden_shield_id, 1, WOODEN_SHIELD_MAX_DURABILITY),
        )]),
        &catalogs,
    );

    // Assert
    assert_eq!(
        with_dagger
            .get(&EquipSlot::OFF_HAND)
            .expect("匕首仍在装备栏里")
            .durability,
        Some(CRUDE_DAGGER_MAX_DURABILITY),
        "副手拿的是武器（只有 on-use 标签）——挨打不该磨损"
    );
    assert_eq!(
        with_shield
            .get(&EquipSlot::OFF_HAND)
            .expect("木盾仍在装备栏里")
            .durability,
        Some(WOODEN_SHIELD_MAX_DURABILITY - 1),
        "副手拿的是盾（带 on-hit 标签）——挨打该磨损"
    );
}

#[test]
fn 同一面盾既因为挥出去而磨损也因为挨打而磨损() {
    // 项目所有者原话：「有的技能像是盾击,他也会变成武器这样」——
    // `examplemod:wooden_shield` 同时带 `lostland:armor` 与
    // `lostland:weapon` 两条标签，因此两条通道都吃得到。
    //
    // 这条**明确推翻**上一批「两组槽位刻意不重叠，没有任何一件装备被
    // 两条规则同时收费」那个不变量——它建立在「槽位就是分类」这个错误
    // 前提上。同一个 `def` 在两个场景里各掉一点：拿在攻击方主手里挥
    // 出去掉一点（on-use），戴在防御方副手上挨打掉一点（on-hit）。
    //
    // 反例：把木盾的 `lostland:weapon` 标签从 gameplay.scm 删掉，
    // 第一条断言立即从 `Some(79)` 变回 `Some(80)`；删掉
    // `lostland:armor`，第二条同样变红。两条互不掩盖。
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);
    let shield = |slot: EquipSlot| {
        BTreeMap::from([(
            slot,
            ItemStack::with_durability(handle.wooden_shield_id, 1, WOODEN_SHIELD_MAX_DURABILITY),
        )])
    };

    // Act：① 攻击方主手拿盾砸出去；② 防御方副手举盾挨一下。
    let (bashing, _) = equipment_after_one_attack(
        &handle,
        shield(EquipSlot::MAIN_HAND),
        BTreeMap::new(),
        &catalogs,
    );
    let (_, blocking) = equipment_after_one_attack(
        &handle,
        BTreeMap::new(),
        shield(EquipSlot::OFF_HAND),
        &catalogs,
    );

    // Assert
    assert_eq!(
        bashing
            .get(&EquipSlot::MAIN_HAND)
            .expect("盾仍在攻击方主手")
            .durability,
        Some(WOODEN_SHIELD_MAX_DURABILITY - 1),
        "「使用」通道：盾击把盾用出去了，该磨损"
    );
    assert_eq!(
        blocking
            .get(&EquipSlot::OFF_HAND)
            .expect("盾仍在防御方副手")
            .durability,
        Some(WOODEN_SHIELD_MAX_DURABILITY - 1),
        "「挨打」通道：同一面盾挡了一下，同样该磨损"
    );
}

#[test]
fn 一次攻击经回合引擎同时扣攻击方主手武器与防御方带防具标签的装备() {
    // 一次钉住「两条通道」的全部三半（见
    // `ll_sim::resolve::resolve_attack` 文档「耐久消耗：两条通道，判据
    // 是标签」一节），全程走 `TurnEngine::advance_ai` 这条生产路径：
    //
    // ① 使用通道——攻击方主手战锤（`lostland:weapon`）−1；
    // ② 挨打通道——防御方脖子上的护符（`lostland:armor`）−1；
    // ③ 挨打通道**按标签放行/拦截**——防御方副手的匕首
    //    （只有 `lostland:weapon`）原样不动。
    //
    // ③ 与上一版的差别是判据换了：上一版拦住的是"副手这个槽位"，本版
    // 拦住的是"这件东西不带 on-hit 标签"——同样占副手的木盾现在会掉，
    // 见 `副手拿刀与副手拿盾在同一次挨打里结果相反`。
    //
    // 反例（手工验证过各自会红）：
    // - 删掉「挨打」通道那段 `effects.extend(...)` → ② 变红；
    // - 去掉该段的 `wear_channels.contains(ON_HIT)` 过滤 → ③ 变红；
    // - 删掉「使用」通道那段 → ① 变红。
    // Arrange
    let handle = load_real_mods();
    // 战锤没有显式公式引用，走全局默认公式那一路——本条只关心耐久，
    // 伤害具体是多少不进任何断言。
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);
    let attacker_equipment = BTreeMap::from([(
        EquipSlot::MAIN_HAND,
        ItemStack::with_durability(handle.war_hammer_id, 1, WAR_HAMMER_MAX_DURABILITY),
    )]);
    let defender_equipment = BTreeMap::from([
        (
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(handle.crude_dagger_id, 1, CRUDE_DAGGER_MAX_DURABILITY),
        ),
        (
            EquipSlot::NECK,
            ItemStack::with_durability(
                handle.acid_ward_amulet_id,
                1,
                ACID_WARD_AMULET_MAX_DURABILITY,
            ),
        ),
    ]);

    // Act
    let (attacker_after, defender_after) =
        equipment_after_one_attack(&handle, attacker_equipment, defender_equipment, &catalogs);

    // Assert
    assert_eq!(
        attacker_after
            .get(&EquipSlot::MAIN_HAND)
            .expect("战锤仍在装备栏里")
            .durability,
        Some(WAR_HAMMER_MAX_DURABILITY - 1),
        "①「使用」通道：攻击方主手武器每打出一下损失一点耐久"
    );
    assert_eq!(
        defender_after
            .get(&EquipSlot::NECK)
            .expect("护符仍在装备栏里")
            .durability,
        Some(ACID_WARD_AMULET_MAX_DURABILITY - 1),
        "②「挨打」通道：带 on-hit 标签的装备挨一下损失一点耐久"
    );
    assert_eq!(
        defender_after
            .get(&EquipSlot::OFF_HAND)
            .expect("匕首仍在装备栏里")
            .durability,
        Some(CRUDE_DAGGER_MAX_DURABILITY),
        "③「挨打」通道只放行带 on-hit 标签的：副手的匕首是武器，不磨损"
    );
}

#[test]
fn 没有耐久概念的装备经回合引擎挨打也不会被凭空赋予耐久() {
    // 上一条的反例：同一套场景、同一个 `TurnEngine`，只把两件装备换成
    // `ItemStack::new`（`durability == None`）——「挨打」通道的判据
    // `stack.durability.is_some()` 因此不成立，一条效果都不该产出,
    // `None` 必须原样保持 `None`,不会变成 `Some(-1)` 或 `Some(0)`。
    // 这条同时守住 `apply` 那一侧「没有耐久概念的物品保持 None」的
    // 既有纪律（见 `Effect::AdjustEquipmentDurability` 的 apply 分支）。
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // Act
    let (attacker_after, defender_after) = equipment_after_one_attack(
        &handle,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.war_hammer_id, 1),
        )]),
        BTreeMap::from([(
            EquipSlot::NECK,
            ItemStack::new(handle.acid_ward_amulet_id, 1),
        )]),
        &catalogs,
    );

    // Assert
    assert_eq!(
        attacker_after
            .get(&EquipSlot::MAIN_HAND)
            .unwrap()
            .durability,
        None
    );
    assert_eq!(
        defender_after.get(&EquipSlot::NECK).unwrap().durability,
        None
    );
}

#[test]
fn 真实注册的抗性天赋经由回合引擎真的降低了受到的伤害() {
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 酸匕首显式声明了公式引用，默认值不会被真的用到——理由同
        // `example_mod_resistance.rs` 同一处注释。
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // Act：两场景只差防御方种族——软泥怪在 `mods/example_mod/items.json5`
    // 里 1 级被授予 `examplemod:acid_hide`（对酸 500‰ 抗性），半精灵
    // 没有任何对酸的抗性声明。
    let baseline = damage_dealt_via_turn_engine(&handle, handle.half_elf_id, &catalogs);
    let ooze = damage_dealt_via_turn_engine(&handle, handle.ooze_id, &catalogs);

    // Assert
    assert!(
        baseline > 0,
        "基准场景必须真的打出伤害，否则下面的比较毫无意义"
    );
    assert!(
        ooze < baseline,
        "软泥怪对酸的抗性应当让它经由 TurnEngine 受到的伤害（{ooze}）严格低于没有抗性的基准（{baseline}）"
    );
}

#[test]
fn 目录从回合引擎摘掉后同一场景里抗性不再生效() {
    // 本条是「接线本身」的守卫，见模块文档「接线本身怎么守住」一节：
    // 同一段场景、同一个 `TurnEngine`，只把目录束换成空的，上一条
    // 测试的差异就必须完全消失。谁把目录从 `TurnEngine` 摘掉（例如把
    // `perform` 改回调用裸 `resolve`），上一条测试就会退化成本条的
    // 结果而变红。
    // Arrange
    let handle = load_real_mods();
    let empty = ResolveCatalogs::empty();

    // Act
    let baseline = damage_dealt_via_turn_engine(&handle, handle.half_elf_id, &empty);
    let ooze = damage_dealt_via_turn_engine(&handle, handle.ooze_id, &empty);

    // Assert
    assert_eq!(
        ooze, baseline,
        "没有目录时抗性无从查起，两个防御方受到的伤害必须完全相同"
    );
}

#[test]
fn 真实注册的职业天赋经由回合引擎在达标等级放出技能() {
    // 职业这一路（`ResolveCatalogs::class_traits`）在目录束上是与种族
    // 那一路并列的**另一个字段**——只验收抗性（种族那一路）守不住
    // 「职业字段被填成空实现」。本条因此让一个真实盗贼经由同一个
    // `TurnEngine` 放出职业天赋授予的技能。
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // Act
    let unlocked = backstab_effects_via_turn_engine(&handle, ROGUE_TRAIT_UNLOCK_LEVEL, &catalogs);
    let too_low =
        backstab_effects_via_turn_engine(&handle, ROGUE_TRAIT_UNLOCK_LEVEL - 1, &catalogs);
    let no_catalogs = backstab_effects_via_turn_engine(
        &handle,
        ROGUE_TRAIT_UNLOCK_LEVEL,
        &ResolveCatalogs::empty(),
    );

    // Assert
    assert!(
        unlocked
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "3 级盗贼经由 TurnEngine 应当放得出职业天赋授予的背刺,实际 effects={unlocked:?}"
    );
    assert!(
        !too_low
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "差一级时职业天赋不该放行技能,实际 effects={too_low:?}"
    );
    assert!(
        !no_catalogs
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "目录束为空时同一个 3 级盗贼放不出背刺——这是接线本身的第二道守卫,实际 effects={no_catalogs:?}"
    );
}

/// 空技能目录常量——同时充当空技能树目录（`NoSkills` 实现了
/// `SkillTreeCatalog`，见 `ll_sim::skill_overview` 里那条 impl 的
/// 文档：不为对称再造第二个语义相同的空对象）。
const NO_SKILLS: NoSkills = NoSkills;
