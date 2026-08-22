//! 条形：背景 + 按比例填充的前景两块矩形。
//!
//! # 只服务有真实分母的场景——经验条,不是生命/法力
//!
//! 任务书列出的条形用途是「生命、法力、经验」，但生命/法力目前只有
//! `Agent::health`/`Agent::mana` 这两个**当前值**，全仓库核实过没有任何
//! `max_health`/`max_mana` 一类的衍生上限公式（`derive_stats` 不产出
//! 它们，`attribute-system.md` 也明确记录该公式尚未落地）——条形需要一
//! 个分母才能算出填充比例，编一个假上限出来会让这个条形从「反映真实
//! 状态」变成「反映一个凭空定的数字」，这与项目「不编造尚未落地的
//! 数值」的一贯纪律冲突（同类判断见
//! `crates/ll-ui/src/hud/status_bar.rs` 模块文档）。
//!
//! 经验（`Agent::experience`/`Agent::xp_to_next_level`）不一样：两者都
//! 是真实存在、已经在结算里生效的数值，比例 `experience /
//! xp_to_next_level` 有明确含义，因此本批次的角色面板只把经验做成条形，
//! 生命/法力仍以数字文本显示（[`crate::hud::status_bar`]）。等衍生生命
//! /法力上限公式真的落地，把它们也换成条形是往
//! [`crate::hud::character_panel`] 加一次 [`bar_quads`] 调用的局部改动，
//! 本模块的接口不需要改。

use super::geometry::Rect;
use super::quad::QuadInstance;
use super::textured_quad::TexturedQuadInstance;

/// 条形的纯色外观数据，理由同
/// [`crate::widget::panel::FlatPanelAppearance`]（没有美术资产，先用
/// 纯色）——同一条皮肤/控件分离纪律：本模块不知道调用方为什么选了
/// 这几个颜色，调用点也不直接构造它，而是向
/// [`crate::widget::skin::Skin`] 要 `BarStyleId::Progress` 对应的外观，
/// 见 `crate::widget::skin` 模块文档。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatBarAppearance {
    /// 背景（未填充部分）颜色。
    pub background_color: [f32; 4],
    /// 前景（已填充部分）颜色。
    pub fill_color: [f32; 4],
}

impl FlatBarAppearance {
    /// 朴素默认样式：深灰背景 + 亮色前景。经验条（唯一用
    /// [`crate::widget::skin::BarStyleId::Progress`] 的场景）用它。
    pub const DEFAULT: FlatBarAppearance = FlatBarAppearance {
        background_color: [0.2, 0.2, 0.22, 0.8],
        fill_color: [0.4, 0.75, 0.95, 0.95],
    };

    /// 生命条样式：暖红——项目所有者反馈生命/法力两条资源条「都是浅蓝、
    /// 都满着，分不出哪条是哪条」（P7 第一批实机截图的真实问题），核实
    /// 结论是两条此前共用同一个 `BarStyleId::HealthMana`，因此外观恒
    /// 相同。这里选颜色区分（而不是加文字标签）：状态栏文本本身已经写
    /// 明「生命 X 法力 Y」，条形只是给这两个已知数字一个更快能扫到的
    /// 视觉锚点，加标签会在已经很拥挤的条形上再叠一层文字，颜色区分
    /// 更轻量，也是这类资源条最通行的读法（红=生命、蓝=法力）。
    pub const HEALTH: FlatBarAppearance = FlatBarAppearance {
        background_color: [0.18, 0.08, 0.08, 0.8],
        fill_color: [0.85, 0.25, 0.25, 0.95],
    };

    /// 法力条样式：饱和蓝——与 [`Self::HEALTH`] 同一次修正，刻意选比旧
    /// 版 `DEFAULT`（浅蓝偏灰）更饱和的蓝，与暖红拉开明显色相差，两条
    /// 并排时不需要凑近看颜色数值也能分清。
    pub const MANA: FlatBarAppearance = FlatBarAppearance {
        background_color: [0.08, 0.1, 0.2, 0.8],
        fill_color: [0.3, 0.55, 0.95, 0.95],
    };
}

/// 产出条形的两块矩形：`[0]` 是背景（恒等于 `rect`），`[1]` 是前景
/// （从 `rect` 左边界起,宽度按 `fraction` 缩放）。
///
/// `fraction` 钳制到 `[0.0, 1.0]`——调用方传入的比例理论上应当恒在此
/// 范围内（例如经验条的 `experience <= xp_to_next_level`），但显示层
/// 不应该因为上游一次意外的越界数值（`experience` 短暂超过阈值、尚未
/// 触发升级结算的那一帧）画出宽度超出面板或为负的矩形——钳制是防御性
/// 的,不代表这类越界是预期状态。
pub fn bar_quads(rect: Rect, fraction: f32, style: &FlatBarAppearance) -> Vec<QuadInstance> {
    let clamped = fraction.clamp(0.0, 1.0);
    vec![
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            color: style.background_color,
        },
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * clamped, rect.height],
            color: style.fill_color,
        },
    ]
}

/// 真实条形贴图的外观数据,理由同
/// [`crate::widget::panel::TexturedPanelAppearance`]。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedBarAppearance {
    /// 背景（未填充部分）贴图在图集里的 UV 矩形。
    pub track_uv: [f32; 4],
    /// 前景（已填充部分）贴图在图集里的 UV 矩形。
    pub fill_uv: [f32; 4],
    /// 背景颜色调制。
    pub track_tint: [f32; 4],
    /// 前景颜色调制。
    pub fill_tint: [f32; 4],
}

/// 产出条形的两块贴图矩形，几何与 [`bar_quads`] 完全相同。
pub fn textured_bar_quads(
    rect: Rect,
    fraction: f32,
    style: &TexturedBarAppearance,
) -> Vec<TexturedQuadInstance> {
    let clamped = fraction.clamp(0.0, 1.0);
    vec![
        TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            uv_rect: style.track_uv,
            color: style.track_tint,
        },
        TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * clamped, rect.height],
            uv_rect: style.fill_uv,
            color: style.fill_tint,
        },
    ]
}

/// 双层条形的纯色外观数据——生命/法力这类「会下降的资源条」用它：
/// 一层立刻反映真实值（[`FlatTwoLayerBarAppearance::fill_color`]），
/// 一层滞后追赶（[`FlatTwoLayerBarAppearance::afterglow_color`]），两者
/// 之间的空隙就是「刚刚掉了多少」的视觉提示——见
/// `crates/ll-ui/src/widget/anim.rs` 模块文档「双层血条」一节。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatTwoLayerBarAppearance {
    /// 背景（完全未填充部分）颜色。
    pub background_color: [f32; 4],
    /// 余晖层颜色——比 `fill_color` 暗/淡，追赶中的旧值。
    pub afterglow_color: [f32; 4],
    /// 立即层颜色——恒等于真实当前值。
    pub fill_color: [f32; 4],
}

/// 产出双层条形的三块矩形：`[0]` 背景（恒等于 `rect`），`[1]` 余晖层
/// （按 `lagging_fraction` 缩放），`[2]` 立即层（按 `immediate_fraction`
/// 缩放）。**绘制顺序即遮挡顺序**——立即层最后画、叠在余晖层之上，
/// 因此只有「余晖比立即多出来的那一段」才会显出余晖色，恰好是「刚刚
/// 掉了多少」这段视觉提示。
///
/// 两个比例各自独立钳制到 `[0.0, 1.0]`，理由同 [`bar_quads`]。
pub fn two_layer_bar_quads(
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    style: &FlatTwoLayerBarAppearance,
) -> Vec<QuadInstance> {
    let immediate = immediate_fraction.clamp(0.0, 1.0);
    let lagging = lagging_fraction.clamp(0.0, 1.0);
    vec![
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            color: style.background_color,
        },
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * lagging, rect.height],
            color: style.afterglow_color,
        },
        QuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * immediate, rect.height],
            color: style.fill_color,
        },
    ]
}

/// 双层条形的真实贴图外观数据，理由同 [`TexturedBarAppearance`]——
/// 三层复用同一张 [`super::skin::NineSliceSkin`] 的条形贴图，只靠
/// `afterglow_tint`/`fill_tint` 两种不同的颜色调制区分「追赶中」与
/// 「立即」两层，不需要为余晖单独准备一张贴图。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedTwoLayerBarAppearance {
    /// 背景贴图 UV。
    pub track_uv: [f32; 4],
    /// 前景（余晖层与立即层共用）贴图 UV。
    pub fill_uv: [f32; 4],
    /// 背景颜色调制。
    pub track_tint: [f32; 4],
    /// 余晖层颜色调制——通常比 `fill_tint` 暗/淡。
    pub afterglow_tint: [f32; 4],
    /// 立即层颜色调制。
    pub fill_tint: [f32; 4],
}

/// 产出双层条形的三块贴图矩形，几何与 [`two_layer_bar_quads`] 完全
/// 相同。
pub fn textured_two_layer_bar_quads(
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    style: &TexturedTwoLayerBarAppearance,
) -> Vec<TexturedQuadInstance> {
    let immediate = immediate_fraction.clamp(0.0, 1.0);
    let lagging = lagging_fraction.clamp(0.0, 1.0);
    vec![
        TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            uv_rect: style.track_uv,
            color: style.track_tint,
        },
        TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * lagging, rect.height],
            uv_rect: style.fill_uv,
            color: style.afterglow_tint,
        },
        TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width * immediate, rect.height],
            uv_rect: style.fill_uv,
            color: style.fill_tint,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_quads的背景矩形恒等于传入的矩形() {
        // Arrange
        let rect = Rect::new(10.0, 20.0, 200.0, 12.0);

        // Act
        let quads = bar_quads(rect, 0.5, &FlatBarAppearance::DEFAULT);

        // Assert
        assert_eq!(quads[0].size, [200.0, 12.0]);
    }

    #[test]
    fn bar_quads的前景宽度按比例缩放() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = bar_quads(rect, 0.25, &FlatBarAppearance::DEFAULT);

        // Assert
        assert_eq!(quads[1].size[0], 50.0);
    }

    #[test]
    fn bar_quads对超过一的比例钳制到满宽() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = bar_quads(rect, 1.5, &FlatBarAppearance::DEFAULT);

        // Assert
        assert_eq!(quads[1].size[0], 200.0);
    }

    #[test]
    fn bar_quads对负比例钳制到零宽() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = bar_quads(rect, -0.5, &FlatBarAppearance::DEFAULT);

        // Assert
        assert_eq!(quads[1].size[0], 0.0);
    }

    const TWO_LAYER_STYLE: FlatTwoLayerBarAppearance = FlatTwoLayerBarAppearance {
        background_color: [0.2, 0.2, 0.22, 0.8],
        afterglow_color: [0.7, 0.3, 0.3, 0.9],
        fill_color: [0.9, 0.2, 0.2, 0.95],
    };

    #[test]
    fn two_layer_bar_quads恒产出三块() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = two_layer_bar_quads(rect, 0.3, 0.6, &TWO_LAYER_STYLE);

        // Assert
        assert_eq!(quads.len(), 3);
    }

    #[test]
    fn two_layer_bar_quads的余晖层宽度大于立即层当数值下降() {
        // Arrange：受伤瞬间——立即层已经掉到 0.3，余晖层还停在 0.6。
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = two_layer_bar_quads(rect, 0.3, 0.6, &TWO_LAYER_STYLE);

        // Assert：quads[1] 是余晖层，quads[2] 是立即层。
        assert!(quads[1].size[0] > quads[2].size[0]);
    }

    #[test]
    fn two_layer_bar_quads的立即层宽度精确等于立即比例乘以宽度() {
        // Arrange：0.25 与 200.0 都是二进制浮点能精确表示的值,避免
        // 0.3 这类十进制小数在 f32 里本就无法精确表示、乘出来带舍入
        // 误差的假阳性失败。
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);

        // Act
        let quads = two_layer_bar_quads(rect, 0.25, 0.6, &TWO_LAYER_STYLE);

        // Assert
        assert_eq!(quads[2].size[0], 50.0);
    }

    #[test]
    fn textured_bar_quads与bar_quads的几何完全一致() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);
        let textured_style = TexturedBarAppearance {
            track_uv: [0.0, 0.0, 0.5, 0.5],
            fill_uv: [0.5, 0.0, 0.5, 0.5],
            track_tint: [1.0, 1.0, 1.0, 1.0],
            fill_tint: [1.0, 1.0, 1.0, 1.0],
        };

        // Act
        let flat = bar_quads(rect, 0.4, &FlatBarAppearance::DEFAULT);
        let textured = textured_bar_quads(rect, 0.4, &textured_style);

        // Assert
        for (f, t) in flat.iter().zip(textured.iter()) {
            assert_eq!(f.position, t.position);
            assert_eq!(f.size, t.size);
        }
    }

    #[test]
    fn textured_two_layer_bar_quads恒产出三块() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 200.0, 12.0);
        let style = TexturedTwoLayerBarAppearance {
            track_uv: [0.0, 0.0, 0.5, 0.5],
            fill_uv: [0.5, 0.0, 0.5, 0.5],
            track_tint: [1.0, 1.0, 1.0, 1.0],
            afterglow_tint: [0.7, 0.3, 0.3, 0.9],
            fill_tint: [0.9, 0.2, 0.2, 0.95],
        };

        // Act
        let quads = textured_two_layer_bar_quads(rect, 0.3, 0.6, &style);

        // Assert
        assert_eq!(quads.len(), 3);
    }
}
