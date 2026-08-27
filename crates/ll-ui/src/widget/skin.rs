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
//! 的场景）。[`NineSliceSkin`] 现在引用的那几张贴图名
//! （[`REQUIRED_SPRITE_KEYS`]）就是普通的图集条目名——任何 mod 只需要在
//! `assets/overrides/lostland/sprites/ui_panel_border.png` 放一张同名
//! 覆盖图，下次运行期打包就会用它，不需要为「UI 用途」新增任何一条
//! 特殊机制,这条路径与世界贴图共用同一套代码,不是分别维护的两条。
//!
//! # 曾经的缺陷：这里查的是裸名字，图集里存的是完整命名空间 ID
//!
//! [`REQUIRED_SPRITE_KEYS`] 现在带 `lostland:` 前缀。此前这五个查找键
//! 写的是裸名字（`"ui_panel_border"`），而运行期图集打包器
//! （`ll_render::atlas_pack::pack_atlas`）用的条目名恒等于精灵的完整
//! 命名空间 ID（见 `ll_mod::asset_vfs::ResolvedSprite::atlas_name`
//! 文档），真实图集里只有 `lostland:ui_panel_border`——于是
//! [`Atlas::uv_rect`] 五次全部返回 `None`，五个 `textured_*` 方法里的
//! `?` 全部短路，`crate::hud::render` 每一帧都静默退回 [`FlatColorSkin`]
//! 的纯色外观。
//!
//! 这条缺陷**不会打任何日志**：`uv_rect` 返回 `None` 是本模块设计上的
//! 正常降级路径（「这张皮肤没有这个贴图」），它分辨不出「本来就没有」
//! 与「有资产但名字对不上」。画面上因此仍然有面板、有血条、有昼夜滑
//! 条，只是全部是纯色的——「看起来在工作」正是它躲过此前每一轮验收的
//! 原因。守住它的现在是 `crates/ll-game/tests/atlas_coverage.rs`：那条
//! 测试用真实 `assets/` 打出真实图集，断言本常量里每一个键都查得到、
//! 且对应矩形里有不透明像素。
//!
//! 注意与「本来就没有资产」区分：`Skin::textured_button` 恒返回 `None`
//! 是**有意的**（本体确实没有按钮贴图），不在这条缺陷范围内。

use ll_render::atlas::Atlas;

use super::bar::{FlatBarAppearance, TexturedBarAppearance, TexturedTwoLayerBarAppearance};
use super::button::FlatButtonAppearance;
use super::day_night_bar::{FlatDayNightBarAppearance, TexturedDayNightBarAppearance};
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
    /// 生命条——双层资源条之一，见
    /// `crate::widget::bar::FlatTwoLayerBarAppearance` 文档。
    ///
    /// 曾经与法力条共用同一个 `HealthMana` 样式名（两条外观因此恒相同,
    /// 是「两条资源条分不清哪条是哪条」这个真实截图问题的根因）,现在
    /// 拆成 [`Self::Health`]/[`Self::Mana`] 两个独立样式名,换皮肤时两条
    /// 依然可以分别决定颜色,不需要在渲染调用点里现改。
    Health,
    /// 法力条——双层资源条之一，理由同 [`Self::Health`]。
    Mana,
}

/// 昼夜滑条的语义样式名，理由同 [`PanelStyleId`]——目前只有一种（状态
/// 栏下方常驻的那一条），单变体枚举是刻意的：与 [`PanelStyleId::Window`]
/// 同一个理由，为将来可能出现的第二种昼夜条（例如某个室内场景专属的
/// 迷你版）预留挂点，不是过度设计——[`Skin`] trait 的形状已经要求调用点
/// 传一个样式名而不是直接问「昼夜条长什么样」，加变体不影响任何既有
/// 调用点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayNightBarStyleId {
    /// 状态栏下方常驻的昼夜滑条——见 `crate::hud::render` 模块文档。
    Clock,
}

/// 按钮的语义样式名，理由同 [`PanelStyleId`]——UI 交互层批次目前只有
/// 一种按钮外观（HUD 测试按钮与将来的确认框「确定/取消」共用），未来
/// 若不同场景需要不同风格（例如危险操作的按钮要更醒目），在这里加新
/// 变体即可，不影响 [`Skin`] trait 本身的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyleId {
    /// 常规按钮——目前唯一的样式。
    Default,
}

/// 按钮的视觉状态——任务书原话「普通/悬停/按下/释放触发」里前三者是
/// 外观状态（第四者「释放触发」是行为,不改外观本身,见
/// `crate::widget::button::update_button` 文档）。四种状态外观必须
/// 走皮肤,不在绘制代码里写死颜色——这是任务书的硬约束,与
/// [`PanelStyleId`]/[`BarStyleId`] 同一条「换皮肤只改数据」纪律。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVisualState {
    /// 未悬停、未按下、未禁用的默认外观。
    Normal,
    /// 光标悬停在按钮上，或按钮当前持有键盘/手柄焦点——两者共用同一
    /// 种视觉反馈，理由见 `crate::widget::button::update_button` 文档
    /// 「焦点也要有可见反馈」一节：纯键盘操作的玩家需要能看见焦点在
    /// 哪，不能只有鼠标悬停才会变化外观。
    Hovered,
    /// 鼠标左键正按在按钮上（尚未松开）。
    Pressed,
    /// 按钮被禁用，不响应任何输入。
    Disabled,
}

/// 皮肤：把语义样式名解析成控件真正认识的外观数据。见模块文档
/// 「`Skin` trait 的两层方法」一节。
pub trait Skin {
    /// 解析面板样式（纯色回退，恒需要实现）。
    fn panel(&self, style: PanelStyleId) -> FlatPanelAppearance;
    /// 解析条形样式（纯色回退，恒需要实现）。
    fn bar(&self, style: BarStyleId) -> FlatBarAppearance;
    /// 解析昼夜滑条样式（纯色回退，恒需要实现），理由同 [`Self::bar`]。
    fn day_night_bar(&self, style: DayNightBarStyleId) -> FlatDayNightBarAppearance;
    /// 解析按钮在给定视觉状态下的外观（纯色回退，恒需要实现），理由同
    /// [`Self::bar`]——四种状态各自独立解析，不是共用一份外观再由绘制
    /// 代码现改颜色。
    fn button(&self, style: ButtonStyleId, visual: ButtonVisualState) -> FlatButtonAppearance;
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
    /// 解析昼夜滑条样式的真实贴图外观，默认 `None`。
    fn textured_day_night_bar(
        &self,
        _style: DayNightBarStyleId,
    ) -> Option<TexturedDayNightBarAppearance> {
        None
    }
    /// 解析按钮样式的真实贴图外观，默认 `None`——本批次没有按钮贴图
    /// 资产（`ll-artgen` 尚未生成),恒回退到 [`Self::button`] 的纯色
    /// 外观,理由同其余 `textured_*` 方法。
    fn textured_button(
        &self,
        _style: ButtonStyleId,
        _visual: ButtonVisualState,
    ) -> Option<super::button::TexturedButtonAppearance> {
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
            BarStyleId::Progress => FlatBarAppearance::DEFAULT,
            BarStyleId::Health => FlatBarAppearance::HEALTH,
            BarStyleId::Mana => FlatBarAppearance::MANA,
        }
    }

    fn day_night_bar(&self, style: DayNightBarStyleId) -> FlatDayNightBarAppearance {
        match style {
            DayNightBarStyleId::Clock => FlatDayNightBarAppearance::DEFAULT,
        }
    }

    fn button(&self, style: ButtonStyleId, visual: ButtonVisualState) -> FlatButtonAppearance {
        match style {
            ButtonStyleId::Default => match visual {
                ButtonVisualState::Normal => FlatButtonAppearance::NORMAL,
                ButtonVisualState::Hovered => FlatButtonAppearance::HOVERED,
                ButtonVisualState::Pressed => FlatButtonAppearance::PRESSED,
                ButtonVisualState::Disabled => FlatButtonAppearance::DISABLED,
            },
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
    /// 昼夜滑条底图的 UV——`ll-artgen` 新增的第五张占位 UI 贴图
    /// （`ui_daynight_bar`），见本文件底部 [`DayNightBarStyleId`] 一节
    /// 与 `tools/ll-artgen/src/ui.rs::decorate_day_night_bar` 文档。
    daynight_bar_uv: Option<[f32; 4]>,
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
        NineSliceSkin::from_uv_lookup(|name| atlas.uv_rect(name))
    }

    /// [`Self::new`] 的**不依赖 GPU** 的形态：查 UV 这一步由调用方给
    /// 的闭包完成，本函数只负责「拿哪几个键去查、查出来放进哪个字段」。
    ///
    /// # 为什么要有这一层
    ///
    /// [`Atlas`] 持有 `wgpu::TextureView`，没有真实 GPU 设备就构造不
    /// 出来——于是「五张 UI 贴图到底有没有查到」这件事此前**没有任何
    /// 脱离窗口的验证途径**，正是模块文档「曾经的缺陷」那一节记的裸
    /// 名字问题能一路躲过验收的结构性原因：唯一能发现它的地方是人眼
    /// 看着一个开着的窗口，而 [ADR
    /// 0025](../../../../knowledge/decisions/0025-demo-interaction-verification-forbids-sendkeys.md)
    /// 又禁止用合成按键自动化那种验收。
    ///
    /// 抽出来的是「键 → 字段」这一段映射本身（[`Self::new`] 现在就是
    /// 它的一行适配器，不是第二份实现）。真实用例见
    /// `crates/ll-game/tests/atlas_coverage.rs`：那里用真实 `assets/`
    /// 打出来的图集元数据当查表源，断言五个 `textured_*` 全部返回
    /// `Some`。
    pub fn from_uv_lookup(lookup: impl Fn(&str) -> Option<[f32; 4]>) -> NineSliceSkin {
        NineSliceSkin {
            panel_border_uv: lookup(PANEL_BORDER_KEY),
            panel_fill_uv: lookup(PANEL_FILL_KEY),
            bar_track_uv: lookup(BAR_TRACK_KEY),
            bar_fill_uv: lookup(BAR_FILL_KEY),
            daynight_bar_uv: lookup(DAYNIGHT_BAR_KEY),
            border_thickness: 4.0,
        }
    }
}

/// 九宫格面板边框贴图的图集键。
pub const PANEL_BORDER_KEY: &str = "lostland:ui_panel_border";
/// 九宫格面板填充贴图的图集键。
pub const PANEL_FILL_KEY: &str = "lostland:ui_panel_fill";
/// 条形底槽贴图的图集键。
pub const BAR_TRACK_KEY: &str = "lostland:ui_bar_track";
/// 条形填充贴图的图集键。
pub const BAR_FILL_KEY: &str = "lostland:ui_bar_fill";
/// 昼夜滑条底图的图集键。
pub const DAYNIGHT_BAR_KEY: &str = "lostland:ui_daynight_bar";

/// [`NineSliceSkin`] 需要、且本体**确实提供**了资产的全部图集键。
///
/// 公开出来只有一个目的：让 `crates/ll-game/tests/atlas_coverage.rs`
/// 能拿真实图集逐个核对，而不是在测试里重抄一份字符串字面量——重抄
/// 一份的话，改名时两边分叉，测试会继续绿着而画面已经退回纯色，正是
/// 模块文档「曾经的缺陷」一节记的那种失效方式。
///
/// **不包含**按钮等「本体本来就没有资产」的贴图：那些恒走纯色路径是
/// 有意的，不是缺陷，见模块文档末尾。
pub const REQUIRED_SPRITE_KEYS: [&str; 5] = [
    PANEL_BORDER_KEY,
    PANEL_FILL_KEY,
    BAR_TRACK_KEY,
    BAR_FILL_KEY,
    DAYNIGHT_BAR_KEY,
];

/// 不透明白——纹理采样结果原样显示，不做任何颜色调制。
const NO_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// 生命条立即层调制——乘在中性偏蓝的 `ui_bar_fill` 贴图上,把它染成
/// 暖红,与法力条的冷蓝调制拉开色相差,理由见
/// [`BarStyleId::Health`] 文档。
const HEALTH_FILL_TINT: [f32; 4] = [1.0, 0.45, 0.35, 1.0];
/// 生命条余晖层调制——比 [`HEALTH_FILL_TINT`] 更暗更透明,理由同旧版
/// `AFTERGLOW_TINT`。
const HEALTH_AFTERGLOW_TINT: [f32; 4] = [0.75, 0.35, 0.3, 0.85];
/// 法力条立即层调制——比原始贴图色更蓝,与生命条的暖红拉开色相差。
const MANA_FILL_TINT: [f32; 4] = [0.45, 0.65, 1.0, 1.0];
/// 法力条余晖层调制，理由同 [`HEALTH_AFTERGLOW_TINT`]。
const MANA_AFTERGLOW_TINT: [f32; 4] = [0.35, 0.45, 0.75, 0.85];
/// 昼夜滑条指针调制——不透明暖黄,在昼夜贴图的深蓝/暖橙背景上都能
/// 保持可辨识（既不像夜晚的深蓝会隐没,也不像正午的暖橙会糊在一起）。
const DAYNIGHT_POINTER_TINT: [f32; 4] = [1.0, 0.92, 0.55, 1.0];

impl Skin for NineSliceSkin {
    fn panel(&self, style: PanelStyleId) -> FlatPanelAppearance {
        // 贴图缺失时的纯色回退——见类型文档。
        FlatColorSkin.panel(style)
    }

    fn bar(&self, style: BarStyleId) -> FlatBarAppearance {
        FlatColorSkin.bar(style)
    }

    fn day_night_bar(&self, style: DayNightBarStyleId) -> FlatDayNightBarAppearance {
        FlatColorSkin.day_night_bar(style)
    }

    fn button(&self, style: ButtonStyleId, visual: ButtonVisualState) -> FlatButtonAppearance {
        // 贴图缺失时的纯色回退——见类型文档,与 `panel`/`bar`/
        // `day_night_bar` 同一条既有纪律。
        FlatColorSkin.button(style, visual)
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
            BarStyleId::Health | BarStyleId::Mana => None,
        }
    }

    fn textured_two_layer_bar(&self, style: BarStyleId) -> Option<TexturedTwoLayerBarAppearance> {
        match style {
            BarStyleId::Health => Some(TexturedTwoLayerBarAppearance {
                track_uv: self.bar_track_uv?,
                fill_uv: self.bar_fill_uv?,
                track_tint: NO_TINT,
                afterglow_tint: HEALTH_AFTERGLOW_TINT,
                fill_tint: HEALTH_FILL_TINT,
            }),
            BarStyleId::Mana => Some(TexturedTwoLayerBarAppearance {
                track_uv: self.bar_track_uv?,
                fill_uv: self.bar_fill_uv?,
                track_tint: NO_TINT,
                afterglow_tint: MANA_AFTERGLOW_TINT,
                fill_tint: MANA_FILL_TINT,
            }),
            BarStyleId::Progress => None,
        }
    }

    fn textured_day_night_bar(
        &self,
        style: DayNightBarStyleId,
    ) -> Option<TexturedDayNightBarAppearance> {
        match style {
            DayNightBarStyleId::Clock => Some(TexturedDayNightBarAppearance {
                track_uv: self.daynight_bar_uv?,
                track_tint: NO_TINT,
                pointer_color: DAYNIGHT_POINTER_TINT,
            }),
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
        assert!(skin.textured_two_layer_bar(BarStyleId::Health).is_none());
        assert!(
            skin.textured_day_night_bar(DayNightBarStyleId::Clock)
                .is_none()
        );
    }

    #[test]
    fn flat_color_skin的生命条与法力条外观颜色不同() {
        // 这是「两条资源条分不清哪条是哪条」问题的直接回归——见
        // `BarStyleId::Health` 文档。
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let health = skin.bar(BarStyleId::Health);
        let mana = skin.bar(BarStyleId::Mana);

        // Assert
        assert_ne!(health.fill_color, mana.fill_color);
    }

    #[test]
    fn flat_color_skin对昼夜滑条样式恒返回默认外观() {
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let appearance = skin.day_night_bar(DayNightBarStyleId::Clock);

        // Assert
        assert_eq!(appearance, FlatDayNightBarAppearance::DEFAULT);
    }

    #[test]
    fn flat_color_skin的按钮悬停外观与普通外观不同() {
        // 四种状态外观必须走皮肤且互相可辨——这是任务书的硬约束,直接
        // 核实颜色确实不同,不是只核实"能调用而不 panic"。
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let normal = skin.button(ButtonStyleId::Default, ButtonVisualState::Normal);
        let hovered = skin.button(ButtonStyleId::Default, ButtonVisualState::Hovered);

        // Assert
        assert_ne!(normal.fill_color, hovered.fill_color);
    }

    #[test]
    fn flat_color_skin的按钮按下外观与悬停外观不同() {
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let hovered = skin.button(ButtonStyleId::Default, ButtonVisualState::Hovered);
        let pressed = skin.button(ButtonStyleId::Default, ButtonVisualState::Pressed);

        // Assert
        assert_ne!(hovered.fill_color, pressed.fill_color);
    }

    #[test]
    fn flat_color_skin的按钮禁用外观与普通外观不同() {
        // Arrange
        let skin = FlatColorSkin;

        // Act
        let normal = skin.button(ButtonStyleId::Default, ButtonVisualState::Normal);
        let disabled = skin.button(ButtonStyleId::Default, ButtonVisualState::Disabled);

        // Assert
        assert_ne!(normal.fill_color, disabled.fill_color);
    }

    #[test]
    fn flat_color_skin的按钮贴图方法恒返回none() {
        // Arrange
        let skin = FlatColorSkin;

        // Act & Assert：trait 默认实现，未被覆盖。
        assert!(
            skin.textured_button(ButtonStyleId::Default, ButtonVisualState::Normal)
                .is_none()
        );
    }

    // `NineSliceSkin` 本身没有单元测试——它的构造需要一个真实
    // `ll_render::atlas::Atlas`，而 `Atlas` 只能从一个真实 GPU 设备
    // 构造（见其类型文档，没有脱离设备的构造路径），与本文件其余
    // `NineSliceSkin` 方法（`panel`/`bar`/`day_night_bar`）此前就已经
    // 是同样的既有空白，`button`/`textured_button` 不是本批次新引入
    // 的例外。`NineSliceSkin::button`/`textured_button` 的实现只是
    // 直接委托给 `FlatColorSkin`（`textured_button` 用 trait 默认
    // 实现，未覆盖），逻辑上不需要真实 Atlas 也能核实正确性,但受限于
    // 现有测试基础设施,这里如实标注为已知空白,不是遗漏。
}
