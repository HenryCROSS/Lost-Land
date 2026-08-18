//! 地形生成入口：把噪声高度阈值化为具体地形种类。
//!
//! # 为什么阈值判断需要自己的接缝测试
//!
//! [`crate::noise`] 的无缝性已经由它自己的属性测试证明（见
//! `tests/noise_blackbox.rs`）。但噪声无缝不等于地形无缝：本模块把连续
//! 的高度值按阈值表切成离散的地形种类，这道切分本身也可能因为浮点/
//! 取整误差、坐标换算错位等原因引入不连续。所以本文件末尾单独有
//! 「东西接缝」「南北接缝」两条测试，直接比对生成结果，而不是依赖
//! 噪声层那条测试。

use ll_core::torus::TorusSize;

use crate::WorldError;
use crate::chunk::ChunkGrid;
use crate::noise::{CELL_SIZE, TileableNoise};
use crate::terrain::{BaseTerrainIds, TerrainKind};

/// 地形生成参数。
///
/// 高度阈值取 [`TileableNoise`] 输出区间同一套千分比整数，全程无浮点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenParams {
    /// 噪声与地形生成的种子，决定整张世界地形的具体分布。
    pub seed: u64,
    /// 深水与浅水的分界高度（千分比）。
    pub sea_level: i32,
    /// 丘陵与山地的分界高度（千分比）。
    pub mountain_level: i32,
    /// 噪声倍频叠加层数，层数越多地形起伏的细节越丰富。
    pub octaves: u32,
}

impl Default for GenParams {
    /// 默认阈值：海平面 400、山地起点 750、四层倍频。
    fn default() -> Self {
        GenParams {
            seed: 0,
            sea_level: 400,
            mountain_level: 750,
            octaves: 4,
        }
    }
}

/// 生成一整张环面地形。
///
/// # 错误
///
/// - 世界宽高必须都是 [`CELL_SIZE`] 的整数倍，否则 [`TileableNoise`] 的
///   格点周期在这个世界尺寸下无法整除，接缝处会出现不连续——返回
///   [`WorldError::WorldNotTileable`]。与其让缺陷以视觉异常的形式出现在
///   运行时（玩家跨越世界边界看到地形突变），不如在生成入口直接拒绝。
/// - 世界尺寸小于视口跨度时，由 [`ChunkGrid::new`] 返回
///   [`WorldError::WorldTooSmall`]。
///
/// `terrain_ids` 是调用方已经注册好的本体地形缓存（见
/// [`crate::terrain::materialize_base_terrain`]）——生成算法本身只挑
/// 「这格该是哪种地形」，具体某个名字对应哪个 [`TerrainKind`] 由调用方
/// 决定，本函数不内置任何编译期常量。
pub fn generate_terrain(
    world: TorusSize,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
) -> Result<ChunkGrid, WorldError> {
    let noise = build_noise(world, params)?;
    let mut grid = ChunkGrid::new(world, terrain_ids.deep_water)?;

    for y in 0..world.height() as i32 {
        for x in 0..world.width() as i32 {
            let kind = terrain_at_coord(&noise, params, x, y, terrain_ids);
            grid.set_terrain(world.wrap(x, y), kind);
        }
    }

    Ok(grid)
}

/// 按世界尺寸建立噪声源，并校验尺寸能被 [`CELL_SIZE`] 整除。
///
/// 抽成独立函数而不是内联在 [`generate_terrain`] 里，是为了让本文件
/// 末尾的接缝测试能拿到与生成入口完全相同的噪声源，在不经过
/// [`ChunkGrid`] 的环绕封装之前直接比较世界边界两侧的坐标——只有这样，
/// 接缝测试验证的才是生成入口真正会跑到的那条代码路径，而不是在测试
/// 里重新拼一遍阈值逻辑。
fn build_noise(world: TorusSize, params: &GenParams) -> Result<TileableNoise, WorldError> {
    let cell_size = CELL_SIZE as u32;
    if !world.width().is_multiple_of(cell_size) || !world.height().is_multiple_of(cell_size) {
        return Err(WorldError::WorldNotTileable {
            width: world.width(),
            height: world.height(),
        });
    }

    let period_x = world.width() / cell_size;
    let period_y = world.height() / cell_size;
    Ok(TileableNoise::new(params.seed, period_x, period_y)
        .expect("宽高已校验为 CELL_SIZE 的整数倍，且 TorusSize 保证宽高非零，周期不可能为零"))
}

/// 在给定的（未经环面环绕的）坐标处求出对应地形。
///
/// 刻意接受未环绕的原始坐标而不是 [`ll_core::torus::TorusPos`]：接缝
/// 测试需要比较 `x = 0` 与 `x = world.width()` 这两个在环绕之后会被
/// 判成同一个点的坐标，若这里的参数类型是已经环绕过的 `TorusPos`，
/// 测试根本无法构造出这两个不同的原始坐标。
fn terrain_at_coord(
    noise: &TileableNoise,
    params: &GenParams,
    x: i32,
    y: i32,
    terrain_ids: &BaseTerrainIds,
) -> TerrainKind {
    let height = noise.octaves(x, y, params.octaves);
    height_to_terrain(height, params, terrain_ids)
}

/// 把噪声高度按阈值表映射为具体地形种类。
///
/// 阈值全部取自 [`GenParams`] 的千分比整数，与 [`TileableNoise`] 的
/// 输出区间保持一致，全程无浮点。
fn height_to_terrain(height: i32, params: &GenParams, terrain_ids: &BaseTerrainIds) -> TerrainKind {
    if height < params.sea_level {
        terrain_ids.deep_water
    } else if height < params.sea_level + 50 {
        terrain_ids.shallow_water
    } else if height < params.sea_level + 100 {
        terrain_ids.sand
    } else if height < params.mountain_level - 150 {
        terrain_ids.grass
    } else if height < params.mountain_level - 50 {
        terrain_ids.forest
    } else if height < params.mountain_level {
        terrain_ids.hill
    } else if height < params.mountain_level + 100 {
        terrain_ids.mountain
    } else {
        terrain_ids.snow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;

    /// 测试世界尺寸：64 是 [`CELL_SIZE`]（16）的整数倍，且大于
    /// [`ChunkGrid`] 要求的视口跨度（43×25），生成不会因尺寸被拒绝。
    fn test_world() -> TorusSize {
        TorusSize::new(64, 64).expect("64x64 满足整除与视口跨度两条约束")
    }

    /// 按世界的规范坐标顺序收集整张地图的地形，供逐格比较。
    fn collect_terrain(grid: &ChunkGrid) -> Vec<TerrainKind> {
        let world = grid.world();
        let mut result = Vec::with_capacity((world.width() * world.height()) as usize);
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                result.push(grid.terrain_at(world.wrap(x, y)));
            }
        }
        result
    }

    fn count_water(grid: &ChunkGrid, terrain_ids: &BaseTerrainIds) -> usize {
        collect_terrain(grid)
            .into_iter()
            .filter(|kind| *kind == terrain_ids.deep_water || *kind == terrain_ids.shallow_water)
            .count()
    }

    #[test]
    fn 相同种子生成完全相同的地形() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = test_world();
        let params = GenParams {
            seed: 42,
            ..GenParams::default()
        };

        // Act
        let first =
            generate_terrain(world, &params, &terrain_ids).expect("64x64 满足生成入口的约束");
        let second =
            generate_terrain(world, &params, &terrain_ids).expect("64x64 满足生成入口的约束");

        // Assert
        assert_eq!(collect_terrain(&first), collect_terrain(&second));
    }

    #[test]
    fn 不同种子生成不同的地形() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = test_world();
        let params_a = GenParams {
            seed: 1,
            ..GenParams::default()
        };
        let params_b = GenParams {
            seed: 2,
            ..GenParams::default()
        };

        // Act
        let a = generate_terrain(world, &params_a, &terrain_ids).expect("64x64 满足生成入口的约束");
        let b = generate_terrain(world, &params_b, &terrain_ids).expect("64x64 满足生成入口的约束");

        // Assert
        assert_ne!(collect_terrain(&a), collect_terrain(&b));
    }

    #[test]
    fn 世界宽度不是格子尺寸整数倍时生成失败() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = TorusSize::new(50, 64).expect("50x64 是合法的 TorusSize");
        let params = GenParams::default();

        // Act
        let result = generate_terrain(world, &params, &terrain_ids);

        // Assert
        assert!(matches!(result, Err(WorldError::WorldNotTileable { .. })));
    }

    #[test]
    fn 海平面调高会增加水域格数() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = test_world();
        let low_sea = GenParams {
            seed: 7,
            sea_level: 400,
            ..GenParams::default()
        };
        let high_sea = GenParams {
            seed: 7,
            sea_level: 700,
            ..GenParams::default()
        };

        // Act
        let low_grid =
            generate_terrain(world, &low_sea, &terrain_ids).expect("64x64 满足生成入口的约束");
        let high_grid =
            generate_terrain(world, &high_sea, &terrain_ids).expect("64x64 满足生成入口的约束");

        // Assert
        assert!(count_water(&high_grid, &terrain_ids) > count_water(&low_grid, &terrain_ids));
    }

    #[test]
    fn 东西接缝两侧的地形一致() {
        // 噪声无缝不等于地形无缝，阈值判断本身也可能引入不连续，
        // 所以这里直接比较生成入口会用到的同一条代码路径，而不是
        // 依赖 noise 模块自己的无缝性测试。
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = test_world();
        let params = GenParams {
            seed: 123,
            ..GenParams::default()
        };
        let noise = build_noise(world, &params).expect("64x64 满足生成入口的约束");

        // Act & Assert
        for y in 0..world.height() as i32 {
            let west = terrain_at_coord(&noise, &params, 0, y, &terrain_ids);
            let east = terrain_at_coord(&noise, &params, world.width() as i32, y, &terrain_ids);
            assert_eq!(west, east);
        }
    }

    #[test]
    fn 南北接缝两侧的地形一致() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let world = test_world();
        let params = GenParams {
            seed: 456,
            ..GenParams::default()
        };
        let noise = build_noise(world, &params).expect("64x64 满足生成入口的约束");

        // Act & Assert
        for x in 0..world.width() as i32 {
            let north = terrain_at_coord(&noise, &params, x, 0, &terrain_ids);
            let south = terrain_at_coord(&noise, &params, x, world.height() as i32, &terrain_ids);
            assert_eq!(north, south);
        }
    }
}
