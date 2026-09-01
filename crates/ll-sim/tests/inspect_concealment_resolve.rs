//! `RuleModifier::InspectionConcealment` 接进 `resolve_inspect` 的集成
//! 测试（盗贼被动两分批次）——项目所有者裁定「被动可以分为 2 种，
//! 不觉得可疑，还有**查不出东西**」里的后一种，在真实 `resolve` 全链路
//! 上的边界验收。
//!
//! 形状照抄 `resistance_resolve.rs`（同一套 `FakeItems`/`FakeRaceTraits`/
//! `FakeTraits` 手法、同一条 `resolve_with_*` 入口）。本文件只验收
//! **纯 Rust 结算侧**的三条边界，真实 mod 脚本 + `TurnEngine` 那条端到端
//! 证据在 `crates/ll-mod/tests/example_mod_rogue_passives.rs`：
//!
//! 1. **顶格的藏匿修正也藏不绝**：判定系统落地批次把这一条从
//!    「`1000‰` → `items_seen` 恒为空」改成了它的反面。旧断言在新模型
//!    下**不可能成立**，而这正是「不允许绝对」那条裁定要的结果：修正
//!    上限 `L` 的推导（`ll_sim::check` 模块文档「不允许绝对」一节）保证
//!    双方在任何合法修正下都还有赢面。本条因此断言两件事：顶格修正下
//!    **既存在一件都藏不住的种子，也存在真的藏住东西的种子**——前者
//!    钉死「没有绝对」，后者钉死「这条被动真的有用」。盘查本身照常
//!    发生、照常消耗一个回合这一半不变。
//! 2. **压根没有这条被动**：背包与装备全部如实被看到，一次随机数都不
//!    消耗——反例，证明第 1 条不是「盘查从来就看不到东西」。这一档与
//!    「显式声明 0」是两回事，后者判定照常发生（见
//!    `ll_sim::rule_modifier::concealment_check_modifier` 文档）。
//! 3. 同一个世界种子、同一个时刻，同一次盘查的结果**逐位可重放**
//!    （约束 C3/C5），且判定用的是**被盘查者**的流——换一个盘查者不
//!    改变结果。
//!
//! 槽位句柄批次（`Effect::Inspect::items_seen` 的元素从裸
//! `ContentIndex` 换成 `InspectedItem`）在此之上加了第 4 条：本文件的
//! 目标身上带的**四堆全是同一种物品**（`lostland:coin`），旧形状下
//! 那四条记录逐字相同、完全无法区分——正是槽位句柄要消灭的那个形态。
//! 第 4 条断言它们现在各自带着不同的句柄，且顺序仍然是「先背包（原始
//! 顺序）、后装备（`EquipSlot` 升序）」。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::check::CHECK_DICE;
use ll_sim::combat::Penetration;
use ll_sim::effect::{CarriedItemSlot, Effect, InspectedItem};
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemCatalog, ItemRule, ItemStack, SlotMask, WearChannels};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::resource_pool::NoResourcePools;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{
    RuleModifier, TraitCatalog, TraitGrant, TraitGrantSource, TraitRule, TypedRuleModifier,
};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 目标身上带的物品件数——两件在背包、两件穿在身上，覆盖
/// `resolve_inspect` 拼快照的两段（先背包、后装备）。
const CARRIED_ITEMS: usize = 4;

struct FakeItems {
    items: BTreeMap<ContentIndex, ItemRule>,
}

impl ItemCatalog for FakeItems {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        self.items.get(&item).cloned()
    }
}

/// 只认识固定种族索引的测试用天赋授予来源，理由同
/// `resistance_resolve.rs::FakeRaceTraits`。
struct FakeRaceTraits {
    race: ContentIndex,
    grants: Vec<TraitGrant>,
}

impl TraitGrantSource for FakeRaceTraits {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        if owner == self.race {
            self.grants.clone()
        } else {
            Vec::new()
        }
    }
}

struct FakeTraits {
    traits: BTreeMap<ContentIndex, TraitRule>,
}

impl TraitCatalog for FakeTraits {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.traits.get(&trait_id).cloned()
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

fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    x: i32,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(x, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
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

/// 一件没有任何特殊规则的普通物品——本文件只关心「它有没有出现在
/// `items_seen` 里」，其余字段取不影响判定的占位值。
fn plain_item_rule() -> ItemRule {
    ItemRule {
        wear_channels: WearChannels::NONE,
        max_durability: None,
        taught_recipes: Vec::new(),
        requires_identification: false,
        study_experience: 0,
        blind_box_pool: Vec::new(),
        furniture: false,
        stack_limit: 1,
        equip_mask: SlotMask::EMPTY,
        stat_bonuses: Vec::new(),
        use_effect: None,
        penetration: Penetration::NONE,
        damage_formula: None,
        damage_category: None,
        rule_modifiers: Vec::new(),
    }
}

/// 摆好「一个卫兵盘查一个身上带四件东西的目标」的场景，目标的种族
/// 天赋按 `conceal_permille` 声明藏匿（`None` = 一条被动都不声明）。
/// 返回那一次盘查看到的物品列表。
///
/// `guard_x` 只影响盘查者是谁/站在哪 —— 第 3 条测试用它验证判定流取
/// 的是**被盘查者**，不是盘查者。
fn inspect_once(
    concealment_modifier: Option<i32>,
    world_seed: u64,
    guard_x: i32,
) -> Vec<InspectedItem> {
    let mut interner = Interner::new();
    let mut index = |raw: &str| interner.intern(NamespacedId::parse(raw).expect("合法标识符"));
    let race = index("lostland:human");
    let thief_race = index("lostland:cutpurse");
    let trait_id = index("lostland:cutpurse_training");
    let coin = index("lostland:coin");

    let mut world = test_world();
    world.seed = world_seed;

    let mut equipment = BTreeMap::new();
    equipment.insert(EquipSlot::MAIN_HAND, ItemStack::new(coin, 1));
    equipment.insert(EquipSlot::OFF_HAND, ItemStack::new(coin, 1));
    let target = spawn_agent(
        &mut world,
        thief_race,
        6,
        vec![ItemStack::new(coin, 1), ItemStack::new(coin, 1)],
        equipment,
    );
    let guard = spawn_agent(&mut world, race, guard_x, Vec::new(), BTreeMap::new());

    let grants = match concealment_modifier {
        Some(_) => vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }],
        None => Vec::new(),
    };
    let race_traits = FakeRaceTraits {
        race: thief_race,
        grants,
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            trait_id,
            TraitRule {
                rule_modifiers: concealment_modifier
                    .map(|points| {
                        vec![TypedRuleModifier {
                            modifier_type: None,
                            modifier: RuleModifier::InspectionConcealment {
                                concealment_modifier: points,
                            },
                        }]
                    })
                    .unwrap_or_default(),
                ..TraitRule::default()
            },
        )]),
    };
    let items = FakeItems {
        items: BTreeMap::from([(coin, plain_item_rule())]),
    };

    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Inspect {
            actor: guard,
            target,
        },
        &NoSkills,
        &race_traits,
        &traits,
        &NoResourcePools,
        &items,
    );

    // 盘查本身必须照常发生并照常消耗一个回合——藏匿改的是「看到了
    // 什么」，不是「查没查」。这条断言写在帮手里，三条测试共享。
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ScheduleNext { .. })),
        "藏匿不该让盘查不再消耗回合"
    );
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Inspect { items_seen, .. } => Some(items_seen.clone()),
            _ => None,
        })
        .expect("盘查必须照常产出 Effect::Inspect，即使一件东西都没查到")
}

/// 硬要求一：顶格的藏匿修正**也藏不绝**，但确实藏得住东西。
///
/// 用的是 `CHECK_DICE.max_modifier()`（`3d20` 下是 28）——比它更大的
/// 声明在装载期就会被拒（`ll_mod::content_schema_gear` 的
/// `checked_check_modifier`），运行期还会再被
/// `CheckDice::clamp_modifier` 钳回来，所以这就是这条被动能达到的
/// **上限**。
#[test]
fn 顶格藏匿既藏不绝也确实藏得住() {
    // Arrange
    let cap = i32::try_from(CHECK_DICE.max_modifier()).expect("28 落在 i32 内");

    // Act：扫一批种子，数出「一件都没藏住」与「至少藏住一件」各出现
    // 过没有。扫种子而不是断言某一个种子的结果，是因为要证明的是两个
    // 结果**都可能**，那本来就是一句关于分布的话。
    //
    // 断言的粒度是**单件**，不是「四件全被看到」：藏匿是逐件判定，
    // 顶格修正下单件被看到的概率约 21‰（`3d20` 净差 −28 的精确值），
    // 四件同时被看到是 21‰ 的四次方，约两千万分之一——那不是「绝对
    // 藏住」，只是小到扫不出来。要证的命题本来就是单件那一条：
    // **存在一件东西被看见**。
    let mut saw_something = false;
    let mut hid_something = false;
    for seed in 0..64u64 {
        let seen = inspect_once(Some(cap), seed, 5);
        if !seen.is_empty() {
            saw_something = true;
        }
        if seen.len() < CARRIED_ITEMS {
            hid_something = true;
        }
    }

    // Assert 一：没有绝对——顶格修正下仍然存在被看见的东西。
    assert!(
        saw_something,
        "顶格藏匿下 64 个种子里一件东西都没被看见过，绝对性可能回来了"
    );
    // Assert 二：这条被动真的有用——否则上一条断言用一个「藏匿完全
    // 不起作用」的实现也能通过。
    assert!(hid_something, "顶格藏匿一件东西都没藏住过，说明修正没接上");
}

/// 硬要求二（反例）：没有这条被动时背包与装备全部如实被看到——证明
/// 上一条不是「盘查本来就看不到东西」。
///
/// 这一档**一次随机数都不消耗**（`concealment_check_modifier` 返回
/// `None`，调用方整段跳过），因此它对任何种子都成立，可以逐字断言。
/// 显式声明 `0` 是另一回事：判定照常发生，结果随种子变，不能这么断言。
#[test]
fn 没有藏匿声明时背包与装备全部如实被看到() {
    // Act
    let without_trait = inspect_once(None, 7, 5);

    // Assert
    assert_eq!(without_trait.len(), CARRIED_ITEMS);

    // 换 32 个种子仍然全看到：证明这一档确实没有掷骰，不是恰好掷赢了。
    for seed in 0..32u64 {
        assert_eq!(
            inspect_once(None, seed, 5).len(),
            CARRIED_ITEMS,
            "没有藏匿声明时不该有任何判定，种子 {seed} 却改变了结果"
        );
    }
}

/// 显式声明 `0` 与压根没有声明是两回事：前者判定照常发生（并因此消耗
/// 随机数、结果随种子变），后者整段跳过。
///
/// 这条钉的是 `ll_sim::rule_modifier::concealment_check_modifier` 文档
/// 「缺省与声明 0」那一节在**全链路**上的可观测差别——聚合层的
/// `None` vs `Some(0)` 若在 `resolve` 侧被同等对待，这条测试就会红。
#[test]
fn 显式声明零藏匿仍然掷骰而没有声明整段跳过() {
    // Act：同一批种子，两档各跑一遍。
    let mut zero_varies = false;
    for seed in 0..64u64 {
        if inspect_once(Some(0), seed, 5).len() != CARRIED_ITEMS {
            zero_varies = true;
        }
    }

    // Assert：声明 0 的那一档存在「没全看到」的种子（判定真的发生了），
    // 而上一条测试已经证明没有声明的那一档恒是全看到。
    assert!(
        zero_varies,
        "显式声明 0 的藏匿在 64 个种子里结果恒定，说明判定被整段跳过了"
    );
}

/// 硬要求四（槽位句柄批次）：四堆同种物品各自带着能把它们分开的句柄。
///
/// 这条钉的是项目所有者裁定要修的那个具体形态——「背包 [0] 铁剑 ×1
/// （自己买的）/ [1] 铁剑 ×1（偷来的）」在旧形状（`Vec<ContentIndex>`）
/// 下是两条逐字相同的记录，`Owner` 落地后的逐堆归属比对拿到它们判不了
/// 罪。本文件的夹具恰好就是这个形态的四堆版本（背包两堆 coin + 主手
/// 副手各一堆 coin）。
#[test]
fn 四堆同种物品各自带着不同的槽位句柄且顺序是先背包后装备() {
    // Act：不声明任何藏匿，四堆全部如实被看到。
    let seen = inspect_once(None, 7, 5);

    // Assert 一：四条记录的物品定义**全部相同**——证明这确实是「同种
    // 物品的多堆」这个旧形状分不开的场景，不是靠 def 不同蒙混过关。
    assert_eq!(seen.len(), CARRIED_ITEMS);
    let first_def = seen[0].def;
    assert!(
        seen.iter().all(|item| item.def == first_def),
        "夹具本身必须是四堆同种物品，否则这条测试证明不了任何事"
    );

    // Assert 二：四条记录的槽位句柄两两不同，且顺序是先背包（下标
    // 升序）后装备（`EquipSlot` 升序，`BTreeMap` 天然有序，约束 C5）。
    let slots: Vec<CarriedItemSlot> = seen.iter().map(|item| item.slot).collect();
    assert_eq!(
        slots,
        vec![
            CarriedItemSlot::Inventory { index: 0 },
            CarriedItemSlot::Inventory { index: 1 },
            CarriedItemSlot::Equipped {
                slot: EquipSlot::MAIN_HAND
            },
            CarriedItemSlot::Equipped {
                slot: EquipSlot::OFF_HAND
            },
        ]
    );
}

/// 硬要求三：确定性重放（约束 C3/C5），且判定流取的是**被盘查者**。
#[test]
fn 藏匿判定可重放且判定流属于被盘查者而不是盘查者() {
    // Arrange & Act：同一个种子跑两遍必须逐位相同。修正取 9（半颗
    // 骰子，扒手训练的真实声明值）。
    let first = inspect_once(Some(9), 12345, 5);
    let second = inspect_once(Some(9), 12345, 5);

    // Assert 一：可重放。
    assert_eq!(first, second);

    // Act：只换盘查者的位置（`EntityId` 不变，因为生成顺序没变），
    // 结果必须不变——`DetRng::for_entity` 的三元组里那一项取的是
    // `target`，不是 `actor`。
    //
    // 注意换成对抗判定之后盘查者**确实**进了式子（他的意志调整值是
    // 主动方修正），但进的是修正，不是随机流的身份；这里换的只是位置,
    // 属性一个字没变，因此结果仍然必须逐位相同。
    let other_guard = inspect_once(Some(9), 12345, 9);

    // Assert 二
    assert_eq!(first, other_guard);

    // Act：换世界种子，结果应当**不同**——否则「这条流真的被消费了」
    // 这句话没有证据（一个恒定返回同一份快照的实现也能通过上面两条）。
    let mut differing = 0;
    for seed in 0..32u64 {
        if inspect_once(Some(9), seed, 5) != first {
            differing += 1;
        }
    }

    // Assert 三：32 个种子里至少有一个给出不同的结果。
    assert!(
        differing > 0,
        "换种子结果恒不变，说明藏匿判定没有真的消费 DetRng 流"
    );
}
