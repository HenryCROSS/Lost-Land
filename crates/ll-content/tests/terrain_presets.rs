//! 四档地形形态预设（`ll_content::world_identity::TERRAIN_PRESETS`）
//! 的**实测**性质回归：每个预设的名字必须与它真正产出的世界对得上。
//!
//! # 为什么这条测试住在 `ll-content`
//!
//! 它要同时看见两样东西：预设表本身（`ll-content`）与地形生成
//! （`ll-world`）。依赖方向是 `ll-content → ll-world`，因此 `ll-content`
//! 是唯一能同时看见两者的地方。反过来把测试放进 `ll-world` 就只能把
//! 四组数字再抄一遍——`crates/ll-world/tests/noise_presets.rs` 模块文档
//! 已经如实记录过一次那种被依赖方向逼出来的重复，没有理由主动再造第二处。
//!
//! # 为什么断言的是「区间」而不是「等于某个数」
//!
//! 逐位钉死实测均值会让这条测试变成第三条黄金基准：任何人调一档预设
//! 的数值都得回来改一串小数，而它本来就该是可调的（项目所有者：「先做
//! 一份，以后我再调」）。这里断言的是**名字成立所必需的性质**——群岛
//! 的水必须显著多于大陆、山地的山必须显著多于大陆、内陆的水必须显著
//! 少于大陆、群岛的陆地必须真的碎成很多块。区间给得宽，但每一条都能
//! 被「把某档预设改成另一档的数值」这个反例打红（见下方每条测试的
//! 「反例」注释）。
//!
//! # 采样方式
//!
//! 本体「标准」尺寸（96×64 区块 = 4608×3072 格）下按 [`STRIDE`] 格
//! 抽样。不逐格遍历：一千四百万格 × 四档预设 × 两个种子在调试构建下
//! 要跑很久，而水陆比例是一个空间统计量，等距抽样对它是无偏的——
//! 抽出来的 576×384 = 22 万个样本足以把比例的抽样误差压到零点几个
//! 百分点，远小于本文件断言的区间宽度。

use ll_content::world_identity::{TERRAIN_PRESETS, TerrainPreset, terrain_preset};
use ll_core::torus::TorusSize;
use ll_world::generate::{GenParams, build_zone_noise, terrain_at_tile};
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 抽样步长（格）。取 8：小于本体最细噪声格子边长（`CELL_SIZE` = 16）
/// 的一半，因此不会因为步长与噪声周期共振而系统性地偏向某一档高度；
/// 同时把样本数压到 22 万，调试构建下每档预设一秒内跑完。
const STRIDE: i32 = 8;

/// 本体「标准」尺寸预设的区块数，与
/// `ll_content::world_identity::RECOMMENDED_PRESETS` 里 `标准` 那一档
/// 一致——四档地形预设的公开实测数据都是在这个尺寸下取得的。
const STANDARD_ZONE_COUNT: (u32, u32) = (96, 64);

/// 用来取均值的种子。两个就够：本文件断言的是区间而不是精确值，而
/// 单个种子的水域比例在默认预设下实测跨度约十个百分点（见
/// `knowledge/design/worldgen-parameters.md`），两个种子取均值已经能把这份抖动压进
/// 断言区间；再多只是让这条测试更慢。
const SEEDS: [u64; 2] = [20_260_820, 7717];

/// 一次抽样得到的、玩家能直接感知的三项性质。
struct Measured {
    /// 水域（深水 + 浅水）占全图的比例，千分比。
    water_permille: u32,
    /// 山地 + 雪地占全图的比例，千分比。
    mountain_permille: u32,
    /// 最大的一块连通陆地占全部陆地的比例，千分比——**碎不碎的判据**。
    /// 一整块大陆的世界这个值接近 1000；真正的群岛则很小。
    largest_landmass_permille: u32,
}

/// 按抽样网格量出一个预设在一个种子下的三项性质。
///
/// 全程整数千分比，与地形生成本身同一套表达（ADR 0020）——这里本可以
/// 用浮点（测试断言不流回世界状态，属于 ADR 0020 的甲区），但既然
/// 被测对象的全部阈值都是千分比整数，断言也用同一套单位读起来不必来回
/// 换算。
fn measure(preset: &TerrainPreset, seed: u64, ids: &BaseTerrainIds) -> Measured {
    let zone_count = TorusSize::new(STANDARD_ZONE_COUNT.0, STANDARD_ZONE_COUNT.1)
        .expect("标准尺寸预设的区块数恒合法");
    let layout = ZoneLayout::new(48, zone_count).expect("区块边长 48 满足全部对齐与跨度约束");
    let params = GenParams {
        seed,
        shape: preset.shape,
    };
    let noise = build_zone_noise(&layout, &params).expect("标准尺寸恒能构造合法噪声源");
    let size = layout.tile_size();

    let cols = size.width() as i32 / STRIDE;
    let rows = size.height() as i32 / STRIDE;
    let total = (cols * rows) as u64;

    let mut is_land = vec![false; (cols * rows) as usize];
    let mut water = 0u64;
    let mut mountain = 0u64;
    for row in 0..rows {
        for col in 0..cols {
            let pos = size.wrap(col * STRIDE, row * STRIDE);
            let kind = terrain_at_tile(&noise, &params, pos, ids);
            if kind == ids.deep_water || kind == ids.shallow_water {
                water += 1;
            } else {
                is_land[(row * cols + col) as usize] = true;
                if kind == ids.mountain || kind == ids.snow {
                    mountain += 1;
                }
            }
        }
    }

    Measured {
        water_permille: permille(water, total),
        mountain_permille: permille(mountain, total),
        largest_landmass_permille: largest_landmass_permille(&is_land, cols, rows),
    }
}

/// `part / whole` 的千分比，四舍五入到最近的整数。
fn permille(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    ((part * 2000 / whole) + 1) as u32 / 2
}

/// 在抽样网格上做四邻连通域分析（环面环绕，与世界本身的拓扑一致），
/// 返回最大的一块陆地占全部陆地的千分比。
///
/// 并查集用数组实现，不涉及任何 `HashMap`/`HashSet`（约束 C5）。
fn largest_landmass_permille(is_land: &[bool], cols: i32, rows: i32) -> u32 {
    let index =
        |x: i32, y: i32| -> usize { (y.rem_euclid(rows) * cols + x.rem_euclid(cols)) as usize };
    let mut parent: Vec<u32> = (0..is_land.len() as u32).collect();
    fn root(parent: &mut [u32], mut node: u32) -> u32 {
        while parent[node as usize] != node {
            parent[node as usize] = parent[parent[node as usize] as usize];
            node = parent[node as usize];
        }
        node
    }

    for y in 0..rows {
        for x in 0..cols {
            if !is_land[index(x, y)] {
                continue;
            }
            // 只看右邻与下邻：环面上每条边都会被它的一个端点看到一次，
            // 四个方向全看是重复劳动。
            for (dx, dy) in [(1, 0), (0, 1)] {
                if !is_land[index(x + dx, y + dy)] {
                    continue;
                }
                let a = root(&mut parent, index(x, y) as u32);
                let b = root(&mut parent, index(x + dx, y + dy) as u32);
                if a != b {
                    parent[a as usize] = b;
                }
            }
        }
    }

    let mut sizes = vec![0u64; is_land.len()];
    let mut land_total = 0u64;
    for node in 0..is_land.len() as u32 {
        if is_land[node as usize] {
            let r = root(&mut parent, node);
            sizes[r as usize] += 1;
            land_total += 1;
        }
    }
    permille(sizes.into_iter().max().unwrap_or(0), land_total)
}

/// 一档预设在 [`SEEDS`] 上的三项性质均值。
fn averaged(preset: &TerrainPreset) -> Measured {
    let (ids, _table) = base_terrain_fixture();
    let samples: Vec<Measured> = SEEDS.iter().map(|&s| measure(preset, s, &ids)).collect();
    let n = samples.len() as u32;
    Measured {
        water_permille: samples.iter().map(|m| m.water_permille).sum::<u32>() / n,
        mountain_permille: samples.iter().map(|m| m.mountain_permille).sum::<u32>() / n,
        largest_landmass_permille: samples
            .iter()
            .map(|m| m.largest_landmass_permille)
            .sum::<u32>()
            / n,
    }
}

fn preset_by_id(id: &str) -> &'static TerrainPreset {
    terrain_preset(id).unwrap_or_else(|| panic!("预设表里应当有 {id} 这一档"))
}

#[test]
fn 大陆预设产出一整块大陆且水域约占三分之一() {
    // 「大陆」这个名字要成立，必须同时满足两件事：水没多到把陆地淹散，
    // 且陆地真的是**一整块**（最大连通陆块占绝大部分陆地）。
    //
    // 反例（已实跑验证会红）：把这一档的 shape 换成群岛那一档的数值，
    // 水域实测冲到 726‰、最大陆块跌到 82‰，两条断言同时红。
    // Arrange
    let preset = preset_by_id("continent");

    // Act
    let measured = averaged(preset);

    // Assert
    assert!(
        (250..=480).contains(&measured.water_permille),
        "大陆预设的水域比例 {}‰ 落在 250..=480 之外",
        measured.water_permille
    );
    assert!(
        measured.largest_landmass_permille >= 800,
        "大陆预设的最大连通陆块只占全部陆地 {}‰，算不上「一整块大陆」",
        measured.largest_landmass_permille
    );
}

#[test]
fn 群岛预设的水显著多于大陆预设且陆地真的碎成很多块() {
    // 这是本批次最需要能被证伪的一条断言（简报点名）。两个条件缺一
    // 不可：只有第一条（水多）成立的话，得到的是「一块被淹得只剩边角
    // 的大陆」而不是群岛——实测把海平面单独抬到 600‰ 时水域已达 822‰，
    // 但最大的那块陆地仍占全部陆地的 403‰，那不叫群岛。第二条正是
    // `TerrainShape::continent_shrink` 这个新旋钮存在的全部理由。
    //
    // 反例（已实跑验证会红）：把群岛那一档的 continent_shrink 从 2 改
    // 回 0（其余数值不动），水域比例几乎不变，但最大陆块从两位数千分比
    // 涨回四百上下，第二条断言当场红——证明这条断言真的在测「碎」，
    // 不是被「水多」顺带满足的。
    // Arrange
    let continent = averaged(preset_by_id("continent"));

    // Act
    let archipelago = averaged(preset_by_id("archipelago"));

    // Assert
    assert!(
        archipelago.water_permille >= continent.water_permille + 250,
        "群岛水域 {}‰ 没有比大陆 {}‰ 高出至少 250‰",
        archipelago.water_permille,
        continent.water_permille
    );
    assert!(
        archipelago.largest_landmass_permille <= 200,
        "群岛的最大连通陆块占了全部陆地的 {}‰——这是「被淹掉大半的大陆」，不是群岛",
        archipelago.largest_landmass_permille
    );
}

#[test]
fn 山地预设的山地比例数倍于大陆预设() {
    // 反例（已实跑验证会红）：把山地那一档的 mountain_level 从 620 调
    // 回默认的 750，山地比例从两百多千分比掉回三十上下，断言当场红。
    // Arrange
    let continent = averaged(preset_by_id("continent"));

    // Act
    let highland = averaged(preset_by_id("highland"));

    // Assert
    assert!(
        highland.mountain_permille >= continent.mountain_permille * 3,
        "山地预设的山地比例 {}‰ 没到大陆预设 {}‰ 的三倍",
        highland.mountain_permille,
        continent.mountain_permille
    );
    assert!(
        highland.water_permille < continent.water_permille,
        "山地预设的水域 {}‰ 不该多于大陆预设 {}‰",
        highland.water_permille,
        continent.water_permille
    );
}

#[test]
fn 内陆预设的水显著少于大陆预设() {
    // 这一条直接对应项目所有者报告的那个现象——「235 个据点里 117 个
    // 靠水，渔夫成了最常见的职业」。内陆预设是这个现象的直接解药：
    // 水少了，靠水的据点自然就少。
    //
    // 反例（已实跑验证会红）：把内陆那一档的 sea_level 从 300 调回默认
    // 的 400，水域从一百五十多千分比涨回三百七上下，断言当场红。
    // Arrange
    let continent = averaged(preset_by_id("continent"));

    // Act
    let inland = averaged(preset_by_id("inland"));

    // Assert
    assert!(
        inland.water_permille + 120 <= continent.water_permille,
        "内陆预设的水域 {}‰ 没有比大陆预设 {}‰ 少出至少 120‰",
        inland.water_permille,
        continent.water_permille
    );
    assert!(
        inland.largest_landmass_permille >= 800,
        "内陆预设的最大连通陆块只占全部陆地 {}‰，与「内陆」这个名字不符",
        inland.largest_landmass_permille
    );
}

#[test]
fn 每一档预设都产出互不相同的世界() {
    // 四个名字必须对应四种真的不一样的世界——若有两档的三项性质完全
    // 撞在一起，那两个名字里至少有一个是空的。
    // Arrange
    let measured: Vec<(&str, Measured)> = TERRAIN_PRESETS
        .iter()
        .map(|preset| (preset.id, averaged(preset)))
        .collect();

    // Act & Assert
    for (i, (id_a, a)) in measured.iter().enumerate() {
        for (id_b, b) in measured.iter().skip(i + 1) {
            let same = a.water_permille == b.water_permille
                && a.mountain_permille == b.mountain_permille
                && a.largest_landmass_permille == b.largest_landmass_permille;
            assert!(
                !same,
                "预设 {id_a} 与 {id_b} 产出的世界在三项性质上完全相同"
            );
        }
    }
}

#[test]
fn 大陆预设与地形形态默认值逐位相同() {
    // 两条黄金基准（determinism.rs 的 EXPECTED_WORLD_DIGEST 与
    // replay.rs 的 EXPECTED_REPLAY_DIGEST）固定的正是默认形态那张地图，
    // 而默认预设标识指向的就是「大陆」这一档。两者一旦分叉，没碰过
    // 配置文件的玩家开出来的世界就不再是黄金基准里那一张了。
    // Arrange & Act
    let continent = preset_by_id("continent");

    // Assert
    assert_eq!(
        continent.shape,
        ll_world::generate::TerrainShape::default(),
        "大陆预设与 TerrainShape::default() 已经分叉"
    );
}
