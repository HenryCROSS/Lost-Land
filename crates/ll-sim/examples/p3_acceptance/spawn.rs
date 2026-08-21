//! 世界与战斗单位的出生逻辑：找可站立的格子、以及玩家与三个敌人各自
//! 的属性/外观/初始位置。
//!
//! 与 `ll-world` 的 `p2_acceptance::spawn` 同样的拆分理由：这里的函数
//! 会改动 [`ChunkGrid`]/[`WorldState`] 内容或产出新实体，与
//! `crate::layout` 里「给定数据现算现出」的纯呈现函数分开一个文件。

use ll_core::ident::{Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_sim::timeline::Timeline;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::naming::NamingRules;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

use crate::layout::{WORLD_HEIGHT, WORLD_WIDTH};

/// 区块边长（格）：取世界边长本身，demo 世界因此正好是单个区块——
/// 128 满足全部对齐（16/32 的整数倍）与视口跨度约束，见
/// `ll_world::zone::ZoneLayout::new` 文档。单区块布局下
/// `WorldState::new` 自带的出生点邻域预热（半径 2，环绕回同一个区块）
/// 已经让整个 demo 世界从构造起就常驻，不需要额外调用 `warm_all`。
const ZONE_SPAN: u32 = WORLD_WIDTH;

fn build_zone_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(ZONE_SPAN, zone_count).expect("ZONE_SPAN 满足全部对齐与跨度约束")
}

/// 出生点搜索的最大环半径（格）。
///
/// 取 64——[`WORLD_WIDTH`]/[`WORLD_HEIGHT`]（128）的一半：环面世界里
/// 任意两点的切比雪夫距离不会超过世界较小维度的一半，取这个值就保证
/// 「除非整张地图没有一格可站立，否则搜索恒能找到」。这不是随手抄一个
/// 够大的数——`ll-world` 的 `p2_acceptance::spawn::SPAWN_SEARCH_MAX_RADIUS`
/// 在那个 512×320 的更大世界上取 64 就够用，是因为出生点从世界中心
/// 出发，附近大概率有陆地；本 demo 的玩家出生点刻意贴着北边界
/// （见 [`PLAYER_SPAWN_TARGET_Y`]），实测某些种子下这一带连着一整片
/// 跨过接缝的大水域，半径 32 会真的搜不到、退回一个不可站立的目标点
/// ——`spawn::tests::出生点搜索结果可以站立` 曾经因此测出过失败。
const SEARCH_MAX_RADIUS: i32 = (if WORLD_WIDTH < WORLD_HEIGHT {
    WORLD_WIDTH
} else {
    WORLD_HEIGHT
} / 2) as i32;

/// 玩家出生点相对世界中心横坐标的目标纵坐标：紧贴北边界（`y = 1`）。
///
/// 刻意不取世界中心，而是贴着北边界——这是「跨南北接缝时遮挡关系正确」
/// 这条验收点（Task 1 的验收）的落地方式：[`ENEMY_SEAM_OFFSET`] 把一个
/// 敌人摆在玩家北面几格，从世界坐标看会绕过 `y = 0`/`y = WORLD_HEIGHT - 1`
/// 这条接缝，但相机（`ll_render::camera::Camera::world_to_screen`）与
/// `DrawOrder` 全程只认屏幕坐标，接缝对它们而言不存在——真人观察时
/// 玩家与这个敌人应表现得像普通相邻两格一样正确前后遮挡，而不是接缝
/// 北侧的单位被南侧错误遮挡。
const PLAYER_SPAWN_TARGET_Y: i32 = 1;

/// 「坦克」敌人相对玩家出生点的偏移：向东南几格，敏捷最低、生命最厚、
/// 占地 2×2（`boss_idle_0`）——同时验证「footprint 从图集条目读取」
/// 这条验收点：它是本 demo 里唯一非 1×1 占地的实体。
const ENEMY_TANK_OFFSET: (i32, i32) = (6, 3);

/// 「中速」敌人相对玩家出生点的偏移：向西几格，敏捷取基准值，作为
/// 「敏捷不同」这条验收点的中间对照组。
const ENEMY_MEDIUM_OFFSET: (i32, i32) = (-5, 2);

/// 「快速」敌人相对玩家出生点的偏移：向北 5 格——玩家出生点纵坐标只有
/// [`PLAYER_SPAWN_TARGET_Y`]（1），向北 5 格会越过 `y = 0` 绕到世界南端
/// （`y = WORLD_HEIGHT - 4`），这正是 [`PLAYER_SPAWN_TARGET_Y`] 文档提到
/// 的跨接缝布局；同时它的敏捷全场最高，是「快角色在慢角色一次行动
/// 窗口内行动多次」这条核心验收点的主角。
const ENEMY_SEAM_OFFSET: (i32, i32) = (0, -5);

/// 一个可战斗单位在渲染层需要的额外信息：图集条目名与颜色调制。
///
/// 不放进 [`Agent`] 本身——`Agent` 是世界状态的一部分（P5 冻结存档
/// 格式时要序列化），而「用哪个图集条目画」「染什么颜色」纯粹是本
/// demo 的呈现选择，与真正的内容系统（图集条目名应该来自
/// `profession`/`race` 指向的注册表定义，而非硬编码）无关——本批次
/// 还没有内容注册表到图集条目名的映射机制，用一张并行表占位，好过
/// 借用 `Agent` 的字段表达一个它不该表达的意思。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Combatant {
    pub(crate) id: EntityId,
    pub(crate) sprite: &'static str,
    pub(crate) tint: [f32; 4],
}

/// 出生完毕的整套战斗单位：玩家与敌人分开存放，因为不少调用方
/// （AI、渲染的名字牌）需要单独知道「谁是玩家」。
pub(crate) struct SpawnedActors {
    pub(crate) player: Combatant,
    pub(crate) enemies: Vec<Combatant>,
}

impl SpawnedActors {
    /// 全部单位（玩家 + 敌人）的只读视图，供渲染与「移动目的地是否
    /// 站着别的实体」这类需要遍历全体单位的场景使用。
    ///
    /// 用 [`Vec`] 现算现出而非缓存：单位数量恒为 4（P3 demo 的规模），
    /// 这里的拷贝成本可以忽略；缓存反而要操心与 `enemies` 增删同步。
    pub(crate) fn all(&self) -> Vec<Combatant> {
        let mut all = vec![self.player];
        all.extend(self.enemies.iter().copied());
        all
    }
}

/// demo 用的命名规则：音素表只用 [`crate::font::CHARSET`] 覆盖的大写
/// 拉丁字母拼接，这样任何生成出的名字都能被本 demo 的极简像素字体
/// （[`crate::font`]）完整画出来，不会出现某个字符查不到字形而被
/// 静默跳过的情况。
pub(crate) fn demo_naming_rules() -> NamingRules {
    let letters = |raw: &[&str]| raw.iter().map(|s| s.to_string()).collect();
    NamingRules {
        onsets: letters(&["K", "T", "R", "N", "M", "L", "S", "G", "D", "B", "V", "Z"]),
        nuclei: letters(&["A", "E", "I", "O", "U"]),
        codas: letters(&["", "N", "R", "S", "M", "L"]),
        syllables: (2, 3),
        surname_first: false,
    }
}

/// 建立演示世界：真实生成的环面地形，时钟拨到正午。
///
/// 地形定义用 [`base_terrain_fixture`] 现造——demo 不牵扯真实的 mod
/// 加载流程，见其文档。返回值附带 [`BaseTerrainIds`]：调用方（渲染层
/// 需要按具体地形种类挑图集条目）不能自己再另起一次
/// `base_terrain_fixture`——那会是一个不同的 `Interner`，索引虽因固定
/// 注册顺序而恰好数值相同，仍不应该在类型层面制造这种隐晦的耦合。
pub(crate) fn build_world() -> (WorldState, BaseTerrainIds) {
    let layout = build_zone_layout();
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("演示世界布局满足生成入口的全部约束");
    world.advance(crate::layout::INITIAL_CLOCK_TICKS);
    (world, terrain_ids)
}

/// 该地形是否可站立（不阻挡移动）。
///
/// # 为什么接受 `&WorldState`（两级坐标系重写，任务 11）
///
/// 见 `ll-world` 的 `p2_acceptance::spawn::is_spawnable` 文档同一节：
/// `terrain` 换成 `SurfaceStore` 之后不再有单一「一张网格」可传，本
/// demo 世界是单个区块（[`ZONE_SPAN`] = 世界边长），`WorldState::new`
/// 自带的出生点邻域预热已经让它整体常驻，`.expect(..)` 因此总能成立。
fn is_walkable(world: &WorldState, pos: TorusPos) -> bool {
    let kind = world
        .terrain_at(pos)
        .expect("demo 世界是单区块布局，WorldState::new 的出生点邻域预热已让它整体常驻");
    !kind.blocks_move(&world.terrain_table)
}

/// 从 `target` 开始按环逐圈向外搜索一格可站立的地形——与
/// `p2_acceptance::spawn::find_spawn` 同一算法，只是把「必须从世界中心
/// 出发」泛化成「从任意目标点出发」，供玩家与三个敌人共用同一套出生点
/// 搜索逻辑。
fn find_walkable_near(world: &WorldState, target: TorusPos) -> TorusPos {
    let size = world.size;
    if is_walkable(world, target) {
        return target;
    }
    for radius in 1..=SEARCH_MAX_RADIUS {
        if let Some(pos) = search_ring(world, size, target, radius) {
            return pos;
        }
    }
    target
}

/// 在距 `center` 切比雪夫距离恰为 `radius` 的环上寻找第一个可站立格。
fn search_ring(
    world: &WorldState,
    size: TorusSize,
    center: TorusPos,
    radius: i32,
) -> Option<TorusPos> {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs().max(dy.abs()) != radius {
                continue;
            }
            let pos = size.wrap(center.x() + dx, center.y() + dy);
            if is_walkable(world, pos) {
                return Some(pos);
            }
        }
    }
    None
}

/// 造一个占位内容标识——demo 不需要真实的职业/种族内容定义，只需要
/// `Agent::profession`/`Agent::race` 两个字段有合法值可填。
fn intern(interner: &mut Interner, raw: &str) -> ll_core::ident::ContentIndex {
    interner.intern(NamespacedId::parse(raw).expect("demo 内置标识符恒合法"))
}

/// 造一个战斗单位：写入 `world.actors`，并把它的初次行动排入
/// `timeline`。
#[allow(clippy::too_many_arguments)]
fn spawn_combatant(
    world: &mut WorldState,
    timeline: &mut Timeline,
    interner: &mut Interner,
    pos: TorusPos,
    dexterity: i32,
    strength: i32,
    health: i32,
    sprite: &'static str,
    tint: [f32; 4],
) -> Combatant {
    let profession = intern(interner, "lostland:wanderer");
    let race = intern(interner, "lostland:human");
    // demo 世界没有任何 Interior，全部战斗单位恒在地表——层属性索引
    // 用占位值即可，本 demo 不消费 Space::profile（P3 的重点是战斗结算
    // 与时间轴，不是任务 12 才接线的进出 Interior）。
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    let id = world.actors.spawn(Agent {
        pos,
        stats: BaseStats {
            dexterity,
            strength,
            ..BaseStats::BASELINE
        },
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: ll_world::space::Space::surface(
            zone,
            ll_core::ident::ContentIndex::default(),
        ),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
    });
    timeline.schedule(id, Tick(0));
    Combatant { id, sprite, tint }
}

/// 生成玩家与三个敌人，全部排入 `timeline`。
///
/// 三个敌人的敏捷刻意拉开梯度（5/10/30，[`BaseStats::BASELINE`] 的
/// 敏捷是 10）——这是「至少三个敌人，各有不同敏捷」这条验收点的具体
/// 落地；配合 `ll_sim::resolve` 已把行动耗时接上敏捷字段（见
/// `crates/ll-sim/src/resolve.rs` 的 `effective_speed_from_dexterity`），
/// 三者在同一段时间窗口内出手的频率会真实不同，而不只是数值上不同。
pub(crate) fn spawn_actors(world: &mut WorldState, timeline: &mut Timeline) -> SpawnedActors {
    let mut interner = Interner::new();

    let player_pos = find_walkable_near(
        world,
        world
            .size
            .wrap(world.size.width() as i32 / 2, PLAYER_SPAWN_TARGET_Y),
    );
    let player = spawn_combatant(
        world,
        timeline,
        &mut interner,
        player_pos,
        12,
        14,
        Agent::STARTING_HEALTH,
        "hero_idle_0",
        [1.0, 1.0, 1.0, 1.0],
    );

    let tank_pos = find_walkable_near(
        world,
        world.size.wrap(
            player_pos.x() + ENEMY_TANK_OFFSET.0,
            player_pos.y() + ENEMY_TANK_OFFSET.1,
        ),
    );
    let tank = spawn_combatant(
        world,
        timeline,
        &mut interner,
        tank_pos,
        5,
        16,
        220,
        "boss_idle_0",
        [1.0, 0.75, 0.75, 1.0],
    );

    let medium_pos = find_walkable_near(
        world,
        world.size.wrap(
            player_pos.x() + ENEMY_MEDIUM_OFFSET.0,
            player_pos.y() + ENEMY_MEDIUM_OFFSET.1,
        ),
    );
    let medium = spawn_combatant(
        world,
        timeline,
        &mut interner,
        medium_pos,
        10,
        9,
        100,
        "hero_walk_0",
        [1.0, 0.75, 0.35, 1.0],
    );

    let fast_pos = find_walkable_near(
        world,
        world.size.wrap(
            player_pos.x() + ENEMY_SEAM_OFFSET.0,
            player_pos.y() + ENEMY_SEAM_OFFSET.1,
        ),
    );
    let fast = spawn_combatant(
        world,
        timeline,
        &mut interner,
        fast_pos,
        30,
        6,
        55,
        "hero_walk_1",
        [0.4, 0.85, 1.0, 1.0],
    );

    SpawnedActors {
        player,
        enemies: vec![tank, medium, fast],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 出生点搜索结果可以站立() {
        // Arrange
        let (world, _terrain_ids) = build_world();
        let size = world.size;
        let target = size.wrap(size.width() as i32 / 2, PLAYER_SPAWN_TARGET_Y);

        // Act
        let pos = find_walkable_near(&world, target);

        // Assert
        assert!(is_walkable(&world, pos));
    }

    #[test]
    fn 世界几乎全是深水时出生点搜索仍会终止() {
        // Arrange：把整个（单区块）世界覆写成深水（阻挡移动）。
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let layout = build_zone_layout();
        let spawn = layout.tile_size().wrap(0, 0);
        let mut world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("demo 世界布局满足全部构造前置条件");
        let size = world.size;
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                world
                    .terrain
                    .set_terrain(size.wrap(x, y), terrain_ids.deep_water);
            }
        }

        // Act & Assert：函数确实返回了（没有死循环/panic），且坐标合法
        // （产出越界坐标会让 terrain_at 返回 None 而不是 panic）。
        let pos = find_walkable_near(&world, size.wrap(0, 0));
        assert!(world.terrain_at(pos).is_some());
    }

    #[test]
    fn 生成的四个单位敏捷各不相同() {
        // 「至少三个敌人，各有不同敏捷」这条验收点的直接回归。
        // Arrange
        let (mut world, _terrain_ids) = build_world();
        let mut timeline = Timeline::new();

        // Act
        let actors = spawn_actors(&mut world, &mut timeline);

        // Assert
        let dexterities: Vec<i32> = actors
            .all()
            .iter()
            .map(|combatant| {
                world
                    .actors
                    .get(combatant.id)
                    .expect("刚生成的实体必然存在")
                    .stats
                    .dexterity
            })
            .collect();
        let mut unique = dexterities.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), dexterities.len(), "四个单位的敏捷应两两不同");
    }

    #[test]
    fn 坦克敌人的占地并非一比一() {
        // 「footprint 从图集条目读取，不要硬编码 1×1」这条验收点要求
        // demo 里确实存在一个非 1×1 占地的实体；这里只锁住它用的图集
        // 条目名——具体的 footprint 数值来自图集元数据，不在本文件
        // 断言范围内（那是 `ll_render::atlas` 的职责）。
        // Arrange
        let (mut world, _terrain_ids) = build_world();
        let mut timeline = Timeline::new();

        // Act
        let actors = spawn_actors(&mut world, &mut timeline);

        // Assert
        assert_eq!(actors.enemies[0].sprite, "boss_idle_0");
    }

    #[test]
    fn 全部单位的初次行动均已排入时间轴() {
        // Arrange
        let (mut world, _terrain_ids) = build_world();
        let mut timeline = Timeline::new();

        // Act
        let actors = spawn_actors(&mut world, &mut timeline);

        // Assert
        assert_eq!(timeline.len(), actors.all().len());
    }
}
