//! 与 GPU 无关的纯计算：世界/区块尺寸常量、地形→图集条目名映射、
//! 小地图版式、层属性→有效光照的呈现换算。
//!
//! 拆成独立文件的理由与 `p2_acceptance::layout`/`p1_acceptance::layout`
//! 一致：这些都是纯函数，脱离窗口/GPU 也能被 `cargo test --workspace`
//! 覆盖。

use ll_core::time::Tick;
use ll_world::light::sight_radius_at;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 区块边长（格）。64 是 [`ll_world::noise::CELL_SIZE`]（16）的整数倍，
/// 也满足 `ZoneLayout` 要求的最小视口跨度（43）。
pub(crate) const ZONE_SPAN: u32 = 64;

/// 世界区块数：6×4 = 24 个区块（世界总尺寸 384×256 格）。
///
/// # 为什么不能更小——这是本 demo 能证明「真流式加载」的关键
///
/// `WorldState::new` 会预热出生点周围 5×5 = 25 个区块（见
/// `ll_world::state` 的 `SPAWN_WARM_RADIUS`）。世界区块数必须明显大于
/// 25，玩家才可能走到一个**出生时没有被预热**的区块——6×4 = 24 个
/// 区块本身仍然小于 25，但因为区块坐标本身是环面（会绕接缝），5×5
/// 邻域在 6 宽的世界上只覆盖 x∈{4,5,0,1,2} 这 5 列（缺 x=3 那一列 4
/// 个区块）；4 高的世界上 5 圈邻域直接绕满整个高度。玩家从出生点向东
/// 走进 x=3 那一列，才是本 demo 真正验证「相邻区块流式加载」（而不是
/// 「出生点一次性预热」）的地方。
pub(crate) const ZONE_COUNT_X: u32 = 6;
pub(crate) const ZONE_COUNT_Y: u32 = 4;

/// 出生点世界坐标：区块 (0,0) 内部，留出充分空间安放 Interior 入口与
/// 后续东移的路径。
pub(crate) const SPAWN_X: i32 = 20;
pub(crate) const SPAWN_Y: i32 = 20;

/// 出生点正东的强制可通行走廊长度（格）——从 `(SPAWN_X, SPAWN_Y)` 向东
/// 覆盖到这个距离，全部强制改写成草地，不依赖噪声生成恰好给出连续
/// 可通行地形（默认种子下这一带以深水为主）。260 格足以跨过区块边长
/// 64 的第 3 列边界（世界 x=192 起，见 [`ZONE_COUNT_X`] 文档），是
/// 验收①②真正需要走到的距离；验收 demo 与自动化回归测试
/// （`walkthrough_test.rs`）共用这同一条走廊，见其模块文档。
pub(crate) const EAST_CORRIDOR_LENGTH: i32 = 260;

/// 向东走多远才能**恰好落在**出生点预热半径覆盖不到的那一列区块内
/// （第 3 列，世界 x∈[192,256)），不多不少——出生点预热是 5×5 邻域
/// （半径 2），在 6 宽的世界上因为区块坐标本身是环面会绕接缝，实际
/// 覆盖 x∈{4,5,0,1,2} 这 5 列，只留第 3 列真正没被预热。走出
/// [`EAST_CORRIDOR_LENGTH`] 那么远会绕出第 3 列、进入同样被预热覆盖过
/// 的第 4 列，反而证明不了「这是移动时才流式加载的」——见
/// `walkthrough_test.rs` 对应测试的文档。
/// 只被 `walkthrough_test.rs`（`#[cfg(test)]`）消费——非测试构建下没有
/// 消费方,加这个属性避免 `cargo build`（不带 `--tests`）报
/// `dead_code`。
#[cfg(test)]
pub(crate) const EAST_WALK_INTO_UNWARMED_ZONE: i32 = 200;

/// Interior 入口相对出生点的偏移（格）：向南 3 格，紧邻出生点，一次
/// 方向键按下即可走到——与 `p4_acceptance` 「熔岩地板紧邻出生点」
/// 同一条教训（见其 `world.rs` 文档「实测撞见的真实缺陷」），不依赖
/// 中间地形是否可通行。
pub(crate) const ENTRANCE_OFFSET_Y: i32 = 3;

/// Interior 楼层的边长（格）——一个小房间，够放下墙、地板与玩家视野
/// 半径萎缩后的演示效果，不需要更大。
pub(crate) const INTERIOR_FLOOR_SIZE: u32 = 12;

/// 玩家在 Interior 内的固定视角中心——本批次不接线 Interior 内部漫游
/// （见 `ll_sim::resolve` 模块文档「Interior 内部移动的范围边界」），
/// 固定取楼层正中央。
pub(crate) const INTERIOR_VIEW_CENTER: (i32, i32) = (
    (INTERIOR_FLOOR_SIZE / 2) as i32,
    (INTERIOR_FLOOR_SIZE / 2) as i32,
);

/// 地表视野基准半径（格），随光照缩放（[`ll_world::light::sight_radius_at`]）。
pub(crate) const BASE_SIGHT_RADIUS: u32 = 12;

/// 流式邻域维护半径（区块为单位）——覆盖「地表视野半径 + 余量」，
/// 保证玩家看到的下一个区块总是在相机/FOV 真正查询到它之前就已经
/// 生成好，见 `ll_world::surface_store::SurfaceStore::stream_neighborhood`
/// 文档。
pub(crate) const STREAM_RADIUS_ZONES: i32 = 2;

/// demo 开局时把时钟从午夜推进到的初始刻度：正午——理由与
/// `p2_acceptance::layout::INITIAL_CLOCK_TICKS` 一致（午夜画面接近全黑，
/// 演示不出「水/沙/草/林/山/雪」的分布）。
pub(crate) const INITIAL_CLOCK_TICKS: i64 = 12 * ll_core::time::TICKS_PER_HOUR;

/// 小地图每格对应的区块数（`continent_map` 的下采样倍率）——世界只有
/// 24 个区块，取 1（不缩小），每个区块在小地图上都有独立一格。
pub(crate) const MINIMAP_DOWNSAMPLE: u32 = 1;

/// 小地图每格在离屏目标上的像素边长。
pub(crate) const MINIMAP_CELL_PX: i32 = 12;

/// 小地图左上角与离屏目标左上角的留白（像素）。
pub(crate) const MINIMAP_MARGIN_PX: i32 = 4;

/// 行走动画每帧停留的游戏帧数，取值与 `p1_acceptance::WALK_FRAMES_PER_STEP`
/// 一致——两个 demo 用的是同一套图集帧（`hero_walk_0`/`hero_walk_1`），
/// 没有理由播放节奏不一样。
pub(crate) const WALK_FRAMES_PER_STEP: u32 = 8;

/// 待机呼吸动画每帧停留的游戏帧数，取值远大于行走的步长——呼吸本就该
/// 比迈步慢得多，步长太接近会让「呼吸」看起来像原地抖动，而不是缓慢
/// 起伏（项目所有者明确要求「不要做成明显的抖动」）。
pub(crate) const IDLE_BREATHE_FRAMES_PER_STEP: u32 = 40;

/// 行走这一触发式动画状态在没有新的移动意图时，继续维持播放多少帧
/// 才回落到待机——即状态退出前的余韵，见
/// [`ll_render::anim::AnimStateMachine`] 文档「要解决的问题」。
///
/// # 为什么是 12
///
/// 回合制的移动意图并非每帧都有：按住方向键时，`InputState` 只在
/// 按键刚按下、以及此后每次自动重复脉冲触发的那一帧才产出
/// `Intent::Move`（见 `ll_platform::input` 模块文档「为什么要区分
/// 『按住』与『刚按下』」），脉冲之间的默认间隔
/// （`ll_platform::input::RepeatConfig::default` 的 `interval`，
/// 90ms）在本 demo 目标帧率（`WindowConfig::default` 的
/// `target_fps` = 60）下约合 5.4 帧。若余韵短于这个间隔，两次脉冲
/// 之间的空档就会先回落到待机再切回行走，正是项目所有者报告的「走
/// 一格闪一下」——这里取约两倍余量（12 帧）覆盖脉冲间隔本身的抖动
/// 与慢帧下重复脉冲被合并/延迟到达的情形。
pub(crate) const WALK_EXIT_GRACE_FRAMES: u32 = 12;

/// 把地形种类映射到图集条目名——覆盖本 demo 用到的地表自然地形，
/// 以及 Interior 楼层用到的地板/墙（复用既有图集条目，本 demo 不新增
/// 美术资产：石地板借用 `terrain_dirt`、石墙借用 `terrain_mountain`，
/// 两者原本就有的图案恰好分别对应「地面」「阻挡」的直觉）。
pub(crate) fn terrain_entry_name(kind: TerrainKind, ids: &BaseTerrainIds) -> Option<&'static str> {
    if kind == ids.deep_water {
        Some("terrain_deep_water")
    } else if kind == ids.shallow_water {
        Some("terrain_shallow_water")
    } else if kind == ids.sand {
        Some("terrain_sand")
    } else if kind == ids.grass {
        Some("terrain_grass")
    } else if kind == ids.forest {
        Some("terrain_forest")
    } else if kind == ids.hill {
        Some("terrain_hill")
    } else if kind == ids.mountain {
        Some("terrain_mountain")
    } else if kind == ids.snow {
        Some("terrain_snow")
    } else if kind == ids.floor_stone {
        Some("terrain_dirt")
    } else if kind == ids.wall_stone {
        Some("terrain_mountain")
    } else {
        None
    }
}

/// 求一个空间在给定世界时刻的「有效光照」与由此推出的视野半径——
/// 主视口与 Interior 视口共用同一套换算，唯一的输入差异是 `profile`
/// （地表 profile 恒 `exposed_to_sky`，Interior profile 恒不）。
///
/// 复用既有 [`effective_ambient_light`]（任务 4）：这里不是第二套光照
/// 实现，只是把它的结果接到 [`sight_radius_at`]（P2 已有的呈现换算）
/// 上——这正是「层属性生效」这条验收点的落点：地下 profile 的
/// `ambient_light_floor` 越低，算出的视野半径越小。
pub(crate) fn effective_sight_radius(profile: &SpaceProfile, clock: Tick) -> u32 {
    let light = effective_ambient_light(profile, clock);
    sight_radius_at(BASE_SIGHT_RADIUS, light)
}

/// 画面整体亮度调制（灰阶，不含季节色相——本 demo 的验收重点是「暗」
/// 这件事本身，不需要 `p2_acceptance` 那一层四季色相）。
pub(crate) fn effective_tint(profile: &SpaceProfile, clock: Tick) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock).0.clamp(0, 1000) as f32 / 1000.0;
    [light, light, light, 1.0]
}

/// 小地图第 `(col, row)` 格在离屏目标像素空间中的左上角位置。
pub(crate) fn minimap_cell_screen_pos(col: u32, row: u32) -> (i32, i32) {
    (
        MINIMAP_MARGIN_PX + col as i32 * MINIMAP_CELL_PX,
        MINIMAP_MARGIN_PX + row as i32 * MINIMAP_CELL_PX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::space_profile::base_space_profile_fixture;

    fn surface_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 200,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        }
    }

    fn dungeon_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 80,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        }
    }

    #[test]
    fn 全部自然地形与interior地板墙都能查到图集条目() {
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let kinds = [
            ids.deep_water,
            ids.shallow_water,
            ids.sand,
            ids.grass,
            ids.forest,
            ids.hill,
            ids.mountain,
            ids.snow,
            ids.floor_stone,
            ids.wall_stone,
        ];

        // Act & Assert
        for kind in kinds {
            assert!(terrain_entry_name(kind, &ids).is_some());
        }
    }

    #[test]
    fn 地下城的视野半径明显小于地表正午() {
        // 这是「层属性生效」验收点的纯函数级证据——渲染层是否正确接线
        // 由 demo 实测确认，但换算本身先在这里锁定预期方向。
        // Arrange
        let noon = Tick(INITIAL_CLOCK_TICKS);
        let surface = surface_profile();
        let dungeon = dungeon_profile();

        // Act
        let surface_radius = effective_sight_radius(&surface, noon);
        let dungeon_radius = effective_sight_radius(&dungeon, noon);

        // Assert
        assert!(dungeon_radius < surface_radius);
    }

    #[test]
    fn 地下城的视野半径不随时钟变化() {
        // Arrange
        let dungeon = dungeon_profile();

        // Act
        let midnight_radius = effective_sight_radius(&dungeon, Tick(0));
        let noon_radius = effective_sight_radius(&dungeon, Tick(INITIAL_CLOCK_TICKS));

        // Assert
        assert_eq!(midnight_radius, noon_radius);
    }

    #[test]
    fn 本体注册的地下城profile在正午视野半径小于地表() {
        // 与上面两条不同：这里直接走 base_space_profile_fixture 注册出
        // 的真实数值（而不是本文件手写的测试用 profile），确认 demo
        // 实际会用到的 BaseSpaceProfileIds 组合同样成立。
        // Arrange
        let (_ids, table) = base_space_profile_fixture();
        let (ids2, _table2) = base_space_profile_fixture();
        let surface = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: table.ambient_light_floor(ids2.surface),
            exposed_to_sky: table.exposed_to_sky(ids2.surface),
            base_temperature: table.base_temperature(ids2.surface),
            diggable: table.diggable(ids2.surface),
            buildable: table.buildable(ids2.surface),
            reverb_tag: table.reverb_tag(ids2.surface),
        };
        let dungeon = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("合法"),
            ambient_light_floor: table.ambient_light_floor(ids2.dungeon),
            exposed_to_sky: table.exposed_to_sky(ids2.dungeon),
            base_temperature: table.base_temperature(ids2.dungeon),
            diggable: table.diggable(ids2.dungeon),
            buildable: table.buildable(ids2.dungeon),
            reverb_tag: table.reverb_tag(ids2.dungeon),
        };
        let noon = Tick(INITIAL_CLOCK_TICKS);

        // Act & Assert
        assert!(effective_sight_radius(&dungeon, noon) < effective_sight_radius(&surface, noon));
    }

    #[test]
    fn 小地图第一格贴着留白角落() {
        // Arrange & Act
        let (x, y) = minimap_cell_screen_pos(0, 0);

        // Assert
        assert_eq!((x, y), (MINIMAP_MARGIN_PX, MINIMAP_MARGIN_PX));
    }
}
