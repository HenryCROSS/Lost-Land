//! 推荐地图尺寸预设（`ll_content::world_identity::RECOMMENDED_PRESETS`
//! 的原始数值）产出的地形多样性回归。
//!
//! # 为什么在 `ll-world` 而不是 `ll-content`
//!
//! 推荐预设的 `SizePreset` 类型定义在 `ll-content`（依赖方向的下游），
//! 但生成地形多样性需要的 `zone_representative_terrain`/
//! `build_zone_noise` 等函数定义在 `ll-world`（依赖方向的上游）——
//! `ll-world` 不能反向依赖 `ll-content`，因此这里直接用与
//! `world_identity::RECOMMENDED_PRESETS` 相同的字面量数值（与
//! `crates/ll-world/src/noise.rs` 里既有的
//! `长方形预设区块数世界本就不触发大陆尺度层退化` 测试同一种重复方式），
//! 不引入跨方向依赖。两处数值必须保持同步，这是本文件与
//! `world_identity.rs` 之间的一处刻意重复，非疏漏。
//!
//! # 为什么按区块代表点采样，不生成整张地图
//!
//! 「浩瀚」预设有 96×64=6144 个区块，逐瓦片生成整张地图（12288×8192
//! 格）对一条单元测试而言开销过大。[`zone_representative_terrain`]
//! 只采样每个区块左上角一点，用于「大致地形分布」场景（见其文档），
//! 恰好是本测试需要的粒度——多样性只关心"出现过哪些地形种类"，不需要
//! 每一格的精确值。

use ll_core::torus::TorusSize;
use ll_world::generate::{GenParams, build_zone_noise, zone_representative_terrain};
use ll_world::terrain::{TerrainKind, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 与 `ll_content::world_identity::RECOMMENDED_PRESETS` 同步的字面量
/// （标签、区块边长、区块数）——见模块文档「为什么在 `ll-world` 而不是
/// `ll-content`」。
const PRESETS: &[(&str, u32, (u32, u32))] = &[
    ("小陆地", 128, (32, 24)),
    ("标准", 128, (48, 32)),
    ("广阔", 128, (64, 48)),
    ("浩瀚", 128, (96, 64)),
];

/// 一个预设、一个种子跑出的全部区块代表地形，去重后的种类数。
///
/// 用线性扫描去重而非 `HashSet`——与 `generate.rs`
/// `distinct_terrain_kind_count` 同一条理由（C5：规避
/// `HashMap`/`HashSet` 迭代顺序参与逻辑判断；地形种类数是个位数量级，
/// 线性扫描的常数代价可忽略）。
fn distinct_kind_count(layout: &ZoneLayout, seed: u64) -> usize {
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    let noise = build_zone_noise(layout, &params).expect("预设尺寸恒能构造合法噪声源");
    let (terrain_ids, _table) = base_terrain_fixture();
    let zone_count = layout.zone_count();

    let mut seen: Vec<TerrainKind> = Vec::new();
    for zy in 0..zone_count.height() as i32 {
        for zx in 0..zone_count.width() as i32 {
            let zone = zone_count.wrap(zx, zy);
            let kind = zone_representative_terrain(&noise, &params, layout, zone, &terrain_ids);
            if !seen.contains(&kind) {
                seen.push(kind);
            }
        }
    }
    seen.len()
}

/// 多样性阈值——现有 8 种地表地形（深水/浅水/沙地/草地/森林/丘陵/山地/
/// 雪地，见 `height_to_terrain`）里，一张覆盖多个大陆尺度格点的地图
/// 理应见到不止一种极端值（不能只有水，也不能只有陆地）。取 4 是比
/// 「不退化成单点常数」（阈值 1）明显更强、又不会因为某个种子恰好没
/// 采到雪地/深水这类稀有地形而误报的折中值。
const DIVERSITY_THRESHOLD: usize = 4;

#[test]
fn 每个推荐预设在多个种子下产出的地形多样性不低于阈值() {
    // Arrange & Act & Assert
    for &(label, zone_span, zone_count) in PRESETS {
        let layout = ZoneLayout::new(
            zone_span,
            TorusSize::new(zone_count.0, zone_count.1).expect("预设区块数恒合法"),
        )
        .expect("预设恒满足 ZoneLayout 构造约束");

        for seed in [1u64, 42, 20_260_819] {
            let count = distinct_kind_count(&layout, seed);
            assert!(
                count >= DIVERSITY_THRESHOLD,
                "预设 {label}（种子 {seed}）只产出 {count} 种地形，低于阈值 {DIVERSITY_THRESHOLD}"
            );
        }
    }
}
