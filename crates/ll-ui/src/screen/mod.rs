//! 模态屏：盖住游戏画面的整屏 UI——游戏内菜单与设置界面。
//!
//! # 与 [`crate::hud`] 的分界
//!
//! 一句话：**`hud` 画在世界之上，`screen` 盖住世界。**
//!
//! | | [`crate::hud`] | 本模块 |
//! |---|---|---|
//! | 世界在它底下 | 照常推进（玩家一按键就结算一回合） | **一个字节都不动**（`ll_game::app::Demo::advance` 早退） |
//! | 输入上下文 | `InputContext::Gameplay` | `InputContext::Menu`（经 `ll_ui::widget::ui_mode::UiModeStack`） |
//! | 占屏 | 贴边的几块面板，中间留给地图视口 | 居中一块，外加一层压暗整屏的背板 |
//!
//! 这条分界不是审美取舍，是[`crate::widget::ui_mode`] 那套上下文切换
//! 机制**只对本模块这一类屏成立**：`UiModeStack` 一压栈就把物理键映射
//! 整体切到菜单表，游戏内主流程的移动/交互键在那之后全部解析不出来。
//! HUD 的背包/制作菜单不走这条路（它们仍在 `Gameplay` 上下文下由
//! `ll_game::player_action` 自己判「菜单开着时方向键归菜单用」），本
//! 批次刻意不动它们。
//!
//! # 为什么只有一种屏，不是「菜单屏」加「设置屏」两套
//!
//! 两块屏在**画法上**完全同构：一块居中面板、一行标题、一列行、其中
//! 一行带光标标记、一行底部提示。区别全在**行里写什么字**——菜单是三
//! 条固定选项，设置是「语言：简体中文」「移动：上 / W」这类带当前值的
//! 行。而「一条设置该显示成什么字」需要同时认识 `GameConfig`、
//! `KeyBindings` 与 `Catalog`，那是 `ll-game`（唯一持有全部这些东西的
//! 那一层）的排版职责，不是本模块的。
//!
//! 这与 [`crate::hud::action_menu`] 的取舍逐字相同（「只收已经排好版的
//! 字符串，不收领域类型」），也是 ADR 0021 的直接应用：真正共用的算法
//! 只有「一列行 + 一个高亮标记 + 现算面板高度」这一段，为两块屏各写
//! 一份只会得到两份会各自漂移的同一个算法。
//!
//! # 为什么不复用 [`crate::hud`] 的 `build_panel`
//!
//! 如实记录，这一处是**刻意的重复**：`hud::build_panel` 与本模块的
//! [`build_screen_panel`] 是同一个「游标推行 + 按行数现算面板高度」的
//! 十行算法。不复用的理由是并行批次风险——本批次落地时另一个批次正在
//! 改 `crates/ll-ui/src/hud/` 下的面板布局（居中），而 git 抓不到这类
//! 语义冲突（交接文档第一节纪律 5 记着这个教训的实例）。十行重复换来
//! 「本模块与 `hud/` 零耦合」，代价明确、可逆：两个批次都落地之后，把
//! 这一段收拢回一个共用 helper 是一次纯机械的重构。

pub mod render;

use ll_i18n::Catalog;
use ll_text::MeasureText;

use crate::widget::geometry::{Anchor, Rect};
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 模态屏的行高（像素）——与 [`crate::hud::DEFAULT_LINE_HEIGHT`] 取同
/// 一个值，两块屏读起来要像同一个产品。
pub const SCREEN_LINE_HEIGHT: f32 = 18.0;
/// 模态屏面板的内边距（像素）。
///
/// 规格 L3 之后它只是 [`crate::widget::metrics::PANEL_PADDING`] 的一个
/// 别名：本条（原 10）与 HUD 那套（`hud::DEFAULT_PADDING`，原 6）合并
/// 成同一个刻度，取 6，理由见那里。**模态屏的内容宽因此从 500 变成
/// 508**（变宽），溢出门禁的行数预算只会更宽松，不会有键新超预算。
pub const SCREEN_PADDING: f32 = crate::widget::metrics::PANEL_PADDING;
/// 模态屏正文字号（像素）。
pub const SCREEN_FONT_SIZE: f32 = 14.0;
/// 模态屏面板的**最小**宽度（像素）。
///
/// # 从「写死」改成「下限」（规格 W3 / F5）
///
/// 此前这是写死的面板宽度，理由是「现算会让面板宽度随光标移动而跳变
/// （不同行的文字长短不一），看起来像画面在抖」。**那条理由针对的是
/// 行，而行从来不是问题**——`knowledge/design/ui-and-navigation.md`
/// 八节实测：全部模态屏的行都在 500 内，最长 285.3。真正放不下的是
/// **提示语、通知与说明文本**：`screen-savename-hint` 在中英两种语言
/// 下都溢出，`screen-chargen-player-died` 的英文是可用宽的 145%，四条
/// 世界预设说明 583.9–649.1 全部撞 500 的内容框。
///
/// 而底部那一行提示是这个游戏**唯一**的按键教学（九节 F5），它被布局
/// 缺陷吃掉不是「不好看」，是玩家不知道能按什么。
///
/// 现在宽度是 [`panel_width`]：`max(本常量, 本屏最长行 + 2 × 内边距)`，
/// **每屏算一次、进屏时固定**——输入是「这一屏的全部行」，光标移动不
/// 改变行的集合，因此原来那条反对理由指的跳变不会发生。
pub const SCREEN_WIDTH: f32 = 520.0;

/// 模态屏面板与窗口边缘之间至少要留的空（像素）——规格 L3 之后它就是
/// [`crate::widget::metrics::SCREEN_MARGIN`] 本身，不再是「碰巧与
/// `hud::render` 那个取同一个值」：两块屏贴边的节奏由同一个常量保证，
/// 改一个另一个自动跟上。
///
/// [`panel_width`] 按内容伸缩时以它为上限：面板再宽也不许长到窗口
/// 外面去，撞到上限之后多出来的文字改走换行（规格 W3 采纳的第二条
/// 手段）。
pub const SCREEN_SIDE_MARGIN: f32 = crate::widget::metrics::SCREEN_MARGIN;

/// 量「这一行本来要多宽」时给的断行宽度——大到任何一条 UI 文案都不
/// 可能被它断开，于是量出来的就是**不换行时的自然宽度**。
///
/// 不用 `f32::INFINITY`：它会被传进 `cosmic_text::Buffer::set_size`，
/// 一个非有限值在那一层的行为没有文档承诺，不值得赌。
const NATURAL_WIDTH_PROBE: f32 = 1.0e6;

// 光标记号**不在文本内容里**（规格 W7 / F7）——它是一块高亮矩形，见
// [`FOCUS_HIGHLIGHT_COLOR`] 与 `crate::screen::render` 的
// `push_row_highlights`。此前这里有一对 `CURSOR_PREFIX = "> "` /
// `IDLE_PREFIX = "  "`，文档写着两者「等宽」——而内嵌字体是比例字体，
// 10.91px 对 6.27px，那句话从来就是假的，整列文字随光标上下移动而
// 左右抖动 4.6px。理由与取舍逐字见 `crate::hud::action_menu` 模块文档
// 「光标标记是一块高亮矩形」一节。

/// 压暗整屏的背板颜色（RGBA，`0.0..=1.0`）。
///
/// 它不只是装饰：这一层是「世界现在不动了」这个事实**唯一的视觉表达**。
/// 玩家看到画面压暗，就知道按方向键不会再走路——没有它，一块居中面板
/// 看起来与 HUD 的背包面板没有区别，而两者的语义完全相反（见模块文档
/// 那张表）。
const BACKDROP_COLOR: [f32; 4] = [0.02, 0.02, 0.04, 0.72];

/// 一块模态屏这一帧要显示的全部内容——形状与
/// [`crate::hud::PanelContent`] 平行（背景矩形 + 文本行），只是多一层
/// 压暗背板。
pub struct ScreenContent {
    /// 压暗整屏的背板矩形，恒为整个窗口大小。
    pub backdrop: Rect,
    /// 居中面板的背景矩形。
    pub panel: Rect,
    /// 这一帧面板内容的全部文本行。
    pub labels: Vec<Label>,
    /// [`ScreenData::rows`] 里第 i 行占的那块矩形，与 `rows` 逐条对应。
    ///
    /// **它就是这块屏的按钮清单**：鼠标点在第几行由
    /// [`crate::widget::hit_test::hit_test`] 拿它现算，聚焦/悬停高亮也
    /// 画在它上面。列表为空（显示占位行）时它是空的——占位行不是一个
    /// 可点的按钮。
    ///
    /// 之所以从这里出来而不是让调用方自己按行高算一遍：那就是两份同一
    /// 个布局算法，改了标题行/提示行的位置就会分叉，而分叉时点击会**静
    /// 悄悄地**落到隔壁那一行上。
    pub row_rects: Vec<Rect>,
}

/// 一块模态屏这一帧要显示的内容——调用方（`ll-game`）已经把每一行排好
/// 版，本模块只负责摆位置、加光标标记。
pub struct ScreenData<'a> {
    /// 屏幕标题的 Fluent 键。
    pub title_key: &'a str,
    /// 全部行，已由调用方排好版（见模块文档「为什么只有一种屏」）。
    pub rows: &'a [String],
    /// 光标落在第几行。超出 `rows` 范围时不标记任何一行——不钳制、也
    /// 不 panic，与 [`crate::hud::action_menu::ActionMenuData::cursor`]
    /// 同一条理由：钳制会掩盖调用方的光标维护缺陷，panic 会让一个纯
    /// 显示问题拖垮整个游戏。
    pub cursor: usize,
    /// 列表为空时显示的占位行的 Fluent 键。
    pub empty_key: &'a str,
    /// 面板底部操作提示行的 Fluent 键。
    pub hint_key: &'a str,
    /// 额外的一行状态/错误提示（**已经解析好**的字符串，`None` 表示
    /// 这一帧没有话要说）。
    ///
    /// 收已解析的字符串而不是 Fluent 键：键位冲突那句话要带参数
    /// （「这个键已经绑给了 X」中的 X 是另一个动作的显示名），参数注入
    /// 是持有 `Catalog` 与领域数据的调用方的事，与
    /// [`crate::hud::render::build_hud_frame`] 的 `feedback` 参数同一条
    /// 理由。
    pub notice: Option<&'a str>,
    /// 指针这一刻悬停在第几行（`None` = 不在任何一行上）。
    ///
    /// # 它为什么与 [`ScreenData::cursor`] 是两个字段
    ///
    /// 「悬停」与「聚焦」在这套 UI 里是两件事：指针划过一行**不改变
    /// 键盘焦点**（`ll_game::pointer` 模块文档约定一），它只是在说
    /// 「点下去会是这一行」。合成一个字段就等于把那条约定抹掉。
    ///
    /// 两者落在同一行时只画聚焦那一块高亮（更亮的那个），见
    /// [`crate::screen::render::build_screen_frame`]。
    pub hovered: Option<usize>,
}

/// 居中一块 [`SCREEN_WIDTH`] 宽、高度按内容行数现算的面板。
///
/// 纵向也居中：面板高度随行数变化（菜单三行、设置二十几行），按内容
/// 高度算出来再居中，比写死一个 y 坐标更不容易在行数变化后错位。
/// `screen_height` 比面板还矮这种极端窗口尺寸下会算出负坐标——**不
/// 钳制**，与 [`crate::hud::render`] 的 `equipment_rect` 同一条
/// 取舍：钳制会掩盖「窗口配置改小了却没人发现面板被塞没了」这种应该
/// 显形的问题。
fn centered_origin(
    screen_width: f32,
    screen_height: f32,
    panel_width: f32,
    panel_height: f32,
) -> Rect {
    // 规格 L2：这一份居中算术走 `Rect::anchored`。
    Rect::anchored(
        (screen_width, screen_height),
        Anchor::Center,
        (panel_width, panel_height),
        0.0,
    )
}

/// 这一屏的面板该多宽：`max(SCREEN_WIDTH, 最长行的自然宽 + 2 * 内边距)`，
/// 再以「不许长到窗口外面」为上限钳一次。
///
/// **每屏算一次**，输入是这一屏的全部行文本（[`screen_text_lines`]），
/// 与光标落在第几行无关——见 [`SCREEN_WIDTH`] 文档。
pub fn panel_width(lines: &[String], measure: &mut dyn MeasureText, screen_width: f32) -> f32 {
    let longest = lines
        .iter()
        .map(|text| {
            measure
                .measure_text(
                    text,
                    SCREEN_FONT_SIZE,
                    SCREEN_LINE_HEIGHT,
                    NATURAL_WIDTH_PROBE,
                )
                .max_line_width
        })
        .fold(0.0_f32, f32::max);
    // 往**上**取整，不是四舍五入：面板宽度是「这一屏最长那一行画得完」
    // 的下限，而规格 L0 的取整（[`Rect::snap`]）最多能把面板削掉不到
    // 一个像素——四舍五入下来正好会把「恰好放得下」变成「差 0.46px
    // 放不下」（落地本条时实测：722.46 + 20 = 742.46 被舍成 742）。
    // 先 `ceil` 一次，取整就再也咬不动它。
    let wanted = SCREEN_WIDTH.max(longest + SCREEN_PADDING * 2.0).ceil();
    // 上限反过来往**下**取整，同一条理由的另一半：面板再宽也不许越过
    // 窗口边缘，取整不能把它推出去。
    let ceiling = (screen_width - SCREEN_SIDE_MARGIN * 2.0).floor();
    // 窗口比最小宽度还窄时 `ceiling` 会小于 `SCREEN_WIDTH`——这时候取
    // `ceiling`（面板缩窄、文字换行），不取 `SCREEN_WIDTH`（面板伸出
    // 窗口、文字直接看不见）。
    wanted.min(ceiling.max(0.0))
}

/// 这一屏这一帧要显示的**全部行的文本**，按从上到下顺序：标题、各行
/// （或占位行）、可选的通知、提示行。
///
/// 抽出来是因为有两个消费者需要**同一份**文本而不需要几何：
/// [`panel_width`]（量宽度）与 [`layout_screen`]（排位置）。两处各拼
/// 一遍就是同一段排版逻辑的两份副本，而副本迟早分叉。
///
/// 返回值第二项是「这些行里从第几条开始是可点的行」（`rows` 在
/// [`ScreenData`] 里的那些），`None` 表示这一屏显示的是占位行——占位行
/// 不是一个可点的按钮。
pub fn screen_text_lines(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
) -> (Vec<String>, Option<usize>) {
    let mut lines = vec![catalog.resolve(language, data.title_key)];
    let rows_start = if data.rows.is_empty() {
        lines.push(catalog.resolve(language, data.empty_key));
        None
    } else {
        let start = lines.len();
        // 行文字就是调用方给的那一串，一个字符都不多——光标记号是一块
        // 高亮矩形，不在文本里（规格 W7 / F7，见本文件 `IDLE_PREFIX`
        // 原址那段注释）。
        lines.extend(data.rows.iter().cloned());
        Some(start)
    };
    if let Some(notice) = data.notice {
        lines.push(notice.to_string());
    }
    lines.push(catalog.resolve(language, data.hint_key));
    (lines, rows_start)
}

/// [`layout_screen`] 的产出：全部文本行、每个可点行的矩形、内容区
/// 实际占用的总高。
struct ScreenLayout {
    labels: Vec<Label>,
    row_rects: Vec<Rect>,
    content_height: f32,
}

/// 把 [`screen_text_lines`] 的文本按行排出位置，**同时**产出每一个
/// 可点行的矩形与内容总高。
///
/// # 行矩形为什么必须与行文字同一个产出点
///
/// 此前行矩形是按 `content_origin.1 + (i + 1) * SCREEN_LINE_HEIGHT`
/// 这条**公式**算的——它假设「标题恰占一行、每一行恰占一行」。规格 W2
/// 落地之后这个假设不再成立（一条长行会占两行），公式与实际画出来的
/// 位置会**静悄悄地**错开，点击落到隔壁那一行上。
///
/// 现在两者出自同一次游标推进：一行的矩形就是这一行推进前后的那一段，
/// 面板高度也是同一个游标的终点。
fn layout_screen(
    lines: &[String],
    rows_start: Option<usize>,
    row_count: usize,
    measure: &mut dyn MeasureText,
    content_origin: (f32, f32),
    wrap_width: f32,
) -> ScreenLayout {
    let mut cursor = RowCursor::new(
        measure,
        content_origin,
        SCREEN_LINE_HEIGHT,
        SCREEN_FONT_SIZE,
        wrap_width,
    );
    let mut labels = Vec::new();
    let mut row_rects = Vec::new();
    for (index, text) in lines.iter().enumerate() {
        let top = cursor.cursor_y();
        cursor.push(&mut labels, text.clone());
        let is_row = rows_start
            .map(|start| index >= start && index < start + row_count)
            .unwrap_or(false);
        if is_row {
            // 横向占满面板去掉两侧内边距之后的整条——按钮的可点区域是
            // **一整行**，不是那几个字所占的宽度：后者会让玩家点在一行
            // 的空白处什么都不发生。
            row_rects.push(Rect::new(
                content_origin.0,
                top,
                wrap_width,
                cursor.cursor_y() - top,
            ));
        }
    }
    let content_height = cursor.cursor_y() - content_origin.1;
    ScreenLayout {
        labels,
        row_rects,
        content_height,
    }
}

/// 产出一块模态屏的全部文本行——纯函数，不接触 GPU，供本模块测试与
/// `ll-game` 的验收测试直接断言「光标真的落在第几行」「某一行真的写着
/// 什么」。
///
/// 与 [`crate::hud::action_menu::action_menu_lines`] 同一条纪律：把
/// 「算出要显示什么」与「把它提交给 GPU」拆成两步，前一半才测得动
/// （ADR 0025 禁止用合成按键验收，程序化断言是本项目的默认验证手段）。
pub fn screen_lines(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    wrap_width: f32,
) -> Vec<Label> {
    let (texts, rows_start) = screen_text_lines(data, catalog, language);
    layout_screen(
        &texts,
        rows_start,
        data.rows.len(),
        measure,
        origin,
        wrap_width,
    )
    .labels
}

/// 建出整块模态屏：压暗背板 + 居中面板 + 全部文本行 + 每一行的矩形。
pub fn build_screen_panel(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    screen_width: f32,
    screen_height: f32,
) -> ScreenContent {
    let geometry = screen_geometry(
        data,
        catalog,
        language,
        measure,
        screen_width,
        screen_height,
    );
    ScreenContent {
        backdrop: Rect::new(0.0, 0.0, screen_width, screen_height),
        panel: geometry.panel,
        row_rects: geometry.row_rects,
        labels: geometry.labels,
    }
}

/// [`build_screen_panel`] 与 [`screen_row_rects`] 共用的那一段：算出
/// 面板宽、面板高、居中原点，再排一遍。
struct ScreenGeometry {
    panel: Rect,
    labels: Vec<Label>,
    row_rects: Vec<Rect>,
}

fn screen_geometry(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    screen_width: f32,
    screen_height: f32,
) -> ScreenGeometry {
    let (texts, rows_start) = screen_text_lines(data, catalog, language);
    let width = panel_width(&texts, measure, screen_width);
    let wrap_width = width - SCREEN_PADDING * 2.0;
    // 先按原点 (0, 0) 排一遍只为量出高度，再把量出来的高度拿去居中、
    // 用真正的原点重排一遍。两遍的代价是几十次字符串格式化，换来的是
    // 「面板高度恒等于内容高度」这条不变式不需要任何调用方去手算行数。
    //
    // 高度按**渲染出的行数**算（游标推进的终点），不是按标签条数算
    // ——规格 W2，见 `crate::widget::list::RowCursor` 模块文档。
    let probe = layout_screen(
        &texts,
        rows_start,
        data.rows.len(),
        measure,
        (0.0, 0.0),
        wrap_width,
    );
    let panel_height = probe.content_height + SCREEN_PADDING * 2.0;
    // 规格 L0：**这一处的取整刻意提前到几何算完那一刻**，不是等到
    // `ScreenFrame::snap_to_pixels` 那个提交出口。
    //
    // 理由是 `row_rects` 有两个消费者：一个是画行高亮（走提交出口，会
    // 被那一道取整），另一个是 `screen_row_rects` 拿去做**点击命中**
    // （压根不进渲染帧，那一道摸不到它）。两者若一取整一不取整，玩家
    // 点在两块行矩形交界那一像素上时，命中的行与高亮的行会差一格——
    // 而这正是本模块 `row_rects` 字段文档点名要防的那种「静悄悄落到
    // 隔壁那一行」。在这里取整一次，两个消费者拿到的就是同一份。
    let panel = centered_origin(screen_width, screen_height, width, panel_height).snap();
    let content_origin = (panel.x + SCREEN_PADDING, panel.y + SCREEN_PADDING);
    let laid = layout_screen(
        &texts,
        rows_start,
        data.rows.len(),
        measure,
        content_origin,
        wrap_width,
    );
    ScreenGeometry {
        panel,
        labels: laid.labels,
        row_rects: laid.row_rects.into_iter().map(|r| r.snap()).collect(),
    }
}

/// 只要这块屏每一行的矩形，不排全部文本——**输入这一侧**（这一帧鼠标
/// 点在第几行）的入口。
///
/// 与 [`build_screen_panel`] 走**同一个** [`screen_geometry`]（同一个
/// 面板宽度、同一次游标推进、同一批行矩形），因此「点击落在第几行」与
/// 「第几行画在哪儿」不可能对不上。
///
/// 此前两者只是「共用同一段公式」（`(i+1) × 行高`）——那个假设在
/// 规格 W2 落地之后不再成立，见 [`layout_screen`] 文档。
///
/// # 为什么输入这一侧要单独一个入口
///
/// 模态屏这一帧的输入处理排在渲染**之前**（`ll_game::app::Demo::on_frame`
/// 先 `update_screen` 再 `draw_screen`），那时候还没有 `ScreenContent`。
/// 而重排一遍全部标签只为了拿几个矩形，是几十次白付的字符串格式化。
pub fn screen_row_rects(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    screen_width: f32,
    screen_height: f32,
) -> Vec<Rect> {
    screen_geometry(
        data,
        catalog,
        language,
        measure,
        screen_width,
        screen_height,
    )
    .row_rects
}

// 行高亮的两个颜色与那段皮肤分支**搬去了 [`crate::widget::highlight`]**
// （规格 F7：HUD 的动作菜单也要画高亮，而 `hud` 与 `screen` 彼此不
// 依赖，唯一都看得见的地方是 `widget`）。这里保留三条重导出，公开
// 路径一条都不断——搬家不该逼调用方改 `use`。
pub use crate::widget::highlight::{
    FOCUS_HIGHLIGHT_COLOR, HOVER_HIGHLIGHT_COLOR, row_highlight_quad,
};

/// 压暗背板这一帧的填色矩形——恒一块，恒 [`BACKDROP_COLOR`]。
pub fn backdrop_quad(rect: Rect) -> crate::widget::quad::QuadInstance {
    crate::widget::quad::QuadInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        color: BACKDROP_COLOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_text::MeasureText;
    use std::path::Path;

    fn 测试目录() -> Catalog {
        Catalog::load_one(
            crate::TEST_LOCALE_NAMESPACE,
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/locales")),
        )
    }

    fn 测试数据<'a>(rows: &'a [String], cursor: usize) -> ScreenData<'a> {
        ScreenData {
            title_key: "screen-menu-title",
            rows,
            cursor,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        }
    }

    #[test]
    fn 每一行的矩形正对着那一行的文字() {
        // 「点击落在第几行」与「第几行画在哪儿」必须永远对得上——两者
        // 走的是同一段布局算法（同一个 `row_rects`），这条盯的就是这
        // 件事：第 i 行矩形的纵向范围必须罩住第 i 行标签的原点。
        //
        // 反例验证（已实跑）：把 `row_rects` 里的 `row + 1.0` 改成
        // `row`（忘掉标题占的那一行），本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let rows: Vec<String> = ["甲", "乙", "丙"].iter().map(|s| s.to_string()).collect();
        let data = 测试数据(&rows, 1);

        // Act
        let content = build_screen_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert_eq!(content.row_rects.len(), rows.len(), "一行一块矩形");
        for (row, rect) in content.row_rects.iter().enumerate() {
            // 第 0 个标签是标题，第 i 行的标签因此是第 i + 1 个。
            let label = &content.labels[row + 1];
            assert!(
                rect.y <= label.y && label.y < rect.y + rect.height,
                "第 {row} 行的矩形应当罩住第 {row} 行文字的原点：矩形 {rect:?}，文字 y={}",
                label.y
            );
            assert!(
                rect.x >= content.panel.x && rect.x + rect.width <= content.panel.right(),
                "行矩形不该伸出面板"
            );
        }
    }

    #[test]
    fn 行矩形横向占满面板内容宽而不是只包住那几个字() {
        // 可点区域是**一整行**：只包住文字宽度的话，玩家点在一行右侧
        // 的空白上什么都不会发生，而那看起来仍然是「那一行」。
        // Arrange
        let catalog = 测试目录();
        let rows = vec!["短".to_string(), "长得多的一行文字".to_string()];
        let data = 测试数据(&rows, 0);

        // Act
        let content = build_screen_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert_eq!(
            content.row_rects[0].width, content.row_rects[1].width,
            "两行宽度应当一样，与各自文字长短无关"
        );
        assert_eq!(
            content.row_rects[0].width,
            SCREEN_WIDTH - SCREEN_PADDING * 2.0
        );
    }

    #[test]
    fn 列表为空时一块行矩形都没有() {
        // 占位行（「没有存档」之类）不是一个可点的按钮。
        // Arrange
        let catalog = 测试目录();
        let rows: Vec<String> = Vec::new();
        let data = 测试数据(&rows, 0);

        // Act
        let content = build_screen_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert!(content.row_rects.is_empty());
    }

    #[test]
    fn 只量行矩形与整块排版算出来的完全一致() {
        // 输入侧（这一帧点在第几行）与渲染侧（第几行画在哪儿）走的是
        // 同一段算法，这条把「同一段」钉死。
        //
        // 反例验证（已实跑）：把 `screen_row_rects` 里的
        // `origin.1 + SCREEN_PADDING` 写成 `origin.1`，本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let rows: Vec<String> = ["甲", "乙", "丙", "丁"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let data = 测试数据(&rows, 2);

        // Act
        let 整块 = build_screen_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1024.0,
            768.0,
        );
        let 只量 = screen_row_rects(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1024.0,
            768.0,
        );

        // Assert
        assert_eq!(只量, 整块.row_rects);
    }

    #[test]
    fn 行文字里不再有任何光标记号() {
        // 规格 W7：光标记号从**文本内容**里拿出来了（改成一块高亮
        // 矩形），行文字就是调用方给的那一串，一个字符都不多。
        //
        // 反例验证（已实跑）：把 `screen_text_lines` 的 `lines.extend`
        // 改回拼前缀，本条红在第 2 行。
        // Arrange
        let rows = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];
        let data = 测试数据(&rows, 1);

        // Act
        let lines = screen_lines(
            &data,
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            crate::测试断行宽,
        );

        // Assert：第 0 行是标题，所以第 1 条行内容在下标 1。
        assert_eq!(lines[1].text, "甲");
        assert_eq!(lines[2].text, "乙");
        assert_eq!(lines[3].text, "丙");
    }

    #[test]
    fn 光标越界时取不到任何一块行矩形() {
        // 此前这条验的是「没有一行以 `"> "` 开头」——那条判据随文本
        // 前缀一起作废了。换成验高亮该画在哪儿：取不到，就一块都不画。
        // Arrange
        let rows = vec!["甲".to_string()];
        let data = 测试数据(&rows, 9);

        // Act
        let rects = screen_row_rects(
            &data,
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            1024.0,
            768.0,
        );

        // Assert：先确认这块屏真的有行（否则「取不到」恒真）。
        assert_eq!(rects.len(), 1);
        assert!(rects.get(data.cursor).is_none());
    }

    #[test]
    fn 提示文字为空时不占一行() {
        // Arrange
        let rows = vec!["甲".to_string()];
        let 无提示 = 测试数据(&rows, 0);
        let mut 有提示 = 测试数据(&rows, 0);
        有提示.notice = Some("出事了");

        // Act
        let 无 = screen_lines(
            &无提示,
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            crate::测试断行宽,
        );
        let 有 = screen_lines(
            &有提示,
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            crate::测试断行宽,
        );

        // Assert
        assert_eq!(有.len(), 无.len() + 1);
    }

    #[test]
    fn 面板高度随行数增长() {
        // 「面板背景比文字短一截」是这条不变式没守住时的典型症状。
        // Arrange
        let 少 = vec!["甲".to_string()];
        let 多 = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];

        // Act
        let 矮 = build_screen_panel(
            &测试数据(&少, 0),
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );
        let 高 = build_screen_panel(
            &测试数据(&多, 0),
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert_eq!(高.panel.height - 矮.panel.height, 2.0 * SCREEN_LINE_HEIGHT);
    }

    #[test]
    fn 面板在屏幕上居中() {
        // Arrange
        let rows = vec!["甲".to_string()];

        // Act
        let content = build_screen_panel(
            &测试数据(&rows, 0),
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert：左右边距相等即为横向居中。
        let 右边距 = 1280.0 - (content.panel.x + content.panel.width);
        assert!((content.panel.x - 右边距).abs() < f32::EPSILON);
    }

    #[test]
    fn 背板铺满整个窗口() {
        // 这是「世界现在不动了」唯一的视觉表达，见 BACKDROP_COLOR 文档。
        // Arrange
        let rows = vec!["甲".to_string()];

        // Act
        let content = build_screen_panel(
            &测试数据(&rows, 0),
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert_eq!(content.backdrop.width, 1280.0);
        assert_eq!(content.backdrop.height, 720.0);
    }

    #[test]
    fn 背板是半透明的不是全遮() {
        // 全遮会让玩家彻底看不见自己站在哪；全透明则看不出世界停了。
        // Act
        let quad = backdrop_quad(Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert!(quad.color[3] > 0.0 && quad.color[3] < 1.0);
    }

    #[test]
    fn 行为空时显示占位行而不是什么都不画() {
        // Arrange
        let rows: Vec<String> = Vec::new();
        let data = 测试数据(&rows, 0);

        // Act
        let lines = screen_lines(
            &data,
            &测试目录(),
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            crate::测试断行宽,
        );

        // Assert：标题 + 占位行 + 提示行。
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn 模态屏居中改走anchored之后逐像素与旧算术相同() {
        // 规格 L2 第 5 处的「改写前后逐像素相同」回归断言。旧写法是
        // `((screen_width - panel_width) / 2.0, (screen_height - panel_height) / 2.0)`。
        //
        // 反例验证（已实跑）：把 `centered_origin` 的
        // `Anchor::Center` 换成 `Anchor::TopCenter`，本条红在 y 上。
        // Arrange & Act
        let rect = centered_origin(1280.0, 720.0, 520.0, 300.0);

        // Assert
        assert_eq!(rect.x, (1280.0 - 520.0) / 2.0);
        assert_eq!(rect.y, (720.0 - 300.0) / 2.0);
        assert_eq!(rect.width, 520.0);
        assert_eq!(rect.height, 300.0);
    }

    #[test]
    fn 面板宽度按本屏最长的一行伸缩() {
        // 规格 W3 / F5：底部那一行提示是这个游戏唯一的按键教学，它被
        // 面板宽度吃掉不是「不好看」。这条用**真实的 en 文案**测，不用
        // 占位符——占位符放得下，断言就永远绿。
        //
        // 反例验证（已实跑）：把 `panel_width` 的
        // `SCREEN_WIDTH.max(longest + ...)` 改回恒 `SCREEN_WIDTH`，
        // 本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        let rows = vec!["短".to_string()];
        let mut data = 测试数据(&rows, 0);
        // `screen-chargen-player-died` 的 en 文案实测 722.5px，远宽于
        // 520 的最小宽度（`knowledge/design/ui-and-navigation.md` 八节
        // O-3）。
        data.hint_key = "screen-chargen-player-died";
        let 提示 = catalog.resolve("en", data.hint_key);
        let 提示宽 = measure
            .measure_text(&提示, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, 1.0e6)
            .max_line_width;
        assert!(
            提示宽 > SCREEN_WIDTH,
            "测试文案必须真的比最小面板宽还宽，实测 {提示宽}"
        );

        // Act
        let content = build_screen_panel(&data, &catalog, "en", &mut measure, 1920.0, 1080.0);

        // Assert：面板宽到足以让那一行一整行画完。
        assert!(
            content.panel.width >= 提示宽 + SCREEN_PADDING * 2.0,
            "面板宽 {} 放不下 {提示宽} 的提示行",
            content.panel.width
        );
    }

    #[test]
    fn 面板再宽也不越过窗口边缘() {
        // 伸缩有上限：窗口窄的时候面板缩回去、文字改走换行，而不是
        // 面板伸到窗口外面（那等于把文字直接藏起来）。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        let rows = vec!["短".to_string()];
        let mut data = 测试数据(&rows, 0);
        data.hint_key = "screen-chargen-player-died";

        // Act：一个刚好放不下 722.5px 那一行的窗口。
        let content = build_screen_panel(&data, &catalog, "en", &mut measure, 640.0, 480.0);

        // Assert
        assert!(
            content.panel.width <= 640.0 - SCREEN_SIDE_MARGIN * 2.0,
            "面板宽 {} 越过了窗口边缘",
            content.panel.width
        );
        assert!(content.panel.x >= SCREEN_SIDE_MARGIN - 0.5);
    }

    #[test]
    fn 面板高度覆盖全部渲染行而不是标签条数() {
        // 规格 W2。一条必然换行的行之后，面板底边必须仍然在最后一行
        // 文字的下面——此前面板高度按**标签条数**算，于是背景比内容矮
        // 一行、最后一行文字掉在面板外面。
        //
        // 反例验证（已实跑）：把 `layout_screen` 的 `content_height`
        // 改成 `lines.len() as f32 * SCREEN_LINE_HEIGHT`，本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        // 一条长到在 500px 内容宽里一定要断行的真实行（拿 en 的死亡
        // 通知当行文字，722.5px），再跟一条短行。
        let 长行 = catalog.resolve("en", "screen-chargen-player-died");
        let rows = vec![长行.clone(), "短".to_string()];
        let data = 测试数据(&rows, 0);

        // Act：窗口窄到面板伸缩被上限钳住，于是那一行必须换行。
        let content = build_screen_panel(&data, &catalog, "en", &mut measure, 560.0, 720.0);
        let wrap = content.panel.width - SCREEN_PADDING * 2.0;
        let 行数 = measure
            .measure_text(&长行, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, wrap)
            .line_count;
        assert!(行数 > 1, "测试文案必须真的会换行，实测 {行数} 行");

        // Assert：最后一条标签的最后一行仍在面板内。
        let 末行 = content.labels.last().expect("至少有标题与提示行");
        let 末行行数 = measure
            .measure_text(&末行.text, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, wrap)
            .line_count;
        let 末行底 = 末行.y + 末行行数 as f32 * SCREEN_LINE_HEIGHT;
        assert!(
            末行底 <= content.panel.bottom() - SCREEN_PADDING + 0.5,
            "最后一行底边 {末行底} 掉出了面板底边 {}",
            content.panel.bottom()
        );
    }

    #[test]
    fn 换行的一行之后行矩形仍然正对着它自己那一行的文字() {
        // 「点击落在第几行」与「第几行画在哪儿」必须永远对得上。此前
        // 行矩形按 `(i + 1) * 行高` 这条公式算，W2 落地后那个假设不再
        // 成立——公式与实际位置会静悄悄地错开，点击落到隔壁那一行上。
        //
        // 反例验证（已实跑）：把 `layout_screen` 里行矩形的高度改回恒
        // `SCREEN_LINE_HEIGHT`，本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        let 长行 = catalog.resolve("en", "screen-chargen-player-died");
        let rows = vec![长行, "第二行".to_string()];
        let data = 测试数据(&rows, 0);

        // Act
        let content = build_screen_panel(&data, &catalog, "en", &mut measure, 560.0, 720.0);

        // Assert 之一：第 0 行换了行，它的矩形高度必须罩住它**自己的
        // 全部渲染行**——否则玩家点在那一行的第二行上会什么都不发生
        // （或者更糟：落到下一行上）。
        //
        // 只断言「第 1 行的矩形罩住第 1 行的文字」是**不够的**：第 0 行
        // 的矩形即使只有一行高，第 1 行的起点仍然是对的（游标本身推进
        // 正确）。这条假绿是 ADR 0022 反例验证当场抓出来的。
        let wrap = content.panel.width - SCREEN_PADDING * 2.0;
        let 第零行 = &content.labels[1];
        let 第零行行数 = measure
            .measure_text(&第零行.text, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, wrap)
            .line_count;
        assert!(
            第零行行数 > 1,
            "测试文案必须真的会换行，实测 {第零行行数} 行"
        );
        assert_eq!(
            content.row_rects[0].height,
            第零行行数 as f32 * SCREEN_LINE_HEIGHT,
            "换了行的那一行，它的可点矩形没盖住自己的全部渲染行"
        );

        // Assert 之二：第 1 行（第二行文字）的矩形要罩住第 1 行标签的
        // 原点。标签顺序是 标题、第 0 行、第 1 行、提示行。
        let 第二行标签 = &content.labels[2];
        let 第二行矩形 = content.row_rects[1];
        assert!(
            第二行矩形.y <= 第二行标签.y + 0.5 && 第二行标签.y < 第二行矩形.bottom() - 0.5,
            "第 1 行矩形 y {}..{} 没罩住那一行文字的 y {}",
            第二行矩形.y,
            第二行矩形.bottom(),
            第二行标签.y
        );
    }

    #[test]
    fn 每一行的断行宽度都等于面板内容宽() {
        // 规格 W1 的不变式：断行宽度是**面板宽度的派生值**，不是某处
        // 写死的常数。
        //
        // 反例验证（已实跑）：把 `screen_geometry` 的 `wrap_width` 改成
        // 一个字面量 `400.0`，本条立刻变红。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        let rows = vec!["甲".to_string(), "乙".to_string()];
        let data = 测试数据(&rows, 0);

        // Act
        let content = build_screen_panel(&data, &catalog, "zh-CN", &mut measure, 1280.0, 720.0);

        // Assert
        let 内容宽 = content.panel.width - SCREEN_PADDING * 2.0;
        for label in &content.labels {
            assert_eq!(label.max_width, 内容宽, "「{}」的断行宽不对", label.text);
        }
    }

    #[test]
    fn 输入侧的行矩形与渲染侧逐个相等() {
        // `screen_row_rects`（输入）与 `build_screen_panel`（渲染）现在
        // 走同一个 `screen_geometry`，两者不可能对不上——这条是那句话的
        // 断言。
        // Arrange
        let catalog = 测试目录();
        let mut measure = crate::测试测量器();
        let 长行 = catalog.resolve("en", "screen-chargen-player-died");
        let rows = vec![长行, "第二行".to_string()];
        let data = 测试数据(&rows, 1);

        // Act
        let 渲染侧 =
            build_screen_panel(&data, &catalog, "en", &mut measure, 560.0, 720.0).row_rects;
        let 输入侧 = screen_row_rects(&data, &catalog, "en", &mut measure, 560.0, 720.0);

        // Assert
        assert_eq!(渲染侧.len(), 输入侧.len());
        for (a, b) in 渲染侧.iter().zip(输入侧.iter()) {
            assert_eq!(a, b);
        }
    }
}
