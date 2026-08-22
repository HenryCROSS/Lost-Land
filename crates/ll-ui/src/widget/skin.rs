//! 皮肤层：把「控件问皮肤要长什么样」和「皮肤具体给什么」分开。
//!
//! # 为什么需要这一层
//!
//! 项目所有者的硬要求（追加约束）：控件要把「布局与行为」和「长什么
//! 样」分开——`ui.panel(rect, PanelStyleId::Window)`，皮肤决定
//! `PanelStyleId::Window` 具体画成什么（现在是纯色矩形，将来可能是
//! 九宫格贴图），控件代码本身不该知道、也不该关心这个决定。
//!
//! 若不做这层分离，[`crate::widget::panel::panel_quads`] 与
//! [`crate::hud::render`] 的调用点会直接写死颜色常量（`[0.75, 0.75,
//! 0.8, 0.9]` 这样的字面量）——换一套美术风格（甚至只是调个色）需要
//! 去翻遍每个用到面板/条形的地方；加九宫格贴图更是要在调用点与控件
//! 内部同时改。本层把两件事切开：**控件（`panel_quads`/`bar_quads`）
//! 只认一份已经算好的外观数据**（[`crate::widget::panel::FlatPanelAppearance`]/
//! [`crate::widget::bar::FlatBarAppearance`]），**调用点只认语义化的
//! 样式名**（本模块的 [`PanelStyleId`]/[`BarStyleId`]），中间由
//! 实现了 [`Skin`] trait 的对象把两者接起来。
//!
//! # 两条验收问题的回答
//!
//! - **换皮肤要改几处？** 一处——`ll-game::app::Demo` 构造 `Skin` 实现
//!   的那一行（现在已经是 [`NineSliceSkin::new`]，回退到
//!   [`FlatColorSkin`] 同样只需要换这一行）。[`crate::widget::panel::panel_quads`]/
//!   [`crate::widget::bar::bar_quads`] 与四块 HUD 面板模块
//!   （`crate::hud::character_panel` 等）的代码一行不动——它们从头到尾
//!   只经手 `PanelStyleId`/`BarStyleId` 与 `Skin` 返回的外观数据,不认识
//!   任何具体颜色字面量或贴图名。
//! - **加九宫格要改几处？** 已经做过一次,真实答案是：新增
//!   [`NineSliceSkin`]（一个 `Skin` 实现，覆盖 `textured_*` 三个方法）+
//!   [`crate::widget::textured_quad::TexturedQuadRenderer`]（一条新的、
//!   支持 UV 采样的原生分辨率图元，`crate::widget::quad::QuadRenderer`
//!   的姊妹）+ `crate::hud::render` 里「`textured_*` 给出 `Some` 就走
//!   贴图路径」这一处分支。四块 HUD 面板模块与
//!   [`crate::widget::panel::panel_quads`]/[`crate::widget::panel::textured_panel_quads`]
//!   共用同一份九宫格**几何**（`crate::widget::panel::nine_slice_rects`），
//!   没有为了贴图重新设计切分方式。
//!
//! # 现在有两种皮肤实现——[`FlatColorSkin`]（纯色回退）与
//! [`NineSliceSkin`]（真实贴图）
//!
//! `ll-artgen` 已经生成了四张占位 UI 贴图（`ui_panel_border`/
//! `ui_panel_fill`/`ui_bar_track`/`ui_bar_fill`，见
//! `tools/ll-artgen/src/ui.rs` 与 `assets/sprites/manifest.json5`），
//! 走的是与地形瓦片/角色精灵完全相同的运行期图集打包管线——
//! [`NineSliceSkin`] 因此能在本批次就真的用贴图画出九宫格边框/条形，
//! 不再只是「留一个接口」。[`FlatColorSkin`] 仍然保留：它不需要任何
//! `Atlas` 引用就能构造（纯数据，`Default`），适合脱离 GPU 的场景
//! （单元测试、`cargo doc` 示例），也是「图集加载失败/贴图缺失」时的
//! 保底——`ll_render::atlas::Atlas` 找不到某个条目名时的既有降级行为
//! 是记一条 error 日志并跳过绘制（见 `GpuResources::lookup` 文档），
//! [`NineSliceSkin`] 因此对查不到的贴图名同样退化到
//! [`FlatColorSkin`] 的对应外观，不会让整块面板凭空消失。
//!
//! # `Skin` trait 的两层方法：flat 是必需的，textured 是可选的
//!
//! `panel`/`bar` 两个方法**必须**实现——任何皮肤都要能在没有图集、
//! 只有几何数据的场景下给出一个可用的纯色外观（这也是
//! [`crate::widget::panel::panel_quads`]/[`crate::widget::bar::bar_quads`]
//! 唯一认识的输入形状，两者本身不知道任何贴图的存在）。`textured_*`
//! 三个方法有默认实现（恒返回 `None`），只有真正持有贴图 UV 数据的
//! 皮肤（如 [`NineSliceSkin`]）才需要覆盖它们——`crate::hud::render`
//! 据此决定：`textured_*` 给出 `Some` 就走贴图渲染路径，否则退回
//! `panel`/`bar` 的纯色路径。这是「加九宫格只加一种皮肤实现」这条
//! 承诺在方法签名层面的体现：新皮肤只需要覆盖它想真正提供贴图的那几
//! 个方法，其余可以什么都不做（继承默认的 `None`）。
//!
//! # mod 换 UI 皮肤贴图是否「白拿」——核实结论：架构上成立，且已验证
//!
//! `ll_mod::asset_vfs` 核实结论：精灵资产的声明（`manifest.json5` 的
//! `name`/`file`/`pivot`/`footprint`）与覆盖解析（`assets/overrides/
//! <命名空间>/sprites/<相对路径>`）**不区分资产的用途**——只要声明了
//! 名字与文件，`pack_atlas` 就会把它打进图集，覆盖机制按名字/相对
//! 路径匹配，与这张图是地形瓦片、角色精灵、还是 UI 边框无关（同一条
//! 机制已经在验收测试里覆盖了 `example_mod` 覆盖本体 `terrain_dirt.png`
//! 的场景）。[`NineSliceSkin`] 现在引用的四张贴图名
//! （`ui_panel_border` 等）就是普通的图集条目名——任何 mod 只需要在
//! `assets/overrides/lostland/sprites/ui_panel_border.png` 放一张同名
//! 覆盖图，下次运行期打包就会用它，不需要为「UI 用途」新增任何一条
//! 特殊机制,这条路径与世界贴图共用同一套代码,不是分别维护的两条。

use ll_render::atlas::Atlas;

use super::bar::{FlatBarAppearance, TexturedBarAppearance, TexturedTwoLayerBarAppearance};
use super::panel::{FlatPanelAppearance, TexturedPanelAppearance};

/// 面板的语义样式名——调用点只认这个，不认具体颜色。目前只有一种
/// 语义（HUD 四块面板共用同一套外观），未来若不同面板需要不同风格
/// （例如错误/警告态面板要更醒目），在这里加新变体即可，不影响
/// [`Skin`] trait 本身的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelStyleId {
    /// HUD 常规窗口面板——状态栏、角色面板、背包、装备栏目前共用的
    /// 唯一样式。
    Window,
}

/// 条形的语义样式名，理由同 [`PanelStyleId`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyleId {
    /// 进度条——经验条目前唯一的用途，见
    /// `crate::widget::bar` 模块文档「只服务有真实分母的场景」一节。
    Progress,
    /// 双层资源条——生命/法力这类会下降的资源用它，见
    /// `crate::widget::bar::FlatTwoLayerBarAppearance` 文档。
    HealthMana,
}

/// 皮肤：把语义样式名解析成控件真正认识的外观数据。见模块文档
/// 「`Skin` trait 的两层方法」一节。
pub trait Skin {
    /// 解析面板样式（纯色回退，恒需要实现）。
    fn panel(&self, style: PanelStyleId) -> FlatPanelAppearance;
    /// 解析条形样式（纯色回退，恒需要实现）。
    fn bar(&self, style: BarStyleId) -> FlatBarAppearance;
    /// 解析面板样式的真实贴图外观——默认没有,返回 `None` 即表示「用
    /// [`Self::panel`] 的纯色回退」。
    fn textured_panel(&self, _style: PanelStyleId) -> Option<TexturedPanelAppearance> {
        None
    }
    /// 解析单层条形样式的真实贴图外观，默认 `None`。
    fn textured_bar(&self, _style: BarStyleId) -> Option<TexturedBarAppearance> {
        None
    }
    /// 解析双层条形样式的真实贴图外观，默认 `None`。
    fn textured_two_layer_bar(&self, _style: BarStyleId) -> Option<TexturedTwoLayerBarAppearance> {
        None
    }
}

/// 纯色回退皮肤：全部样式都给纯色——见模块文档。没有任何内部状态，
/// `Skin::panel`/`Skin::bar` 恒返回同一份数据，与调用了多少次、调用
/// 顺序无关；`textured_*` 全部用 trait 默认实现（恒 `None`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct FlatColorSkin;

impl Skin for FlatColorSkin {
    fn panel(&self, style: PanelStyleId) -> FlatPanelAppearance {
        match style {
            PanelStyleId::Window => FlatPanelAppearance::DEFAULT,
        }
    }

    fn bar(&self, style: BarStyleId) -> FlatBarAppearance {
        match style {
            BarStyleId::Progress | BarStyleId::HealthMana => FlatBarAppearance::DEFAULT,
        }
    }
}

/// 真实贴图皮肤：引用 `ll-artgen` 生成的四张占位 UI 贴图（见模块文档）
/// 在图集里的 UV 矩形，构造时一次性查出全部需要的 UV（`Atlas::uv_rect`
/// 本身不便宜到可以每帧调用，见其模块文档），之后 `textured_*` 方法
/// 只是克隆已经查好的数据。
pub struct NineSliceSkin {
    panel_border_uv: Option<[f32; 4]>,
    panel_fill_uv: Option<[f32; 4]>,
    bar_track_uv: Option<[f32; 4]>,
    bar_fill_uv: Option<[f32; 4]>,
    /// 边框厚度（像素）——贴图本身是 16×16，与
    /// [`FlatPanelAppearance::DEFAULT`] 的 `2.0` 不同,选一个能让四个角
    /// 看起来像「边框」而不是「贴图被拉伸变形」的厚度。
    border_thickness: f32,
}

impl NineSliceSkin {
    /// 从 `atlas` 查出全部需要的贴图 UV——任何一张查不到（贴图缺失/
    /// 图集打包失败）时对应字段是 `None`，`textured_*` 方法据此退化到
    /// `None`（调用方回退到 [`FlatColorSkin`] 的纯色外观），不会
    /// panic,也不会让整块面板凭空消失。
    pub fn new(atlas: &Atlas) -> NineSliceSkin {
        NineSliceSkin {
            panel_border_uv: atlas.uv_rect("ui_panel_border"),
            panel_fill_uv: atlas.uv_rect("ui_panel_fill"),
            bar_track_uv: atlas.uv_rect("ui_bar_track"),
            bar_fill_uv: atlas.uv_rect("ui_bar_fill"),
            border_thickness: 4.0,
        }
    }
}

/// 不透明白——纹理采样结果原样显示，不做任何颜色调制。
const NO_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// 余晖层调制——比立即层更暗更透明，见
/// `crate::widget::bar::FlatTwoLayerBarAppearance` 文档。
const AFTERGLOW_TINT: [f32; 4] = [0.75, 0.45, 0.45, 0.85];

impl Skin for NineSliceSkin {
    fn panel(&self, style: PanelStyleId) -> FlatPanelAppearance {
        // 贴图缺失时的纯色回退——见类型文档。
        FlatColorSkin.panel(style)
    }

    fn bar(&self, style: BarStyleId) -> FlatBarAppearance {
        FlatColorSkin.bar(style)
    }

    fn textured_panel(&self, style: PanelStyleId) -> Option<TexturedPanelAppearance> {
        match style {
            PanelStyleId::Window => Some(TexturedPanelAppearance {
                border_uv: self.panel_border_uv?,
                fill_uv: self.panel_fill_uv?,
                border_tint: NO_TINT,
                fill_tint: NO_TINT,
                border_thickness: self.border_thickness,
            }),
        }
    }

    fn textured_bar(&self, style: BarStyleId) -> Option<TexturedBarAppearance> {
        match style {
            BarStyleId::Progress => Some(TexturedBarAppearance {
                track_uv: self.bar_track_uv?,
                fill_uv: self.bar_fill_uv?,
                track_tint: NO_TINT,
                fill_tint: NO_TINT,
            }),
            BarStyleId::HealthMana => None,
        }
    }

    fn textured_two_layer_bar(&self, style: BarStyleId) -> Option<TexturedTwoLayerBarAppearance> {
        match style {
            BarStyleId::HealthMana => Some(TexturedTwoLayerBarAppearance {
                track_uv: self.bar_track_uv?,
                fill_uv: self.bar_fill_uv?,
                track_tint: NO_TINT,
                afterglow_tint: AFTERGLOW_TINT,
                fill_tint: NO_TINT,
            }),
            BarStyleId::Progress => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_color_skin对窗口样式恒返回默认面板外观() {
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let appearance = skin.panel(PanelStyleId::Window);

        // Assert
        assert_eq!(appearance, FlatPanelAppearance::DEFAULT);
    }

    #[test]
    fn flat_color_skin对进度样式恒返回默认条形外观() {
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let appearance = skin.bar(BarStyleId::Progress);

        // Assert
        assert_eq!(appearance, FlatBarAppearance::DEFAULT);
    }

    #[test]
    fn flat_color_skin的贴图方法恒返回none() {
        // Arrange
        let skin = FlatColorSkin;

        // Act & Assert：trait 默认实现，未被覆盖。
        assert!(skin.textured_panel(PanelStyleId::Window).is_none());
        assert!(skin.textured_bar(BarStyleId::Progress).is_none());
        assert!(
            skin.textured_two_layer_bar(BarStyleId::HealthMana)
                .is_none()
        );
    }
}
