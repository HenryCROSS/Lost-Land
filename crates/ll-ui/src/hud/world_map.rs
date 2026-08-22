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

use ll_world::overview::OverviewCell;
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

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
    } else if kind == ids.floor_stone {
        [0.45, 0.42, 0.40, 1.0]
    } else if kind == ids.wall_stone {
        [0.35, 0.33, 0.32, 1.0]
    } else {
        UNKNOWN_TERRAIN_COLOR
    }
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
}

/// 把一份世界地图数据变成这一帧要画的格子矩形——铺满 `rect`，每格
/// `rect.width / cols` 宽、`rect.height / rows` 高。战争迷雾在这里
/// 生效：`explored` 为假的格子恒画 [`FOG_COLOR`]，见模块文档。
///
/// 地图格子没有真实贴图可采样（不是精灵动画，是按地形分类现算的纯
/// 色），因此恒产出 [`QuadInstance`]，不区分纯色/贴图两条路径——与
/// `crate::hud::render::push_day_night_bar` 里指针「恒是纯色矩形」
/// 同一个理由。
///
/// `cols`/`rows` 任一为零时返回空列表，不做除零运算。
pub fn world_map_cell_quads(data: &WorldMapPanelData<'_>, rect: Rect) -> Vec<QuadInstance> {
    if data.cols == 0 || data.rows == 0 {
        return Vec::new();
    }
    let cell_width = rect.width / data.cols as f32;
    let cell_height = rect.height / data.rows as f32;
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
            QuadInstance {
                position: [
                    rect.x + col as f32 * cell_width,
                    rect.y + row as f32 * cell_height,
                ],
                size: [cell_width, cell_height],
                color,
            }
        })
        .collect()
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
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Assert
        assert_eq!(quads.len(), 4);
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
        };

        // Act
        let quads = world_map_cell_quads(&data, Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert_eq!(quads[0].color, UNKNOWN_TERRAIN_COLOR);

        // Cleanup: 无——`interner`/`extra_id` 只是本地栈上的值。
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
        };

        // Act
        let frame = world_map_frame(&data, Rect::new(0.0, 0.0, 200.0, 100.0), &FlatColorSkin);

        // Assert
        assert_eq!(frame.quads.len(), 4 + 2);
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
