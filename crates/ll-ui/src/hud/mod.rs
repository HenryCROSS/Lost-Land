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
//! # 动作菜单（[`action_menu`]）是「只读」的第二个例外，也是唯一一个
//! **能写世界状态**的例外
//!
//! 上一节那条「只读，不做任何交互」在本 crate 内部仍然成立——
//! [`action_menu`] 自己一个字节的世界状态都不写，它产出的仍然只是
//! [`crate::widget::label::Label`]。变的是**它画的东西有选中态**：光标
//! 落在第几行由调用方（`ll-game`）持有，玩家据此提交的
//! `ll_sim::intent::Intent` 经 `ll_sim::turn::TurnEngine` 改变世界。
//!
//! 换句话说，写世界的仍然是结算层，HUD 只是第一次开始**显示一个选择**。
//! 这条分界必须守住：本 crate 一旦自己去改 `WorldState`，「呈现层不写
//! 世界」这条纪律就没有任何结构性保障了。
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

pub mod action_menu;
pub mod bottom_rows;
pub mod character_panel;
pub mod equipment_panel;
pub mod inventory_panel;
pub mod placement;
pub mod render;
pub mod skinned_push;
pub mod status_bar;
pub mod world_map;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_i18n::{Catalog, FluentArgs};
use ll_mod::item::ItemTable;

use ll_text::MeasureText;

use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 全部四块面板共用的默认行高（像素）——与
/// [`crate::load_report_view::DEFAULT_LINE_HEIGHT`] 取值一致,保持
/// 同一套字号/行高节奏,读起来是同一个产品而不是两套风格拼起来的。
pub const DEFAULT_LINE_HEIGHT: f32 = 18.0;
/// 全部四块面板共用的默认内边距（像素）——面板背景（见
/// [`crate::widget::panel::panel_quads`]）与内容文字之间的留白。
///
/// 规格 L3 之后它只是 [`crate::widget::metrics::PANEL_PADDING`] 的一个
/// 别名：模态屏那套（原 `screen::SCREEN_PADDING` = 10）与本条（原 6）
/// 合并成同一个刻度，理由与取值见那里。这条路径保留是为了不动全部
/// 调用点与两道门禁。
pub const DEFAULT_PADDING: f32 = crate::widget::metrics::PANEL_PADDING;
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

/// 一块 `width` 宽的面板去掉两侧内边距之后剩下的**内容宽**——也就是
/// 这块面板里每一行文字的断行宽度。
///
/// # 这是「面板宽度是唯一真相源」那条不变式的落点
///
/// 此前 HUD 全部文本的断行宽度是 `crate::hud::render` 里一个写死的
/// `400.0`，而六块面板的内容宽是 608/248/208/208/348/408——**没有一块
/// 等于 400**，于是每块面板要么提前换行（第二行掉出面板）要么根本不
/// 换行（直接画到面板外面），见
/// `knowledge/design/ui-and-navigation.md` §8.2 那张表。
///
/// 现在断行宽度只有这一个产出点，它的输入只有面板宽度一个。
pub fn content_width(panel_width: f32) -> f32 {
    // 面板比两侧内边距还窄这种极端配置下会算出负数——**不钳制**，与
    // `render::equipment_rect` 同一条取舍：钳制会把「面板宽度被配
    // 错了」这种应该显形的问题掩盖成一块看起来正常、内容却被挤没了的
    // 面板。
    panel_width - DEFAULT_PADDING * 2.0
}

/// 建一块面板：在 `origin` 处开一个宽 `width` 的面板，`fill` 负责用
/// 传入的 [`RowCursor`] 逐行写内容，本函数收尾时按 `fill` 实际写了
/// 多少行现算出面板矩形的高度。四个面板模块的公开入口都是这个函数的
/// 薄封装,不重复这套「游标 + 现算高度」的样板。
pub(crate) fn build_panel(
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    width: f32,
    fill: impl FnOnce(&mut RowCursor<'_>, &mut Vec<Label>),
) -> PanelContent {
    let content_origin = (origin.0 + DEFAULT_PADDING, origin.1 + DEFAULT_PADDING);
    let mut cursor = RowCursor::new(
        measure,
        content_origin,
        DEFAULT_LINE_HEIGHT,
        DEFAULT_FONT_SIZE,
        content_width(width),
    );
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
///
/// # 未鉴定的东西显示成什么（未鉴定物品批次）
///
/// `identified` 是**观察者**已经认得的物品种类列表
/// （[`ll_world::entity::Agent::identified_items`]）。一件声明了
/// `requires_identification` 而又不在这个列表里的东西，名字换成
/// `hud-item-unidentified`——「未鉴定不影响任何结算，只影响呈现」这条
/// 裁定的**唯一**落点就在这个函数里（见该字段文档）。
///
/// 参数是「观察者认得哪些」而不是「玩家认得哪些」：同一件物品在两个
/// 不同角色的面板上本就该显示成不同的名字，把它做成全局状态会在有随从
/// 面板的那一天立刻塌掉。
///
/// # 为什么只有一句笼统的「未鉴定的物品」，不是「一把未知的剑」
///
/// 分门别类的说法（剑/药水/卷轴）需要内容作者再声明一条「这属于哪个
/// 外观类别」的字段，而那个字段今天没有任何别的消费者——YAGNI 与
/// ADR 0021 同时指向「先不加」。真要加，加法是 `ItemDef` 上一条指向
/// 本地化键的 `unidentified_name_key`，本函数改成「有就用它、没有就
/// 退回这句笼统的」，不需要改这里的任何调用点。
pub fn item_display_name(
    def: ContentIndex,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    identified: &[ContentIndex],
) -> String {
    match items.get(def) {
        Some(view) => {
            if view.requires_identification && !identified.contains(&def) {
                return catalog.resolve(language, "hud-item-unidentified");
            }
            if let Some(species_key) = items.corpse_species_name_key(def) {
                return corpse_display_name(view.display_name_key, species_key, catalog, language);
            }
            catalog.resolve(language, &view.display_name_key.to_string())
        }
        None => format!("#{}", def.get()),
    }
}

/// 一具尸体的显示名：拿物种自己的显示名键查出物种名，再插进通用的
/// 「{ $species }的尸体」消息（`item-corpse-display_name`）。
///
/// # 为什么尸体名要在呈现层拼，不是在内容里写死
///
/// 因为**第三方 mod 加一个种族必须自动获得能显示的尸体名**（规格
/// §10.3、ADR 0018 的「本体即 Mod」检验）。若每个物种一条 Fluent 键，
/// 本体的九个种族补得上，第三方 mod 的种族补不上——mod 的 `.ftl` 装载
/// 至今没有落地（`ll_i18n` 模块文档「五、mod 的 `.ftl`」）。走参数插值
/// 之后，物种那一半复用种族**早就有的** `display_name_key`，尸体不多
/// 欠任何一条翻译。完整论证见 `ll_mod::corpse_item` 模块文档。
///
/// `name_key` 是尸体 `ItemDef` 自己的显示名键（全部尸体共用一条，
/// `ll_mod::corpse_item::CORPSE_DISPLAY_NAME_KEY`）——传进来而不是在
/// 这里写死字面量，好让「那条键叫什么」只有注册那一处一个真相源。
fn corpse_display_name(
    name_key: &NamespacedId,
    species_key: &NamespacedId,
    catalog: &Catalog,
    language: &str,
) -> String {
    let species = catalog.resolve(language, &species_key.to_string());
    let mut args = FluentArgs::new();
    args.set("species", species);
    catalog.resolve_with_args(language, &name_key.to_string(), Some(&args))
}
