//! 在世界地图上选出生地。
//!
//! # 所有者裁定的粒度
//!
//! > 「接着就是选择地图上在哪重生。」
//! >
//! > 「重生点就是随机点一个格子，然后在那区块内随机出生在陆地上。」
//!
//! 也就是**玩家点一个区块（zone），引擎在那个区块内随机挑一格陆地**。
//! 不是「玩家精确点一格瓦片」——世界 4608×3072 格，一屏地图一格代表
//! 48×48 个瓦片，精确到瓦片既点不准也没有意义。
//!
//! # 顺序：世界必须先生成
//!
//! 角色创建 → 世界配置 → **生成世界** → 选出生地 → 进入。
//!
//! 反过来（先选点再生成）做不到：不生成就没有地图可看。这一点值得写
//! 下来，因为「先选出生地再生成世界」听起来更自然，实际是不可能的。
//!
//! # 全图可见：传一份「全部已探索」的记忆，没有 `reveal_all` 标志
//!
//! `ll_world::world_map::world_map_slice` 显式要求调用方传一份
//! `&ExplorationMemory`（见 `ll_world::exploration` 模块文档「为什么读取
//! 接口要求显式传入」一节，以及
//! `ll_ui::hud::world_map::site_marker_quads` 文档里点名本批次的那一
//! 段）。选点屏传
//! [`ll_world::exploration::ExplorationMemory::fully_explored`] 进去，
//! `explored` 恒为真，**同一份呈现代码**自然变成全图可见——不加分支、
//! 不加标志、不碰 `WorldState`。
//!
//! # 换算：`world_map_zone_at_pixel` 与 `zone_at_cell` 是同一条链的两端
//!
//! - 鼠标：`ll_ui::hud::world_map::world_map_zone_at_pixel`（上上批备好
//!   的，带 4 条属性测试，参数与画图函数对齐、自己按皮肤边框内缩）。
//! - 键盘：`ll_world::world_map::WorldMapSlice::zone_at_cell`。
//!
//! 前者内部**就是**调后者，因此两条入口不可能对「这一格是哪个区块」
//! 给出不同答案。

use ll_core::rng::DetRng;
use ll_core::torus::TorusPos;
use ll_platform::input::{GameKey, InputState};
use ll_world::chunk::ChunkGrid;
use ll_world::generate::{GenParams, generate_zone_window};
use ll_world::land::largest_walkable_component;
use ll_world::noise::TileableNoise;
use ll_world::space::ZoneCoord;
use ll_world::terrain::{BaseTerrainIds, TerrainTable};
use ll_world::world_map::WorldMapSlice;
use ll_world::zone::ZoneLayout;

use crate::menu_screen::{ScreenNotice, ScreenState, SpawnOrigin};

/// 「在这个区块里挑一格出生地」专用的确定性流标识。
///
/// 与 `ll_mod::roster::ROSTER_STREAM_ID`/`ROSTER_GENDER_STREAM_ID` 同一
/// 条约定：每一件独立的事各占一条流，坐标（这里是区块坐标）作为
/// [`DetRng::for_entity`] 的第三个输入。
///
/// **改动它会让同一个种子 + 同一个区块挑出另一格。** 那不影响任何黄金
/// 基准（本函数只在玩家真的按下确认时才被调用，两条基准的世界都不经过
/// 它），但会让「同一个种子里点同一个区块出生在同一格」这条承诺失效。
pub const SPAWN_PICK_STREAM_ID: u64 = 0x5350_4157_4E50_0001;

/// 在一个区块内挑一格可站立的陆地出生。
///
/// 返回 `None` 表示**这个区块里没有任何可站立的格子**（全是水/全是
/// 山），调用方应当提示玩家重选，见模块外的调用点与本函数
/// 「退化策略」一节。
///
/// # 挑的是「最大连通陆地」里的一格，不是随便一格陆地
///
/// 一格可通行的礁石被深水包住时，玩家出生在那里寸步难行——而同一个
/// 区块里可能就有一大块陆地。因此先用
/// [`largest_walkable_component`] 找出窗口内**最大**的连通可行走分量
/// （与 `crate::world::find_spawn_site` 和据点选址**共用同一份算法**，
/// 见 `ll_world::land` 模块文档），再在那个分量内部挑。
///
/// 阈值取 1 而不是 `MIN_SPAWN_LAND_AREA`（500）：那个阈值服务的是
/// 「引擎替玩家选一个好地方」，而这里是**玩家自己点的**——他点了一座
/// 小岛就该出生在那座小岛上，引擎没有立场替他否决。玩家看得见地图。
///
/// # 退化策略：返回 `None`，由调用方提示重选；**不自动换邻近区块**
///
/// 理由（本批的判断，规格没有裁定，如实登记）：
///
/// 1. **玩家点了哪里就是哪里。** 悄悄换到隔壁等于答非所问——与批次 6
///    「首页读档失败不悄悄回退到新游戏」是同一条已被采纳的纪律。
/// 2. 自动换需要定义「邻近」的搜索顺序、上限、以及「换出去多远算换错
///    了」，那是一套新的确定性判据与一堆边界情形，所有者没有裁定过。
/// 3. **最容易反转**：真要自动换，只需把 `None` 那条分支换成搜索，
///    换算、界面、测试一个字都不用动。
/// 4. 玩家看得见地图（水是蓝的），点到全水区块是他自己一眼能纠正的
///    操作，不是引擎需要替他兜住的意外。
///
/// # 确定性（约束 C3）
///
/// 同一个 `(seed, zone)` 恒挑出同一格：随机数来自
/// `DetRng::for_entity(seed, SPAWN_PICK_STREAM_ID, 区块光栅序号)`，
/// 分量成员按窗口光栅序收集（不经任何哈希容器，约束 C5）。
pub fn pick_spawn_in_zone(
    layout: &ZoneLayout,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    table: &TerrainTable,
    zone: ZoneCoord,
) -> Option<TorusPos> {
    let window = generate_zone_window(noise, params, layout, zone, terrain_ids).ok()?;
    let local_size = layout.local_size();
    // `min_area = 1`：见本函数文档「挑的是最大连通陆地里的一格」一节。
    let component = largest_walkable_component(&window, local_size, table, 1)?;
    let members = component_members(&window, local_size, table, component.start);
    if members.is_empty() {
        return None;
    }
    let zone_count = layout.zone_count();
    let raster =
        u64::from(zone.y() as u32) * u64::from(zone_count.width()) + u64::from(zone.x() as u32);
    let mut rng = DetRng::for_entity(params.seed, SPAWN_PICK_STREAM_ID, raster);
    let pick = rng.gen_range(members.len() as u64) as usize;
    let local = members.get(pick).copied()?;
    let span = layout.zone_span() as i32;
    Some(
        layout
            .tile_size()
            .wrap(zone.x() * span + local.x(), zone.y() * span + local.y()),
    )
}

/// 从 `start` 出发，把它所在的那个连通可行走分量的全部成员按**窗口光栅
/// 序**收集出来。
///
/// # 为什么重扫一遍，而不是让 `LandComponent` 带上成员列表
///
/// [`largest_walkable_component`] 在据点选址的热路径上（世界生成期逐
/// 区块调用），给它加一个 `Vec<TorusPos>` 的产出等于让每一次调用都多
/// 分配一个最多 `zone_span²` 长的向量。本函数只在**玩家按下确认的那
/// 一帧**跑一次，多扫一遍 48×48 是可以忽略的代价。
///
/// 邻居顺序与 `largest_walkable_component` 一致（上、右、下、左），
/// 起点相同，因此走出来的必然是同一个分量。**但成员按光栅序排序后
/// 返回**，与遍历顺序无关——这样即便将来那边的邻居顺序改了，同一个
/// 分量给出的成员列表仍然逐位相同（约束 C3/C5）。
fn component_members(
    window: &ChunkGrid,
    local_size: ll_core::torus::TorusSize,
    table: &TerrainTable,
    start: TorusPos,
) -> Vec<TorusPos> {
    let span = local_size.width() as i32;
    let mut visited = vec![false; (span * span) as usize];
    let mut queue = std::collections::VecDeque::new();
    let mut members = Vec::new();
    let index_of = |pos: TorusPos| (pos.y() * span + pos.x()) as usize;

    visited[index_of(start)] = true;
    queue.push_back(start);
    while let Some(pos) = queue.pop_front() {
        members.push(pos);
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let (nx, ny) = (pos.x() + dx, pos.y() + dy);
            // **区块内部不做环绕**——与 `largest_walkable_component`
            // 逐字同一条纪律：把窗口一条边界接到另一条边界，会把两个
            // 本不相邻的世界坐标误判成相邻。
            if nx < 0 || ny < 0 || nx >= span || ny >= span {
                continue;
            }
            let neighbour = local_size.wrap(nx, ny);
            if visited[index_of(neighbour)] {
                continue;
            }
            let kind = window.terrain_at(neighbour);
            if kind.blocks_move(table) {
                continue;
            }
            visited[index_of(neighbour)] = true;
            queue.push_back(neighbour);
        }
    }
    // 光栅序：与遍历顺序解耦，见本函数文档最后一段。
    members.sort_by_key(|pos| (pos.y(), pos.x()));
    members
}

/// 键盘光标移动一格；结果恒落在 `0..cols` / `0..rows` 内。
///
/// **到头就停，不回绕。** 与 `WorldMapView::zoom_in` 同一条理由：地图
/// 是一块有边的画面，按住方向键突然从左边缘跳到右边缘会让玩家以为自己
/// 按错了键。世界本身是环面，但**这块屏上看到的是一整张展开图**，
/// 展开图的边就是边。
pub fn move_cell_cursor(cell: (u32, u32), cols: u32, rows: u32, dx: i32, dy: i32) -> (u32, u32) {
    let clamp = |value: u32, delta: i32, limit: u32| -> u32 {
        if limit == 0 {
            return 0;
        }
        let next = value as i64 + delta as i64;
        next.clamp(0, limit as i64 - 1) as u32
    };
    (clamp(cell.0, dx, cols), clamp(cell.1, dy, rows))
}

/// 光标那一格对应哪个区块——键盘那条入口。
///
/// 与鼠标那条入口（`ll_ui::hud::world_map::world_map_zone_at_pixel`）
/// **最终调的是同一个** [`WorldMapSlice::zone_at_cell`]，因此两者不可能
/// 给出不同答案。
pub fn zone_of_cursor(
    slice: &WorldMapSlice,
    layout: &ZoneLayout,
    cell: (u32, u32),
) -> Option<ZoneCoord> {
    slice.zone_at_cell(layout, cell.0, cell.1)
}

/// 处理完选出生地屏这一帧输入之后，调用方该做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnPickUpdate {
    /// 要切到哪一块屏，`None` 表示留在选点屏。
    pub next: Option<ScreenState>,
    /// 这一帧要说的一句话。
    pub notice: Option<ScreenNotice>,
    /// 玩家已经真的进世界了——调用方该把整块屏关掉。
    ///
    /// 与 `next` 分开而不是加一个 `ScreenState::None`：`next` 回答的是
    /// 「换到哪块屏」，本字段回答的是「不再有屏」，两者不是同一个问题。
    pub entered: bool,
}

impl SpawnPickUpdate {
    /// 什么都没发生，留在选点屏。
    pub fn idle() -> SpawnPickUpdate {
        SpawnPickUpdate {
            next: None,
            notice: None,
            entered: false,
        }
    }

    /// 切到另一块屏。
    pub fn going(next: ScreenState) -> SpawnPickUpdate {
        SpawnPickUpdate {
            next: Some(next),
            notice: None,
            entered: false,
        }
    }

    /// 只说一句话，留在选点屏。
    pub fn saying(notice: ScreenNotice) -> SpawnPickUpdate {
        SpawnPickUpdate {
            next: None,
            notice: Some(notice),
            entered: false,
        }
    }

    /// 玩家进世界了。
    pub fn entered() -> SpawnPickUpdate {
        SpawnPickUpdate {
            next: None,
            notice: None,
            entered: true,
        }
    }
}

/// 选出生地屏这一帧的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnPickDecision {
    /// 屏的状态怎么变。
    pub update: SpawnPickUpdate,
    /// 玩家这一帧确认了哪个区块，`None` 表示没确认。
    ///
    /// 与 `update` 分开的理由：**「挑哪一格」需要地形，而地形不在这块
    /// 屏的职责范围内**。本函数是纯状态机（只认光标、切片与输入），
    /// 由调用方拿着这个区块去调 [`pick_spawn_in_zone`]——那一步要噪声
    /// 源、生成参数与地形表，全都只有 `ll_game::app` 那一层够得着。
    pub confirmed: Option<ZoneCoord>,
}

/// 处理选出生地屏这一帧的输入——**纯状态机**，不碰地形、不碰世界。
///
/// `clicked_zone` 是鼠标那条入口的产出：调用方用
/// `ll_ui::hud::world_map::world_map_zone_at_pixel` 算好再传进来。
/// 那个函数要面板矩形与皮肤（按边框粗细内缩），两样都只有渲染那一侧
/// 拿得到——把它们塞进本函数的参数表，等于让一个纯状态机认识 GPU 皮肤。
pub fn update_spawn_pick(
    cursor: &mut (u32, u32),
    slice: &WorldMapSlice,
    layout: &ZoneLayout,
    input: &InputState,
    clicked_zone: Option<ZoneCoord>,
    origin: SpawnOrigin,
) -> SpawnPickDecision {
    let (dx, dy) = (
        i32::from(input.was_just_pressed(GameKey::Right))
            - i32::from(input.was_just_pressed(GameKey::Left)),
        i32::from(input.was_just_pressed(GameKey::Down))
            - i32::from(input.was_just_pressed(GameKey::Up)),
    );
    if dx != 0 || dy != 0 {
        *cursor = move_cell_cursor(*cursor, slice.cols, slice.rows, dx, dy);
    }

    // 鼠标点了某一格：**先把光标挪过去**，再当作一次确认。两步合一
    // 而不是直接确认，是为了让点歪了的那一下有迹可循——玩家看得到
    // 光标停在哪，也能接着用方向键微调。
    if let Some(zone) = clicked_zone {
        if let Some(cell) = cell_of_zone(slice, layout, zone) {
            *cursor = cell;
        }
        return SpawnPickDecision {
            update: SpawnPickUpdate::idle(),
            confirmed: Some(zone),
        };
    }

    if input.was_just_pressed(GameKey::Cancel) {
        // **回到来处，不是回一块写死的屏。**
        //
        // 这里此前写死的是 `ScreenState::WorldSetup`，而这块屏有三个
        // 入口，其中「死亡转生」那一条按 `crate::chargen` 自己的论证
        // **必须跳过**世界配置屏——把玩家送到那里，他按一下「生成世界」
        // 就会用一个全新的世界覆盖掉自己玩过的那一局
        // （`knowledge/design/ui-and-navigation.md` 2.2 节 D1）。
        return SpawnPickDecision {
            update: SpawnPickUpdate::going(origin.screen()),
            confirmed: None,
        };
    }
    if !input.was_just_pressed(GameKey::Confirm) {
        return SpawnPickDecision {
            update: SpawnPickUpdate::idle(),
            confirmed: None,
        };
    }
    SpawnPickDecision {
        update: SpawnPickUpdate::idle(),
        confirmed: zone_of_cursor(slice, layout, *cursor),
    }
}

/// 一个区块落在本屏的哪一格——[`zone_of_cursor`] 的反向，用于把鼠标点
/// 到的区块同步给键盘光标。
///
/// 走 [`WorldMapSlice::cell_of_tile`]（同一个换算的正反两向写在同一个
/// 类型上，见其文档），不手写任何环面取模。
fn cell_of_zone(slice: &WorldMapSlice, layout: &ZoneLayout, zone: ZoneCoord) -> Option<(u32, u32)> {
    let span = layout.zone_span() as i32;
    // 取区块正中那一格的世界坐标：区块边角可能落进相邻那一格
    // （视野原点未必对齐区块边界，见 `zone_at_cell` 文档）。
    let center = layout
        .tile_size()
        .wrap(zone.x() * span + span / 2, zone.y() * span + span / 2);
    slice.cell_of_tile(center)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::terrain_shape::TerrainShape;

    /// 一份够本模块用的区块布局：区块跨度 48（与本体一致），4×4 个区块。
    fn layout() -> ZoneLayout {
        let zone_count = ll_core::torus::TorusSize::new(4, 4).expect("4x4 是合法尺寸");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    fn noise_of(layout: &ZoneLayout, params: &GenParams) -> TileableNoise {
        ll_world::generate::build_zone_noise(layout, params).expect("测试布局恒能建出噪声")
    }

    #[test]
    fn 同一个种子与同一个区块恒挑出同一格() {
        // 约束 C3。这条是「同一个种子里点同一个区块出生在同一格」这条
        // 承诺的直接验证。
        // Arrange
        let layout = layout();
        let params = GenParams::default();
        let noise = noise_of(&layout, &params);
        let (ids, table) = base_terrain_fixture();

        // Act：把全部区块各挑两次。
        let mut differed = 0usize;
        let mut found = 0usize;
        for y in 0..4i32 {
            for x in 0..4i32 {
                let zone = layout.zone_count().wrap(x, y);
                let first = pick_spawn_in_zone(&layout, &noise, &params, &ids, &table, zone);
                let second = pick_spawn_in_zone(&layout, &noise, &params, &ids, &table, zone);
                if first != second {
                    differed += 1;
                }
                if first.is_some() {
                    found += 1;
                }
            }
        }

        // Assert
        assert_eq!(differed, 0, "同一个 (种子, 区块) 两次挑出了不同的格子");
        assert!(found > 0, "十六个区块里一个可站立的都没有，夹具本身有问题");
    }

    #[test]
    fn 挑出来的那一格真的可站立且落在被点的那个区块里() {
        // Arrange
        let layout = layout();
        let params = GenParams::default();
        let noise = noise_of(&layout, &params);
        let (ids, table) = base_terrain_fixture();

        // Act / Assert
        let mut checked = 0usize;
        for y in 0..4i32 {
            for x in 0..4i32 {
                let zone = layout.zone_count().wrap(x, y);
                let Some(pos) = pick_spawn_in_zone(&layout, &noise, &params, &ids, &table, zone)
                else {
                    continue;
                };
                checked += 1;
                let (landed_zone, _) = layout.tile_to_zone(pos);
                assert_eq!(landed_zone, zone, "挑出来的格子落在了别的区块里");
                let window = generate_zone_window(&noise, &params, &layout, zone, &ids)
                    .expect("窗口恒能生成");
                let (_, local) = layout.tile_to_zone(pos);
                let kind = window.terrain_at(local);
                assert!(!kind.blocks_move(&table), "挑出来的格子不可站立");
            }
        }
        assert!(checked > 0, "一个区块都没检查到，夹具本身有问题");
    }

    #[test]
    fn 全是深水的区块返回none() {
        // 退化策略的正面验证。造「全是水」的方式是把海平面抬到高度上界
        // 之上（2000 > TerrainShape::HEIGHT_MAX 1000）——`height_to_terrain`
        // 的第一条判据 `height < sea_level` 因此对每一格都成立，全图深水，
        // 而深水 `blocks_move == true`。
        //
        // 这个取值**故意越界**（`TerrainShape::validate` 会拒绝它），这
        // 正是重点：本函数是纯查询，不做参数校验，测试因此能用它构造一个
        // 确定的退化世界，而不必去碰运气找一个恰好全是水的种子——后者会在
        // 噪声算法一改就失效。
        // Arrange
        let layout = layout();
        let params = GenParams {
            seed: 20260828,
            shape: TerrainShape {
                sea_level: 2000,
                ..TerrainShape::default()
            },
        };
        let noise = noise_of(&layout, &params);
        let (ids, table) = base_terrain_fixture();

        // Act / Assert：十六个区块一个都挑不出来。
        for y in 0..4i32 {
            for x in 0..4i32 {
                let zone = layout.zone_count().wrap(x, y);
                assert_eq!(
                    pick_spawn_in_zone(&layout, &noise, &params, &ids, &table, zone),
                    None,
                    "全是深水的区块 ({x}, {y}) 却挑出了出生点" // i18n-exempt：测试断言的失败消息，只在测试失败时打给开发者看
                );
            }
        }
    }

    #[test]
    fn 光标到边就停不回绕() {
        assert_eq!(move_cell_cursor((0, 0), 10, 8, -1, -1), (0, 0));
        assert_eq!(move_cell_cursor((9, 7), 10, 8, 1, 1), (9, 7));
        assert_eq!(move_cell_cursor((4, 4), 10, 8, 1, -1), (5, 3));
    }

    #[test]
    fn 零尺寸网格不做越界运算() {
        assert_eq!(move_cell_cursor((3, 3), 0, 0, 1, 1), (0, 0));
    }

    /// 一份够本模块用的地图切片——选点屏的取消目标与地形无关，这里只是
    /// 把 `update_spawn_pick` 的参数凑齐。
    fn slice_of(layout: &ZoneLayout, params: &GenParams) -> WorldMapSlice {
        let noise = noise_of(layout, params);
        let (ids, _) = base_terrain_fixture();
        let field = ll_world::overview::generate_continent_field(layout, &noise, params, &ids);
        let exploration = ll_world::exploration::ExplorationMemory::fully_explored(layout);
        let view = ll_world::world_map::WorldMapView::centered_on_tile(
            &field,
            layout.tile_size().wrap(0, 0),
        );
        ll_world::world_map::world_map_slice(&field, layout, &exploration, &view)
    }

    fn 按下取消(origin: SpawnOrigin) -> Option<ScreenState> {
        let layout = layout();
        let params = GenParams::default();
        let slice = slice_of(&layout, &params);
        let mut input = InputState::new();
        input.press(GameKey::Cancel);
        let mut cursor = (0u32, 0u32);
        update_spawn_pick(&mut cursor, &slice, &layout, &input, None, origin)
            .update
            .next
    }

    #[test]
    fn 从世界配置进来的按取消回世界配置() {
        // 规格 N5 判据 1：守住既有行为不被 D1 的修法改坏。
        //
        // 反例验证（已实跑）：把 `update_spawn_pick` 里那句
        // `going(origin.screen())` 换回写死的
        // `going(ScreenState::CharacterCreation { cursor: 0 })`，本条当场
        // 变红（而下一条仍绿——两条互为对照，缺一条就盯不住「读的是来处」
        // 这件事）。
        assert_eq!(
            按下取消(SpawnOrigin::WorldSetup),
            Some(ScreenState::WorldSetup { cursor: 0 })
        );
    }

    #[test]
    fn 从转生进来的按取消回角色创建而不是那块会抹掉世界的屏() {
        // **规格 N5 判据 2，也是 D1 那条数据丢失路径的入口。**
        // 死亡转生从角色创建屏直接跳到这里，世界早就存在；把玩家送回
        // 世界配置屏，他按一下「生成世界」就会把自己玩过的那一局抹掉。
        //
        // 反例验证（已实跑）：把取消目标换回写死的 `WorldSetup`（落地前
        // 的样子），本条当场变红。
        assert_eq!(
            按下取消(SpawnOrigin::CharacterCreation),
            Some(ScreenState::CharacterCreation { cursor: 0 })
        );
    }
}
