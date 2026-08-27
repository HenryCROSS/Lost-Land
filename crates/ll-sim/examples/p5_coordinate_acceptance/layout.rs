//! 与 GPU 无关的纯计算：世界/区块尺寸常量、地形→图集条目名映射、
//! 小地图版式、层属性→有效光照的呈现换算。
//!
//! 拆成独立文件的理由与 `p2_acceptance::layout`/`p1_acceptance::layout`
//! 一致：这些都是纯函数，脱离窗口/GPU 也能被 `cargo test --workspace`
//! 覆盖。

use ll_core::time::Tick;
use ll_world::light::sight_radius_at;

/// 「这个调用方不知道谁在看」时传给暗视参数的取值。
///
/// `0` 在 [`ll_world::light::sight_radius_at`] 里被解读成**未声明**
/// 暗视，落回 [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`]——本
/// demo 不区分种族，行为与该函数长出这个参数之前逐格相同。
const NO_DARKVISION: u32 = 0;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};
use ll_world::weather::Weather;

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

/// 把地形种类映射到图集条目名——覆盖本 demo 用到的地表自然地形，
/// 以及 Interior 楼层用到的地板/墙。
///
/// # 与 `ll_game::layout::terrain_entry_name` 的关系：**一致**
///
/// 本表与生产渲染路径那张表是同一套换算的两份独立实现（理由见本模块
/// 文档「保持逻辑一致但物理上各自独立」一节）——**一致是被守住的性质，
/// 不是巧合**：两处对同一种地形选出的贴图必须是同一张。
///
/// 这条一致性曾经破过一次。据点建筑地形补图那一批给
/// `floor_stone`/`wall_stone` 各画了专属贴图
/// （`terrain_floor_stone`/`terrain_wall_stone`），只改了生产路径那张
/// 表；本表仍按更早的**借用**关系走——石地板借 `terrain_dirt`、石墙借
/// `terrain_mountain`。两张表就此分叉，且分叉被写进注释当成「对本 demo
/// 依然成立」。项目所有者的裁定是**统一**，原话「第三条的话先统一了吧,
/// 避免以后有什么问题」——即不留一处记录在案、却会持续漂移的偏差。
/// 本表因此改回与生产路径同一张贴图，借用关系随之解除。
///
/// 注意**不是**所有 `terrain_dirt` 的用法都叫借用：
/// `crates/ll-render/examples/p1_acceptance` 拿它铺棋盘格、
/// `ll_game::content` 的 mod 资产覆盖验收拿它当被覆盖目标，那两处
/// `terrain_dirt` 就是泥土本身，不在这次统一的范围内。
///
/// 条目名在本 demo 里是**裸名字**（`terrain_floor_stone` 而非
/// `lostland:terrain_floor_stone`）：本 demo 直接
/// `include_bytes!` 遗留共享画布 `assets/atlas/placeholder.png` 与它的
/// `placeholder.json`，那份元数据里的条目名从来就是裸名字；生产路径走
/// 的是运行期打包的资产 VFS，条目名是完整命名空间 ID。两套字符串空间
/// 的差别与本次统一无关，不要顺手一起改——改了这个 demo 立刻查不到图。
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
        Some("terrain_floor_stone")
    } else if kind == ids.wall_stone {
        Some("terrain_wall_stone")
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
///
/// # 为什么恒传 `Weather::CLEAR`
///
/// 天气系统批次给 `effective_ambient_light` 加了第三个参数。本 demo 的
/// 验收点是**坐标系与层属性**，不是天气——让一个随机天气参与进来只会
/// 让「地下比地表暗」这条断言多一个与验收无关的变量。显式传晴空基准
/// 是如实声明「本 demo 不演示天气」，不是忘了接线：真实的天气消费者在
/// `ll_game::layout`（生产渲染路径）。
pub(crate) fn effective_sight_radius(profile: &SpaceProfile, clock: Tick) -> u32 {
    let light = effective_ambient_light(profile, clock, Weather::CLEAR);
    sight_radius_at(BASE_SIGHT_RADIUS, light, NO_DARKVISION)
}

/// 画面整体亮度调制（灰阶，不含季节色相——本 demo 的验收重点是「暗」
/// 这件事本身，不需要 `p2_acceptance` 那一层四季色相）。
pub(crate) fn effective_tint(profile: &SpaceProfile, clock: Tick) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock, Weather::CLEAR)
        .0
        .clamp(0, 1000) as f32
        / 1000.0;
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

    /// 本 demo 会画到的十种地形，与 [`terrain_entry_name`] 里返回
    /// `Some` 的那十支逐条对应。顺序固定（数组字面量，不经任何哈希
    /// 容器），符合约束 C5。
    fn demo_terrains(ids: &ll_world::terrain::BaseTerrainIds) -> [(&'static str, TerrainKind); 10] {
        [
            ("deep_water", ids.deep_water),
            ("shallow_water", ids.shallow_water),
            ("sand", ids.sand),
            ("grass", ids.grass),
            ("forest", ids.forest),
            ("hill", ids.hill),
            ("mountain", ids.mountain),
            ("snow", ids.snow),
            ("floor_stone", ids.floor_stone),
            ("wall_stone", ids.wall_stone),
        ]
    }

    #[test]
    fn 全部自然地形与interior地板墙都能查到图集条目() {
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act & Assert
        for (label, kind) in demo_terrains(&ids) {
            assert!(
                terrain_entry_name(kind, &ids).is_some(),
                "地形 {label} 算不出图集条目名"
            );
        }
    }

    #[test]
    fn 十种地形两两不共用同一个图集条目() {
        // 「查得到条目」不等于「看得出区别」：本表此前让 `floor_stone`
        // 借 `terrain_dirt`、`wall_stone` 借 `terrain_mountain`，两条
        // 查找都成功，屏幕上却分不出哪格是山、哪格是石墙。所有者裁定
        // 统一到生产路径那张表之后，借用关系解除——这条测试就是那次
        // 统一的可执行版本：把任何一支改回借用，它立刻变红。
        //
        // 反例（本次开发实跑）：把 `wall_stone` 那支改回
        // `Some("terrain_mountain")`，本条报「wall_stone 与 mountain
        // 共用同一个图集条目」。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let named: Vec<(&str, &str)> = demo_terrains(&ids)
            .into_iter()
            .map(|(label, kind)| {
                (
                    label,
                    terrain_entry_name(kind, &ids).expect("上一条已经保证全部是 Some"),
                )
            })
            .collect();

        // Act & Assert
        for (i, (label_a, name_a)) in named.iter().enumerate() {
            for (label_b, name_b) in &named[i + 1..] {
                assert_ne!(
                    name_a, name_b,
                    "地形 {label_a} 与 {label_b} 共用同一个图集条目 {name_a}——                     屏幕上分不出这两种地形"
                );
            }
        }
    }

    #[test]
    fn 十种地形的图集条目在遗留共享画布的元数据里都存在() {
        // 上一条只证明「十个名字互不相同」，不证明「这些名字在本 demo
        // 真正 include_bytes! 的那份元数据里查得到」——改成一个不存在
        // 的名字同样能通过上一条。本 demo 装的是遗留共享画布
        // `assets/atlas/placeholder.json`（条目名是**裸名字**），因此
        // 直接对着那份 JSON 文本核对。
        //
        // 反例（本次开发实跑）：把 `floor_stone` 那支改成
        // `Some("terrain_floor_stone_typo")`，本条报「条目名 …_typo 不在
        // placeholder.json 里」。
        // Arrange
        let atlas_json = include_str!("../../../../assets/atlas/placeholder.json");
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act & Assert
        for (label, kind) in demo_terrains(&ids) {
            let name = terrain_entry_name(kind, &ids).expect("上一条已经保证全部是 Some");
            let needle = format!("\"name\": \"{name}\"");
            assert!(
                atlas_json.contains(&needle),
                "地形 {label} 查的条目名 {name} 不在 placeholder.json 里——                 本 demo 跑起来这一格画不出来"
            );
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
