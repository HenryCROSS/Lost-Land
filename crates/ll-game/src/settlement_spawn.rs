//! 据点物化：把一座据点的**人**与**家**一次性放进世界。
//!
//! # 这个模块是从 [`crate::world`] 搬出来的，不是新写的
//!
//! `materialize_nearby_settlements` / `place_roster` / `NPC_PLACEMENT_RADIUS`
//! 三样原本住在 `crate::world`。搬家有两个理由，第二个才是决定性的：
//!
//! 1. **职责**：`world.rs` 是「建一局游戏」的流水线（地形、编年史、
//!    出生点、玩家实体）；「一座据点里住着谁、屋里摆着什么」是它自己
//!    的一件事。
//! 2. **行数棘轮**：`crates/ll-game/src/world.rs` 在
//!    `scripts/ci/file_size_budget.json` 的快照里（960 代码行，规格 §13
//!    上限 800），那道门禁**只许缩不许涨**。本批次要往这条路径上加
//!    「摆家具」，加在原地就会被门禁当场拦下——而门禁给的提示正是
//!    「先把要加的那部分按职责拆进一个新模块」。**搬家因此不是为了
//!    绕开门禁，是照它说的做。**
//!
//! `crate::world` 保留一行 `pub use`，既有调用点
//! （`ll_game::world::materialize_nearby_settlements`，包括两个整合
//! 测试文件）一个字都不用改。
//!
//! # 摆家具：所有者要的那件事
//!
//! > 「建筑需要根据他的类型填入不同的家具，例如箱子，椅子，床，书柜等。」
//! > 「每个物品都会有个主人，一个建筑内的物品通常都是属于某个人的。」
//!
//! 「摆什么、摆在哪」由 [`ll_world::building::settlement_furnishing`]
//! 算出（纯函数，读的是文化声明的建筑类型）；本模块负责**把它写进
//! 世界**——那一步需要地形（这一格真的是已经铺好的木地板吗）、需要
//! 实体表（这一格有人站着吗）、需要物品表（这件东西真的是家具吗），
//! 三样都只有这一层拿得到。
//!
//! # 为什么家具挂在「据点物化」这一趟，而不是「区块首次物化」
//!
//! 与 NPC 逐字同一条理由（见 [`materialize_nearby_settlements`] 文档
//! 「为什么不是『区块首次物化时生成』」）：一座据点可以横跨最多 8 个
//! 区块，按区块触发会把同一座据点触发多次。按据点触发则一次到位，
//! `WorldState::settlement_is_materialized` 已经记着谁物化过了，不需要
//! 第二套触发机制。
//!
//! # 与「每格至多站一人」那条不变式的相互作用
//!
//! 放置的家具（[`ll_world::item::GroundItemStack::placed`]）**独占那一
//! 格**：别的东西丢不进来（`ll_sim::resolve` 的 `resolve_drop`/
//! `resolve_place` 前置）。但它**不阻挡实体站上去**——两条不变式管的
//! 是不同的东西。真正的风险是「家具把 NPC 的生成位置堵死」，本模块用
//! 三条互相独立的保证把它消掉：
//!
//! 1. **人先摆、家具后摆**：[`furnish_settlement`] 在
//!    [`place_roster`] 之后跑，并且跳过任何已被实体占住的格。
//! 2. **每栋屋子正中恒空**：[`ll_world::building::MAX_FURNITURE_PER_BUILDING`]
//!    是 8 而不是 9，正中那一格结构上就不参与摆放。
//! 3. **`place_roster` 跳过已有放置物的格**：上一座据点摆下的家具，
//!    不会被下一批 NPC 站进去。
//!
//! 三条合起来：家具永不挤掉 NPC，NPC 也不会站进家具里。

use ll_core::torus::TorusPos;
use ll_world::building::settlement_furnishing;
use ll_world::entity::EntityId;
use ll_world::item::{GroundItemStack, ItemStack};
use ll_world::ownership::Owner;
use ll_world::settlement::SettlementStatus;
use ll_world::state::WorldState;
use ll_world::zone::ZoneLayout;

use crate::content::LoadedContent;

/// 物化 NPC 时，从据点锚点向外最多搜多少格找可站立的位置。
///
/// 取 26 = [`ll_world::settlement::MAX_FOOTPRINT_RADIUS`] 的量级：一座
/// 长满的据点（80 栋建筑）外廓半径正是这个数，搜到这里就等于「把整座
/// 村子找了一遍」。再往外就会走进荒野，NPC 会站在离家很远的地方。
const NPC_PLACEMENT_RADIUS: i32 = 26;

/// 把**当前常驻区块里、还有人住、且此前从未物化过**的据点，各自的名册
/// 物化成真正的 `Agent`。
///
/// 返回这一次新生成的实体，调用方负责把它们排进时间轴
/// （[`ll_sim::turn::TurnEngine::schedule`]）——本函数不碰时间轴，与
/// `ll-world`/`ll-sim` 的分层一致（世界状态归 `WorldState`，调度归回合
/// 引擎）。
///
/// # 时机：跟着流式加载走，不另起一套触发机制
///
/// 调用点是 [`crate::app::Demo::maintain_streaming`]——**已经存在的、
/// 每帧真跑一遍的那个钩子**，与 `cleanup_aged_ground_items` 并列（同一
/// 条「复用已有的每帧钩子，比新建一套框架更小、更诚实」的既有理由）。
/// 它排在 `stream_neighborhood` **之后**：物化要读地形判断「这一格能不
/// 能站人」，而那些区块正是上一行刚刚装进来的。
///
/// # 为什么不是「区块首次物化时生成」
///
/// 那是最直觉的时机，也是上一批标记为「唯一没有现成先例」的那个设计
/// 问题——它的麻烦在于**一座据点可以横跨最多 8 个区块**（见
/// `ll_world::settlement` 模块文档「实测」一节）：按区块触发，同一座
/// 据点会被触发多次，每次只该生成「属于这个区块的那几个人」，而名册
/// 本身是按据点派生的、不按区块切分。按**据点**触发则一次到位：一座
/// 据点物化一次，全部人一起出现，`materialized_settlements` 里也只记
/// 一条。
///
/// 代价如实标注：玩家走到一座跨界据点的边缘时，据点另一头（可能还在
/// 未加载区块里）的那几个人会因为找不到常驻地形而被跳过——见
/// [`place_roster`]。他们不会在下次靠近时补上（这座据点已经记成「物化
/// 过」了）。修它需要把「物化」从一次性事件改成可增量的，那要的正是
/// 上面否掉的逐 NPC 身份，属独立批次。
///
/// # 确定性
///
/// 本函数不取任何随机数：名册来自 [`ll_mod::roster::settlement_roster`]
/// （自己走 `DetRng::for_entity`），位置来自一次确定性的方环扫描。据点
/// 的处理顺序由 [`ll_world::chronicle::WorldChronicle::sites_touching_zone`]
/// 与区块光栅序共同决定，不依赖任何 `HashMap` 迭代顺序（约束 C5）。
pub fn materialize_nearby_settlements(
    world: &mut WorldState,
    content: &LoadedContent,
    roles: &ll_mod::roster::SettlementRoles,
) -> Vec<EntityId> {
    let Some(chronicle) = world.terrain.chronicle_handle() else {
        // 没装编年史就没有据点，因此也不该有任何 NPC——「有 NPC 的地方
        // 必然存在一个据点」这条裁定在这里表现为一次直接返回。
        return Vec::new();
    };
    let layout = *world.terrain.layout();

    // 先把「附近有哪些据点」收齐再动世界。
    //
    // 「附近」的口径取**当前常驻的区块集合**，不是「以玩家为中心画一个
    // 方框」：两者在正常游玩时几乎重合（常驻集合就是流式加载按玩家位置
    // 维护出来的），但常驻集合额外满足一条本函数真正需要的性质——
    // [`place_roster`] 只在常驻地形上摆人，非常驻区块里的据点就算被算作
    // 「附近」也一个人都摆不出来，却会被记成「已物化」而**永久错过**。
    // 按常驻集合取，这种情形从源头上不会发生。
    //
    // 一座跨区块的据点会被它覆盖到的每个常驻区块各报一次，按 id 去重
    // （排序 + dedup，不碰任何 HashSet，约束 C5）。
    let mut nearby: Vec<ll_world::settlement::SettlementSite> = Vec::new();
    for zone in world.terrain.resident_zones() {
        for site in chronicle.sites_touching_zone(zone) {
            nearby.push(*site);
        }
    }
    nearby.sort_by_key(|site| site.id.get());
    nearby.dedup_by_key(|site| site.id.get());

    let mut spawned = Vec::new();
    for site in nearby {
        if world.settlement_is_materialized(site.id) {
            continue;
        }
        let roster = ll_mod::roster::settlement_roster(&site, roles, world.seed);
        if roster.is_empty() {
            // 废墟与空据点：名册本来就是空的，**照样记成已物化**——否则
            // 每一帧都会为同一片废墟重跑一次名册派生。
            world.mark_settlement_materialized(site.id);
            continue;
        }
        let spots = place_roster(world, &layout, site.anchor, roster.len());
        let ctx = ll_mod::roster::MaterializeContext {
            races: &content.race_table,
            items: &content.item_table,
            surface_profile: content.space_ids.surface,
            now: world.clock,
            // 据点的文化直接转发给物化——`Agent::affiliations` 的第一个
            // 生产者，见 `ll_mod::roster::build_npc_agent` 文档「文化
            // 归属」一节。
            culture: site.culture,
            // 人口直接转发给物化——NPC 初始钱包的两个因子之一（所有者
            // 裁定第 4 条），见 `ll_mod::npc_wallet` 模块文档。
            population: site.population,
        };
        let mut made = Vec::new();
        for (profile, pos) in roster.iter().zip(spots) {
            let zone = layout.tile_to_zone(pos).0;
            made.push(ll_mod::roster::build_npc_agent(
                profile, pos, zone, roles, &ctx,
            ));
        }
        for agent in made {
            spawned.push(world.actors.spawn(agent));
        }
        // 人先摆、家具后摆——顺序是这两行之间唯一重要的事，理由见本模块
        // 文档「与『每格至多站一人』那条不变式的相互作用」。
        let placed = furnish_settlement(world, content, &site, &layout);
        world.mark_settlement_materialized(site.id);
        tracing::info!(
            site = site.id.get(),
            population = site.population,
            roster = roster.len(),
            spawned = spawned.len(),
            furniture = placed,
            "据点 NPC 物化完成"
        );
    }
    spawned
}

/// 为一份名册找 `wanted` 个可站立、互不重叠的位置。
///
/// 从锚点开始按方环由内向外扫描（半径 0、1、2……直到
/// [`NPC_PLACEMENT_RADIUS`]），每一环内按行主序——完全确定，不取随机数。
///
/// 只认**已常驻**的地形（[`ll_world::surface_store::SurfaceStore::terrain_at_resident`]）：
/// 落在尚未加载区块里的那些格一律跳过，绝不为了摆一个 NPC 去触发一次
/// 区块生成（那会让物化路径悄悄把整座跨界据点的邻区块都拉进内存）。
/// 找不到足够多的位置就返回少于 `wanted` 个，调用方按 `zip` 自然截断
/// ——多出来的那几个人这一局就不出现，见
/// [`materialize_nearby_settlements`] 文档「代价如实标注」一节。
///
/// 已被其他实体占着的格子也跳过：本仓库的移动结算不允许两个实体站在
/// 同一格，物化时就摞在一起会让他们互相卡死。
fn place_roster(
    world: &WorldState,
    layout: &ZoneLayout,
    anchor: TorusPos,
    wanted: usize,
) -> Vec<TorusPos> {
    let tile_size = layout.tile_size();
    let occupied: Vec<TorusPos> = world.actors.iter().map(|agent| agent.pos).collect();
    let mut spots = Vec::with_capacity(wanted);
    for ring in 0..=NPC_PLACEMENT_RADIUS {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if spots.len() >= wanted {
                    return spots;
                }
                // 只取这一环最外圈的格子，内圈上一轮已经看过了。
                if ring > 0 && dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let pos = tile_size.wrap(anchor.x() + dx, anchor.y() + dy);
                let Some(kind) = world.terrain.terrain_at_resident(pos) else {
                    continue;
                };
                if kind.blocks_move(&world.terrain_table) {
                    continue;
                }
                if occupied.contains(&pos) || spots.contains(&pos) {
                    continue;
                }
                // 这一格立着家具就跳过（据点建筑类型批次）。
                //
                // 放置物**不阻挡通行**，因此不摆这一条并不会造成任何
                // 崩溃或卡死——它是一条**观感**判据：一个 NPC 一出生
                // 就站在自家的床上／锻炉里，看起来像是穿模。
                //
                // 咬得到的只有**上一座**据点摆下的家具（本座的家具在
                // 本函数之后才摆，见 `furnish_settlement`），以及玩家
                // 自己放下的东西。
                if world.placed_at(pos).is_some() {
                    continue;
                }
                spots.push(pos);
            }
        }
    }
    spots
}

/// 把这座据点的家具摆进世界，返回真的摆下去了几件。
///
/// 「摆什么、摆在哪」全部由 [`settlement_furnishing`] 算出——那是一个
/// 纯函数，读的是这座据点信的那份文化声明的建筑类型
/// （[`ll_world::culture::CultureAttrs::buildings`]）。本函数只做三件
/// 这一层才做得了的事：**筛掉摆不下的格**、**构造带主人的物品堆**、
/// **写进 [`WorldState::ground_items`]**。
///
/// # 三道筛子，每一道都有自己的理由
///
/// 1. **这一格必须是已经铺好的木地板**（`ids.floor_wood`）。这一条同时
///    挡掉四种情形，不需要各写一条判据：区块还没常驻（查不到地形）、
///    这块地没盖成房子（[`ll_world::settlement`] 的 `plot_is_clear` 判
///    它是水面或山体）、这是废墟（废墟的地板是 `floor_stone`）、以及
///    玩家已经把这一格改掉了。
/// 2. **这一格不能有人站着**。见本模块文档第三节：这是观感判据，不是
///    安全判据。
/// 3. **这一格不能已经立着别的东西**。放置物独占一格
///    （[`ll_world::item::GroundItemStack::placed`]），同一格两件放置物
///    会让 `WorldState::placed_at` 的「取第一条」变成一个说不清的选择。
///
/// # 归属：`Owner::Faction(据点 id)`，**构造那一刻就带上**
///
/// [`ll_world::ownership::Owner`] 的 `Faction` 变体文档里已经写好了这条
/// 裁定（「『据点归属』用的就是本变体……表示法是
/// `Owner::Faction(SettlementSite::id)`」，三条理由在那里）。本函数是那
/// 条裁定落地之后的**第一个构造点**。
///
/// **归属写在构造 [`ItemStack`] 的那一处，不是事后回填**：几百座据点、
/// 每座几十件家具，回填等于一次全量迁移，而且它会留下一个「已经写进
/// 世界、还没安上主人」的中间状态——`WorldState::hash` 会把那个中间
/// 状态也算进去。
///
/// **为什么不用 `FactionTable::faction_of(site.id)` 换成势力号**：一个
/// 势力下属多座据点，「这是某某势力的东西」比「这是这座据点的东西」
/// **更宽**；而 `ownership.rs` 已经写明将来的收窄方向是
/// `Faction(据点) → Npc(住这儿的那个人)`，用势力号会让那条收窄多绕
/// 一层。换算随时可做，反过来不行。
fn furnish_settlement(
    world: &mut WorldState,
    content: &LoadedContent,
    site: &ll_world::settlement::SettlementSite,
    layout: &ZoneLayout,
) -> usize {
    if site.status != SettlementStatus::Inhabited {
        return 0;
    }
    let Some(chronicle) = world.terrain.chronicle_handle() else {
        return 0;
    };
    let plan = settlement_furnishing(
        site,
        chronicle.culture_table(),
        world.seed,
        layout.tile_size(),
    );
    if plan.is_empty() {
        return 0;
    }

    let floor = content.terrain_ids.floor_wood;
    let owner = Owner::Faction(site.id);
    let now = world.clock;
    let mut placed = 0usize;
    for slot in plan {
        if world.terrain.terrain_at_resident(slot.pos) != Some(floor) {
            continue;
        }
        if world.actors.iter().any(|agent| agent.pos == slot.pos) {
            continue;
        }
        if world.placed_at(slot.pos).is_some() {
            continue;
        }
        // 内容写错了 id、或者写了一件不是家具的东西：跳过并留一条日志。
        // **不 panic**（规格 §10.2「降级而非崩溃」），也不静默——真正
        // 当场点名的那道门在装载期（`ll_mod::content_audit` 的跨表引用
        // 检查），这里是运行期的最后一道网。
        let is_furniture = content
            .item_table
            .get(slot.item)
            .is_some_and(|view| view.furniture);
        if !is_furniture {
            tracing::warn!(
                site = site.id.get(),
                item = slot.item.get(),
                "据点家具声明指向的物品不存在或不是家具，跳过"
            );
            continue;
        }
        world.ground_items.push(GroundItemStack {
            pos: slot.pos,
            stack: ItemStack {
                owner,
                ..ItemStack::new(slot.item, 1)
            },
            dropped_at: now,
            contents: Vec::new(),
            placed: true,
        });
        placed += 1;
    }
    placed
}

/// 出生点落进谁家屋里的时候，把它挪到最近的一块**屋外**空地。
///
/// # 这修的是一个**先于本批次就存在**的缺陷，本批次只是把它踩了出来
///
/// [`crate::world`] 的 `find_spawn_site` 读的是**基础地形**（噪声算出
/// 来的那一层，据点还没铺上去），它保证的是「这一片连通陆地足够大」。
/// 但据点是**之后**才盖上去的：那片大陆地上可能正好立着一座村子，而
/// 光栅序最先那一格恰好落在某栋屋子的 3×3 内壁里。玩家于是在一间关着
/// 门的屋子里开局——门推得开，所以不是死局，但「连通可行走面积」当场
/// 从几千掉到 9。
///
/// 这条路径在街道落地之前就走得到，只是本体默认种子没踩上。街道把
/// 建筑摊开之后，`crates/ll-game/tests/worldgen_params_e2e.rs` 的
/// `四档预设都能建出带玩家实体且出生点连得开的世界` 当场红了
/// （「预设 continent 的出生点周围只有 9 格连通可行走地面」）——**那条
/// 断言抓到的是一个真缺陷，不是本批次引入的回归**。
///
/// # 判据：屋外、站得住、离得最近
///
/// 从出生点按方环由内向外扫（每环内 `(dy, dx)` 行主序，完全确定、
/// 不取随机数，约束 C3/C5），取第一块同时满足三条的格：
///
/// 1. 地形已常驻且不阻挡通行；
/// 2. 不落在任何一栋建筑的外廓里；
/// 3. 在 [`SPAWN_ESCAPE_RADIUS`] 之内。
///
/// 找不到就**原样返回**（规格 §10.2「降级而非崩溃」）：出生点仍在屋
/// 里，玩家推开门就出来了，比 panic 或者把玩家扔到地图另一头强。
pub fn spawn_outside_buildings(
    world: &WorldState,
    layout: &ZoneLayout,
    spawn: TorusPos,
) -> TorusPos {
    let Some(chronicle) = world.terrain.chronicle_handle() else {
        return spawn;
    };
    let tile_size = layout.tile_size();

    // 收齐「可能盖到搜索范围里」的那些建筑外廓格。
    //
    // 取出生点那一格所在区块**及其八邻**的据点：一栋跨区块的建筑在它
    // 覆盖到的每个区块里都会被 `sites_touching_zone` 报出来，而搜索半径
    // （10 格）远小于区块边长，八邻已经足够。
    let (spawn_zone, _) = layout.tile_to_zone(spawn);
    let zone_count = layout.zone_count();
    let mut sites: Vec<ll_world::settlement::SettlementSite> = Vec::new();
    for dy in -1..=1 {
        for dx in -1..=1 {
            let zone = zone_count.wrap(spawn_zone.x() + dx, spawn_zone.y() + dy);
            for site in chronicle.sites_touching_zone(zone) {
                sites.push(*site);
            }
        }
    }
    sites.sort_by_key(|site| site.id.get());
    sites.dedup_by_key(|site| site.id.get());

    let mut footprint: Vec<(i32, i32)> = Vec::new();
    for site in &sites {
        for building in 0..site.building_count.min(ll_world::settlement::MAX_BUILDINGS) {
            let (left, top) = ll_world::settlement::building_origin(site, building);
            for dy in 0..ll_world::settlement::BUILDING_SPAN {
                for dx in 0..ll_world::settlement::BUILDING_SPAN {
                    let pos = tile_size.wrap(left + dx, top + dy);
                    footprint.push((pos.y(), pos.x()));
                }
            }
        }
    }
    footprint.sort_unstable();
    footprint.dedup();
    let covered = |pos: TorusPos| footprint.binary_search(&(pos.y(), pos.x())).is_ok();

    if !covered(spawn) {
        return spawn;
    }

    for ring in 1..=SPAWN_ESCAPE_RADIUS {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let pos = tile_size.wrap(spawn.x() + dx, spawn.y() + dy);
                let Some(kind) = world.terrain.terrain_at_resident(pos) else {
                    continue;
                };
                if kind.blocks_move(&world.terrain_table) || covered(pos) {
                    continue;
                }
                tracing::info!(
                    from_x = spawn.x(),
                    from_y = spawn.y(),
                    to_x = pos.x(),
                    to_y = pos.y(),
                    "出生点落在某栋建筑里，已挪到最近的屋外空地"
                );
                return pos;
            }
        }
    }
    tracing::warn!(
        spawn_x = spawn.x(),
        spawn_y = spawn.y(),
        radius = SPAWN_ESCAPE_RADIUS,
        "出生点落在建筑里，但半径内找不到屋外空地，原样保留"
    );
    spawn
}

/// [`spawn_outside_buildings`] 往外找空地的最大半径（格）。
///
/// 取 `2 × BUILDING_SPAN`（10）：一栋屋子的外廓是 5 格，最坏情形是出生
/// 点在正中、外面还紧挨着另一栋屋子，两栋加上中间的巷子仍在 10 格之内。
/// 再往外就不该叫「挪一格」了——那等于替玩家换了个出生地。
const SPAWN_ESCAPE_RADIUS: i32 = 2 * ll_world::settlement::BUILDING_SPAN;
