//! 「这次 FOV 需要的地形，此刻全都在内存里吗」——[`SurfaceWindow`] 那条
//! panic 前置的**唯一可复用判据**。
//!
//! [`SurfaceWindow::terrain_at`] 对未常驻区块的查询直接 panic，这个
//! panic 是**故意的**（见其文档「前置条件与任务 14 的关系」：保留而不是
//! 改成静默兜底，是为了在这条纪律被违反时尽早、吵闹地暴露出来）。它守
//! 的是渲染路径——渲染必须调用
//! [`SurfaceStore::stream_neighborhood`](crate::surface_store::SurfaceStore::stream_neighborhood)
//! 把玩家周围铺好，漏调了就该当场炸出来，而不是把一块黑洞悄悄留在画面上。
//!
//! # 但非渲染路径不能靠 panic 守
//!
//! 所有者实机撞到过这一幕：
//!
//! ```text
//! SurfaceWindow 假定视野范围内的区块都已经常驻，
//! TorusPos { x: 1008, y: 0 } 所属区块尚未加载
//! ```
//!
//! 链条是——玩家一路走过去物化了多座据点的 NPC，它们永远留在
//! `WorldState::actors`；玩家继续走，常驻区块超过上限开始 LRU 驱逐，把
//! 那些 NPC 脚下的区块驱逐掉；但它们照样被时间轴弹出来跑行为树；某个
//! 卫兵调 `ll_sim::ai_query::nearest_visible_actor` 算一次 FOV → 脚下
//! 区块已不在内存 → 游戏当场崩。
//!
//! 这里没有「调用方漏调了 `stream_neighborhood`」这回事可供责备：常驻
//! 集合**只围着玩家维护**，一个离屏 NPC 脚下的地形本来就不该被强行
//! 留在内存里。对它而言「地形不在内存」不是纪律被违反，是正常状态。
//!
//! # 判据落在调用方，不落在 `SurfaceWindow`
//!
//! 正确的修法不是把那个 panic 改哑——改哑等于把上面那条渲染纪律永久
//! 拆掉，将来渲染路径真的漏调 `stream_neighborhood` 时就再也没人告诉
//! 你了。正确的修法是**在算 FOV 之前先问一句**：这次 FOV 会碰到的区块
//! 都常驻吗？不都常驻，就根本不构造 [`SurfaceWindow`]，让上层按自己的
//! 语义降级（感知路径：看不见就是看不见，ADR 0015「查不到就是查不到」）。
//!
//! 这不是新发明。`ll_sim::resolve` 的 `resolve_move` 早就这么做了：
//! 目的地区块非常驻时静默作废、不产出任何效果，且所有者在批次 1 里明确
//! 裁定「不改」。移动已经这么降级了，感知跟着降级是一致的。
//!
//! # 为什么只有一份实现
//!
//! [`fov_neighborhood_resident`] 目前有两个消费者：`ll_sim::ai_query`
//! 的 `nearest_visible_actor`（AI 感知）与 `ll_sim::apply` 落地
//! `Effect::MarkExplored`（探索记忆）。两处判据一旦各写一遍就会漂移，
//! 而漂移的症状是「AI 觉得看得见、apply 觉得看不见」这种极难归因的
//! 不一致。所以判据只有这一份，两处共用同一条纪律。
//!
//! # 与 C5 的关系
//!
//! 本模块**只做 O(1) 单键查找**
//! （[`SurfaceStore::is_resident`](crate::surface_store::SurfaceStore::is_resident)），
//! 从不遍历 `SurfaceStore` 内部那个 `resident: HashMap`——遍历它就把
//! 哈希桶序带进了逻辑判断，正是约束 C5 禁止的事。要检查哪些区块，由
//! 观察者坐标与半径**算**出来，不是从容器里**读**出来。
//!
//! [`SurfaceWindow`]: crate::surface_store::SurfaceWindow
//! [`SurfaceWindow::terrain_at`]: crate::surface_store::SurfaceWindow

use ll_core::torus::TorusPos;

use crate::surface_store::SurfaceStore;

/// 以 `origin` 为中心、半径 `radius` 的一次 [`crate::fov::compute_fov`]
/// 会碰到的**每一个区块**是否都已常驻。
///
/// 全都常驻时返回 `true`——此时把 `store` 包成
/// [`SurfaceWindow`](crate::surface_store::SurfaceWindow) 喂给
/// `compute_fov` 恒不会触发那条 panic。只要有一个区块不在内存里就返回
/// `false`，调用方应当据此**在构造 `SurfaceWindow` 之前**就降级。
///
/// # 判据为什么是一个方框
///
/// `compute_fov` 逐八分象限按行向外扫描，行数上限取
/// `SightGrid::max_scan_row(radius)`；每一行内的列号满足
/// `0 <= col <= row`（见 `fov::octant_offset` 文档）。因此它查询过的
/// 每一个坐标相对 `origin` 的偏移都满足 `max(|dx|, |dy|) <= max_scan_row`
/// ——一个以 `origin` 为心、半边长 `max_scan_row` 的**正方形**恰好是
/// 查询范围的紧上界。`SurfaceWindow` 的 `max_scan_row` 实现是
/// `radius.min(世界宽/2).min(世界高/2)`，本函数原样复刻这条钳制：不复刻
/// 的话，一个大于世界半宽的 `radius` 会让本函数去检查根本不会被查询的
/// 区块，把一次本该正常的 FOV 误判成「不常驻」。
///
/// FOV 实际查询到的格子通常远少于整个方框（挡光的墙会让扫描提前停），
/// 所以这个判据是**保守**的：可能出现「方框里有个角落不常驻、但这次
/// FOV 其实根本不会查到那个角落」而被判成 `false`。这个方向的误判是
/// 安全的一侧——代价是偶尔多降级一次（少看见一个目标），收益是判据
/// 不依赖 FOV 的内部扫描顺序，不会随遮挡关系变来变去。反过来那一侧的
/// 误判（该降级却说不必降级）就是崩溃。
///
/// # 环面与循环上界
///
/// 区块坐标的换算全程走 [`ZoneLayout`](crate::zone::ZoneLayout) 的
/// `tile_to_zone` 与区块级 `TorusSize::wrap`，**没有一处手写取模**
/// ——跨接缝的换算必须走既有类型，与仓库「禁止手写欧氏距离」同一条
/// 纪律。
///
/// 循环次数有上界：半边长已被钳到不超过世界半宽/半高，因此每个轴上
/// 需要检查的区块数不超过 `zone_count / 2 + 2`，不存在无界循环。世界
/// 很小时同一个区块可能被查到多次——那只是重复做同一次 O(1) 查找，
/// 无害。
///
/// # 边界情形
///
/// - `radius == 0`：方框退化成单格，只要求 `origin` 自己那个区块常驻。
///   （`compute_fov` 在半径 0 时其实一次地形都不查，这里仍然要求那一
///   个区块在场——保守的一侧，且让「观察者脚下的地形不在内存里」这件
///   事在任何半径下都得到同一个答案。）
/// - 观察者不存在：不是本函数的问题，调用方在取 `origin` 时就该先
///   短路掉。
pub fn fov_neighborhood_resident(store: &SurfaceStore, origin: TorusPos, radius: u32) -> bool {
    let layout = store.layout();
    let tiles = layout.tile_size();
    // 与 SurfaceWindow::max_scan_row 逐字同一条钳制，见本函数文档。
    let half = radius.min(tiles.width() / 2).min(tiles.height() / 2) as i32;
    let span = layout.zone_span() as i32;
    let zone_count = layout.zone_count();
    let (center, local) = layout.tile_to_zone(origin);

    let (min_dx, max_dx) = zone_offset_range(local.x(), half, span);
    let (min_dy, max_dy) = zone_offset_range(local.y(), half, span);

    for dy in min_dy..=max_dy {
        for dx in min_dx..=max_dx {
            let zone = zone_count.wrap(center.x() + dx, center.y() + dy);
            if !store.is_resident(zone) {
                return false;
            }
        }
    }
    true
}

/// 一个轴上需要检查的区块偏移闭区间。
///
/// `local` 是观察者在自己所属区块内的局部坐标（恒在 `0..span`），
/// `half` 是方框半边长。方框在这个轴上覆盖局部坐标 `local - half ..=
/// local + half`，除以 `span` **向下取整**就得到相对观察者所属区块的
/// 区块偏移范围。
///
/// 用 [`i32::div_euclid`] 而不是 `/`：Rust 的 `/` 对负数向零取整，
/// `-1 / 48 == 0`，会把左边那个区块整个漏掉；`div_euclid` 在除数为正时
/// 就是向下取整，`(-1i32).div_euclid(48) == -1`。这一个字符的差别正是
/// 「观察者站在区块第 0 列、视野往左伸进上一个区块」这类情形会不会被
/// 漏判的分水岭。
fn zone_offset_range(local: i32, half: i32, span: i32) -> (i32, i32) {
    (
        (local - half).div_euclid(span),
        (local + half).div_euclid(span),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::GenParams;
    use crate::terrain::base_terrain_fixture;
    use crate::zone::ZoneLayout;
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;

    /// 16×16 个区块 × 边长 48（世界 768×768 格），只预热出生点周围
    /// 5×5 个区块——与生产环境同构：常驻集合只围着一个点维护，其余
    /// 区块从未进过内存。
    fn store_with_warm_spawn() -> SurfaceStore {
        let zone_count = TorusSize::new(16, 16).expect("16x16 是合法尺寸");
        let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐约束");
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("噪声可构造");
        let mut store = SurfaceStore::new(layout, crate::state::DEFAULT_RESIDENT_CAP);
        store.stream_neighborhood(
            &noise,
            &params,
            &terrain_ids,
            layout.tile_size().wrap(0, 0),
            2,
            Tick(0),
        );
        store
    }

    #[test]
    fn 观察者所在区块本身没常驻就是假() {
        // Arrange
        let store = store_with_warm_spawn();
        let far = store.layout().tile_size().wrap(8 * 48 + 24, 8 * 48 + 24);

        // Act & Assert
        assert!(!fov_neighborhood_resident(&store, far, 12));
    }

    #[test]
    fn 观察者所在区块常驻但视野伸出去就是假() {
        // 只判观察者脚下那一格是不够的——这是最容易漏的一半。
        // Arrange：区块 (2, 0) 的最后一列，区块 3 没预热。
        let store = store_with_warm_spawn();
        let edge = store.layout().tile_size().wrap(2 * 48 + 47, 24);
        let (edge_zone, _) = store.layout().tile_to_zone(edge);
        assert!(store.is_resident(edge_zone), "前置：脚下这个区块是常驻的");

        // Act & Assert
        assert!(!fov_neighborhood_resident(&store, edge, 12));
    }

    #[test]
    fn 视野完全落在常驻区域内就是真() {
        // Arrange：区块 (0, 0) 正中，半径 12 连隔壁区块都够不到。
        let store = store_with_warm_spawn();
        let inside = store.layout().tile_size().wrap(24, 24);

        // Act & Assert
        assert!(fov_neighborhood_resident(&store, inside, 12));
    }

    #[test]
    fn 跨接缝往回绕的方框走的是环面换算不是负坐标() {
        // 出生点在 (0, 0)，半径 12 的方框有一大半绕到世界另一端去了
        // （x = -1 环绕成 767，属于区块 15）。预热半径 2 覆盖了区块
        // 14/15/0/1/2，所以这里应当是真——若换算写成手写取模或用了
        // 向零取整的除法，区块 15 会被漏判或算错，结果就是假。
        // Arrange
        let store = store_with_warm_spawn();
        let seam = store.layout().tile_size().wrap(0, 0);

        // Act & Assert
        assert!(fov_neighborhood_resident(&store, seam, 12));
    }

    #[test]
    fn 半径零只要求脚下那一个区块() {
        // Arrange
        let store = store_with_warm_spawn();
        let inside = store.layout().tile_size().wrap(0, 0);
        let far = store.layout().tile_size().wrap(8 * 48, 8 * 48);

        // Act & Assert
        assert!(fov_neighborhood_resident(&store, inside, 0));
        assert!(!fov_neighborhood_resident(&store, far, 0));
    }

    #[test]
    fn 半径大于世界半宽时被钳住而不是误判成不常驻() {
        // 复刻 SurfaceWindow::max_scan_row 的钳制：一个荒谬的大半径不
        // 该让本函数去检查根本不会被查询的区块。这里造一个**整体只有
        // 2×2 区块**、且已整体常驻的小世界，半径给到远超世界尺寸。
        // Arrange
        let zone_count = TorusSize::new(2, 2).expect("2x2 是合法尺寸");
        let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐约束");
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("噪声可构造");
        let mut store = SurfaceStore::new(layout, crate::state::DEFAULT_RESIDENT_CAP);
        store.stream_neighborhood(
            &noise,
            &params,
            &terrain_ids,
            layout.tile_size().wrap(0, 0),
            2,
            Tick(0),
        );

        // Act & Assert
        assert!(fov_neighborhood_resident(
            &store,
            layout.tile_size().wrap(0, 0),
            u32::MAX
        ));
    }
}
