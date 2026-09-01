//! 据点的**街道与密度**：房子不再挨着，而且大城比村落密。
//!
//! # 所有者原话
//!
//! > 「聚居地的建筑靠这么近……这不像是一个能正常运作的聚居地。」
//!
//! 本批次之前的状态是一个常量：`BUILDING_SPACING = BUILDING_SPAN + 1`
//! ——全大陆每一座据点、每两栋屋子之间**恒隔 1 格**，没有街道、没有
//! 疏密之分。本文件钉住替代它的那两条规则：
//!
//! 1. **巷宽按人口分档**（大城 1 格、镇 2 格、村 3 格）。
//! 2. **每三栋插一条街**，街道净宽 = 巷宽 + 2。
//!
//! # 为什么是整合测试而不是 `settlement.rs` 里的单测
//!
//! `crates/ll-world/src/settlement.rs` 的代码行数已经逼近规格 §13 的
//! 800 行上限（本批次开工时 701 行）。把这一组断言写进去会把它推过线，
//! 而 `scripts/ci/check_file_size_budget.py` 是阻断门禁。放这里没有任何
//! 损失：本文件用到的三样东西（`building_origin`、`BUILDING_SPAN`、
//! `MAX_FOOTPRINT_RADIUS`）全部是 `pub`。
//!
//! # 判据怎么读出来：沿着方环第 4 圈的顶行横着量一遍
//!
//! `spiral_offset` 把第 n 栋映射到一个**格位**（第几栋，不是第几格），
//! 方环由内向外排、同一圈内按 `(dy, dx)` 光栅序。半径 4 那一圈的第一个
//! 格位是 `(-4, -4)`，其后八个依次是 `dx = -3..=4`——也就是说
//! **第 49 到第 57 栋恰好是同一行上、格位从 -4 到 4 的九栋屋子**。
//! 量它们左边缘之间的距离，就量到了「两栋屋子之间隔多远」。
//!
//! 本文件不把「49」这个数字当已知条件用：下面第一条断言先证明这九栋
//! 确实同一行、且左边缘严格递增，再拿它们量间距。

use ll_core::ident::WorldId;
use ll_core::torus::TorusSize;
use ll_world::settlement::{
    BUILDING_SPAN, MAX_BUILDINGS, MAX_FOOTPRINT_RADIUS, SITE_RESOURCE_SLOTS, SettlementSite,
    SettlementStatus, building_origin,
};
use ll_world::zone::ZoneLayout;

/// 半径 4 那一圈顶行第一栋的序号：半径 3 为止累计 `(2×3+1)² = 49` 栋，
/// 因此第 49 栋是半径 4 那一圈的第一个格位。
const RING4_ROW_FIRST: u32 = 49;
/// 那一行有几栋（格位 -4..=4）。
const RING4_ROW_LEN: u32 = 9;

fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(4, 4).expect("4x4 合法");
    ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
}

/// 一座人口为 `population` 的满额据点（八十栋）。
fn site_with_population(population: u32) -> SettlementSite {
    let layout = test_layout();
    // 锚点放在世界正中，离环面接缝远远的：本文件量的是**未环绕的原始
    // 整数坐标**（`building_origin` 的契约），锚点贴边会让这些整数在
    // 世界尺寸附近跨过接缝，量出来的间距失去意义。
    let anchor = layout.tile_size().wrap(96, 96);
    let mut counter = 0u32;
    SettlementSite {
        id: WorldId::next(&mut counter),
        zone: layout.tile_to_zone(anchor).0,
        anchor,
        status: SettlementStatus::Inhabited,
        founded_epoch: 0,
        abandoned_epoch: None,
        population,
        peak_population: population,
        building_count: MAX_BUILDINGS,
        resource_profile: [None; SITE_RESOURCE_SLOTS],
        culture: None,
    }
}

/// 半径 4 那一圈顶行九栋屋子**左边缘之间的空隙**（格）。
///
/// 返回 8 个数：第 i 个是第 i 栋右外墙与第 i+1 栋左外墙之间空着几格。
fn row_gaps(site: &SettlementSite) -> Vec<i32> {
    let origins: Vec<(i32, i32)> = (0..RING4_ROW_LEN)
        .map(|i| building_origin(site, RING4_ROW_FIRST + i))
        .collect();
    // 先证明这九栋真的同一行、且从左到右排开——「49 是那一圈的第一栋」
    // 这个前提因此不是抄来的，是当场验的。
    for window in origins.windows(2) {
        assert_eq!(window[0].1, window[1].1, "这九栋应当在同一行上");
        assert!(
            window[1].0 > window[0].0,
            "这九栋应当从左到右排开：{:?} → {:?}",
            window[0],
            window[1]
        );
    }
    origins
        .windows(2)
        .map(|w| w[1].0 - (w[0].0 + BUILDING_SPAN))
        .collect()
}

#[test]
fn 两栋屋子之间恒留出至少一格巷子() {
    // 这一条本批次之前就成立（间距 6、外廓 5），保留是因为它是「不许
    // 连成一整块实心墙」这条最底线的性质，新的街道逻辑不许把它弄丢。
    for population in [0, 8, 32, 96, 400] {
        let gaps = row_gaps(&site_with_population(population));
        for (i, gap) in gaps.iter().enumerate() {
            assert!(
                *gap >= 1,
                "人口 {population} 的据点里第 {i} 与第 {} 栋之间只隔了 {gap} 格",
                i + 1
            );
        }
    }
}

#[test]
fn 大城比村落密() {
    // Arrange：三档人口。
    let village = row_gaps(&site_with_population(4));
    let town = row_gaps(&site_with_population(48));
    let city = row_gaps(&site_with_population(200));

    // Act：取每一档的**最小**空隙——那就是这座据点的巷宽。
    let alley = |gaps: &[i32]| *gaps.iter().min().expect("八个空隙");

    // Assert：三档严格递减，不是「差不多」。
    assert_eq!(alley(&village), 3, "村落最疏");
    assert_eq!(alley(&town), 2, "镇居中");
    assert_eq!(alley(&city), 1, "大城最密");
}

#[test]
fn 每三栋屋子之后留出一条街() {
    // 这是本批次的核心断言：**街道真的存在**。
    //
    // 反例（ADR 0018，人工验证过）：把 `grid_to_tile` 改回
    // `cell * (BUILDING_SPAN + 1)`（也就是本批次之前那个恒 1 格间距），
    // 八个空隙全部是 1，`streets` 为空，本条当场红。
    for population in [4, 48, 200] {
        // Arrange
        let gaps = row_gaps(&site_with_population(population));
        let alley = *gaps.iter().min().expect("八个空隙");

        // Act：比巷子宽的那些空隙就是街。
        let streets: Vec<usize> = gaps
            .iter()
            .enumerate()
            .filter(|(_, gap)| **gap > alley)
            .map(|(i, _)| i)
            .collect();

        // Assert ①：九个格位里恰好有两条街（格位 -3 与 +3 各起一个新
        // 街区，正中那个街区因为对称而宽 5 个格位，见 `grid_to_tile`
        // 文档）。
        assert_eq!(
            streets.len(),
            2,
            "人口 {population}：九栋一行里应当有两条街，实际空隙是 {gaps:?}"
        );
        // Assert ②：街道净宽 = 巷宽 + 2，且**至少 3 格**——一眼分得出
        // 「这是路」而不是「两栋房子之间的缝」。
        for i in streets {
            assert_eq!(gaps[i], alley + 2, "街道净宽应当是巷宽 + 2");
            assert!(gaps[i] >= 3, "街道至少 3 格宽");
        }
    }
}

#[test]
fn 街道相对锚点左右对称() {
    // `grid_to_tile` 的第一版用 `div_euclid`，数学上对但**不对称**：
    // 负半轴的第一个街区只有两个格位，于是格位 -4 比 +4 多推出去两格，
    // 占地半径因此在负方向超出上界（`settlement.rs` 的单测
    // `外廓半径上界真的是上界` 当场抓住了它：「第 49 栋伸到了
    // (38, 38)，超过外廓半径上界 36」）。本条把「对称」这件事本身钉住。
    // Arrange
    let site = site_with_population(48);
    let gaps = row_gaps(&site);

    // Assert：八个空隙关于中点镜像。
    let mirrored: Vec<i32> = gaps.iter().rev().copied().collect();
    assert_eq!(gaps, mirrored, "街道布局应当关于锚点左右对称");
}

#[test]
fn 最疏的满额据点仍在占地半径上界之内() {
    // `MAX_FOOTPRINT_RADIUS` 是 `min_settlement_spacing` 那条几何论证的
    // 前提（两座长满的据点不许互相压进对方的街区）。巷宽随人口变之后，
    // 「最坏情况」是**最疏**的那一档，本条按那一档验。
    // Arrange
    let site = site_with_population(0);

    // Act & Assert
    for building in 0..MAX_BUILDINGS {
        let (left, top) = building_origin(&site, building);
        let near_x = left - site.anchor.x();
        let near_y = top - site.anchor.y();
        let far_x = near_x + BUILDING_SPAN - 1;
        let far_y = near_y + BUILDING_SPAN - 1;
        let reach = [near_x, near_y, far_x, far_y]
            .into_iter()
            .map(i32::abs)
            .max()
            .expect("四个数");
        assert!(
            reach <= MAX_FOOTPRINT_RADIUS,
            "第 {building} 栋伸到离锚点 {reach} 格，超过上界 {MAX_FOOTPRINT_RADIUS}"
        );
    }
}

#[test]
fn 据点最小间距仍然大于两倍占地半径() {
    // 这一条守的是**跨批次的那条不变式**：两座长满的据点不会互相压进
    // 对方的街区。街道把占地半径从 26 推到了 36，这条断言就是那次推大
    // 之后仍然安全的证据。
    let spacing = ll_world::chronicle::ChronicleParams::default().min_settlement_spacing as i32;
    assert!(
        spacing > 2 * MAX_FOOTPRINT_RADIUS,
        "据点最小间距 {spacing} 必须大于两倍占地半径 {}",
        2 * MAX_FOOTPRINT_RADIUS
    );
}

#[test]
fn 废墟保留它鼎盛时的密度() {
    // 用峰值人口分档而不是当前人口：人走了，房子和街道还在原地。
    // 反例：把 `alley_width` 改成只看 `site.population`，本条当场红
    // ——废墟的当前人口恒为 0，会掉进最疏那一档。
    // Arrange
    let mut ruin = site_with_population(200);
    ruin.status = SettlementStatus::Ruined;
    ruin.population = 0;
    let living = site_with_population(200);

    // Act
    let ruin_gaps = row_gaps(&ruin);
    let living_gaps = row_gaps(&living);

    // Assert：同一座城，塌了之后墙不会自己挪位置。
    assert_eq!(ruin_gaps, living_gaps);
}
