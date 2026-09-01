//! `Intent::Trade` 的结算：五道闸门、占位价格公式、货币守恒，
//! **以及「交易不消耗回合」**（对话系统的批次 5，计划文档
//! `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`）。
//!
//! 只用公开入口（`resolve_with_catalogs` / `TurnEngine` / `apply`），
//! 不碰任何私有函数——与 `dialogue_choose.rs`、`dialogue_quest_outcomes.rs`
//! 同一条纪律。
//!
//! # 本文件咬住的几条
//!
//! | 能力 | 断言 |
//! |---|---|
//! | 买进：货过来、钱过去 | `买进一件东西货和钱各自换手` |
//! | 卖出：方向整个对调 | `卖出一件东西货和钱各自换手` |
//! | **钱不够就不成交** | `钱不够时买不成` |
//! | **owner 校验与对话赠送共用同一条** | `卖不属于自己的东西时零效果` |
//! | 货币守恒 | `一次成交两条钱的和恒为零` |
//! | 价格公式的方向与量纲 | `声望越高买得越便宜`、`价格读的是milli的原始值不是取整` |
//! | **交易不消耗回合** | `交易不消耗回合` |

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
use ll_core::scaled::Milli;
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::combat::Penetration;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::{ItemCatalog, ItemRule, SlotMask, WearChannels};
use ll_sim::resolve::resolve_with_catalogs;
use ll_sim::trade::{TRADE_STANDING_SWING_PERMILLE, TradeDirection, trade_price};
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::{
    Affiliation, AffiliationKind, Agent, BaseStats, EntityId, OrgInstance, OrgRef,
};
use ll_world::faction::{Faction, FactionStatus, FactionTable};
use ll_world::generate::GenParams;
use ll_world::item::ItemStack;
use ll_world::ownership::Owner;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 本体一份烤肉的基础价（`mods/lostland/items.json5` 里那个数量级）。
/// **刻意取一个小于 1000 的值**：它正是「按 `Milli::whole()` 取整会变成
/// 白拿」的那一档，见 `价格读的是milli的原始值不是取整`。
const 烤肉基础价: Milli = Milli(900);

/// 一份只回答「这件东西多少钱、堆到几件」的最小物品目录。
struct 定价目录 {
    item: ContentIndex,
    base_price: Milli,
}

impl ItemCatalog for 定价目录 {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        (item == self.item).then(|| ItemRule {
            stack_limit: 20,
            base_price: self.base_price,
            equip_mask: SlotMask::EMPTY,
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: Penetration::NONE,
            damage_formula: None,
            damage_category: None,
            rule_modifiers: Vec::new(),
            wear_channels: WearChannels::NONE,
            max_durability: None,
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
            furniture: false,
        })
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

fn spawn_agent(world: &mut WorldState) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
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
        inventory: Vec::new(),
        equipment: BTreeMap::new(),
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

/// 一位玩家（钱包 `player_wallet`）与一位商贩（钱包 `partner_wallet`，
/// 背包里 `stock` 件那种东西，归属 `owner`）。
///
/// 玩家真的登记成 `world.player_entity`——`ll_sim::ownership::holder_owner`
/// 就是靠它把「东西到手之后归谁」判成 `Owner::Player` 的。
fn 一桩买卖(
    player_wallet: i64,
    partner_wallet: i64,
    stock: u32,
    owner: Owner,
) -> (WorldState, EntityId, EntityId, ContentIndex) {
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let partner = spawn_agent(&mut world);
    world.player_entity = Some(actor);
    let item = ContentIndex::default();
    {
        let player = world.actors.get_mut(actor).expect("刚生成的实体必然存在");
        player.wallet = player_wallet;
    }
    {
        let vendor = world.actors.get_mut(partner).expect("刚生成的实体必然存在");
        vendor.wallet = partner_wallet;
        vendor.inventory = vec![ItemStack {
            owner,
            ..ItemStack::new(item, stock)
        }];
    }
    (world, actor, partner, item)
}

fn 目录<'a>(items: &'a 定价目录) -> ResolveCatalogs<'a> {
    ResolveCatalogs {
        items,
        ..ResolveCatalogs::empty()
    }
}

fn 钱包(world: &WorldState, who: EntityId) -> i64 {
    world.actors.get(who).expect("实体在").wallet
}

fn 背包(world: &WorldState, who: EntityId) -> Vec<ItemStack> {
    world.actors.get(who).expect("实体在").inventory.clone()
}

/// 把一串效果真的落到世界上（`apply` 是唯一写入口，C1）。
fn 落地(world: &mut WorldState, effects: &[Effect]) {
    for effect in effects {
        apply(world, effect);
    }
}

// ── 主线：两个方向各一条 ──────────────────────────────────────────

/// 买进：货从商贩到玩家，钱从玩家到商贩。
///
/// 故意改坏的反例（本批实测）：把 `resolve_trade` 里那条
/// `Effect::ConsumeInventoryItem` 去掉，本条当场红（商贩手里还是 3 件）。
#[test]
fn 买进一件东西货和钱各自换手() {
    // Arrange
    let (mut world, actor, partner, item) = 一桩买卖(10_000, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    // 先断言对象存在，否则下面的数量断言可能恒真。
    assert_eq!(背包(&world, partner).len(), 1, "商贩手里真的有货");

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );
    落地(&mut world, &effects);

    // Assert
    assert_eq!(背包(&world, partner)[0].count, 2, "商贩少一件");
    let 到手 = 背包(&world, actor);
    assert_eq!(到手.len(), 1, "玩家多出一堆");
    assert_eq!(到手[0].count, 1, "一次一件");
    assert_eq!(到手[0].owner, Owner::Player, "到手之后归玩家");
    assert_eq!(钱包(&world, actor), 10_000 - 900, "玩家付了 900");
    assert_eq!(钱包(&world, partner), 900, "商贩收了 900");
}

/// 卖出：方向整个对调——货从玩家到商贩，钱从商贩到玩家。
///
/// 故意改坏的反例（本批实测）：把
/// `TradeDirection::seller_and_buyer` 的两支对调，本条与上一条**同时**
/// 红（两条互为对方的对照组）。
#[test]
fn 卖出一件东西货和钱各自换手() {
    // Arrange：这一次货在玩家手里、钱在商贩手里。
    let (mut world, actor, partner, item) = 一桩买卖(0, 10_000, 0, Owner::Unowned);
    world
        .actors
        .get_mut(actor)
        .expect("玩家在")
        .inventory
        .push(ItemStack {
            owner: Owner::Player,
            ..ItemStack::new(item, 2)
        });
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    assert_eq!(背包(&world, actor).len(), 1, "玩家手里真的有货");

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Sell,
        },
        &目录(&items),
    );
    落地(&mut world, &effects);

    // Assert
    assert_eq!(背包(&world, actor)[0].count, 1, "玩家少一件");
    assert_eq!(背包(&world, partner).len(), 1, "商贩多出一堆");
    assert_eq!(钱包(&world, actor), 900, "玩家收了 900");
    assert_eq!(钱包(&world, partner), 10_000 - 900, "商贩付了 900");
}

// ── 五道闸门 ──────────────────────────────────────────────────────

/// **钱不够就不成交**（第五道闸门）。
///
/// 故意改坏的反例（本批实测）：把 `resolve_trade` 里那句
/// `if buyer_agent.wallet < price { return Vec::new(); }` 删掉，本条当场
/// 红——玩家会拿着 899 块钱买走一件 900 块的东西，钱包变成 `-1`。
#[test]
fn 钱不够时买不成() {
    // Arrange：差一块钱。
    let (world, actor, partner, item) = 一桩买卖(899, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    // 先断言「差一块钱」这个前提确实成立——否则本条可能是因为别的闸门
    // 拦下来的，那样它守的就不是自己想守的那一条。
    assert_eq!(trade_price(烤肉基础价, 0), 900);
    assert_eq!(钱包(&world, actor), 899);

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert!(effects.is_empty(), "钱不够就零效果，实际是 {effects:?}");
}

/// 刚好够就成交——上一条的对照组。
///
/// 没有它，`钱不够时买不成` 可能是因为**任何**闸门拦住了，而不是钱。
#[test]
fn 钱刚好够时买得成() {
    // Arrange
    let (world, actor, partner, item) = 一桩买卖(900, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert_eq!(effects.len(), 4, "两条搬运 + 两条钱");
}

/// **owner 校验硬前置**：不属于卖方的东西卖不出去。
///
/// 这一条与批次 4 的 `说话人送不属于自己的东西时give_item零效果`
/// **调的是同一个函数**（`ll_sim::ownership::may_give_away`）。
///
/// 故意改坏的反例（本批实测）：把 `may_give_away` 的
/// `Owner::Player => giver == Owner::Player` 改成恒真，**本条与对话赠送
/// 那一侧同时红**——那正是「复用了同一条校验，不是另写一份」的证据。
#[test]
fn 卖不属于自己的东西时零效果() {
    // Arrange：商贩背包里那一堆挂着**玩家**的名字（例如玩家刚寄放在
    // 他那儿），他卖不掉。
    let (world, actor, partner, item) = 一桩买卖(10_000, 0, 3, Owner::Player);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    // 先断言对象存在：那一堆真的在他背包里、真的挂着别人的名字。
    let 货 = 背包(&world, partner);
    assert_eq!(货.len(), 1);
    assert_eq!(货[0].owner, Owner::Player);

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert!(
        effects.is_empty(),
        "不是他的东西卖不出去，实际是 {effects:?}"
    );
}

/// 无主的那一档照常卖得出去——上一条的对照组。
///
/// 今天 NPC 背包里全是 `Owner::Unowned` 的出生装备，这一档若也被拦下，
/// 交易整条就是死的，而 `卖不属于自己的东西时零效果` 照样绿。
#[test]
fn 卖无主的东西照常成交() {
    // Arrange
    let (world, actor, partner, item) = 一桩买卖(10_000, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert_eq!(effects.len(), 4);
}

/// 卖方拿不出这件东西 ⇒ 零效果（第二道闸门）。
#[test]
fn 卖方没有那件东西时零效果() {
    // Arrange：商贩背包空。
    let (world, actor, partner, item) = 一桩买卖(10_000, 0, 0, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    // `ItemStack::new(item, 0)` 仍然是一条堆——把它整条清掉才是「没有」。
    let mut world = world;
    world
        .actors
        .get_mut(partner)
        .expect("商贩在")
        .inventory
        .clear();
    assert!(背包(&world, partner).is_empty());

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert!(effects.is_empty());
}

/// 目录里查不到这件东西的定价 ⇒ **不成交**，而不是按 0 白送
/// （第四道闸门）。
#[test]
fn 查不到定价时不成交() {
    // Arrange：目录只认另一个索引。
    let (world, actor, partner, item) = 一桩买卖(10_000, 0, 3, Owner::Unowned);
    let 另一个 = {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:other").expect("合法标识符"));
        interner.intern(NamespacedId::parse("lostland:another").expect("合法标识符"))
    };
    assert_ne!(另一个, item, "夹具前提：目录认的不是被交易的那一件");
    let items = 定价目录 {
        item: 另一个,
        base_price: 烤肉基础价,
    };

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    assert!(effects.is_empty(), "不知道多少钱就不成交");
}

// ── 货币守恒 ──────────────────────────────────────────────────────

/// 一次成交里两条 `AdjustWallet` 的和恒为零。
///
/// 故意改坏的反例（本批实测）：把卖方那条 `AdjustWallet` 去掉（钱凭空
/// 消失）或把它的符号改成负（钱凭空产生），本条当场红。
#[test]
fn 一次成交两条钱的和恒为零() {
    // Arrange
    let (world, actor, partner, item) = 一桩买卖(10_000, 5_000, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
    );

    // Assert
    let 钱: Vec<i64> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::AdjustWallet { delta, .. } => Some(*delta),
            _ => None,
        })
        .collect();
    assert_eq!(钱.len(), 2, "一次成交恰好两条钱：一付一收");
    assert_eq!(钱.iter().sum::<i64>(), 0, "货币守恒：两条钱的和恒为零");
    assert!(钱.contains(&-900) && 钱.contains(&900));
}

// ── 占位价格公式 ──────────────────────────────────────────────────

/// 价格读的是 `Milli` 的**原始值**，不是 `whole()` 取整。
///
/// 故意改坏的反例（本批实测）：把 `trade_price` 里的 `base.0` 换成
/// `base.whole()`，本条当场红（900 变成 0，一份烤肉白拿）。
#[test]
fn 价格读的是milli的原始值不是取整() {
    // Arrange & Act & Assert
    assert_eq!(烤肉基础价.whole(), 0, "夹具前提：这一档取整之后就是 0");
    assert_eq!(trade_price(烤肉基础价, 0), 900, "但它真的值 900");
}

/// 声望越高买得越便宜，且摆动幅度就是那个常量说的两成。
///
/// 故意改坏的反例（本批实测）：把 `trade_price` 里那个减号改成加号
/// （声望越高越贵），本条当场红。
#[test]
fn 声望越高买得越便宜() {
    // Arrange
    let base = Milli(10_000);
    let 满值 = Affiliation::STANDING_FULL;

    // Act
    let 中立 = trade_price(base, 0);
    let 满声望 = trade_price(base, 满值);
    let 满敌对 = trade_price(base, -满值);

    // Assert
    assert_eq!(中立, 10_000, "中立就是原价");
    assert_eq!(
        满声望,
        10_000 - 10_000 * TRADE_STANDING_SWING_PERMILLE / 1000,
        "满声望打八折"
    );
    assert_eq!(
        满敌对,
        10_000 + 10_000 * TRADE_STANDING_SWING_PERMILLE / 1000,
        "满敌对加价两成"
    );
    assert!(满声望 < 中立 && 中立 < 满敌对);
}

/// 越界的声望先被夹住——`trade_price` 不该因为一个野值算出负价。
#[test]
fn 越界的声望先被夹进量纲两端() {
    // Arrange & Act & Assert
    let base = Milli(10_000);
    assert_eq!(
        trade_price(base, Affiliation::STANDING_FULL * 100),
        trade_price(base, Affiliation::STANDING_FULL),
    );
    assert_eq!(
        trade_price(base, -Affiliation::STANDING_FULL * 100),
        trade_price(base, -Affiliation::STANDING_FULL),
    );
}

/// 基础价非零的东西**不可能白拿**（下界 1），基础价本身为零的仍然是零。
///
/// 故意改坏的反例（本批实测）：把 `trade_price` 末尾的 `.max(1)` 去掉，
/// 本条当场红（`Milli(1)` 在满声望下算出 0）。
#[test]
fn 非零基础价至少收一块而零价仍然是零() {
    // Arrange & Act & Assert
    assert_eq!(
        trade_price(Milli(1), Affiliation::STANDING_FULL),
        1,
        "满声望也不能把一件有价的东西变成免费"
    );
    assert_eq!(
        trade_price(Milli::ZERO, 0),
        0,
        "内容作者说它不值钱，公式不替他定价"
    );
}

/// 玩家加入了对方的势力之后，**同一件东西真的变便宜**。
///
/// 这是批次 3 那条 `standing` 的第一个读者，端到端地把
/// 「归属 → 价格」这条链走通。
///
/// 故意改坏的反例（本批实测）：把 `ll_sim::trade::partner_standing`
/// 改成恒返回 `0`，本条当场红（两次价格一样）。
#[test]
fn 加入对方势力之后同一件东西更便宜() {
    // Arrange
    let (mut world, actor, partner, item) = 一桩买卖(100_000, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: Milli(10_000),
    };
    let mut counter = 0u32;
    let site = WorldId::next(&mut counter);
    let faction = WorldId::next(&mut counter);
    world.actors.get_mut(partner).expect("商贩在").home = Some(site);
    world.factions = FactionTable::rebuild(vec![Faction {
        org: OrgInstance {
            id: faction,
            def: None,
            authored: None,
        },
        seat: site,
        founded_epoch: 0,
        status: FactionStatus::Active,
        members: vec![site],
    }])
    .expect("一个势力一座据点满足全部不变式");
    let 成交价 = |world: &WorldState| -> i64 {
        let effects = resolve_with_catalogs(
            world,
            &Intent::Trade {
                actor,
                partner,
                item,
                direction: TradeDirection::Buy,
            },
            &目录(&items),
        );
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::AdjustWallet { delta, .. } if *delta < 0 => Some(-*delta),
                _ => None,
            })
            .expect("这一笔必须成交，否则本条比的是两个空")
    };

    // Act
    let 陌生人价 = 成交价(&world);
    world
        .actors
        .get_mut(actor)
        .expect("玩家在")
        .affiliations
        .push(Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(faction),
            standing: ll_sim::dialogue::JOIN_SETTLEMENT_STANDING,
        });
    let 自己人价 = 成交价(&world);

    // Assert
    assert_eq!(陌生人价, 10_000, "没有归属就是中立原价");
    assert!(
        自己人价 < 陌生人价,
        "加入之后应当更便宜：{自己人价} vs {陌生人价}"
    );
}

// ── 不消耗回合 ────────────────────────────────────────────────────

/// **交易不消耗回合**——**本批自裁，规格没写**（计划文档三节 3.6 与
/// 十一节第 1 条）。
///
/// 每一支新能力都要有自己的这一条：批次 3 记下过一个陷阱，批次 2 那条
/// 「不消耗回合」只走 `set-flag` 一支，加了新变体之后它不再覆盖新的
/// 那一支。`open-trade` 那一条在 `dialogue_choose.rs`，这一条守
/// `Intent::Trade`。
///
/// 故意改坏的反例（本批实测）：给 `resolve_trade` 的返回值补一条
/// `Effect::ScheduleNext`，本条当场红。
#[test]
fn 交易不消耗回合() {
    // Arrange
    let (mut world, actor, partner, item) = 一桩买卖(10_000, 0, 3, Owner::Unowned);
    let items = 定价目录 {
        item,
        base_price: 烤肉基础价,
    };
    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(actor, Tick(0));
    let mut engine = TurnEngine::new(timeline);
    let clock_before = world.clock;
    let next_before = world.actors.get(actor).expect("玩家在").next_action_at;

    // Act
    let mut on_effect = |_world: &WorldState, _effect: &Effect| {};
    let mut ai =
        |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
    engine.advance_ai(&mut world, actor, &mut ai, &目录(&items), &mut on_effect);
    let outcome = engine.try_player_intent(
        &mut world,
        actor,
        Intent::Trade {
            actor,
            partner,
            item,
            direction: TradeDirection::Buy,
        },
        &目录(&items),
        &mut on_effect,
    );

    // Assert：**先确认这一笔真的成交了**，否则「时钟没动」只是因为
    // 什么都没发生。
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(钱包(&world, actor), 10_000 - 900, "对照组：钱真的付了");
    assert_eq!(world.clock, clock_before, "交易不消耗回合：世界时钟不动");
    assert_eq!(
        world.actors.get(actor).expect("玩家在").next_action_at,
        next_before,
        "交易不消耗回合：下次行动时刻不动"
    );
}
