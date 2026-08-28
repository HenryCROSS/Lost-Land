//! 世界地图面板：M 键切换的只读大陆概览，见 `ll_platform::input::GameKey::Map`
//! 文档——本 crate 不依赖 `ll-platform`，故此处只是文字引用，不是可解析
//! 的文档内链，与 `crate::hud::render` 模块文档同一条既有写法。
//!
//! # 数据源：`continent_map`，不是 `minimap`
//!
//! [`ll_world::overview`] 提供两种只读概览：`minimap`（以玩家为中心的
//! 一小片瓦片级切片，需要 `&mut WorldState` 因为可能触发按需生成，见
//! 其文档）与 `continent_map`（整个世界的区块级概览，只读一份世界创建
//! 时就生成好的 [`ll_world::overview::ContinentField`]，不触碰
//! `SurfaceStore`，见其文档「不触发任何区块的按需生成」测试）。世界地图
//! 要展示的是「整个世界长什么样」，不是「我脚下这一小片」——这正是
//! `continent_map` 的设计目标，`minimap` 服务的是完全不同的场景（沿旧
//! 有小地图 HUD 元素，本批次未落地）。选 `continent_map` 还有一个本批次
//! 「只读」硬约束直接相关的理由：它的签名只接受 `&ZoneLayout`/
//! `&ExplorationMemory`，不接受 `&mut WorldState`，调用方结构上就不可能
//! 意外改动世界状态或触发流式加载，这条保证由类型系统给出，不依赖调用
//! 纪律。
//!
//! # 战争迷雾：未探索格子不展示真实地形
//!
//! [`ll_world::overview::OverviewCell::explored`] 已经是真实的探索记忆
//! （落地探索记忆批次），但它只是一个标志位——`OverviewCell::terrain`
//! 字段本身对未探索格子仍然是真实地形，`continent_map`/`minimap` 是
//! 「只读视图,不做任何过滤」的纯函数（见其模块文档），过滤的责任落在
//! 消费者这一层。[`world_map_cell_quads`] 就是这层过滤：`explored` 为
//! 假的格子恒画 [`FOG_COLOR`]，完全不读取该格的 `terrain` 字段来选色
//! ——不是「地形本身变暗」，是「根本不告诉你这是什么地形」，这才是
//! 「没去过的地方就黑着」（`crate::hud` 模块文档同一条战争迷雾原话在
//! 世界地图上的延伸）的正确落点。

use ll_i18n::Catalog;
use ll_world::overview::OverviewCell;
use ll_world::space::ZoneCoord;
use ll_world::terrain::{BaseTerrainIds, TerrainKind};
use ll_world::world_map::WorldMapSlice;
use ll_world::zone::ZoneLayout;

use crate::widget::geometry::Rect;
use crate::widget::quad::QuadInstance;
use crate::widget::skin::{PanelStyleId, Skin};

/// 未探索格子的显示色——中性深色，与 [`terrain_color`] 给出的任何真实
/// 地形色都明显可区分，读起来像「这里确实有地图，但你没去过」，而不是
/// 某种具体的地形。
pub const FOG_COLOR: [f32; 4] = [0.05, 0.05, 0.07, 1.0];

/// mod 注册的自定义地形查不到对应颜色时的回退色——刻意选一个不与任何
/// [`terrain_color`] 已列出的自然地形色接近的洋红，让「这种地形没有
/// 配色」在地图上直接可见，而不是悄悄复用某种自然地形的颜色掩盖过去。
const UNKNOWN_TERRAIN_COLOR: [f32; 4] = [0.85, 0.10, 0.85, 1.0];

/// 把地形种类映射到世界地图上的显示色。
///
/// 与 `ll_game::layout::terrain_entry_name` 同一套「按 [`BaseTerrainIds`]
/// 具名字段逐一比较」写法，理由也相同：[`TerrainKind`] 是注册期物化的
/// `ContentIndex`，数值由加载顺序决定，不能再写编译期 `match` 字面量
/// （见 `ll_world::terrain` 模块文档「从硬编码 match 到注册表」一节）。
/// 本函数只覆盖本体注册的自然地形——mod 注册的自定义地形没有专属配色，
/// 查不到时退化到 [`UNKNOWN_TERRAIN_COLOR`]。
pub fn terrain_color(kind: TerrainKind, ids: &BaseTerrainIds) -> [f32; 4] {
    if kind == ids.deep_water {
        [0.09, 0.20, 0.45, 1.0]
    } else if kind == ids.shallow_water {
        [0.20, 0.45, 0.70, 1.0]
    } else if kind == ids.sand {
        [0.82, 0.74, 0.48, 1.0]
    } else if kind == ids.grass {
        [0.30, 0.55, 0.25, 1.0]
    } else if kind == ids.forest {
        [0.13, 0.35, 0.16, 1.0]
    } else if kind == ids.hill {
        [0.55, 0.45, 0.30, 1.0]
    } else if kind == ids.mountain {
        [0.55, 0.55, 0.58, 1.0]
    } else if kind == ids.snow {
        [0.92, 0.92, 0.95, 1.0]
    } else if kind == ids.desert {
        // 与海岸 `sand` 刻意拉开：两者在地图上相邻的情形很常见（沙漠
        // 一直铺到海边），配成近似色会让玩家分不出「这里是海滩」还是
        // 「这里是沙漠」。沙漠取更饱和、更偏橙的一档。
        [0.88, 0.68, 0.32, 1.0]
    } else if kind == ids.tundra {
        // 与高山 `snow` 刻意拉开，理由同上：冻原是极地**低海拔**，雪地
        // 是雪线**以上**，两者在极圈附近会同框。冻原取带灰蓝的冷色，
        // 保留「冷」的读感但明显暗于雪白。
        [0.72, 0.78, 0.80, 1.0]
    } else if kind == ids.floor_stone {
        [0.45, 0.42, 0.40, 1.0]
    } else if kind == ids.wall_stone {
        [0.35, 0.33, 0.32, 1.0]
    } else {
        UNKNOWN_TERRAIN_COLOR
    }
}

/// 玩家位置标记的显示色——刻意选一个不与 [`terrain_color`] 任何自然
/// 地形色、也不与 [`FOG_COLOR`]/[`UNKNOWN_TERRAIN_COLOR`] 接近的暖橙：
/// 标记要在深蓝的海、深绿的林、灰白的雪山上**同样一眼可见**，因此不能
/// 取任何一种「某些底色上看得清、另一些上看不清」的颜色。
pub const PLAYER_MARKER_COLOR: [f32; 4] = [1.0, 0.55, 0.05, 1.0];

/// 玩家标记方块相对格子边长的内缩比例——每边各缩这么多，标记因此是
/// 格子正中一个边长为 `1 - 2 × 该值` 的小方块。
///
/// 取 0.25（标记占格子边长的一半）：再大就把这一格的地形整个盖住
/// （玩家看不出自己站在什么地形上），再小在最远档位那种小格子上会缩到
/// 一两个像素、等于没画。
const PLAYER_MARKER_INSET_FRACTION: f32 = 0.25;

/// 有人住的据点在地图上的显示色——明亮的暖白，读起来像「灯还亮着」。
pub const INHABITED_SITE_COLOR: [f32; 4] = [1.0, 0.93, 0.70, 1.0];

/// 废墟据点的显示色——冷灰，与 [`INHABITED_SITE_COLOR`] 一眼可分：
/// 「这里有过人」和「这里现在有人」对玩家挑落脚点是完全不同的信息。
pub const RUINED_SITE_COLOR: [f32; 4] = [0.62, 0.60, 0.58, 1.0];

/// 据点标记相对格子边长的内缩比例。
///
/// 比 [`PLAYER_MARKER_INSET_FRACTION`] 大（标记更小）：同一格里可能同时
/// 有据点和玩家，玩家标记必须压得住据点标记——「我在哪」是玩家最先要
/// 找的东西，不能被一串村庄点淹掉。
const SITE_MARKER_INSET_FRACTION: f32 = 0.34;

/// 一座要画在世界地图上的据点。
///
/// 只带「画在哪一格」与「还有没有人住」两条信息，不带据点的 id、人口、
/// 文化——呈现层需要的就这两条，多带一条就多一分让 UI 去解读世界数据
/// 的机会（那是 `ll_game::app` 装配这一层的职责）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMapSite {
    /// 落在哪一格（列, 行）。
    pub cell: (u32, u32),
    /// 是否仍有人居住（否即废墟）。
    pub inhabited: bool,
}

/// 世界地图这一帧需要的全部输入——`cells` 通常来自
/// `ll_world::overview::continent_map`，按行主序排列，长度恒等于
/// `cols * rows`（与 `OverviewCell` 自身「按行主序排列」的既有约定
/// 一致，见 `continent_map`/`minimap` 文档）。
pub struct WorldMapPanelData<'a> {
    /// 按行主序排列的格子。
    pub cells: &'a [OverviewCell],
    /// `cells` 的列数。
    pub cols: u32,
    /// `cells` 的行数。
    pub rows: u32,
    /// 地形 → 颜色查表需要的具名地形索引。
    pub terrain_ids: &'a BaseTerrainIds,
    /// 玩家当前落在哪个格子（列, 行），不在本屏视野内时为 `None`。
    ///
    /// # 为什么玩家位置从这里进来，而不是长在 `OverviewCell` 上
    ///
    /// 「玩家在哪」是**呈现**，不是世界事实：`ll_world::overview` 的
    /// `minimap`/`continent_map` 是只读纯查询（见其模块文档「为什么是
    /// 只读视图」），它们回答的是「世界长什么样」，把观察者塞进
    /// `OverviewCell` 会让每一个格子都多背一个与该格地形无关的字段，
    /// 也会让那两个函数从此需要知道「这是谁在看」。标记因此是呈现层
    /// **叠上去的一层**：数据由调用方（`ll_game::app`）用玩家坐标现算，
    /// 世界状态一个字节都不为它改动。
    ///
    /// 列行下标而不是世界坐标：这样标记与地形格用的是**同一个**坐标
    /// 系，不存在「格子按 A 算、标记按 B 算」而在缩放/接缝处错位的
    /// 可能——环面换算在算出这对下标之前就已经做完了。
    pub player: Option<(u32, u32)>,
    /// 这一帧要画的据点，按调用方给出的顺序绘制。
    ///
    /// # 顺序必须由调用方定死（约束 C5）
    ///
    /// 两座据点落在同一格时，后画的那个盖住先画的。若这个顺序来自任何
    /// 哈希容器的迭代，同一份世界在两次运行里可能画出不同的颜色。生产
    /// 调用方（`ll_game::app`）取的是 `WorldChronicle::sites()`——一个
    /// **已按区块光栅序排好的切片**（见其文档），顺序因此是世界数据
    /// 自身的确定性顺序，不是桶序。
    pub sites: &'a [WorldMapSite],
    /// 当前缩放档位下，一个地图格覆盖多少个世界瓦片——比例尺文案要显示
    /// 的就是这个数。为 0 时不显示比例尺（调用方还没接缩放）。
    pub tiles_per_cell: u32,
}

/// 网格在 `rect` 内的落位：左上角像素坐标与格子边长（正方形）。
///
/// 抽成独立类型是为了让格子矩形、玩家标记、以及反向的「像素落在哪一
/// 格」共用**同一份**几何，而不是各写一遍 `min` 与居中偏移——三处一旦
/// 分叉，标记就会画在离它该在的格子半格远的地方，而这种偏差在小格子上
/// 肉眼几乎看不出来。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapGrid {
    /// 网格左上角的屏幕 x 像素坐标。
    pub origin_x: f32,
    /// 网格左上角的屏幕 y 像素坐标。
    pub origin_y: f32,
    /// 格子边长（像素），横纵相同。
    pub cell_size: f32,
    /// 列数。
    pub cols: u32,
    /// 行数。
    pub rows: u32,
}

impl WorldMapGrid {
    /// 按 `rect` 与列行数现算网格落位。`cols`/`rows` 任一为零时返回
    /// `None`，不做除零运算。
    ///
    /// # 为什么不能各轴独立除（曾经的缺陷）
    ///
    /// 早先的实现分别用 `rect.width / cols`、`rect.height / rows` 算出
    /// 格宽与格高——`rect` 来自 `world_map_rect`（按屏幕原生宽高比现算，
    /// 见 `crate::hud::render::world_map_rect` 文档），一旦屏幕不是正方形
    /// （常态，例如 16:9），只要 `cols`/`rows` 的比例与屏幕宽高比不一致
    /// （同样是常态：`cols`/`rows` 来自世界的网格尺寸，与屏幕形状没有
    /// 任何关系），两个轴独立算出的格宽格高就不相等——所有者实测反馈
    /// 「世界地图格子不是正方形」正是这条路径。
    ///
    /// 现在取单一的 `cell_size = min(rect.width / cols, rect.height /
    /// rows)`，两个轴用同一个值，格子因此恒为正方形；网格整体尺寸通常
    /// 小于 `rect`，取居中留白（而不是拉伸填满、破坏正方形）处理多出来
    /// 的空间。
    pub fn new(rect: Rect, cols: u32, rows: u32) -> Option<Self> {
        if cols == 0 || rows == 0 {
            return None;
        }
        let cell_size = (rect.width / cols as f32).min(rect.height / rows as f32);
        let grid_width = cell_size * cols as f32;
        let grid_height = cell_size * rows as f32;
        Some(WorldMapGrid {
            origin_x: rect.x + (rect.width - grid_width) / 2.0,
            origin_y: rect.y + (rect.height - grid_height) / 2.0,
            cell_size,
            cols,
            rows,
        })
    }

    /// 某一格左上角的屏幕像素坐标。不校验下标是否越界——调用方拿到的
    /// 列行恒来自本网格自身的遍历或 [`Self::cell_at_pixel`]。
    fn cell_origin(&self, col: u32, row: u32) -> (f32, f32) {
        (
            self.origin_x + col as f32 * self.cell_size,
            self.origin_y + row as f32 * self.cell_size,
        )
    }

    /// 屏幕像素落在哪一格；落在网格之外（`rect` 内居中留出的空白、或
    /// 干脆在面板外）时返回 `None`。
    ///
    /// 像素坐标是浮点（呈现层允许，见 ADR 0002 的整数纪律只约束世界
    /// 状态相关的判定），但**产出的列行是整数**，且由 `floor` 得出而不
    /// 是四舍五入：格子是左闭右开的区间，四舍五入会让格子边界上的像素
    /// 归到隔壁格。
    pub fn cell_at_pixel(&self, pixel: (f32, f32)) -> Option<(u32, u32)> {
        if self.cell_size <= 0.0 {
            return None;
        }
        let local_x = pixel.0 - self.origin_x;
        let local_y = pixel.1 - self.origin_y;
        if local_x < 0.0 || local_y < 0.0 {
            return None;
        }
        let col = (local_x / self.cell_size).floor();
        let row = (local_y / self.cell_size).floor();
        if col >= self.cols as f32 || row >= self.rows as f32 {
            return None;
        }
        Some((col as u32, row as u32))
    }
}

/// 把一份世界地图数据变成这一帧要画的格子矩形——格子恒为正方形，
/// 网格在 `rect` 内居中，不铺满整个 `rect`（几何见 [`WorldMapGrid`]）。
/// 战争迷雾在这里生效：`explored` 为假的格子恒画 [`FOG_COLOR`]，见模块
/// 文档。
///
/// 地图格子没有真实贴图可采样（不是精灵动画，是按地形分类现算的纯
/// 色），因此恒产出 [`QuadInstance`]，不区分纯色/贴图两条路径——与
/// `crate::hud::render::push_day_night_bar` 里指针「恒是纯色矩形」
/// 同一个理由。
///
/// `cols`/`rows` 任一为零时返回空列表，不做除零运算。
pub fn world_map_cell_quads(data: &WorldMapPanelData<'_>, rect: Rect) -> Vec<QuadInstance> {
    let Some(grid) = WorldMapGrid::new(rect, data.cols, data.rows) else {
        return Vec::new();
    };
    data.cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let col = (index as u32) % data.cols;
            let row = (index as u32) / data.cols;
            let color = if cell.explored {
                terrain_color(cell.terrain, data.terrain_ids)
            } else {
                FOG_COLOR
            };
            let (x, y) = grid.cell_origin(col, row);
            QuadInstance {
                position: [x, y],
                size: [grid.cell_size, grid.cell_size],
                color,
            }
        })
        .collect()
}

/// 玩家位置标记这一帧的矩形——`data.player` 为 `None`（玩家不在本屏
/// 视野内）或列行越界时返回空列表。
///
/// # 为什么标记不受战争迷雾影响
///
/// 迷雾遮的是「世界长什么样」这个玩家还不知道的信息；「我自己在哪」
/// 玩家一直都知道，遮住它只会让地图对刚开局的玩家彻底无用（整屏全黑
/// 且没有任何参照点）。这与 `world_map_cell_quads` 对地形「没探索就
/// 不告诉你是什么」并不矛盾：两者遮的不是同一类信息。
///
/// 标记是格子正中一个内缩的小方块（[`PLAYER_MARKER_INSET_FRACTION`]），
/// 不是整格填色——底下那一格的地形仍然露得出来，玩家因此能同时看出
/// 「我在哪」和「我脚下是什么」。
pub fn player_marker_quads(data: &WorldMapPanelData<'_>, rect: Rect) -> Vec<QuadInstance> {
    let Some((col, row)) = data.player else {
        return Vec::new();
    };
    let Some(grid) = WorldMapGrid::new(rect, data.cols, data.rows) else {
        return Vec::new();
    };
    if col >= grid.cols || row >= grid.rows {
        return Vec::new();
    }
    let inset = grid.cell_size * PLAYER_MARKER_INSET_FRACTION;
    let (x, y) = grid.cell_origin(col, row);
    vec![QuadInstance {
        position: [x + inset, y + inset],
        size: [grid.cell_size - inset * 2.0, grid.cell_size - inset * 2.0],
        color: PLAYER_MARKER_COLOR,
    }]
}

/// 据点标记这一帧的矩形。
///
/// # 战争迷雾**照样生效**（与玩家标记相反）
///
/// 玩家标记不受迷雾影响，因为「我自己在哪」玩家一直都知道；据点不是
/// ——「那边山谷里有座村子」正是探索要换来的东西，开局就全图显示等于
/// 把这份游戏内容白送。因此本函数对每个标记先查它所在那一格的
/// `explored`，未探索就整个跳过，与 [`world_map_cell_quads`] 对地形的
/// 处理是同一条规则。
///
/// # 给「开局在地图上选重生点」那批的说明
///
/// 那个界面需要「全图可见」才有意义（玩家还没探索过任何地方，却要据此
/// 挑落脚点）。**不需要给本函数加任何 `reveal_all` 旗标**：
/// `ll_world::world_map::world_map_slice` 与
/// `ll_world::overview::continent_map` 都要求调用方**显式传入**一份
/// `&ExplorationMemory`（见 `ll_world::exploration` 模块文档「为什么读取
/// 接口要求显式传入」），选点界面只要传一份「全部已探索」的记忆进来，
/// `explored` 就恒为真，同一份呈现代码自然变成全图可见。
pub fn site_marker_quads(data: &WorldMapPanelData<'_>, rect: Rect) -> Vec<QuadInstance> {
    let Some(grid) = WorldMapGrid::new(rect, data.cols, data.rows) else {
        return Vec::new();
    };
    data.sites
        .iter()
        .filter(|site| site.cell.0 < grid.cols && site.cell.1 < grid.rows)
        .filter(|site| {
            data.cells
                .get((site.cell.1 * data.cols + site.cell.0) as usize)
                .is_some_and(|cell| cell.explored)
        })
        .map(|site| {
            let inset = grid.cell_size * SITE_MARKER_INSET_FRACTION;
            let (x, y) = grid.cell_origin(site.cell.0, site.cell.1);
            QuadInstance {
                position: [x + inset, y + inset],
                size: [grid.cell_size - inset * 2.0, grid.cell_size - inset * 2.0],
                color: if site.inhabited {
                    INHABITED_SITE_COLOR
                } else {
                    RUINED_SITE_COLOR
                },
            }
        })
        .collect()
}

/// 比例尺与操作提示这一行文案。
///
/// # 为什么比例尺是必要的，不是装饰
///
/// 缩放之后，同一块屏幕面积代表的世界范围变了，但画面本身看不出这一点
/// ——格子数恒定、格子像素尺寸恒定（见
/// `ll_world::world_map::ZOOM_LADDER` 文档）。没有这行字，玩家分不清
/// 自己在看整个世界还是八分之一个世界，「放大」这个操作因此失去参照。
///
/// # 文案全部走 i18n
///
/// 规格 §11.3：代码中不得出现任何硬编码的用户可见字符串
/// （`scripts/ci/check_i18n_strings.py` 门禁）。本函数只做拼接与数字
/// 格式化，两段文字都从 [`Catalog`] 里解析。
pub fn scale_caption(tiles_per_cell: u32, catalog: &Catalog, language: &str) -> String {
    let scale = catalog.resolve(language, "hud-world-map-scale-label");
    let hint = catalog.resolve(language, "hud-world-map-hint");
    format!("{scale} 1:{tiles_per_cell}   {hint}")
}

/// 屏幕像素 → 世界**区块**（`ZoneCoord`）。落在网格之外时返回 `None`。
///
/// # 这个函数是给谁用的
///
/// **下一批「开局在地图上选重生点」。** 那批要做的是所有者裁定的这条：
///
/// > 「重生点就是随机点一个格子，然后在那区块内随机出生在陆地上。」
///
/// 也就是「玩家在地图上点一个 **zone**，然后在那个区块内随机挑一格陆地
/// 出生」。本函数就是「点了哪里」到「哪个 zone」的唯一换算，是那条链路
/// 的第一步。本批**不做**选点交互本身，只把换算做成一个有测试的公开
/// 函数交出去。
///
/// # 参数为什么长这样
///
/// `rect` 与 `skin` 与 [`world_map_frame`] 的**同名参数完全一致**——传
/// 面板外框矩形，本函数自己按皮肤的边框粗细内缩，得到与格子实际绘制时
/// 一模一样的内容矩形。若改成让调用方传已经内缩过的矩形，就多出一处
/// 「谁负责内缩」的约定，一旦两边理解不同，玩家点的地方与选中的区块会
/// 系统性偏移一个边框宽度——而这种偏差小到肉眼看不出来，只会表现为
/// 「偶尔点到隔壁那格」。
///
/// `slice` 而不是 [`WorldMapPanelData`]：反算需要视野原点与缩放档位，
/// 那些长在 `ll_world::world_map::WorldMapSlice` 上（`cells`/`cols`/
/// `rows` 只是它的一部分被抄进面板数据）。直接吃切片，就不可能出现
/// 「面板数据与切片来自不同的两帧」这种错位。
///
/// # 整数纪律（ADR 0002）
///
/// 像素 → 列行是浮点（呈现层允许：屏幕坐标本来就是像素），但**列行
/// 之后全程整数**——列行 → 采样点 → 瓦片 → 区块由
/// `WorldMapSlice::zone_at_cell` 完成，那条链上没有任何一步是浮点，
/// 环绕全部走 `TorusSize`，一次手写取模都没有。
pub fn world_map_zone_at_pixel(
    slice: &WorldMapSlice,
    layout: &ZoneLayout,
    rect: Rect,
    skin: &dyn Skin,
    pixel: (f32, f32),
) -> Option<ZoneCoord> {
    let content_rect = rect.inset(skin.panel(PanelStyleId::Window).border_thickness);
    let grid = WorldMapGrid::new(content_rect, slice.cols, slice.rows)?;
    let (col, row) = grid.cell_at_pixel(pixel)?;
    slice.zone_at_cell(layout, col, row)
}

/// 世界地图整块面板这一帧的产出：边框 + 格子，恒是纯色矩形。
pub struct WorldMapFrame {
    /// 边框 + 格子的全部填色矩形。
    pub quads: Vec<QuadInstance>,
}

/// 只画四条边框窄条（不含中心填充）——与
/// `crate::widget::panel::panel_quads` 的九宫格不同，本函数刻意不产出
/// 中心填充块，见 [`world_map_frame`] 文档「为什么不用九宫格面板背景」
/// 一节。
fn border_only_quads(rect: Rect, color: [f32; 4], thickness: f32) -> Vec<QuadInstance> {
    let inner = rect.inset(thickness);
    vec![
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, thickness],
            color,
        },
        QuadInstance {
            position: [rect.x, rect.bottom() - thickness],
            size: [rect.width, thickness],
            color,
        },
        QuadInstance {
            position: [rect.x, inner.y],
            size: [thickness, inner.height],
            color,
        },
        QuadInstance {
            position: [rect.right() - thickness, inner.y],
            size: [thickness, inner.height],
            color,
        },
    ]
}

/// 建出世界地图这一整块面板：边框 + 格子。
///
/// # 为什么不用 [`crate::widget::panel::panel_quads`] 那套九宫格面板背景
///
/// 四块常驻 HUD 面板（状态栏/角色/背包/装备栏）背景与内容分处两个不同
/// 的东西——背景是矩形，内容是文本行，文本经 [`ll_text::TextRenderer`]
/// 在第三道渲染 pass（`crate::hud::render::render_hud` 里最后提交）绘制，
/// 天然画在任何背景之上，背景选纯色还是贴图（两道 pass 谁先谁后)不影响
/// 结果，见 `render_hud` 文档「三道 pass」一节。
///
/// 世界地图的内容不是文本，是与背景**同属 `QuadInstance` 这一层**的格子
/// 矩形——若背景仍然套用九宫格面板（[`crate::widget::panel::panel_quads`]/
/// `crate::widget::panel::textured_panel_quads`），中心填充块与格子矩形
/// 会覆盖同一块区域，而两道 pass（纯色/贴图）按固定顺序先后提交,不由
/// `quads`/`textured_quads` 两个切片内部的推入顺序决定——若皮肤恰好给出
/// 贴图背景（贴图 pass 在纯色 pass 之后提交),贴图中心填充块会整个盖住
/// 本该在它之上的格子矩形,把世界地图渲染成一块纯色/贴图矩形,格子完全
/// 不可见。本函数因此刻意只画边框（[`border_only_quads`]，不含中心
/// 填充)、格子恒画在边框内侧——两者都在同一个 `quads` 切片、同一道纯色
/// pass 里，推入顺序（边框先、格子后）就能保证格子盖在边框之上，不存在
/// 跨 pass 的顺序不确定性。
pub fn world_map_frame(data: &WorldMapPanelData<'_>, rect: Rect, skin: &dyn Skin) -> WorldMapFrame {
    let appearance = skin.panel(PanelStyleId::Window);
    let mut quads = border_only_quads(rect, appearance.border_color, appearance.border_thickness);
    let content_rect = rect.inset(appearance.border_thickness);
    quads.extend(world_map_cell_quads(data, content_rect));
    // 推入顺序 = 遮挡顺序（本函数文档「为什么不用九宫格面板背景」一节
    // 同一条理由：同一道纯色 pass 里，后推的盖住先推的）。因此顺序是
    // 地形格 → 据点 → 玩家：标记恒盖在地形之上而不会被后画的邻格盖住，
    // 而玩家又恒盖在据点之上——同一格里同时有村子和玩家时，「我在哪」
    // 是玩家最先要找的东西。
    quads.extend(site_marker_quads(data, content_rect));
    quads.extend(player_marker_quads(data, content_rect));
    WorldMapFrame { quads }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::skin::FlatColorSkin;
    use ll_world::terrain::base_terrain_fixture;

    fn sample_cell(terrain: TerrainKind, explored: bool) -> OverviewCell {
        OverviewCell { terrain, explored }
    }

    #[test]
    fn 格子数为零列时不产出任何矩形() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 0,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Assert
        assert!(quads.is_empty());
    }

    #[test]
    fn 格子矩形数等于列数乘行数() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [
            sample_cell(ids.grass, true),
            sample_cell(ids.deep_water, true),
            sample_cell(ids.mountain, true),
            sample_cell(ids.sand, true),
        ];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 2,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Assert
        assert_eq!(quads.len(), 4);
    }

    #[test]
    fn 非正方形面板与非正方形网格下格子仍为正方形() {
        // 这是所有者实测反馈的缺陷本身：面板矩形按屏幕宽高比现算，
        // 与地图的列数/行数比例通常不一致（这里故意选一个宽屏矩形
        // 200x100 配 3 列 2 行——3:2 与 200:100=2:1 不相等），若格宽
        // 格高各自独立除（旧实现），两者就会不等；改用统一的
        // `cell_size` 后必须恒相等，不分辨率如何。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 6];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 3,
            rows: 2,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Assert
        for quad in &quads {
            assert_eq!(quad.size[0], quad.size[1]);
        }
    }

    #[test]
    fn 已探索格子按其真实地形着色() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert_eq!(quads[0].color, terrain_color(ids.grass, &ids));
    }

    #[test]
    fn 未探索格子恒画战争迷雾色而不是真实地形色() {
        // 这是战争迷雾在世界地图上生效的核心断言：即使这一格的真实
        // 地形是草地，只要没探索过，画出来的颜色也必须是 FOG_COLOR，
        // 不能泄漏 terrain_color(grass) 这个真实答案。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, false)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert_eq!(quads[0].color, FOG_COLOR);
        assert_ne!(quads[0].color, terrain_color(ids.grass, &ids));
    }

    #[test]
    fn 同一份格子里已探索与未探索互不影响各自的着色() {
        // Arrange：两格真实地形相同，探索状态不同。
        let (ids, _table) = base_terrain_fixture();
        let cells = [
            sample_cell(ids.mountain, true),
            sample_cell(ids.mountain, false),
        ];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 20.0, 10.0));

        // Assert
        assert_eq!(quads[0].color, terrain_color(ids.mountain, &ids));
        assert_eq!(quads[1].color, FOG_COLOR);
    }

    #[test]
    fn 未注册的地形回退到显眼的未知色而不是复用某种自然地形色() {
        // Arrange：一个不等于 BaseTerrainIds 任何具名字段的 TerrainKind。
        // ContentIndex 没有公开的裸整数构造函数（见 `ll_core::ident`
        // 模块文档：索引只能来自 `Interner::intern` 的插入顺序），因此
        // 不能凭空捏造一个「看起来没被用过」的索引——必须用
        // `materialize_base_terrain` 同一个 interner 再 intern 一个全新
        // 的命名空间 ID：interner 保证同一个 ID 只会被分配一次索引，
        // 这个新索引因此必然与 `ids` 里已登记的 17 个具名字段互不相同。
        let mut interner = ll_core::ident::Interner::new();
        let (ids, _table) =
            ll_world::terrain::materialize_base_terrain(&mut |id| interner.intern(id))
                .expect("本体地形声明表内部一致，注册恒不失败");
        let extra_id = ll_core::ident::NamespacedId::parse("lostland_test:unregistered_terrain")
            .expect("字面量恒合法");
        let unregistered = TerrainKind::from_index(interner.intern(extra_id));
        let cells = [sample_cell(unregistered, true)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert_eq!(quads[0].color, UNKNOWN_TERRAIN_COLOR);

        // Cleanup: 无——`interner`/`extra_id` 只是本地栈上的值。
    }

    #[test]
    fn 有人住的据点与废墟画成不同颜色() {
        // 「那边有座村子」和「那边只剩废墟」对玩家挑落脚点是完全不同的
        // 信息，两者必须一眼可分。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 4];
        let sites = [
            WorldMapSite {
                cell: (0, 0),
                inhabited: true,
            },
            WorldMapSite {
                cell: (1, 0),
                inhabited: false,
            },
        ];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 2,
            terrain_ids: &ids,
            player: None,
            sites: &sites,
            tiles_per_cell: 0,
        };

        // Act
        let quads = site_marker_quads(&data, Rect::new(0.0, 0.0, 200.0, 200.0));

        // Assert
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].color, INHABITED_SITE_COLOR);
        assert_eq!(quads[1].color, RUINED_SITE_COLOR);
        assert_ne!(INHABITED_SITE_COLOR, RUINED_SITE_COLOR);
    }

    #[test]
    fn 未探索格上的据点不画因此不泄漏战争迷雾() {
        // 「那边山谷里有座村子」正是探索要换来的东西。开局就全图显示
        // 等于把这份游戏内容白送，也与本模块对地形「没去过就不告诉你
        // 是什么」直接矛盾。
        // Arrange：两格地形相同、都有据点，只有第一格探索过。
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true), sample_cell(ids.grass, false)];
        let sites = [
            WorldMapSite {
                cell: (0, 0),
                inhabited: true,
            },
            WorldMapSite {
                cell: (1, 0),
                inhabited: true,
            },
        ];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &sites,
            tiles_per_cell: 0,
        };

        // Act
        let quads = site_marker_quads(&data, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Assert：只画探索过的那一座。
        assert_eq!(quads.len(), 1);
        assert_eq!(
            quads[0].position[0],
            0.0 + 100.0 * SITE_MARKER_INSET_FRACTION
        );
    }

    #[test]
    fn 据点标记比玩家标记小因此同格时玩家压得住() {
        // Arrange：同一格上既有据点又有玩家。
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let sites = [WorldMapSite {
            cell: (0, 0),
            inhabited: true,
        }];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: Some((0, 0)),
            sites: &sites,
            tiles_per_cell: 0,
        };
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);

        // Act
        let site = site_marker_quads(&data, rect);
        let player = player_marker_quads(&data, rect);

        // Assert：玩家标记严格更大，且在整块面板里排在据点之后。
        assert!(player[0].size[0] > site[0].size[0]);
        let frame = world_map_frame(&data, rect, &FlatColorSkin);
        let site_index = frame
            .quads
            .iter()
            .position(|q| q.color == INHABITED_SITE_COLOR)
            .expect("据点标记必须在这一帧里");
        let player_index = frame
            .quads
            .iter()
            .position(|q| q.color == PLAYER_MARKER_COLOR)
            .expect("玩家标记必须在这一帧里");
        assert!(
            site_index < player_index,
            "玩家标记必须推在据点之后才能盖住它"
        );
    }

    #[test]
    fn 据点列行越界时跳过而不是画到网格外() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let sites = [WorldMapSite {
            cell: (5, 5),
            inhabited: true,
        }];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &sites,
            tiles_per_cell: 0,
        };

        // Act
        let quads = site_marker_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Assert
        assert!(quads.is_empty());
    }

    #[test]
    fn 据点绘制顺序逐位跟随调用方给出的顺序() {
        // 约束 C5 在呈现层的落点：同一格上的两座据点谁盖住谁，必须由
        // 调用方给出的确定性顺序决定（生产调用方给的是编年史按区块光栅
        // 序排好的切片），不能由任何哈希容器的桶序决定。本函数只做
        // `filter` + `map`，不排序、不去重——这条测试锁住这一点。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let forward = [
            WorldMapSite {
                cell: (0, 0),
                inhabited: true,
            },
            WorldMapSite {
                cell: (0, 0),
                inhabited: false,
            },
        ];
        let backward = [forward[1], forward[0]];
        let make = |sites: &[WorldMapSite]| -> Vec<[f32; 4]> {
            let data = WorldMapPanelData {
                cells: &cells,
                cols: 1,
                rows: 1,
                terrain_ids: &ids,
                player: None,
                sites,
                tiles_per_cell: 0,
            };
            site_marker_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0))
                .into_iter()
                .map(|q| q.color)
                .collect()
        };

        // Act
        let a = make(&forward);
        let b = make(&backward);

        // Assert：顺序原样保留，因此两种输入产出的顺序不同。
        assert_eq!(a, vec![INHABITED_SITE_COLOR, RUINED_SITE_COLOR]);
        assert_eq!(b, vec![RUINED_SITE_COLOR, INHABITED_SITE_COLOR]);
    }

    #[test]
    fn 比例尺文案不硬编码走本地化目录并带上每格瓦片数() {
        // 规格 §11.3：代码里不得出现硬编码的用户可见字符串。这条测试
        // 用一份只有两条键的临时目录，确认文案真的是查出来的——若哪天
        // 有人把字面量写回代码里，解析结果就不再等于这里给的值。
        // Arrange
        let dir =
            std::env::temp_dir().join(format!("ll-ui-world-map-caption-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        std::fs::write(
            dir.join("zh-CN.ftl"),
            "hud-world-map-scale-label = 比例尺\nhud-world-map-hint = 提示\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load_dir(&dir);

        // Act
        let caption = scale_caption(96, &catalog, "zh-CN");

        // Assert
        assert!(caption.contains("比例尺"), "标签必须来自本地化目录");
        assert!(caption.contains("提示"), "提示必须来自本地化目录");
        assert!(caption.contains("96"), "必须带上每格覆盖的瓦片数");

        // Cleanup
        std::fs::remove_dir_all(&dir).expect("测试用临时目录应当能删掉");
    }

    #[test]
    fn world_map_frame的矩形数等于四条边框加格子数() {
        // Arrange：border_only_quads 恒产出 4 块（上下左右各一条），
        // 见其文档——与 `panel_quads` 的九宫格不同,世界地图刻意不画
        // 中心填充块（见 `world_map_frame` 文档「为什么不用九宫格面板
        // 背景」一节),这条测试直接锁住块数,防止未来有人把边框实现
        // 悄悄换回带中心填充的九宫格,重新引入格子被背景盖住的缺陷。
        let (ids, _table) = base_terrain_fixture();
        let cells = [
            sample_cell(ids.grass, true),
            sample_cell(ids.deep_water, false),
        ];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let frame = world_map_frame(&data, Rect::new(0.0, 0.0, 200.0, 100.0), &FlatColorSkin);

        // Assert
        assert_eq!(frame.quads.len(), 4 + 2);
    }

    #[test]
    fn 玩家标记画在它所在那一格的正中且比格子小() {
        // Arrange：3x3 网格，玩家在中间那格 (1,1)。
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 9];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 3,
            rows: 3,
            terrain_ids: &ids,
            sites: &[],
            tiles_per_cell: 0,
            player: Some((1, 1)),
        };
        let rect = Rect::new(0.0, 0.0, 300.0, 300.0);

        // Act
        let marker = player_marker_quads(&data, rect);

        // Assert：格边长 100，内缩 25% → 标记在 (125,125) 处、边长 50。
        assert_eq!(marker.len(), 1);
        assert_eq!(marker[0].color, PLAYER_MARKER_COLOR);
        assert_eq!(marker[0].position, [125.0, 125.0]);
        assert_eq!(marker[0].size, [50.0, 50.0]);
    }

    #[test]
    fn 玩家不在视野内时不画标记() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 4];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 2,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let marker = player_marker_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Assert
        assert!(marker.is_empty());
    }

    #[test]
    fn 玩家列行越界时不画标记而不是画到网格外() {
        // 越界不该 panic，也不该画出一个落在网格之外的橙块——那看起来
        // 像是地图边上多了一个没来由的装饰，比不画更糟。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 4];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 2,
            terrain_ids: &ids,
            sites: &[],
            tiles_per_cell: 0,
            player: Some((2, 0)),
        };

        // Act
        let marker = player_marker_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Assert
        assert!(marker.is_empty());
    }

    #[test]
    fn 玩家标记恒画在地形格之后因此不会被邻格盖住() {
        // 「标记盖在格子之上」这条不变式靠推入顺序保证（见
        // `world_map_frame` 文档），这条测试核实真的是先格子、后标记。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true); 4];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 2,
            terrain_ids: &ids,
            sites: &[],
            tiles_per_cell: 0,
            player: Some((0, 0)),
        };

        // Act
        let frame = world_map_frame(&data, Rect::new(0.0, 0.0, 200.0, 200.0), &FlatColorSkin);

        // Assert：4 条边框 + 4 个格子 + 1 个标记，标记恒是最后一块。
        assert_eq!(frame.quads.len(), 4 + 4 + 1);
        assert_eq!(
            frame.quads.last().expect("刚断言过至少九块").color,
            PLAYER_MARKER_COLOR
        );
    }

    #[test]
    fn 玩家标记不受战争迷雾影响() {
        // 迷雾遮的是「世界长什么样」，不是「我自己在哪」——玩家所在的
        // 那一格哪怕标着未探索（刚开局、探索记忆还没写进去的那一帧），
        // 标记也必须画出来，见 `player_marker_quads` 文档。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, false)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            sites: &[],
            tiles_per_cell: 0,
            player: Some((0, 0)),
        };

        // Act
        let marker = player_marker_quads(&data, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Assert
        assert_eq!(marker.len(), 1);
        assert_eq!(marker[0].color, PLAYER_MARKER_COLOR);
    }

    #[test]
    fn world_map_frame的格子矩形排在边框之后() {
        // 「格子盖在边框之上」这条不变式靠推入顺序保证（见
        // `world_map_frame` 文档），这条测试核实真的是先边框、后格子，
        // 而不是恰好数量对但顺序反了。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let cells = [sample_cell(ids.grass, true)];
        let data = WorldMapPanelData {
            cells: &cells,
            cols: 1,
            rows: 1,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };
        let border_color = FlatColorSkin.panel(PanelStyleId::Window).border_color;

        // Act
        let frame = world_map_frame(&data, Rect::new(0.0, 0.0, 100.0, 100.0), &FlatColorSkin);

        // Assert：前 4 块是边框色,最后 1 块是格子的地形色,两者不同色
        // 才具备区分度（`FlatColorSkin` 边框色与草地色本就不同）。
        assert!(frame.quads[0..4].iter().all(|q| q.color == border_color));
        assert_eq!(frame.quads[4].color, terrain_color(ids.grass, &ids));
    }
}

/// 「屏幕像素 → zone」换算的属性测试（规格 §14.2 点名环面坐标要有属性
/// 测试）。
///
/// 单独成一个 `mod`：它需要一份真实的 `ContinentField`（因此要建噪声、
/// 建布局），与上面那些只喂几个 `OverviewCell` 字面量的单元测试是两种
/// 完全不同的搭建成本。
#[cfg(test)]
mod pixel_to_zone_properties {
    use super::*;
    use crate::widget::skin::FlatColorSkin;
    use ll_world::exploration::ExplorationMemory;
    use ll_world::generate::{GenParams, build_zone_noise};
    use ll_world::overview::{ContinentField, generate_continent_field};
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::world_map::{WorldMapView, world_map_slice};
    use ll_world::zone::ZoneLayout;
    use proptest::prelude::*;

    /// 测试世界：8×8 个区块、区块边长 48（与生产默认值一致），采样场
    /// 因此是 32×32 个采样点。
    ///
    /// 建一次存进 `OnceLock`：每个 proptest 用例都重建一份噪声场是纯粹
    /// 的浪费（几百次噪声采样 × 上百个用例），而这份数据在整个测试期间
    /// 完全不变。
    fn world() -> &'static (ZoneLayout, ContinentField) {
        static WORLD: std::sync::OnceLock<(ZoneLayout, ContinentField)> =
            std::sync::OnceLock::new();
        WORLD.get_or_init(|| {
            let zone_count = ll_core::torus::TorusSize::new(8, 8).expect("8x8 是合法尺寸");
            let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束");
            let (terrain_ids, _table) = base_terrain_fixture();
            let params = GenParams::default();
            let noise = build_zone_noise(&layout, &params).expect("测试布局满足全部约束");
            let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
            (layout, field)
        })
    }

    /// 按给定的档位与平移量建一屏切片。
    fn slice_at(level: usize, pan_x: i32, pan_y: i32) -> ll_world::world_map::WorldMapSlice {
        let (layout, field) = world();
        let mut view = WorldMapView::centered_on_tile(field, layout.tile_size().wrap(0, 0));
        for _ in 0..level {
            view.zoom_in();
        }
        view.pan(field, pan_x, pan_y);
        world_map_slice(field, layout, &ExplorationMemory::new(), &view)
    }

    /// 面板矩形：刻意选一个宽高比与网格宽高比都对不上的尺寸，让居中
    /// 留白真的存在（网格铺不满 `rect`），这样「落在留白里的像素返回
    /// `None`」这条路径才会被真正走到。
    const PANEL: Rect = Rect {
        x: 37.0,
        y: 19.0,
        width: 901.0,
        height: 613.0,
    };

    #[test]
    fn 像素反算用的几何与实际画出的格子逐格一致() {
        // 这是「反算用的几何与画格子用的几何是同一份」这条不变式最直接
        // 的断言：拿 `world_map_frame` **真正画出来**的每一块格子矩形，
        // 用它自身左上角往内一个像素处去反算，必须得到那一格的区块。
        //
        // 为什么取左上角 + 1 像素，不取格子中心：中心离两侧边界都有半格
        // 远（本例格宽 150 像素上下），几个像素的系统性偏移根本推不动
        // 它——贴着边界取样才对小偏移有分辨力。
        //
        // **不要把这一条当成边框内缩的守卫**：本批做 ADR 0018 反例验证
        // 时，把 `world_map_zone_at_pixel` 里的「按边框粗细内缩」整个
        // 删掉，本条测试照样全绿（真正咬住那处破坏的是
        // `网格之外的像素返回空而不是钳到最近的一格`）。本条守的是
        // 「画出来的格子与反算出的格子一一对应」这条整体自洽性。
        // Arrange
        let (layout, field) = world();
        let mut view = WorldMapView::centered_on_tile(field, layout.tile_size().wrap(0, 0));
        view.zoom_in();
        let slice = world_map_slice(field, layout, &ExplorationMemory::new(), &view);
        let (ids, _table) = base_terrain_fixture();
        let data = WorldMapPanelData {
            cells: &slice.cells,
            cols: slice.cols,
            rows: slice.rows,
            terrain_ids: &ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let frame = world_map_frame(&data, PANEL, &FlatColorSkin);

        // Assert：前 4 块是边框（见 `border_only_quads`），其后按行主序
        // 是格子。
        let cell_quads = &frame.quads[4..4 + (slice.cols * slice.rows) as usize];
        for (index, quad) in cell_quads.iter().enumerate() {
            let col = index as u32 % slice.cols;
            let row = index as u32 / slice.cols;
            let pixel = (quad.position[0] + 1.0, quad.position[1] + 1.0);
            assert_eq!(
                world_map_zone_at_pixel(&slice, layout, PANEL, &FlatColorSkin, pixel),
                slice.zone_at_cell(layout, col, row),
                "第 ({col}, {row}) 格画在 {:?}，反算却指向别处",
                quad.position
            );
        }
    }

    #[test]
    fn 网格之外的像素返回空而不是钳到最近的一格() {
        // 网格在面板内**居中**，四周通常有留白（格子恒为正方形，见
        // `WorldMapGrid::new` 文档）。点在留白或面板之外，正确答案是
        // 「什么都没点到」——钳到最近的一格会让玩家在地图边上随手一点
        // 就选中边缘的区块，而他其实什么都没瞄准。对下一批的选重生点
        // 来说，这是一次不可撤销的误选。
        // Arrange
        let (layout, field) = world();
        let view = WorldMapView::centered_on_tile(field, layout.tile_size().wrap(0, 0));
        let slice = world_map_slice(field, layout, &ExplorationMemory::new(), &view);
        let content = PANEL.inset(FlatColorSkin.panel(PanelStyleId::Window).border_thickness);
        let grid = WorldMapGrid::new(content, slice.cols, slice.rows).expect("列行恒非零");
        // 前置：网格确实没铺满面板，左侧真的存在留白。
        assert!(
            grid.origin_x > content.x,
            "前置：本例的面板宽高比应当留出居中空白"
        );

        // Act & Assert
        let outside = [
            (grid.origin_x - 1.0, grid.origin_y + 1.0),
            (
                grid.origin_x + grid.cell_size * grid.cols as f32 + 1.0,
                grid.origin_y + 1.0,
            ),
            (
                grid.origin_x + 1.0,
                grid.origin_y + grid.cell_size * grid.rows as f32 + 1.0,
            ),
            (-500.0, -500.0),
        ];
        for pixel in outside {
            assert_eq!(
                world_map_zone_at_pixel(&slice, layout, PANEL, &FlatColorSkin, pixel),
                None,
                "网格之外的像素 {pixel:?} 不该选中任何区块"
            );
        }
    }

    proptest! {
        /// 任意缩放档位 × 任意平移 × 任意像素，结果要么是 `None`，要么
        /// 是一个**合法**的区块坐标。
        ///
        /// 「合法」在这里是硬要求而不是形式主义：下一批会拿这个返回值
        /// 直接去 `WorldChronicle`/地形场里查那个区块，越界的坐标要么
        /// panic、要么静默读到别的区块的数据。
        #[test]
        fn 任意档位与平移下像素换算出的区块恒落在合法范围内(
            level in 0usize..4,
            pan_x in -40i32..40,
            pan_y in -40i32..40,
            px in -200.0f32..1200.0,
            py in -200.0f32..900.0,
        ) {
            let (layout, _field) = world();
            let slice = slice_at(level, pan_x, pan_y);

            let zone = world_map_zone_at_pixel(&slice, layout, PANEL, &FlatColorSkin, (px, py));

            if let Some(zone) = zone {
                let zone_count = layout.zone_count();
                prop_assert!(zone.x() >= 0 && (zone.x() as u32) < zone_count.width());
                prop_assert!(zone.y() >= 0 && (zone.y() as u32) < zone_count.height());
            }
        }

        /// 网格**之内**的像素恒有答案——`None` 只该出现在留白或面板外。
        ///
        /// 少了这一条，上一条断言可以被一个「恒返回 `None`」的退化实现
        /// 满足（ADR 0018 的同一条思路：断言必须能被改坏的实现咬到）。
        #[test]
        fn 网格之内的像素恒能换算出一个区块(
            level in 0usize..4,
            pan_x in -40i32..40,
            pan_y in -40i32..40,
            col_frac in 0.0f32..1.0,
            row_frac in 0.0f32..1.0,
        ) {
            let (layout, _field) = world();
            let slice = slice_at(level, pan_x, pan_y);
            let content = PANEL.inset(FlatColorSkin.panel(PanelStyleId::Window).border_thickness);
            let grid = WorldMapGrid::new(content, slice.cols, slice.rows)
                .expect("列行恒非零");
            // 取网格内部一个点：按比例落在整片网格范围内，再往内缩半个
            // 像素避开右下边界（格子是左闭右开的区间）。
            let px = grid.origin_x + col_frac * (grid.cell_size * grid.cols as f32 - 0.5);
            let py = grid.origin_y + row_frac * (grid.cell_size * grid.rows as f32 - 0.5);

            let zone = world_map_zone_at_pixel(&slice, layout, PANEL, &FlatColorSkin, (px, py));

            prop_assert!(zone.is_some(), "网格内的像素 ({px}, {py}) 换算不出区块");
        }

        /// **接缝两侧连续**：屏幕上横向相邻的两格，换算出的区块在环面上
        /// 的 Chebyshev 距离不超过「一格覆盖多少个区块」。
        ///
        /// 这条是环面正确性的核心断言。视野跨接缝时，相邻两格的区块坐标
        /// 在数值上可能是 7 和 0（世界宽 8 个区块）——朴素的相减会得到
        /// 7，判定为「不连续」；`TorusSize::chebyshev` 得到 1，因为环面上
        /// 它们本来就是紧邻的。用它来断言，等于同时要求实现也走环面语义：
        /// 任何一处忘了环绕，都会让某一对相邻格的距离炸成整个世界宽。
        ///
        /// 纵向不该有任何位移——只往右挪一列，行号没变。
        #[test]
        fn 屏幕上相邻的两格换算出的区块在环面上也相邻(
            level in 0usize..4,
            pan_x in -40i32..40,
            pan_y in -40i32..40,
            col_pick in 0u32..64,
            row_pick in 0u32..64,
        ) {
            let (layout, field) = world();
            let slice = slice_at(level, pan_x, pan_y);
            // 取余而不是 `prop_assume!`：视野只有几列几行，直接生成一个
            // 大范围的列号会让绝大多数用例被拒，proptest 会因为「拒绝
            // 太多」直接放弃整条属性（本批实测：1024 次全局拒绝）。
            let col = col_pick % (slice.cols - 1);
            let row = row_pick % slice.rows;

            let left = slice.zone_at_cell(layout, col, row).expect("刚断言过在范围内");
            let right = slice.zone_at_cell(layout, col + 1, row).expect("刚断言过在范围内");

            // 一格覆盖多少个区块（向上取整）——最远档位下一格 96 个瓦片、
            // 区块 48 个瓦片，因此相邻两格的区块相差 2。
            let tiles_per_cell = slice.samples_per_cell * field.sample_span();
            let bound = tiles_per_cell.div_ceil(layout.zone_span()).max(1);

            let zone_count = layout.zone_count();
            let distance = zone_count.chebyshev(
                zone_count.wrap(left.x(), left.y()),
                zone_count.wrap(right.x(), right.y()),
            );
            prop_assert!(
                distance <= bound,
                "相邻两格 ({col},{row}) 与 ({},{row}) 的区块在环面上相距 {distance}，超过上限 {bound}",
                col + 1
            );
            prop_assert_eq!(left.y(), right.y(), "只往右挪一列，行不该变");
        }

        /// 正反换算自洽：像素落在哪一格算出的区块，与直接问那一格算出的
        /// 区块必须是同一个。
        ///
        /// 这条锁住的是「反算用的几何与画格子用的几何是同一份」——两者
        /// 一旦分叉（例如一边内缩了边框、另一边没有），玩家点的位置与
        /// 高亮的格子会差一个边框宽度。
        #[test]
        fn 格子中心的像素换算回来就是那一格的区块(
            level in 0usize..4,
            pan_x in -40i32..40,
            pan_y in -40i32..40,
            col_pick in 0u32..64,
            row_pick in 0u32..64,
        ) {
            let (layout, _field) = world();
            let slice = slice_at(level, pan_x, pan_y);
            // 取余，理由同上一条属性。
            let col = col_pick % slice.cols;
            let row = row_pick % slice.rows;
            let content = PANEL.inset(FlatColorSkin.panel(PanelStyleId::Window).border_thickness);
            let grid = WorldMapGrid::new(content, slice.cols, slice.rows).expect("列行恒非零");
            let px = grid.origin_x + (col as f32 + 0.5) * grid.cell_size;
            let py = grid.origin_y + (row as f32 + 0.5) * grid.cell_size;

            let by_pixel = world_map_zone_at_pixel(&slice, layout, PANEL, &FlatColorSkin, (px, py));
            let by_cell = slice.zone_at_cell(layout, col, row);

            prop_assert_eq!(by_pixel, by_cell);
        }
    }
}
