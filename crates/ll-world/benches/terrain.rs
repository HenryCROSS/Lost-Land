//! 地形属性查询的性能基准——ADR 0017「迁入注册表后不得变慢」的验收
//! 证据。
//!
//! `TerrainKind` 从硬编码 `match`（P4 Task 8 之前）迁成了按
//! [`ll_core::ident::ContentIndex`] 索引的列式表（[`TerrainTable`]）。
//! ADR 0017 的论断是「消掉分支是净赚，不是打平」——本文件不满足于
//! 直觉判断，而是把迁移前的 `match` 实现原样复刻在本文件内部
//! （`legacy_move_cost`，**只供基准对照使用，不是生产代码**），与
//! 迁移后的真实实现跑同一套访问模式，实测两者差异。
//!
//! 两组基准都遍历同一个按固定顺序循环的地形序列（模拟逐格扫描时
//! 「同一批地形反复出现」的真实访问模式），求和它们的移动代价——
//! 求和是为了防止编译器把整个循环当死代码优化掉，而不是这个基准真正
//! 关心的数值本身。

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ll_world::terrain::{TerrainKind, TerrainTable, base_terrain_fixture};

/// 迁移前 `TerrainKind::move_cost` 的原样复刻（`crates/ll-world/src/terrain.rs`
/// 迁移前版本，见 git 历史）：先按 `blocks_move` 的 `match` 分支判断是
/// 否不可通行，再按具体地形种类查第二个 `match`。**仅供本文件基准
/// 对照使用**——迁移后的生产代码不应再出现这种写法。
fn legacy_blocks_move(raw: u16) -> bool {
    matches!(raw, 0 | 102 | 103 | 104 | 106)
}

/// 见 [`legacy_blocks_move`] 文档。
fn legacy_move_cost(raw: u16) -> u32 {
    if legacy_blocks_move(raw) {
        return u32::MAX;
    }
    match raw {
        1 => 200,
        2 => 120,
        4 => 150,
        5 => 150,
        6 => 400,
        7 => 150,
        107 | 108 => 150,
        _ => 100,
    }
}

/// 旧版 17 个地形的原始 `u16` 编号（自然地形 0..8，建筑地形
/// 100..109），顺序与新版 [`base_terrain_fixture`] 的注册顺序一一对应
/// ——两组基准循环遍历同一条逻辑序列，只是新版走数组下标、旧版走
/// `match`，保证是公平的同访问模式对照。
const LEGACY_RAW_KINDS: [u16; 17] = [
    0, 1, 2, 3, 4, 5, 6, 7, 100, 101, 102, 103, 104, 105, 106, 107, 108,
];

/// 单次基准迭代内的访问次数——远大于地形种类数（17），让循环里真正
/// 有反复访问同一批分支/下标的负载,而不是一次性访问就结束。
const SWEEP_LEN: usize = 100_000;

fn 地形移动代价基准(c: &mut Criterion) {
    let (ids, table) = base_terrain_fixture();
    let kinds: [TerrainKind; 17] = [
        ids.deep_water,
        ids.shallow_water,
        ids.sand,
        ids.grass,
        ids.forest,
        ids.hill,
        ids.mountain,
        ids.snow,
        ids.floor_wood,
        ids.floor_stone,
        ids.wall_wood,
        ids.wall_stone,
        ids.door_closed,
        ids.door_open,
        ids.window,
        ids.stairs_up,
        ids.stairs_down,
    ];

    c.bench_function("table_move_cost_sweep", |bencher| {
        bencher.iter(|| sweep_table(black_box(&kinds), black_box(&table)))
    });

    c.bench_function("legacy_match_move_cost_sweep", |bencher| {
        bencher.iter(|| sweep_legacy(black_box(&LEGACY_RAW_KINDS)))
    });
}

/// 迁移后：按 `TerrainTable` 查表，逐个累加 `move_cost`。
fn sweep_table(kinds: &[TerrainKind; 17], table: &TerrainTable) -> u64 {
    let mut sum: u64 = 0;
    for i in 0..SWEEP_LEN {
        let kind = kinds[i % kinds.len()];
        sum += u64::from(kind.move_cost(table));
    }
    sum
}

/// 迁移前：按 `match` 分支判断，逐个累加 `move_cost`。
fn sweep_legacy(raw_kinds: &[u16; 17]) -> u64 {
    let mut sum: u64 = 0;
    for i in 0..SWEEP_LEN {
        let raw = raw_kinds[i % raw_kinds.len()];
        sum += u64::from(legacy_move_cost(raw));
    }
    sum
}

criterion_group!(benches, 地形移动代价基准);
criterion_main!(benches);
