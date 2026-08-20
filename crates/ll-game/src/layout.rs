//! 与 GPU 无关的纯计算：地形 → 图集条目名映射、光照 → 视野半径换算。
//!
//! 拆成独立文件、脱离窗口/GPU 也能被 `cargo test --workspace` 覆盖的
//! 理由，与 `ll-sim` 的 `p5_coordinate_acceptance::layout` 一致，见其
//! 模块文档。本文件的 [`terrain_entry_name`]/[`effective_sight_radius`]/
//! [`effective_tint`] 是同一套换算的独立实现（不是同一份代码的引用
//! ——`p5_coordinate_acceptance` 是 `ll-sim` 的一个 `examples/` 目录，
//! 不是可供下游 crate 依赖的库 API，见 Cargo 对 `examples/` 的可见性
//! 规则），保持逻辑一致但物理上各自独立。

use ll_core::time::Tick;
use ll_world::light::sight_radius_at;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 地表视野基准半径（格），随光照缩放。
pub const BASE_SIGHT_RADIUS: u32 = 12;

/// 把地形种类映射到图集条目名——覆盖本体注册的全部自然地形。
pub fn terrain_entry_name(kind: TerrainKind, ids: &BaseTerrainIds) -> Option<&'static str> {
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

/// 给定空间在某一世界时刻的有效光照换算出的视野半径。
pub fn effective_sight_radius(profile: &SpaceProfile, clock: Tick) -> u32 {
    let light = effective_ambient_light(profile, clock);
    sight_radius_at(BASE_SIGHT_RADIUS, light)
}

/// 画面整体亮度的下限——再暗的夜晚也不会低于这个值。
///
/// 项目所有者的要求是「黑夜要有一个还算能看的亮度」。原先午夜的调制
/// 系数是 0.1，连当前视野内的格子都被压得几乎看不出地形。
///
/// 这条是**纯表现层**决策（ADR 0020 甲区：结果只变成像素），与视野半径
/// 的下限 [`ll_world::light::MIN_SIGHT_RADIUS`] 分属两件事——一个管
/// 「看得清不清」，一个管「看得到多远」。前者可以自由用浮点、不进
/// `WorldState`、不参与 `hash()`；后者会经 FOV 影响探索记忆，是世界状态。
///
/// 取 0.4 而不是更高：夜晚仍要明显暗于白天（正午为 1.0），否则昼夜就
/// 只剩计时意义。已探索但当前无视野的格子还会再乘
/// [`EXPLORED_MEMORY_DIM_FACTOR`]，因此夜里的记忆层约为 0.14——能看出
/// 轮廓，但一眼能和当前视野区分开。
pub const MIN_VISIBLE_TINT: f32 = 0.4;

/// 画面整体亮度调制（灰阶），下限为 [`MIN_VISIBLE_TINT`]。
pub fn effective_tint(profile: &SpaceProfile, clock: Tick) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock).0.clamp(0, 1000) as f32 / 1000.0;
    let light = light.max(MIN_VISIBLE_TINT);
    [light, light, light, 1.0]
}

/// 已探索但当前无视野的格子（战争迷雾「记忆」层）在 [`effective_tint`]
/// 基础上再压暗的系数。
///
/// 只影响像素颜色，不进 [`ll_world::state::WorldState`]——世界状态禁止
/// 浮点（约束见 `ll_world::exploration` 模块文档「只存位图」一节：
/// `ExplorationMemory` 只记「看没看过」这一个 bit，暗化多少是纯表现层
/// 决策，不该反过来污染世界状态）。取值小于 1 让记忆层比当前视野暗、
/// 大于零让它比「从未探索」（完全不画、留黑）更亮——三层可见性
/// （项目所有者原话：「没有视野的地方就暗下来一些……没去过的地方就
/// 黑着」）因此不是三个离散色阶,而是「不画」与「按此系数压暗」两种
/// 处理叠加在同一套 `effective_tint` 光照调制之上。
const EXPLORED_MEMORY_DIM_FACTOR: f32 = 0.35;

/// 把当前光照色调换算成「已探索但当前无视野」格子应使用的记忆色调。
///
/// 见 [`EXPLORED_MEMORY_DIM_FACTOR`] 文档：只压暗 RGB，不动 alpha——
/// 记忆层格子仍需完全不透明地画出来，只是比当前视野内的格子暗。
pub fn memory_tint(tint: [f32; 4]) -> [f32; 4] {
    [
        tint[0] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[1] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[2] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[3],
    ]
}

/// 三层可见性判定：给定一格「当前是否在玩家视野内」与「是否已被探索
/// 过」，返回这一帧该不该画这一格、画的话用哪种色调。
///
/// 从 [`crate::app`] 的 `render_surface` 抽成与 GPU 无关的纯函数——三层
/// 可见性本身只是一张判定表（项目所有者原话：「没有视野的地方就暗
/// 下来一些，有视野的地方就没问题。而没去过的地方就黑着」），不需要
/// 靠跑起整条渲染管线才能验证：
///
/// - 当前有视野 → 画，用 `tint`（当前光照色调）。
/// - 当前无视野但已探索过 → 画，用 [`memory_tint`]（记忆层，比当前
///   光照暗）。
/// - 既无视野也没探索过 → 不画（`None`），调用方应当跳过这一格，让
///   `ll-render` 的黑色清屏背景顶替「从未探索」的黑。
pub fn tile_tint(currently_visible: bool, explored: bool, tint: [f32; 4]) -> Option<[f32; 4]> {
    if currently_visible {
        Some(tint)
    } else if explored {
        Some(memory_tint(tint))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 全部自然地形都能查到图集条目() {
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
        ];

        // Act & Assert
        for kind in kinds {
            assert!(terrain_entry_name(kind, &ids).is_some());
        }
    }

    #[test]
    fn 光照全灭时视野半径缩小到基准值以下() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_dark").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        };

        // Act
        let radius = effective_sight_radius(&profile, Tick(0));

        // Assert
        assert!(radius < BASE_SIGHT_RADIUS);
    }

    /// 开局那一刻玩家到底看得见什么——这条是**组合断言**。
    ///
    /// 午夜环境光（千分之一百）、`sight_radius_at` 的缩放、`effective_tint`
    /// 的整体调制、以及三层可见性里「从未探索就不画」，四条规则各自都
    /// 正确、各自都有测试守着，叠在一起却让 `Tick(0)` 开局变成纯黑屏加
    /// 正中央五个格子——项目所有者实测报告了这个现象。缺的从来不是某
    /// 一块的测试，而是「这些块凑在一起时开局长什么样」这一条。
    #[test]
    fn 新游戏起始时刻的地表视野远大于最小半径() {
        // Arrange：露天地表，没有额外的环境光下限加成。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let radius = effective_sight_radius(&profile, crate::world::NEW_GAME_START_TICK);

        // Assert：至少要有基准半径的一半，否则开局仍然近乎瞎。
        assert!(radius >= BASE_SIGHT_RADIUS / 2);
    }

    /// 与上一条配套：起始时刻的画面整体亮度不能低到把可见格子也压黑。
    #[test]
    fn 新游戏起始时刻的画面亮度过半() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, crate::world::NEW_GAME_START_TICK);

        // Assert
        assert!(tint[0] > 0.5);
    }

    /// 项目所有者的要求：「让黑夜有个最低视野范围以及一个还算能看的
    /// 亮度」。这条锁住亮度那一半，视野那一半由
    /// `ll_world::light` 的 `午夜视野不低于最小半径` 锁住。
    #[test]
    fn 午夜的画面亮度不低于可见下限() {
        // Arrange：露天地表，午夜，没有任何额外光源。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, Tick(0));

        // Assert
        assert!(tint[0] >= MIN_VISIBLE_TINT);
    }

    /// 但夜晚仍必须明显暗于正午，否则昼夜只剩计时意义。
    #[test]
    fn 午夜画面明显暗于正午() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let midnight = effective_tint(&profile, Tick(0));
        let noon = effective_tint(&profile, Tick(12 * ll_core::time::TICKS_PER_HOUR));

        // Assert
        assert!(midnight[0] < noon[0]);
    }

    #[test]
    fn 记忆色调比原始光照色调暗() {
        // Arrange
        let tint = [1.0, 1.0, 1.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert!(dimmed[0] < tint[0]);
    }

    #[test]
    fn 记忆色调不改变透明度() {
        // Arrange
        let tint = [0.6, 0.6, 0.6, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed[3], tint[3]);
    }

    #[test]
    fn 全黑光照下记忆色调仍是全黑() {
        // Arrange：夜间/无光照场景，压暗系数不该把零变成非零。
        let tint = [0.0, 0.0, 0.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn 当前有视野的格子用原始色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(true, false, tint);

        // Assert
        assert_eq!(result, Some(tint));
    }

    #[test]
    fn 探索过但当前无视野的格子用记忆色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, true, tint);

        // Assert
        assert_eq!(result, Some(memory_tint(tint)));
    }

    #[test]
    fn 从未探索且当前无视野的格子不绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, false, tint);

        // Assert
        assert_eq!(result, None);
    }
}
