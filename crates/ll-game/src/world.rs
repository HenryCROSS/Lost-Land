//! 新游戏的世界搭建：区块布局、噪声、出生点、玩家实体。
//!
//! 与 `ll-sim` 的 `p5_coordinate_acceptance::world` 同一套思路（先建
//! 布局与噪声、再强制铺一小片确定可站立的出生地，理由见其模块文档
//! 「验收 demo 需要确定性场景」），区别是这里的地形/种族索引来自
//! [`crate::content::LoadedContent`] 的真实装载结果，不是测试用的
//! `*_fixture` 便捷函数——本体二进制走的是与 mod 完全相同的注册通道。

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::{GenParams, build_zone_noise};
use ll_world::noise::TileableNoise;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::zone::ZoneLayout;
use ll_world::{WorldError, terrain::TerrainTable};

use crate::content::LoadedContent;

/// 区块边长（格）：固定 48，与 `ll_content::world_identity` 推荐预设表
/// 一致（该模块文档：区块边长固定 48）。
const ZONE_SPAN: u32 = 48;
/// 世界区块数：取推荐预设表「小陆地」一档——本体二进制目前没有开局
/// 选择尺寸的界面（P7 才有，见规格 §15），固定用最小的一档,保持新游戏
/// 启动与流式加载都足够快。
const ZONE_COUNT: (u32, u32) = (64, 48);

/// 出生点周围强制铺成草地的半径（格）——不依赖噪声生成恰好在
/// `(0, 0)` 给出可站立地形，见模块文档。半径 3 覆盖玩家开局能立刻
/// 看到、走到的一小片范围，不是整张地图。
const SPAWN_CLEARING_RADIUS: i32 = 3;

/// 流式邻域维护半径（区块数）——与 `p5_coordinate_acceptance` 同一
/// 取值，见其 `layout::STREAM_RADIUS_ZONES` 文档。
pub const STREAM_RADIUS_ZONES: i32 = 2;

/// 建立本体默认使用的区块布局。
pub fn build_zone_layout() -> Result<ZoneLayout, WorldError> {
    let zone_count = ll_core::torus::TorusSize::new(ZONE_COUNT.0, ZONE_COUNT.1).ok_or(
        WorldError::WorldTooSmall {
            width: ZONE_COUNT.0,
            height: ZONE_COUNT.1,
        },
    )?;
    ZoneLayout::new(ZONE_SPAN, zone_count)
}

/// 新游戏的完整世界：世界状态、噪声源与生成参数（流式加载持续需要）、
/// 玩家实体 id。
pub struct GameWorld {
    /// 世界状态本身——存档写出/读入的对象。
    pub world: WorldState,
    /// 地形噪声源，流式加载持续需要。
    pub noise: TileableNoise,
    /// 地形生成参数，流式加载持续需要。
    pub params: GenParams,
    /// 玩家实体 id。
    pub player: EntityId,
}

/// 建立一局新游戏：区块布局 → 噪声 → 世界状态 → 出生点铺地 → 玩家
/// 实体。`seed` 决定地形具体分布——不同种子产出不同世界,同一种子加
/// 同一批内容必然复现同一个世界（世界身份三要素之一,见
/// `ll_content::world_identity` 模块文档）。
pub fn build_new_world(content: &LoadedContent, seed: u64) -> Result<GameWorld, WorldError> {
    let layout = build_zone_layout()?;
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    let noise = build_zone_noise(&layout, &params)?;

    let spawn = layout.tile_size().wrap(0, 0);
    // TerrainTable 不是 Copy——WorldState::new 需要拿走它的所有权,
    // 出生点铺地之后本函数还要用 &content.terrain_ids（不需要表本身），
    // 故这里克隆一份而不是尝试在 new 之后再找回同一张表。地形属性表
    // 本身很小（每种地形一条定长记录），克隆开销可忽略。
    let mut world = WorldState::new(
        layout,
        &params,
        &content.terrain_ids,
        content.terrain_table.clone(),
        spawn,
    )?;
    world.surface_profile = content.space_ids.surface;

    carve_spawn_clearing(&mut world, &noise, &params, content, spawn);

    let (zone, _) = layout.tile_to_zone(spawn);
    let player = spawn_player(&mut world, spawn, zone, content);
    // 必须显式赋值——见 `ll_world::state::WorldState::player_entity`
    // 字段文档「调用方应在 spawn 产出玩家的 EntityId 之后显式赋值」
    // 一节：漏掉这一步，探索记忆的写入路径会永远收不到任何写入（同一
    // 缺陷 P5-A 阶段在 `p5_coordinate_acceptance` 里出现过一次，本体
    // 二进制不重演）。
    world.player_entity = Some(player);

    Ok(GameWorld {
        world,
        noise,
        params,
        player,
    })
}

/// 出生点周围 [`SPAWN_CLEARING_RADIUS`] 格内强制改写成草地：先用
/// `terrain_at` 触发按需生成（半径外沿可能落在出生邻域预热半径覆盖
/// 不到的区块），再 `set_terrain` 覆写——这正是流式加载的正常使用
/// 方式,不是绕过它,见 `p5_coordinate_acceptance::world` 模块文档同一
/// 段说明。
fn carve_spawn_clearing(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    content: &LoadedContent,
    spawn: TorusPos,
) {
    let size = world.terrain.layout().tile_size();
    for dy in -SPAWN_CLEARING_RADIUS..=SPAWN_CLEARING_RADIUS {
        for dx in -SPAWN_CLEARING_RADIUS..=SPAWN_CLEARING_RADIUS {
            let pos = size.wrap(spawn.x() + dx, spawn.y() + dy);
            world
                .terrain
                .terrain_at(noise, params, &content.terrain_ids, pos, Tick(0));
            world.terrain.set_terrain(pos, content.terrain_ids.grass);
        }
    }
}

/// 生成玩家单位，写入 `world.actors`，`current_space` 取地表。
fn spawn_player(
    world: &mut WorldState,
    pos: TorusPos,
    zone: ll_world::space::ZoneCoord,
    content: &LoadedContent,
) -> EntityId {
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        // 本体目前没有注册任何职业内容（职业只经 mod 脚本
        // `register-class` 注册，见 `ll_mod::class` 模块文档）——占位索引
        // 是诚实的「尚无职业」表达，不是缺陷。
        profession: ll_core::ident::ContentIndex::default(),
        goals: Vec::new(),
        race: content.race_ids.human,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, content.space_ids.surface),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
    })
}

/// 供渲染/存档使用的一张干净地形表克隆——存档读入（`load_full`）需要
/// 「当前会话按同一次装载重新注册出的表」，与写出时的表逐字段相同但
/// 不是同一个实例（读档之后 `WorldState` 是另一份反序列化出的世界，
/// 见 `ll_content::save_file::load_full` 文档）。
pub fn cloned_terrain_table(content: &LoadedContent) -> TerrainTable {
    content.terrain_table.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_content() -> LoadedContent {
        let dir =
            std::env::temp_dir().join(format!("ll-game-world-test-content-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        let content = crate::content::load_content(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    #[test]
    fn 出生点在世界建成后立刻可站立() {
        // Arrange & Act
        let content = test_content();
        let game_world = build_new_world(&content, 1).expect("测试用布局满足全部前置条件");

        // Assert
        let pos = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成，必然存在")
            .pos;
        let kind = game_world
            .world
            .terrain_at(pos)
            .expect("出生点所在区块已被出生邻域预热");
        assert!(!kind.blocks_move(&game_world.world.terrain_table));
    }

    #[test]
    fn player_entity字段被显式赋值而非保持默认空值() {
        // 直接对应模块文档强调的那处已知易漏坑：漏赋值会让探索记忆
        // 永远收不到写入,见 build_new_world 文档。
        // Arrange & Act
        let content = test_content();
        let game_world = build_new_world(&content, 1).expect("测试用布局满足全部前置条件");

        // Assert
        assert_eq!(game_world.world.player_entity, Some(game_world.player));
    }

    #[test]
    fn 相同种子产出的两个世界出生点地形一致() {
        // 世界身份三要素之一（种子）在「世界创建」这一步的直接体现：
        // 同一种子、同一批内容,出生点地形必须逐位一致。
        // Arrange
        let content = test_content();

        // Act
        let first = build_new_world(&content, 42).expect("测试用布局满足全部前置条件");
        let second = build_new_world(&content, 42).expect("测试用布局满足全部前置条件");

        // Assert
        assert_eq!(first.world.terrain_at(first.world.size.wrap(10, 10)), {
            second.world.terrain_at(second.world.size.wrap(10, 10))
        });
    }
}
