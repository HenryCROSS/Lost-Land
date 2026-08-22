//! 只读游戏内 HUD——P7 第一批范围：状态栏（时间/生命/法力，常驻）、
//! 角色面板（六项属性 + 等级 + 经验 + 生效中的属性修正）、背包（物品
//! 列表 + 数量 + 耐久）、装备栏（各槽位当前装了什么）。
//!
//! # 只读，不做任何交互
//!
//! 任务书「本批次范围」一节明确排除：不能点、不能选、不能拖，不做
//! 设置界面/主菜单，不修改任何世界状态的能力。本模块与 crate 顶层的
//! [`crate::load_report_view`] 同一条纪律——只消费上游已经算好的世界
//! 状态，把它变成屏幕上的文字，从不写回。
//!
//! # 建在 [`crate::widget`] 之上，不再是裸字符串拼位置
//!
//! 四块面板各自的「产出这一帧的显示内容」函数（[`status_bar`]/
//! [`character_panel`]/[`inventory_panel`]/[`equipment_panel`]）现在
//! 产出 [`crate::widget::label::Label`]（经 [`crate::widget::list::RowCursor`]
//! 逐行推进），[`render`] 模块再把每块面板的内容矩形交给
//! [`crate::widget::panel::panel_quads`] 画出九宫格背景、经验条交给
//! [`crate::widget::bar::bar_quads`]——这是项目所有者裁定「不能只是把
//! 字符串画在固定位置，要做出这四块屏幕真正需要的控件」之后的落地，
//! 见 [`crate::widget`] 模块文档「不是通用控件库,是把这五样做对」
//! 一节的完整论证。
//!
//! # 四块面板全部常驻，不做按键切换
//!
//! 任务书把「状态栏必须常驻」定成硬约束（ADR 0025 禁止合成按键验证，
//! 任何需要按键才能看到的内容都无法被截图证明），但把「角色面板/背包/
//! 装备栏要不要做成可切换」留给实现判断——本批次选择**全部常驻**，
//! 不引入任何面板切换键，理由：
//!
//! 1. **可验证性一致**——若这三块做成按键切换，实机截图就只能证明
//!    「切换前」或「切换后」两种状态里的一种，且切换动作本身受 ADR
//!    0025 约束（不得用合成键盘事件盲注切换键），验证链路会比常驻更
//!    脆弱、更绕。全部常驻使得一张截图能同时证明四块面板都真的画出来
//!    了，这是最直接、最不需要额外解释的验证路径。
//! 2. **YAGNI**——本批次是「只读观测层」的第一批，尚不涉及任何交互
//!    （连点选都不做），提前设计一套面板切换状态机（哪个键切哪块、
//!    切换后其余面板挡住了地图视野怎么办）没有真实需求驱动，属于
//!    「像素级打磨」的一部分，任务书已把这类打磨排给 P7 完整批次。
//!
//! 布局因此是四块面板固定分区平铺，不重叠、不遮挡地图核心视口太多——
//! 具体坐标见 [`render::render_hud`] 与各面板模块的 `DEFAULT_*` 常量。
//!
//! # 世界地图（[`world_map`]）是第一个例外：M 键切换
//!
//! 上一节的论证只覆盖状态栏/角色面板/背包/装备栏这四块——它们不做
//! 切换的第 1 条理由（可验证性）在世界地图这里不成立：世界地图不是
//! 「同一份内容换个显示时机」，而是一块只有玩家主动查看才需要占据
//! 屏幕的独立面板（长期常驻会一直遮住地图核心视口），且实机验收不再
//! 只能靠「截一张图」，可以拆成程序化断言（切换状态真的翻转、面板真的
//! 被加入渲染帧）与「初始状态设为已打开」的实机截图两层，见
//! `ll_game::app` 模块文档与 `ll_platform::input::GameKey::Map` 文档
//! ——因此世界地图选择做成 M 键切换,不进四块常驻面板的行列。

pub mod character_panel;
pub mod equipment_panel;
pub mod inventory_panel;
pub mod render;
pub mod status_bar;
pub mod world_map;

use ll_core::ident::ContentIndex;
use ll_i18n::Catalog;
use ll_mod::item::ItemTable;

use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 全部四块面板共用的默认行高（像素）——与
/// [`crate::load_report_view::DEFAULT_LINE_HEIGHT`] 取值一致,保持
/// 同一套字号/行高节奏,读起来是同一个产品而不是两套风格拼起来的。
pub const DEFAULT_LINE_HEIGHT: f32 = 18.0;
/// 全部四块面板共用的默认内边距（像素）——面板背景（见
/// [`crate::widget::panel::panel_quads`]）与内容文字之间的留白。
pub const DEFAULT_PADDING: f32 = 6.0;
/// 全部四块面板共用的默认字号（像素）。
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// 一块面板的完整产出：背景矩形（喂给
/// [`crate::widget::panel::panel_quads`]）+ 这一帧的全部文本行。
///
/// 面板高度由内容行数现算（`行数 * line_height + 2 * padding`），不是
/// 写死的常量——面板内容长度会变（背包物品数量、生效中的修正条数都
/// 不固定），高度跟着内容走才不会出现「面板背景比文字短一截」或者
/// 「背景比文字长一大截」的错位。
pub struct PanelContent {
    /// 面板背景矩形，喂给 [`crate::widget::panel::panel_quads`]。
    pub rect: Rect,
    /// 这一帧面板内容的全部文本行。
    pub labels: Vec<Label>,
}

/// 建一块面板：在 `origin` 处开一个宽 `width` 的面板，`fill` 负责用
/// 传入的 [`RowCursor`] 逐行写内容，本函数收尾时按 `fill` 实际写了
/// 多少行现算出面板矩形的高度。四个面板模块的公开入口都是这个函数的
/// 薄封装,不重复这套「游标 + 现算高度」的样板。
pub(crate) fn build_panel(
    origin: (f32, f32),
    width: f32,
    fill: impl FnOnce(&mut RowCursor, &mut Vec<Label>),
) -> PanelContent {
    let content_origin = (origin.0 + DEFAULT_PADDING, origin.1 + DEFAULT_PADDING);
    let mut cursor = RowCursor::new(content_origin, DEFAULT_LINE_HEIGHT);
    let mut labels = Vec::new();
    fill(&mut cursor, &mut labels);
    let content_height = cursor.cursor_y() - content_origin.1;
    let rect = Rect::new(
        origin.0,
        origin.1,
        width,
        content_height + DEFAULT_PADDING * 2.0,
    );
    PanelContent { rect, labels }
}

/// 查一件物品的显示名——[`inventory_panel`]/[`equipment_panel`] 共用，
/// 查不到定义时退化成 `#<索引>`（见 [`inventory_panel`] 模块文档「查不
/// 到物品定义时怎么办」一节），不 panic、不悄悄跳过。
pub(crate) fn item_display_name(
    def: ContentIndex,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    match items.get(def) {
        Some(view) => catalog.resolve(language, &view.display_name_key.to_string()),
        None => format!("#{}", def.get()),
    }
}
