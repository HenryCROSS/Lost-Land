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

use ll_core::torus::{TorusPos, TorusSize};

use crate::WorldError;
use crate::chunk::ChunkGrid;
use crate::noise::{CELL_SIZE, TileableNoise};
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainKind};
use crate::zone::ZoneLayout;

/// 地形**形态**参数：同一个种子下，这组数值决定世界长什么样——水陆
/// 比例、山地多少、陆地碎成几块。
///
/// 全部取 [`TileableNoise`] 输出区间同一套千分比整数，全程无浮点
/// （[ADR 0020](../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
/// 乙区：这些数值直接流进世界状态，必须是量化整数）。
///
/// # 为什么与 `seed` 分成两个类型
///
/// 不是为了对称（[ADR 0021](../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)
/// 明令禁止那种理由），是因为存档里这两半的归属**本来就不同**：
/// `seed` 早就作为 `ll_world::state::WorldState::seed` 单独持久化了，
/// 而形态参数此前根本没进存档。把形态参数聚成一个类型，
/// `WorldState` 就只需要新增**一个**字段（
/// `ll_world::state::WorldState::terrain_shape`）承接它，
/// 不必把种子再存第二遍、也不必把四个散字段各自铺进存档结构与
/// `remap` 的穷尽解构里。
///
/// # 为什么它必须进存档（ADR 0009「默认派生，只存偏差」）
///
/// ADR 0009 的规则是「能派生的不进存档」。形态参数**不可派生**——它是
/// 玩家在建档那一刻做出的选择，世界建成之后没有任何其它数据能反推出
/// 「玩家当初选的是海平面 400 还是 620」（地形本身是这组参数的函数，
/// 反过来不成立：不同参数组合可能在已常驻的那几块区块上产出相同地形）。
/// 它与 `seed`、世界尺寸、生成期 mod 集合同属
/// `ll_content::world_identity` 描述的「缺一，世界都复现不出来」那一
/// 类，因此进存档是正当的，不是对 ADR 0009 的例外。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainShape {
    /// 深水与浅水的分界高度（千分比）。**水陆比例的主旋钮**：噪声高度
    /// 分布集中在 300～700 之间（见 `knowledge/design/worldgen-parameters.md` 实测直方
    /// 图），这个值每上调 50，全图水域比例约上升 11 个百分点；调到 200
    /// 以下或 700 以上旋钮会饱和（几乎全陆 / 几乎全水）。
    pub sea_level: i32,
    /// 丘陵与山地的分界高度（千分比）。**山地比例的主旋钮**，且与
    /// [`Self::sea_level`] 正交——实测调这个值不改变水陆比例一个百分点。
    pub mountain_level: i32,
    /// 噪声倍频叠加层数，层数越多地形起伏的细节越丰富。**地形破碎程度
    /// 的旋钮**：层数越多，高度分布越向中位数收拢（多层平均的必然结果），
    /// 于是极端高度变少（山地占比从 1 层的 19% 掉到 8 层的 2.6%）、
    /// 海岸线越曲折、独立陆块越多。
    pub octaves: u32,
    /// 大陆尺度缩减档位：每 +1 档，噪声最粗一层的格子边长减半，也就是
    /// 「一块大陆」的典型尺寸减半。见
    /// [`TileableNoise::shrink_continents`]——**这是「群岛」形态唯一
    /// 真正需要的旋钮**，原有三个阈值旋钮表达不了它。
    pub continent_shrink: u32,
}

impl Default for TerrainShape {
    /// 默认形态：海平面 400、山地起点 750、四层倍频、不缩减大陆尺度。
    ///
    /// 这四个值必须**逐位**保持不变——它们是
    /// `crates/ll-world/tests/determinism.rs` 的 `EXPECTED_WORLD_DIGEST`
    /// 与 `crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`
    /// 两条黄金基准所固定的那张地图。
    fn default() -> Self {
        TerrainShape {
            sea_level: 400,
            mountain_level: 750,
            octaves: 4,
            continent_shrink: 0,
        }
    }
}

/// [`TerrainShape`] 各字段的合法取值范围——越界即拒绝，见
/// [`TerrainShape::validate`]。
///
/// 越界值不会让生成 panic（阈值链只是让某些地形带变空，
/// `shrink_continents` 自己在 1 处饱和），但会静默产出一张没人想要的
/// 地图——玩家手改配置写错一个零，应该看到一条明确的拒绝日志，而不是
/// 开出一个全是深水的世界还以为是自己运气差。
impl TerrainShape {
    /// 高度千分比的取值上界（含），与 [`TileableNoise`] 输出区间一致。
    pub const HEIGHT_MAX: i32 = 1000;
    /// [`Self::mountain_level`] 至少要比 [`Self::sea_level`] 高出多少
    /// ——阈值链里从海平面到山地之间铺了浅水(+50)/沙地(+100)与
    /// 草地/森林/丘陵三段（`mountain_level - 150` 起算），少于 150 这
    /// 几段就会互相穿插，地形带的先后顺序不再成立。
    pub const MIN_LEVEL_GAP: i32 = 150;
    /// 倍频层数的合法区间（含）。上界取 12：振幅每层减半，第 11 层起
    /// 振幅整数除法归零、[`TileableNoise::octaves`] 自己就会提前跳出，
    /// 再大的值只是无效声明。
    pub const OCTAVES_RANGE: std::ops::RangeInclusive<u32> = 1..=12;
    /// 大陆尺度缩减档位的上界（含）。取 8：本体最大预设尺寸下自动推导
    /// 出的 `coarse_scale` 也不过 32（五档），8 档足以让任何尺寸都饱和
    /// 到 1，再大没有额外效果。
    pub const MAX_CONTINENT_SHRINK: u32 = 8;

    /// 校验这组形态参数是否落在合法区间；不合法时返回一句可直接写进
    /// 日志的中文原因。
    ///
    /// 这是系统边界上的输入校验（玩家手写的配置文件是不可信输入）：
    /// 调用方（`ll_game` 的配置解析）应当在拒绝时记一条日志并退回
    /// [`Self::default`]，与 `ll_platform::config::load_or_default`
    /// 对损坏配置的处理同一条纪律——绝不 panic。
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=Self::HEIGHT_MAX).contains(&self.sea_level) {
            return Err(format!(
                "海平面 {} 超出合法区间 0..={}",
                self.sea_level,
                Self::HEIGHT_MAX
            ));
        }
        if !(0..=Self::HEIGHT_MAX).contains(&self.mountain_level) {
            return Err(format!(
                "山地阈值 {} 超出合法区间 0..={}",
                self.mountain_level,
                Self::HEIGHT_MAX
            ));
        }
        if self.mountain_level - self.sea_level < Self::MIN_LEVEL_GAP {
            return Err(format!(
                "山地阈值 {} 与海平面 {} 的差不足 {}，地形带会互相穿插",
                self.mountain_level,
                self.sea_level,
                Self::MIN_LEVEL_GAP
            ));
        }
        if !Self::OCTAVES_RANGE.contains(&self.octaves) {
            return Err(format!(
                "倍频层数 {} 超出合法区间 {}..={}",
                self.octaves,
                Self::OCTAVES_RANGE.start(),
                Self::OCTAVES_RANGE.end()
            ));
        }
        if self.continent_shrink > Self::MAX_CONTINENT_SHRINK {
            return Err(format!(
                "大陆尺度缩减档位 {} 超出合法区间 0..={}",
                self.continent_shrink,
                Self::MAX_CONTINENT_SHRINK
            ));
        }
        Ok(())
    }
}

/// 地形生成参数：种子 + 形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenParams {
    /// 噪声与地形生成的种子，决定整张世界地形的具体分布。
    pub seed: u64,
    /// 地形形态参数，见 [`TerrainShape`]。
    pub shape: TerrainShape,
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
        .expect("宽高已校验为 CELL_SIZE 的整数倍，且 TorusSize 保证宽高非零，周期不可能为零")
        .shrink_continents(params.shape.continent_shrink))
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
    let height = noise.octaves(x, y, params.shape.octaves);
    height_to_terrain(height, params, terrain_ids)
}

/// 把噪声高度按阈值表映射为具体地形种类。
///
/// 阈值全部取自 [`GenParams`] 的千分比整数，与 [`TileableNoise`] 的
/// 输出区间保持一致，全程无浮点。
fn height_to_terrain(height: i32, params: &GenParams, terrain_ids: &BaseTerrainIds) -> TerrainKind {
    let shape = &params.shape;
    if height < shape.sea_level {
        terrain_ids.deep_water
    } else if height < shape.sea_level + 50 {
        terrain_ids.shallow_water
    } else if height < shape.sea_level + 100 {
        terrain_ids.sand
    } else if height < shape.mountain_level - 150 {
        terrain_ids.grass
    } else if height < shape.mountain_level - 50 {
        terrain_ids.forest
    } else if height < shape.mountain_level {
        terrain_ids.hill
    } else if height < shape.mountain_level + 100 {
        terrain_ids.mountain
    } else {
        terrain_ids.snow
    }
}

/// 按区块布局建立世界尺度噪声源，供窗口化生成与 `SurfaceStore` 复用。
///
/// 委托给 [`build_noise`]（本文件内部实现，逻辑不改一行）——只是把入参
/// 从「世界瓦片尺寸」换成「区块布局」，因为流式加载场景下调用方通常
/// 先有 [`ZoneLayout`]，而不是先手算出一个世界瓦片 `TorusSize`。构造
/// 一次，此后所有区块窗口共用同一个实例（设计文档五节：`build_noise`
/// 是 O(1) 操作）。
pub fn build_zone_noise(
    layout: &ZoneLayout,
    params: &GenParams,
) -> Result<TileableNoise, WorldError> {
    build_noise(layout.tile_size(), params)
}

/// 只生成一个区块窗口的地形，不遍历整个世界——[`generate_terrain`] 的
/// 窗口化版本，复用同一个 `noise` 源与同一套阈值逻辑
/// （[`terrain_at_coord`]，不改一行）。
///
/// `noise` 由调用方预先用 [`build_zone_noise`] 构造一次并长期复用——
/// `build_noise` 本身是 O(1) 操作（只依赖世界总尺寸），不需要每个区块
/// 窗口各自重新构造一份（设计文档五节）。
///
/// # 错误
///
/// 一个正确构造的 [`ZoneLayout`]（经 `ZoneLayout::new` 校验过
/// `zone_span >= 43`）恒能生成成功——这里返回 `Result` 只是与
/// [`ChunkGrid::new`] 的签名保持一致，不代表调用方需要为「正常配置下
/// 不可能发生」的失败分支编写实际处理逻辑。
pub fn generate_zone_window(
    noise: &TileableNoise,
    params: &GenParams,
    layout: &ZoneLayout,
    zone: ZoneCoord,
    terrain_ids: &BaseTerrainIds,
) -> Result<ChunkGrid, WorldError> {
    let span = layout.zone_span();
    let local_size = layout.local_size();
    let mut grid = ChunkGrid::new(local_size, terrain_ids.deep_water)?;

    let origin_x = zone.x() * span as i32;
    let origin_y = zone.y() * span as i32;

    for ly in 0..span as i32 {
        for lx in 0..span as i32 {
            let world_x = origin_x + lx;
            let world_y = origin_y + ly;
            let kind = terrain_at_coord(noise, params, world_x, world_y, terrain_ids);
            grid.set_terrain(local_size.wrap(lx, ly), kind);
        }
    }

    Ok(grid)
}

/// 求一个世界瓦片坐标上的**基础地形**——据点、玩家改动等任何后续
/// 写入都还没发生的那一层。
///
/// 与 [`generate_zone_window`] 用的是同一个 [`terrain_at_coord`]，因此
/// 「窗口里第 (lx, ly) 格」与「本函数在对应世界坐标上」的结果恒相同，
/// 不存在两份阈值逻辑。
///
/// # 为什么需要一个「按世界坐标」的入口
///
/// [`crate::settlement::stamp_settlement`] 铺一栋**横跨区块**的建筑时，
/// 要判断这块 5×5 的地能不能盖房，而其中一部分格子根本不在当前窗口
/// 里。它不能去读邻区块（那要么触发一次隐式加载、要么撞上
/// `SurfaceStore::set_terrain` 那类常驻契约），但它可以直接问噪声
/// ——地形本来就是坐标的纯函数，这个问题不需要任何区块常驻。
///
/// 接受已经环绕过的 [`TorusPos`] 而不是裸坐标：调用方拿到的是世界
/// 瓦片坐标，环绕由 `TorusSize::wrap` 统一负责，本函数不再重复一遍
/// 环面语义。
pub fn terrain_at_tile(
    noise: &TileableNoise,
    params: &GenParams,
    pos: TorusPos,
    terrain_ids: &BaseTerrainIds,
) -> TerrainKind {
    terrain_at_coord(noise, params, pos.x(), pos.y(), terrain_ids)
}

/// 区块级粗粒度采样：只取该区块**左上角一点**的地形，不生成整个区块
/// 窗口（区块通常是 48×48 格，生成整窗只为了取一个代表点是纯粹的
/// 浪费）——供 [`crate::overview::generate_continent_field`]（任务 13）
/// 这类只需要「大致轮廓」的调用方使用。
///
/// 取左上角而非区块中心/平均：与既有 [`crate::overview::continent_map`]
/// 「每格取块内左上角地形而非平均」的既有惯例一致（地形是离散分类值，
/// 平均没有意义，见该函数文档），这里只是把同一条惯例从「瓦片块」搬到
/// 「区块」这个更粗的分辨率。
pub fn zone_representative_terrain(
    noise: &TileableNoise,
    params: &GenParams,
    layout: &ZoneLayout,
    zone: ZoneCoord,
    terrain_ids: &BaseTerrainIds,
) -> TerrainKind {
    let span = layout.zone_span() as i32;
    terrain_at_coord(noise, params, zone.x() * span, zone.y() * span, terrain_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;
    use crate::zone::ZoneLayout;

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

    /// 统计一张地图上出现过的不同地形种类数。
    ///
    /// 用线性扫描去重而非 `HashSet`：规避 C5（禁止哈希表迭代顺序参与
    /// 任何逻辑判断）——这里只用得到集合大小，`HashSet::len()` 本身不
    /// 依赖迭代顺序，风险其实很低，但地形种类数就是个位数量级，
    /// `O(格数 × 种类数)` 的线性扫描完全够用，没有必要为了省这点常数
    /// 就引入一个本项目其余地方刻意回避的容器。
    fn distinct_terrain_kind_count(grid: &ChunkGrid) -> usize {
        let mut seen: Vec<TerrainKind> = Vec::new();
        for kind in collect_terrain(grid) {
            if !seen.contains(&kind) {
                seen.push(kind);
            }
        }
        seen.len()
    }

    #[test]
    fn 正方形且边长为二的幂的世界产出的地形种类数不再退化到个位以下() {
        // 这是本次要修的缺陷本身的回归测试。64×64、128×128、256×256都
        // 是「正方形 + 2 的幂」——TileableNoise 大陆尺度层曾经在这些
        // 尺寸下退化成整图常数（见 noise 模块文档「一个更隐蔽的退化」），
        // 权重最高的一层因此不携带任何空间变化，只剩细节层的小抖动不足
        // 以跨过深水到山地的阈值区间，实测多数种子只产出 1～3 种地形。
        // 这三个尺寸恰好是开局界面最直觉的方形选项，必须覆盖到；用五个
        // 种子取最小值，避免单个种子恰好落在地形种类偏少的巧合区域，
        // 掩盖了退化本身。
        //
        // 阈值取 4（deep_water/shallow_water/sand/grass 至少要出现,
        // 更高地形不强求）：8 种地形的默认阈值表在 4 层倍频、没有粗层
        // 兜底的纯细节噪声下也不一定次次都凑齐山地雪地，但连基本的
        // 水陆过渡都凑不出 4 种，就已经足以证明「几乎全是水」这个描述。
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let sizes = [64u32, 128, 256];
        let seeds = [0u64, 1, 2, 3, 4];

        // Act & Assert
        for side in sizes {
            let world = TorusSize::new(side, side).expect("边长满足整除与视口跨度两条约束");
            let min_kinds = seeds
                .iter()
                .map(|&seed| {
                    let params = GenParams {
                        seed,
                        ..GenParams::default()
                    };
                    let grid = generate_terrain(world, &params, &terrain_ids)
                        .expect("正方形二的幂尺寸满足生成入口的约束");
                    distinct_terrain_kind_count(&grid)
                })
                .min()
                .expect("种子列表非空");
            assert!(
                min_kinds >= 4,
                "{side}x{side} 世界在种子 0..5 中最少只产出 {min_kinds} 种地形，疑似大陆尺度层退化"
            );
        }
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
            shape: TerrainShape {
                sea_level: 400,
                ..TerrainShape::default()
            },
        };
        let high_sea = GenParams {
            seed: 7,
            shape: TerrainShape {
                sea_level: 700,
                ..TerrainShape::default()
            },
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

    /// 测试用区块布局：边长 64（满足 `>=43`、是 16 与 32 的倍数），
    /// 2×1 个区块，凑出一个 128×64 的世界，便于和整图生成结果比较。
    fn test_zone_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(2, 1).expect("2x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    #[test]
    fn 相邻区块窗口生成的地形在共享边界上与整图生成结果一致() {
        // 这是本任务最重要的正确性回归：直接验证设计文档五节「窗口化
        // 调用不需要改这个函数一行」这条论断在区块粒度上真的成立——
        // 逐格比较两个区块窗口拼接的结果与一次性整图生成的结果,而不是
        // 只比较噪声层或只比较边界那一列。
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let layout = test_zone_layout();
        let params = GenParams {
            seed: 99,
            ..GenParams::default()
        };
        let noise = build_zone_noise(&layout, &params).expect("test_zone_layout 满足全部约束");
        let full_world = layout.tile_size();
        let full_grid =
            generate_terrain(full_world, &params, &terrain_ids).expect("128x64 满足生成入口约束");
        let span = layout.zone_span() as i32;

        // Act & Assert：逐个区块窗口与整图对应位置比较。
        for zone_x in 0..layout.zone_count().width() as i32 {
            let zone = layout.zone_count().wrap(zone_x, 0);
            let window = generate_zone_window(&noise, &params, &layout, zone, &terrain_ids)
                .expect("test_zone_layout 满足全部约束");
            for ly in 0..span {
                for lx in 0..span {
                    let local = layout.local_size().wrap(lx, ly);
                    let world_pos = full_world.wrap(zone_x * span + lx, ly);
                    assert_eq!(
                        window.terrain_at(local),
                        full_grid.terrain_at(world_pos),
                        "区块 {zone:?} 局部坐标 ({lx},{ly}) 与整图结果不一致"
                    );
                }
            }
        }
    }

    #[test]
    fn 区块代表地形与该区块窗口左上角地形一致() {
        // zone_representative_terrain 不应该另外实现一套采样逻辑——它
        // 取的必须恰好是 generate_zone_window 会写进局部坐标 (0,0) 的
        // 那一格，两条路径的结果必须逐位相同。
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let layout = test_zone_layout();
        let params = GenParams {
            seed: 7,
            ..GenParams::default()
        };
        let noise = build_zone_noise(&layout, &params).expect("test_zone_layout 满足全部约束");
        let zone = layout.zone_count().wrap(1, 0);
        let window = generate_zone_window(&noise, &params, &layout, zone, &terrain_ids)
            .expect("test_zone_layout 满足全部约束");

        // Act
        let representative =
            zone_representative_terrain(&noise, &params, &layout, zone, &terrain_ids);

        // Assert
        assert_eq!(
            representative,
            window.terrain_at(layout.local_size().wrap(0, 0))
        );
    }
}
