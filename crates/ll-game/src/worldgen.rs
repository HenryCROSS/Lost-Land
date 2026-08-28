//! 把玩家在 `config.json5` 里写的新游戏设置解析成真正的地形生成参数。
//!
//! # 这个模块补的是「第 33 处声明了但从没接线」
//!
//! `ll_world::generate::TerrainShape` 的三个阈值字段（海平面 / 山地
//! 阈值 / 倍频层数）从地形生成落地那天起就在类型上可调，也一直有一条
//! 「海平面调高会增加水域格数」的单元测试证明机制真的有效——但全部
//! 生产路径清一色 `GenParams::default()`，没有任何入口能真正调到它们。
//! 项目所有者实测报告「235 个据点里 117 个靠水，渔夫成了最常见的职业」，
//! 根因正是那个从来没人能改的默认海平面。本模块是那条缺失的通路。
//!
//! # 为什么解析住在 `ll-game` 而不是配置文件所在的 `ll-platform`
//!
//! 依赖方向：`ll-platform` 不依赖 `ll-world`/`ll-content`（见
//! `ll_platform::config` 模块文档），它只能装 `String`/`Option<i32>`
//! 这类原始值。「`"archipelago"` 是哪一组阈值」需要同时看见
//! `ll_content::world_identity::TERRAIN_PRESETS` 与
//! `ll_world::generate::TerrainShape`——`ll-game` 是最上层、两者都看得
//! 见的那一层，也是唯一一层。
//!
//! # 不可信输入的处理纪律：记日志、退回、绝不 panic
//!
//! `config.json5` 是玩家手改的明文文件，随时可能写着不认识的预设名或
//! 越界的阈值。本模块对这两类问题一视同仁：记一条**说清楚发生了什么
//! 以及退回到了哪里**的警告日志，然后退回上一层合法值——不认识的预设
//! 名退回默认预设，越界的覆盖值退回该预设原本的形态。这与
//! `ll_platform::config::load_or_default` 对损坏配置的处理是同一条
//! 纪律：一个游戏因为配置文件写错一个数字就打不开，比忽略那个数字更糟。

use ll_content::world_identity::{DEFAULT_TERRAIN_PRESET_ID, terrain_preset};
use ll_platform::config::NewGameConfig;
use ll_world::generate::{GenParams, TerrainShape};

/// 新游戏使用的默认地形种子——本体目前没有开局选择种子的界面（P7），
/// 玩家没在 `config.json5` 里指定时固定用这个值，保证「同一份构建反复
/// 运行产出同一个世界」，便于开发期复现问题。
pub const DEFAULT_SEED: u64 = 20_260_820;

/// 把新游戏配置解析成地形生成参数。
///
/// 三步，每一步失败都只退回上一步的结果并记日志（见模块文档「不可信
/// 输入的处理纪律」）：
///
/// 1. 按 [`NewGameConfig::terrain_preset`] 查预设表，查不到退回默认预设。
/// 2. 把四个 `Option` 覆盖项逐项盖上去。
/// 3. 校验合成结果（[`TerrainShape::validate`]），不合法则整组退回第
///    一步得到的预设形态——**不是退回全局默认**：玩家选的那档预设本身
///    没有错，错的是他手写的那几个覆盖值，退回预设比退回默认更贴近他
///    的意图。
pub fn resolve_gen_params(config: &NewGameConfig) -> GenParams {
    let preset = match terrain_preset(&config.terrain_preset) {
        Some(preset) => preset,
        None => {
            let fallback = terrain_preset(DEFAULT_TERRAIN_PRESET_ID)
                .expect("默认预设标识必然在预设表里，由 ll-content 的测试钉死");
            tracing::warn!(
                requested = config.terrain_preset,
                fallback = fallback.id,
                "配置里的地形预设标识不认识，退回默认预设"
            );
            fallback
        }
    };

    let overridden = apply_overrides(preset.shape, config);
    let shape = match overridden.validate() {
        Ok(()) => overridden,
        Err(reason) => {
            tracing::warn!(
                preset = preset.id,
                %reason,
                "配置里的地形参数覆盖值不合法，整组退回该预设原本的形态"
            );
            preset.shape
        }
    };

    let seed = config.seed.unwrap_or(DEFAULT_SEED);
    tracing::info!(
        preset = preset.id,
        seed,
        sea_level = shape.sea_level,
        mountain_level = shape.mountain_level,
        octaves = shape.octaves,
        continent_shrink = shape.continent_shrink,
        climate_band_width = shape.climate_band_width,
        "新世界的地形生成参数已确定"
    );
    GenParams { seed, shape }
}

/// 把配置里写了的那几项盖到预设形态上，没写的原样保留。
///
/// 拆成独立函数只是为了让 [`resolve_gen_params`] 的三步流程一眼看得
/// 完；这里是纯粹的逐字段 `unwrap_or`，没有任何判断逻辑。返回新值而
/// 不是原地改写（本仓库的不可变纪律）。
fn apply_overrides(base: TerrainShape, config: &NewGameConfig) -> TerrainShape {
    TerrainShape {
        sea_level: config.sea_level.unwrap_or(base.sea_level),
        mountain_level: config.mountain_level.unwrap_or(base.mountain_level),
        octaves: config.octaves.unwrap_or(base.octaves),
        continent_shrink: config.continent_shrink.unwrap_or(base.continent_shrink),
        climate_band_width: config.climate_band_width.unwrap_or(base.climate_band_width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_content::world_identity::TERRAIN_PRESETS;

    #[test]
    fn 默认配置解析出的参数与地形形态默认值逐位相同() {
        // 这是两条黄金基准（determinism.rs 的 EXPECTED_WORLD_DIGEST 与
        // replay.rs 的 EXPECTED_REPLAY_DIGEST）所依赖的前提：没碰过
        // 配置文件的玩家开出来的必须还是原来那张地图。
        // Arrange
        let config = NewGameConfig::default();

        // Act
        let params = resolve_gen_params(&config);

        // Assert
        assert_eq!(params.shape, TerrainShape::default());
        assert_eq!(params.seed, DEFAULT_SEED);
    }

    #[test]
    fn 配置默认预设标识在预设表里查得到() {
        // ll-platform 不能依赖 ll-content，NewGameConfig 的默认预设名
        // 因此是一处刻意的字面量重复（见其 default_terrain_preset 文档）。
        // 这条测试是那两处不会分叉的唯一保证。
        // Arrange
        let config = NewGameConfig::default();

        // Act
        let found = terrain_preset(&config.terrain_preset);

        // Assert
        assert!(found.is_some(), "配置默认预设标识与预设表已经分叉");
        assert_eq!(config.terrain_preset, DEFAULT_TERRAIN_PRESET_ID);
    }

    #[test]
    fn 每一档预设都能被自己的标识解析出来() {
        // Arrange & Act & Assert
        for preset in TERRAIN_PRESETS {
            let config = NewGameConfig {
                terrain_preset: preset.id.to_string(),
                ..NewGameConfig::default()
            };
            assert_eq!(
                resolve_gen_params(&config).shape,
                preset.shape,
                "预设 {} 没能按标识解析回它自己的形态",
                preset.id
            );
        }
    }

    #[test]
    fn 不认识的预设标识退回默认预设而不panic() {
        // Arrange
        let config = NewGameConfig {
            terrain_preset: "玩家手滑打错的名字".to_string(),
            ..NewGameConfig::default()
        };

        // Act
        let params = resolve_gen_params(&config);

        // Assert
        assert_eq!(params.shape, TerrainShape::default());
    }

    #[test]
    fn 逐项覆盖只改写下的那一项其余取预设值() {
        // Arrange：选群岛预设，只覆盖海平面。
        let archipelago = terrain_preset("archipelago").expect("群岛预设存在");
        let config = NewGameConfig {
            terrain_preset: "archipelago".to_string(),
            sea_level: Some(500),
            ..NewGameConfig::default()
        };

        // Act
        let shape = resolve_gen_params(&config).shape;

        // Assert
        assert_eq!(shape.sea_level, 500);
        assert_eq!(shape.mountain_level, archipelago.shape.mountain_level);
        assert_eq!(shape.octaves, archipelago.shape.octaves);
        assert_eq!(shape.continent_shrink, archipelago.shape.continent_shrink);
    }

    #[test]
    fn 越界的覆盖值整组退回该预设而不是退回全局默认() {
        // 「退回预设」而不是「退回全局默认」是本模块的一处刻意取舍
        // （见 resolve_gen_params 文档第三步）：玩家选的那档预设本身
        // 没错。若实现改成退回 TerrainShape::default()，这条会红。
        // Arrange
        let highland = terrain_preset("highland").expect("山地预设存在");
        let config = NewGameConfig {
            terrain_preset: "highland".to_string(),
            sea_level: Some(-1),
            ..NewGameConfig::default()
        };

        // Act
        let shape = resolve_gen_params(&config).shape;

        // Assert
        assert_eq!(shape, highland.shape);
        assert_ne!(shape, TerrainShape::default());
    }

    #[test]
    fn 山地阈值与海平面挨得太近时被拒绝() {
        // TerrainShape::validate 的 MIN_LEVEL_GAP 那条分支——两个值
        // 各自都在 0..=1000 之内，只有它们的差不合法。
        // Arrange
        let config = NewGameConfig {
            sea_level: Some(600),
            mountain_level: Some(700),
            ..NewGameConfig::default()
        };

        // Act
        let shape = resolve_gen_params(&config).shape;

        // Assert
        assert_eq!(shape, TerrainShape::default());
    }

    #[test]
    fn 气候条带带宽越界时被拒绝() {
        // TerrainShape::validate 的 MAX_CLIMATE_BAND_WIDTH 那条分支：
        // 干热带与极地带各占一侧带宽，两侧合计超过全部纬度之后温带就被
        // 挤没了，地形分带里「温带那一支」永远走不到。
        //
        // 反例（本次开发实跑）：删掉 validate 里那条分支，本条报
        // `assertion `left == right` failed`——退回的形态里带宽还是 501。
        // Arrange
        let config = NewGameConfig {
            climate_band_width: Some(TerrainShape::MAX_CLIMATE_BAND_WIDTH + 1),
            ..NewGameConfig::default()
        };

        // Act
        let shape = resolve_gen_params(&config).shape;

        // Assert
        assert_eq!(shape, TerrainShape::default());
    }

    #[test]
    fn 配置里写零可以关掉气候条带() {
        // `0` 不是「没写」——`NewGameConfig::climate_band_width` 是
        // `Option`，`Some(0)` 必须真的把气候条带关掉（整图温带），而不是
        // 被当成缺省值忽略掉。这条同时是「玩家想要一个没有气候条带的
        // 世界」这个用例的接线证明。
        //
        // 反例（本次开发实跑）：把 apply_overrides 里那一支写成
        // `config.climate_band_width.filter(|w| *w > 0).unwrap_or(..)`，
        // 本条报 250 != 0。
        // Arrange
        let config = NewGameConfig {
            climate_band_width: Some(0),
            ..NewGameConfig::default()
        };

        // Act
        let shape = resolve_gen_params(&config).shape;

        // Assert
        assert_eq!(shape.climate_band_width, 0);
        assert_ne!(shape, TerrainShape::default());
    }

    #[test]
    fn 配置里写了种子时用玩家的种子() {
        // Arrange
        let config = NewGameConfig {
            seed: Some(777),
            ..NewGameConfig::default()
        };

        // Act & Assert
        assert_eq!(resolve_gen_params(&config).seed, 777);
    }
}
