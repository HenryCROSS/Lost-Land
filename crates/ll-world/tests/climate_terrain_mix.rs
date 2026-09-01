//! 气候条带对**整张世界地形构成**的影响：逐格数一遍，钉住「气候只调制
//! 一段高度带」这条设计裁定。
//!
//! # 为什么是测试而不是一个 `probe_*` example
//!
//! 本批次需要一组「改前/改后」的地形占比数字。仓库里没有现成的地形占比
//! 测量工具（`probe_conquest` 只测战争结局），第一版因此写了一个
//! `probe_climate` example——**那是错的取舍**：当时的门禁
//! （`scripts/ci/run_acceptance_demos.sh`）要求每个 example 都被显式登记
//! 进 RUN_LIST/SKIP_LIST，一次性的测量却要因此长期维护一条 demo；而且
//! example 只会打印，不会断言，下一次有人改坏气候分带时它一声不吭。
//!
//! **这个判断在 2026-08-29 被所有者裁定推到了尽头**：全部 `examples/`
//! 删除，门禁改形为「工作区一个 example target 都不许有」
//! （`scripts/ci/check_no_examples.sh`，见 ADR 0030）。本文件当初选对了
//! 落点，因此这次一个字都不用动。
//!
//! 写成测试之后，同一批数字变成了**断言**：
//!
//! - 「改前」那一列由 `climate_band_width = 0` 产出。这不是近似基线——
//!   0 是**精确恒等**（`ll_world::climate::band_from_warmth` 用严格
//!   不等号，带宽为零时两条判据恒假、整图温带），这条恒等性由
//!   `ll_world::generate` 的
//!   `气候带宽为零时整张地形与气候条带落地之前逐格相同` 钉死。
//! - 数字本身用 `cargo test -p ll-world --test climate_terrain_mix -- --nocapture`
//!   打出来，所以它同时还是那个测量工具，只是不用再养一条 example。

use ll_world::climate::{ClimateBand, band_at};
use ll_world::generate::{GenParams, TerrainShape, generate_terrain};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 测量用的世界尺寸：与 `p2_acceptance` 演示世界一致（512×320）。够大到
/// 每条气候带都完整出现，又小到三个种子几秒钟跑得完。
const WORLD_WIDTH: u32 = 512;
/// 测量用的世界高度，理由同 [`WORLD_WIDTH`]。
const WORLD_HEIGHT: u32 = 320;

/// 测量用的三个种子——与当时的 `crates/ll-game/examples/probe_conquest.rs`
/// 用的是同一批，这样「地形怎么变了」与「战争结局怎么变了」说的是同一批
/// 世界。（那个一次性排查探针已于 2026-08-29 随所有者裁定删除，见
/// ADR 0030；这三个数值留在这里，是为了让将来任何一次重测仍与当初那批
/// 数字可比。）
///
/// **〔2026-08-31，批次 24〕「战争结局怎么变了」那一半现在也有落点了**：
/// `crates/ll-game/tests/conquest_mix.rs` 照本文件的做法把那个探针改写
/// 成了一条会断言的测试，并且**复用这里这三个种子**（它的 `SEEDS` 与本
/// 常量逐字相同，理由就是上面那句话）。改种子的话两边要一起改，否则
/// 「说的是同一批世界」这条前提当场失效。
const SEEDS: [u64; 3] = [20260826, 7, 99];

/// 逐格统计要分的那几类地形，顺序固定（数组字面量，不经任何哈希容器），
/// 符合约束 C5。
fn terrain_labels(ids: &BaseTerrainIds) -> [(&'static str, TerrainKind); 10] {
    [
        ("深水", ids.deep_water),
        ("浅水", ids.shallow_water),
        ("海滩", ids.sand),
        ("草地", ids.grass),
        ("沙漠", ids.desert),
        ("冻原", ids.tundra),
        ("森林", ids.forest),
        ("丘陵", ids.hill),
        ("山地", ids.mountain),
        ("雪地", ids.snow),
    ]
}

/// 用给定带宽把三个种子的世界各生成一遍，返回十类地形各自的**总格数**
/// （下标与 [`terrain_labels`] 一致）。
fn count_terrain(band_width: i32) -> [u64; 10] {
    let world = ll_core::torus::TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT)
        .expect("512x320 是合法的世界尺寸");
    let (ids, _table) = ll_world::terrain::base_terrain_fixture();
    let labels = terrain_labels(&ids);
    let mut counts = [0u64; 10];

    for seed in SEEDS {
        let params = GenParams {
            seed,
            shape: TerrainShape {
                climate_band_width: band_width,
                ..TerrainShape::default()
            },
        };
        let grid = generate_terrain(world, &params, &ids).expect("512x320 满足生成入口的约束");
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                let kind = grid.terrain_at(world.wrap(x, y));
                if let Some(slot) = labels.iter().position(|(_, id)| *id == kind) {
                    counts[slot] += 1;
                }
            }
        }
    }
    counts
}

fn permille(part: u64, whole: u64) -> u64 {
    (part * 1000).checked_div(whole).unwrap_or(0)
}

#[test]
fn 气候条带只改草地那一段其余七类地形逐格不动() {
    // 这是「气候只调制海岸带以上第一段陆地」这条设计裁定
    // （`docs/superpowers/plans/2026-08-27-batch3-climate-bands.md` D3）的
    // 可执行版本，也是本批次「下游影响」那组数字的来源。
    //
    // 断言用的是**精确相等**而不是「差不多」：气候是纬度的函数、高度
    // 阈值链其余七段一个字没改，水域/海滩/森林/丘陵/山地/雪地的格数就
    // 必须**逐格**相同。任何一格的差异都意味着调制泄漏到了别的高度段。
    //
    // 反例（本次开发实跑）：把 `height_to_terrain` 的森林那一段也改成
    // 按气候带三选一，本条报「森林 在带宽 0 与 250 下格数不同」。
    // Arrange
    let (ids, _table) = ll_world::terrain::base_terrain_fixture();
    let labels = terrain_labels(&ids);

    // Act
    let before = count_terrain(0);
    let after = count_terrain(TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH);
    let total: u64 = before.iter().sum();

    println!(
        "世界 {WORLD_WIDTH}x{WORLD_HEIGHT}，种子 {SEEDS:?}，单位千分比\n\
         带宽\t{}",
        labels
            .iter()
            .map(|(label, _)| *label)
            .collect::<Vec<_>>()
            .join("\t")
    );
    for (width, counts) in [
        (0, &before),
        (TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH, &after),
    ] {
        let row: Vec<String> = counts
            .iter()
            .map(|count| permille(*count, total).to_string())
            .collect();
        println!("{width}\t{}", row.join("\t"));
    }

    // Assert：草地/沙漠/冻原三项之外，其余七类逐格相同。
    for (slot, (label, _)) in labels.iter().enumerate() {
        if matches!(*label, "草地" | "沙漠" | "冻原") {
            continue;
        }
        assert_eq!(
            before[slot],
            after[slot],
            "{label} 在带宽 0 与 {} 下格数不同（{} → {}）——气候调制泄漏到了别的高度段",
            TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH,
            before[slot],
            after[slot]
        );
    }
}

#[test]
fn 草地那一段被原样切成草地加沙漠加冻原一格不多一格不少() {
    // 上一条证明「别的段没被碰」，这一条证明「被碰的那一段是**重新
    // 分配**而不是增删」：气候不改变任何一格的高度，只改变那一段该叫
    // 什么地形，所以三者之和必须精确等于原来的草地格数。
    //
    // 反例（本次开发实跑）：把干热带那一支从 `terrain_ids.desert` 改成
    // `terrain_ids.sand`（即「复用海岸沙地」那个被推翻的旧方案），本条
    // 报守恒等式不成立——沙漠那一列恒为 0，缺口跑进了海滩。
    // Arrange
    let (ids, _table) = ll_world::terrain::base_terrain_fixture();
    let labels = terrain_labels(&ids);
    let grass = labels
        .iter()
        .position(|(l, _)| *l == "草地")
        .expect("有草地");
    let desert = labels
        .iter()
        .position(|(l, _)| *l == "沙漠")
        .expect("有沙漠");
    let tundra = labels
        .iter()
        .position(|(l, _)| *l == "冻原")
        .expect("有冻原");

    // Act
    let before = count_terrain(0);
    let after = count_terrain(TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH);

    // Assert
    assert_eq!(before[desert], 0, "带宽为零时不该有任何一格沙漠");
    assert_eq!(before[tundra], 0, "带宽为零时不该有任何一格冻原");
    assert!(after[desert] > 0, "默认带宽下应当真的生成出沙漠");
    assert!(after[tundra] > 0, "默认带宽下应当真的生成出冻原");
    assert!(
        after[grass] < before[grass],
        "草地被切走了一部分，总量必须减少"
    );
    assert_eq!(
        before[grass],
        after[grass] + after[desert] + after[tundra],
        "草地那一段是被重新分配，不是被增删——三者之和必须精确等于原来的草地格数"
    );
}

#[test]
fn 三条气候带按带宽切分纬度且温带占一半() {
    // 各气候带占**纬度**的比例只由带宽决定，与地形、种子都无关——这条
    // 把默认带宽 250‰ 的语义（干热带与极地带各占 25%、温带占 50%）钉
    // 成可执行的。三角波是线性的，所以比例就是带宽本身。
    //
    // 反例（本次开发实跑）：把 `warmth_at` 的三角波改成「前半周期直接
    // 归零」这类非线性形状，本条报温带占比不再是 500‰ 附近。
    // Arrange
    let height = WORLD_HEIGHT;
    let width = TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH;

    // Act
    let mut hot = 0u64;
    let mut temperate = 0u64;
    let mut polar = 0u64;
    for y in 0..height as i32 {
        match band_at(y, height, width) {
            ClimateBand::Hot => hot += 1,
            ClimateBand::Temperate => temperate += 1,
            ClimateBand::Polar => polar += 1,
        }
    }
    let total = u64::from(height);

    // Assert：允许 ±10‰ 的离散化误差（世界高度有限，条带边界不一定
    // 正好落在整格上）。
    assert!(
        permille(hot, total).abs_diff(u64::try_from(width).expect("带宽非负")) <= 10,
        "干热带占纬度 {}‰，应当接近带宽 {width}‰",
        permille(hot, total)
    );
    assert!(
        permille(polar, total).abs_diff(u64::try_from(width).expect("带宽非负")) <= 10,
        "极地带占纬度 {}‰，应当接近带宽 {width}‰",
        permille(polar, total)
    );
    assert!(
        permille(temperate, total).abs_diff(1000 - 2 * u64::try_from(width).expect("带宽非负"))
            <= 20,
        "温带占纬度 {}‰，应当接近 {}‰",
        permille(temperate, total),
        1000 - 2 * width
    );
}
